// LogCrab - GPL-3.0-or-later
// This file is part of LogCrab.
//
// Copyright (C) 2025 Daniel Freiermuth
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

//! Global background worker for filter computations.
//!
//! This module provides a shared worker thread that processes filter requests
//! from both `FilterState` and `HighlightState`, avoiding duplicate threading logic.

use crate::core::log_store::StoreID;
use crate::core::LogStore;
use fancy_regex::Regex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, OnceLock};

/// Request to compute filtered indices in background
#[derive(Clone)]
pub struct FilterRequest {
    pub filter_id: usize, // Unique identifier for each filter/highlight instance
    pub regex: Regex,
    pub store: Arc<LogStore>, // Shared read-only access to log store
    pub result_tx: Sender<FilterResult>, // Each filter has its own result channel
}

/// Result from background filtering
pub struct FilterResult {
    pub filtered_indices: Vec<StoreID>,
}

/// Global filter worker channels
pub struct GlobalFilterWorker {
    request_tx: Sender<FilterRequest>,
    pub is_filtering: Arc<AtomicBool>,
}

impl GlobalFilterWorker {
    pub fn get() -> &'static Self {
        static INSTANCE: OnceLock<GlobalFilterWorker> = OnceLock::new();
        let is_filtering = Arc::new(AtomicBool::new(false));
        let is_filtering_copy = is_filtering.clone();
        INSTANCE.get_or_init(|| {
            let (request_tx, request_rx) = channel::<FilterRequest>();

            // Spawn the single global worker thread
            std::thread::spawn(move || {
                Self::filter_worker(&request_rx, &is_filtering_copy);
            });

            Self {
                request_tx,
                is_filtering,
            }
        })
    }

    /// Send a filter request to the background worker
    /// Can be used by both `FilterState` and `HighlightState`
    pub fn send_request(request: FilterRequest) {
        let _ = Self::get().request_tx.send(request);
    }

    /// Single background worker that processes all filter requests
    fn filter_worker(request_rx: &Receiver<FilterRequest>, is_filtering: &Arc<AtomicBool>) {
        profiling::function_scope!();

        log::debug!("Filter worker thread started");

        // Persistent HashMap of pending requests per filter
        // This allows us to accumulate and update requests while processing others
        let mut pending_requests: HashMap<usize, FilterRequest> = HashMap::new();

        // Helper to drain all available requests into the HashMap
        let drain_pending = |pending: &mut HashMap<usize, FilterRequest>| {
            while let Ok(request) = request_rx.try_recv() {
                let filter_id = request.filter_id;
                if pending.contains_key(&filter_id) {
                    log::trace!("Updating pending request for filter {filter_id}",);
                }
                pending.insert(filter_id, request);
            }
        };

        // Main processing loop
        while let Ok(first_request) = request_rx.recv() {
            is_filtering.store(true, Ordering::Relaxed);
            profiling::scope!("process_filter_request");
            pending_requests.insert(first_request.filter_id, first_request);

            // Collect any additional pending requests
            drain_pending(&mut pending_requests);

            while !pending_requests.is_empty() {
                let first_key = *pending_requests.keys().next().unwrap();
                let request = pending_requests.remove(&first_key).unwrap().clone();
                let filter_id = request.filter_id;

                profiling::scope!("process_single_filter");
                log::trace!("Processing filter request (search: '{:?}')", request.regex);

                // Filter lines in parallel
                let filtered_indices = {
                    profiling::scope!("filter_lines");

                    // Parallel filtering with rayon
                    request.store.get_matching_ids(|line| {
                        request.regex.is_match(&line.message).unwrap_or(false)
                            || request.regex.is_match(&line.raw).unwrap_or(false)
                    })
                };

                log::trace!(
                    "Filter {} complete: {} matches",
                    filter_id,
                    filtered_indices.len(),
                );

                let result = FilterResult { filtered_indices };

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
