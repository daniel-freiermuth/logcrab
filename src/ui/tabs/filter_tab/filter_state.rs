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
use crate::filter_worker::{FilterRequest, FilterResult, GlobalFilterWorker};
use crate::ui::tabs::filter_tab::histogram::HistogramCache;
use egui::Color32;
use fancy_regex::{Error, Regex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Global counter for assigning unique filter IDs
static NEXT_FILTER_ID: AtomicUsize = AtomicUsize::new(0);
/// Represents a single filter view with its own search criteria and cached results
pub struct FilterState {
    filter_id: usize, // Unique identifier for this filter instance
    pub search_text: String,
    pub search_regex: Result<Regex, Error>,
    pub case_sensitive: bool,
    pub filtered_indices: Vec<usize>,
    pub last_rendered_selection: Option<usize>,
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
            last_rendered_selection: None,
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

        // Send request to global worker
        GlobalFilterWorker::send_request(request);
    }

    /// Check for completed filter results from background thread
    pub fn check_filter_results(&mut self) {
        profiling::function_scope!();

        if let Ok(result) = self.filter_result_rx.try_recv() {
            self.filtered_indices = result.filtered_indices;
            self.last_rendered_selection = None;
            log::debug!(
                "Filter {}: Completed background filtering (found {} matches)",
                self.filter_id,
                self.filtered_indices.len()
            );
        }
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

    pub fn get_id(&self) -> usize {
        self.filter_id
    }
}
