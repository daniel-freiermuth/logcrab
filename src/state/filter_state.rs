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

use crate::parser::line::LogLine;
use egui::{text::LayoutJob, Color32, TextFormat};
use fancy_regex::Regex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, OnceLock};

/// Request to compute filtered indices in background
#[derive(Clone)]
struct FilterRequest {
    filter_id: usize, // Unique identifier for each filter instance
    search_text: String,
    case_insensitive: bool,
    min_score: f64,
    generation: u64,
    lines: Arc<Vec<LogLine>>,        // Shared read-only access to log lines
    result_tx: Sender<FilterResult>, // Each filter has its own result channel
}

/// Result from background filtering
struct FilterResult {
    filtered_indices: Vec<usize>,
    generation: u64,
}

/// Global filter worker channels
struct GlobalFilterWorker {
    request_tx: Sender<FilterRequest>,
}

impl GlobalFilterWorker {
    fn get() -> &'static GlobalFilterWorker {
        static INSTANCE: OnceLock<GlobalFilterWorker> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            let (request_tx, request_rx) = channel::<FilterRequest>();

            // Spawn the single global worker thread
            std::thread::spawn(move || {
                Self::filter_worker(request_rx);
            });

            GlobalFilterWorker { request_tx }
        })
    }

    /// Single background worker that processes all filter requests
    fn filter_worker(request_rx: Receiver<FilterRequest>) {
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_function!();

        log::debug!("Filter worker thread started");

        // Persistent HashMap of pending requests per filter
        // This allows us to accumulate and update requests while processing others
        let mut pending_requests: HashMap<usize, FilterRequest> = HashMap::new();

        // Helper to drain all available requests into the HashMap
        let drain_pending = |pending: &mut HashMap<usize, FilterRequest>| {
            while let Ok(request) = request_rx.try_recv() {
                let filter_id = request.filter_id;
                if let Some(existing) = pending.get(&filter_id) {
                    log::trace!(
                        "Updating pending request for filter {} (gen {} -> {})",
                        filter_id,
                        existing.generation,
                        request.generation
                    );
                }
                pending.insert(filter_id, request);
            }
        };

        // Main processing loop
        while let Ok(first_request) = request_rx.recv() {
            #[cfg(feature = "cpu-profiling")]
            puffin::profile_scope!("process_filter_request");
            pending_requests.insert(first_request.filter_id, first_request);

            // Collect any additional pending requests
            drain_pending(&mut pending_requests);

            while !pending_requests.is_empty() {
                let first_key = *pending_requests.keys().next().unwrap();
                let request = pending_requests.remove(&first_key).unwrap().clone();
                let filter_id = request.filter_id;

                #[cfg(feature = "cpu-profiling")]
                puffin::profile_scope!("process_single_filter", format!("filter_{}", filter_id));
                log::trace!(
                    "Processing filter request (generation {}, search: '{}')",
                    request.generation,
                    request.search_text
                );

                // Build regex for search
                let search_regex = {
                    #[cfg(feature = "cpu-profiling")]
                    puffin::profile_scope!("build_regex");

                    if !request.search_text.is_empty() {
                        // Use fancy-regex with (?i) inline flag for case-insensitive matching
                        let pattern = if request.case_insensitive {
                            format!("(?i){}", request.search_text)
                        } else {
                            request.search_text.clone()
                        };
                        match Regex::new(&pattern) {
                            Ok(r) => Some(r),
                            Err(e) => {
                                log::warn!(
                                    "Failed to build regex for '{}': {}",
                                    request.search_text,
                                    e
                                );
                                None
                            }
                        }
                    } else {
                        None
                    }
                };

                // Filter lines
                let filtered_indices = {
                    #[cfg(feature = "cpu-profiling")]
                    puffin::profile_scope!(
                        "filter_lines",
                        format!("{} lines", request.lines.len())
                    );

                    let mut indices = Vec::with_capacity(request.lines.len() / 10);
                    for (idx, line) in request.lines.iter().enumerate() {
                        // Check score filter
                        if line.anomaly_score < request.min_score {
                            continue;
                        }

                        // Check search filter
                        if let Some(ref regex) = search_regex {
                            // fancy-regex returns Result<bool>, handle it
                            let matches = regex.is_match(&line.message).unwrap_or(false)
                                || regex.is_match(&line.raw).unwrap_or(false);
                            if !matches {
                                continue;
                            }
                        }

                        indices.push(idx);
                    }
                    indices
                };

                // Check one more time if a newer request arrived during processing
                drain_pending(&mut pending_requests);

                log::trace!(
                    "Filter {} complete: {} matches (generation {})",
                    filter_id,
                    filtered_indices.len(),
                    request.generation
                );

                let result = FilterResult {
                    filtered_indices,
                    generation: request.generation,
                };

                // Send result back to the specific filter (ignore errors if filter is gone)
                {
                    #[cfg(feature = "cpu-profiling")]
                    puffin::profile_scope!("send_result");

                    let _ = request.result_tx.send(result);
                }
            }
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
    pub search_regex: Option<Regex>,
    pub regex_error: Option<String>,
    pub case_insensitive: bool,
    pub filtered_indices: Vec<usize>,
    pub filter_dirty: bool,
    pub last_rendered_selection: Option<usize>,
    pub highlight_color: Color32,
    pub is_favorite: bool,
    pub name: Option<String>,

    // Background filtering - each filter has its own result channel
    filter_result_rx: Receiver<FilterResult>,
    filter_result_tx: Sender<FilterResult>, // Keep sender to create requests
    filter_generation: u64,
    pending_min_score: f64,
    pub is_filtering: bool, // True when request sent but result not yet received
}

impl FilterState {
    pub fn new(highlight_color: Color32) -> Self {
        // Create result channel for this specific filter
        let (result_tx, filter_result_rx) = channel::<FilterResult>();

        // Assign unique filter ID
        let filter_id = NEXT_FILTER_ID.fetch_add(1, Ordering::Relaxed);

        FilterState {
            filter_id,
            search_text: String::new(),
            search_regex: None,
            regex_error: None,
            case_insensitive: false,
            filtered_indices: Vec::new(),
            filter_dirty: true,
            last_rendered_selection: None,
            highlight_color,
            is_favorite: false,
            name: None,
            filter_result_rx,
            filter_result_tx: result_tx,
            filter_generation: 0,
            pending_min_score: 0.0,
            is_filtering: false,
        }
    }

    /// Update the search regex based on current search text
    pub fn update_search_regex(&mut self) {
        if self.search_text.is_empty() {
            self.search_regex = None;
            self.regex_error = None;
        } else {
            // Use fancy-regex with (?i) inline flag for case-insensitive matching
            let pattern = if self.case_insensitive {
                format!("(?i){}", self.search_text)
            } else {
                self.search_text.clone()
            };
            match Regex::new(&pattern) {
                Ok(regex) => {
                    self.search_regex = Some(regex);
                    self.regex_error = None;
                }
                Err(e) => {
                    self.search_regex = None;
                    self.regex_error = Some(e.to_string());
                }
            }
        }
        self.filter_dirty = true;
    }

    /// Send a filter request to the background thread
    pub fn request_filter_update(&mut self, lines: Arc<Vec<LogLine>>, min_score_filter: f64) {
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_function!();

        self.filter_generation += 1;
        self.pending_min_score = min_score_filter;

        // Only mark as filtering if we have search text
        // (min_score filtering is usually fast enough to not need indication)
        if !self.search_text.is_empty() {
            self.is_filtering = true;
            log::debug!(
                "Filter {}: Started background filtering for search: '{}'",
                self.filter_id,
                self.search_text
            );
        }

        let request = FilterRequest {
            filter_id: self.filter_id,
            search_text: self.search_text.clone(),
            case_insensitive: self.case_insensitive,
            min_score: min_score_filter,
            generation: self.filter_generation,
            lines,
            result_tx: self.filter_result_tx.clone(),
        };

        // Send request to global worker (ignore error if worker thread is gone)
        let _ = GlobalFilterWorker::get().request_tx.send(request);
    }

    /// Check for completed filter results from background thread
    pub fn check_filter_results(&mut self) -> bool {
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_function!();

        let mut updated = false;

        // Drain all available results, keeping only the newest
        while let Ok(result) = self.filter_result_rx.try_recv() {
            // Only apply results from the current or newer generation
            if result.generation >= self.filter_generation {
                self.filtered_indices = result.filtered_indices;
                self.filter_dirty = false;
                if self.is_filtering {
                    log::debug!(
                        "Filter {}: Completed background filtering (found {} matches)",
                        self.filter_id,
                        self.filtered_indices.len()
                    );
                }
                self.is_filtering = false; // Filtering complete
                updated = true;
            }
        }

        updated
    }

    /// Check if a line matches the current search criteria
    pub fn matches_search(&self, line: &LogLine) -> bool {
        if let Some(ref regex) = self.search_regex {
            regex.is_match(&line.message).unwrap_or(false)
                || regex.is_match(&line.raw).unwrap_or(false)
        } else {
            true
        }
    }

    /// Rebuild the filtered indices based on current filter criteria
    pub fn rebuild_filtered_indices(
        &mut self,
        lines: &[LogLine],
        min_score_filter: f64,
        selected_line_index: Option<usize>,
    ) -> Option<usize> {
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_function!();

        self.filtered_indices.clear();
        self.filtered_indices.reserve(lines.len() / 10);

        for (idx, line) in lines.iter().enumerate() {
            if line.anomaly_score >= min_score_filter && self.matches_search(line) {
                self.filtered_indices.push(idx);
            }
        }
        self.filter_dirty = false;
        self.is_filtering = false; // Clear filtering flag since we did it synchronously

        // Find selected line in filtered results
        if let Some(selected_line_idx) = selected_line_index {
            if let Some(position) = self
                .filtered_indices
                .iter()
                .position(|&idx| idx == selected_line_idx)
            {
                return Some(position);
            }
            return self.find_closest_timestamp_index(selected_line_idx);
        }

        None
    }

    /// Find the closest line by timestamp in the filtered results
    // TODO Implement via binary search
    pub fn find_closest_timestamp_index(&self, target_idx: usize) -> Option<usize> {
        if self.filtered_indices.is_empty() {
            return None;
        }

        let mut closest_idx = 0;
        let mut min_diff = i64::MAX;

        for (filtered_idx, &line_idx) in self.filtered_indices.iter().enumerate() {
            let diff = (line_idx as i64 - target_idx as i64).abs();
            if diff < min_diff {
                min_diff = diff;
                closest_idx = filtered_idx;
            }
        }

        Some(closest_idx)
    }

    /// Highlight search matches in text with background color
    pub fn highlight_matches(&self, text: &str, base_color: Color32) -> LayoutJob {
        let mut job = LayoutJob::default();

        if let Some(ref regex) = self.search_regex {
            let mut last_end = 0;

            for mat in regex.find_iter(text).flatten() {
                if mat.start() > last_end {
                    job.append(
                        &text[last_end..mat.start()],
                        0.0,
                        TextFormat {
                            color: base_color,
                            ..Default::default()
                        },
                    );
                }

                job.append(
                    mat.as_str(),
                    0.0,
                    TextFormat {
                        color: Color32::BLACK,
                        background: self.highlight_color,
                        ..Default::default()
                    },
                );

                last_end = mat.end();
            }

            if last_end < text.len() {
                job.append(
                    &text[last_end..],
                    0.0,
                    TextFormat {
                        color: base_color,
                        ..Default::default()
                    },
                );
            }
        } else {
            job.append(
                text,
                0.0,
                TextFormat {
                    color: base_color,
                    ..Default::default()
                },
            );
        }

        job
    }
}
