// LogCrab - GPL-3.0-or-later
// This file is part of LogCrab.
//
// Copyright (C) 2026 Daniel Freiermuth
//
// LogCrab is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// LogCrab is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with LogCrab.  If not, see <https://www.gnu.org/licenses/>.

//! Background worker for filter computations.
//!
//! This module provides a worker thread that processes filter requests
//! from both `FilterState` and `HighlightState`, avoiding duplicate threading logic.
//!
//! The worker is owned by the application and shuts down gracefully when dropped.

use crate::core::log_store::StoreID;
use crate::core::LogStore;
use crate::parser::line::LogLineCore;
use fancy_regex::Regex;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Request to compute filtered indices in background
#[derive(Clone)]
pub struct FilterRequest {
    pub filter_id: usize, // Unique identifier for each filter/highlight instance
    pub regex: Regex,
    pub exclude_regex: Option<Regex>,
    pub store: Arc<LogStore>, // Shared read-only access to log store
    pub result_tx: Sender<FilterResult>, // Each filter has its own result channel
    /// The search text this request was made for (for result tracking)
    pub search_text: String,
    /// The exclude text this request was made for (for result tracking)
    pub exclude_text: String,
    /// Whether case sensitivity was enabled (for result tracking)
    pub case_sensitive: bool,
}

/// Result from background filtering
pub struct FilterResult {
    pub filtered_indices: Vec<StoreID>,
    /// The search text these indices were computed for
    pub search_text: String,
    /// The exclude text these indices were computed for
    pub exclude_text: String,
    /// Whether case sensitivity was enabled
    pub case_sensitive: bool,
}

/// Handle to send filter requests to the background worker.
///
/// Clone this to send requests from multiple places.
/// When all handles are dropped, the worker thread exits gracefully.
#[derive(Clone)]
pub struct FilterWorkerHandle {
    request_tx: Sender<FilterRequest>,
    pub is_filtering: Arc<AtomicBool>,
}

impl FilterWorkerHandle {
    /// Send a filter request to the background worker
    pub fn send_request(&self, request: FilterRequest) {
        let _ = self.request_tx.send(request);
    }
}

/// Background filter worker that processes filter requests.
///
/// When dropped, the sender channel closes and the worker thread exits.
pub struct FilterWorker {
    /// Handle for sending requests (can be cloned and shared)
    handle: FilterWorkerHandle,
    /// Thread handle (joined on drop for clean shutdown)
    _thread: std::thread::JoinHandle<()>,
}

impl FilterWorker {
    /// Create a new filter worker with a background thread.
    pub fn new() -> Self {
        let (request_tx, request_rx) = channel::<FilterRequest>();
        let is_filtering = Arc::new(AtomicBool::new(false));
        let is_filtering_copy = is_filtering.clone();

        let thread = std::thread::spawn(move || {
            Self::worker_loop(&request_rx, &is_filtering_copy);
        });

        Self {
            handle: FilterWorkerHandle {
                request_tx,
                is_filtering,
            },
            _thread: thread,
        }
    }

    /// Get a handle to send requests to this worker.
    /// The handle can be cloned and shared across the application.
    pub fn handle(&self) -> FilterWorkerHandle {
        self.handle.clone()
    }

    /// Background worker loop that processes filter requests.
    fn worker_loop(request_rx: &Receiver<FilterRequest>, is_filtering: &Arc<AtomicBool>) {
        profiling::function_scope!();

        log::debug!("Filter worker thread started");

        // Persistent map of pending requests per filter
        // This allows us to accumulate and update requests while processing others
        let mut pending_requests: BTreeMap<usize, FilterRequest> = BTreeMap::new();

        // Helper to drain all available requests into the map
        let drain_pending = |pending: &mut BTreeMap<usize, FilterRequest>| {
            while let Ok(request) = request_rx.try_recv() {
                let filter_id = request.filter_id;
                if pending.contains_key(&filter_id) {
                    log::trace!("Updating pending request for filter {filter_id}",);
                }
                pending.insert(filter_id, request);
            }
        };

        // Main processing loop - exits when all senders are dropped
        while let Ok(first_request) = request_rx.recv() {
            is_filtering.store(true, Ordering::Relaxed);
            profiling::scope!("process_filter_request");
            pending_requests.insert(first_request.filter_id, first_request);

            // Collect any additional pending requests
            drain_pending(&mut pending_requests);

            while let Some((filter_id, request)) = pending_requests.pop_first() {

                profiling::scope!("process_single_filter");
                log::trace!("Processing filter request (search: '{:?}')", request.regex);

                // Filter lines in parallel
                let filtered_indices = {
                    profiling::scope!("filter_lines");

                    // Parallel filtering with rayon
                    request.store.get_matching_ids(|line| {
                        let matches_include =
                            request.regex.is_match(&line.message()).unwrap_or(false)
                                || request.regex.is_match(&line.raw()).unwrap_or(false);

                        if !matches_include {
                            return false;
                        }

                        // If there's an exclude pattern, check if the line matches it
                        request.exclude_regex.as_ref().is_none_or(|exclude_regex| {
                            let matches_exclude =
                                exclude_regex.is_match(&line.message()).unwrap_or(false)
                                    || exclude_regex.is_match(&line.raw()).unwrap_or(false);
                            // Return true only if it doesn't match the exclusion pattern
                            !matches_exclude
                        })
                    })
                };

                log::trace!(
                    "Filter {} complete: {} matches",
                    filter_id,
                    filtered_indices.len(),
                );

                let result = FilterResult {
                    filtered_indices,
                    search_text: request.search_text.clone(),
                    exclude_text: request.exclude_text.clone(),
                    case_sensitive: request.case_sensitive,
                };

                // Send result back to the specific filter (ignore errors if filter is gone)
                {
                    profiling::scope!("send_result");

                    let _ = request.result_tx.send(result);
                }

                // Check one more time if a newer request arrived during processing
                drain_pending(&mut pending_requests);
            }
            is_filtering.store(false, Ordering::Relaxed);
        }
        log::debug!("Filter worker thread shutting down (channel closed)");
    }
}
