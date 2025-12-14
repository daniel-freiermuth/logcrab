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

use crate::core::LogStore;
use crate::ui::tabs::filter_tab::histogram::HistogramCache;
use egui::Color32;
use fancy_regex::{Error, Regex};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, OnceLock};

/// Request to compute filtered indices in background
#[derive(Clone)]
struct FilterRequest {
    filter_id: usize, // Unique identifier for each filter instance
    regex: Option<Regex>,
    store: Arc<LogStore>,            // Shared read-only access to log store
    result_tx: Sender<FilterResult>, // Each filter has its own result channel
}

/// Result from background filtering
struct FilterResult {
    filtered_indices: Vec<usize>,
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

                    if let Some(ref regex) = request.regex {
                        // Parallel filtering with rayon
                        request.store.get_matching_ids(|line| {
                            regex.is_match(&line.message).unwrap_or(false)
                                || regex.is_match(&line.raw).unwrap_or(false)
                        })
                    } else {
                        // No filter: all indices match
                        (0..request.store.total_lines()).collect()
                    }
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

/// Global counter for assigning unique filter IDs
static NEXT_FILTER_ID: AtomicUsize = AtomicUsize::new(0);

/// Represents a single filter view with its own search criteria and cached results
pub struct FilterState {
    filter_id: usize, // Unique identifier for this filter instance
    pub search_text: String,
    pub search_regex: Result<Regex, Error>,
    pub case_sensitive: bool,
    pub filtered_indices: Vec<usize>,
    pub last_rendered_selection: usize,
    pub name: String,
    pub color: Color32,
    pub globally_visible: bool, // Whether this filter's highlights should be shown in all tabs
    pub show_in_histogram: bool, // Whether to show vertical markers in the histogram

    // Version-based cache invalidation (Step 9)
    pub cached_for_version: u64,

    // Histogram cache for expensive bucket computations
    pub histogram_cache: HistogramCache,

    // Background filtering - each filter has its own result channel
    filter_result_rx: Receiver<FilterResult>,
    filter_result_tx: Sender<FilterResult>, // Keep sender to create requests
}

impl FilterState {
    pub fn new(name: String, color: Color32) -> Self {
        // Create result channel for this specific filter
        let (result_tx, filter_result_rx) = channel::<FilterResult>();

        // Assign unique filter ID
        let filter_id = NEXT_FILTER_ID.fetch_add(1, Ordering::Relaxed);

        let initial_filter = String::new();
        let initial_regex = Regex::new(&initial_filter);

        Self {
            filter_id,
            search_text: initial_filter,
            search_regex: initial_regex,
            case_sensitive: false,
            filtered_indices: Vec::new(),
            last_rendered_selection: 0,
            name,
            color,
            globally_visible: true,
            show_in_histogram: false,
            cached_for_version: 0,
            histogram_cache: HistogramCache::default(),
            filter_result_rx,
            filter_result_tx: result_tx,
        }
    }

    /// Update the search regex based on current search text
    pub fn update_search_regex(&mut self) {
        // Use fancy-regex with (?i) inline flag for case-insensitive matching
        let pattern = if !self.case_sensitive {
            format!("(?i){}", self.search_text)
        } else {
            self.search_text.clone()
        };
        self.search_regex = Regex::new(&pattern);
    }

    /// Send a filter request to the background thread
    pub fn request_filter_update(&mut self, store: Arc<LogStore>) {
        self.update_search_regex();

        profiling::function_scope!();

        self.cached_for_version = store.version();

        // Only mark as filtering if we have search text
        if !self.search_text.is_empty() {
            log::debug!(
                "Filter {}: Started background filtering for search: '{}' (version: {})",
                self.filter_id,
                self.search_text,
                store.version()
            );
        }

        let request = FilterRequest {
            filter_id: self.filter_id,
            regex: self.search_regex.as_ref().ok().cloned(),
            store,
            result_tx: self.filter_result_tx.clone(),
        };

        // Send request to global worker (ignore error if worker thread is gone)
        let _ = GlobalFilterWorker::get().request_tx.send(request);
    }

    /// Check for completed filter results from background thread
    pub fn check_filter_results(&mut self) -> bool {
        profiling::function_scope!();

        if let Ok(result) = self.filter_result_rx.try_recv() {
            self.filtered_indices = result.filtered_indices;
            log::debug!(
                "Filter {}: Completed background filtering (found {} matches)",
                self.filter_id,
                self.filtered_indices.len()
            );
            return true;
        }
        false
    }

    /// Find the closest line by timestamp in the filtered results
    // TODO Implement via binary search
    pub fn find_closest_timestamp_index(&self, target_idx: usize) -> usize {
        let mut closest_idx = 0;
        let mut min_diff = i64::MAX;

        for (filtered_idx, &line_idx) in self.filtered_indices.iter().enumerate() {
            let diff = (line_idx as i64 - target_idx as i64).abs();
            if diff < min_diff {
                min_diff = diff;
                closest_idx = filtered_idx;
            }
        }
        closest_idx
    }
}
