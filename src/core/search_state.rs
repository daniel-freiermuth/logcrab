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

//! Shared search state used by both filters and highlights.
//!
//! This module provides the core regex-based search functionality
//! with background filtering support via the global filter worker.

use crate::core::filter_worker::{FilterRequest, FilterResult, FilterWorkerHandle};
use crate::core::log_store::StoreID;
use crate::core::LogStore;
use fancy_regex::{Error, Regex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Global counter for assigning unique search IDs
static NEXT_SEARCH_ID: AtomicUsize = AtomicUsize::new(0);

/// Core search state shared between filters and highlights.
///
/// Handles regex compilation, background filtering, and result caching.
pub struct SearchState {
    /// Unique identifier for this search instance
    id: usize,
    /// The search pattern text
    pub search_text: String,
    /// Pattern to exclude from results (removes false positives)
    pub exclude_text: String,
    /// Whether the search is case-sensitive
    pub case_sensitive: bool,
    /// Cached indices of matching lines
    filtered_indices: Vec<StoreID>,

    /// What we last requested from the worker (optimistic tracking)
    last_requested_version: u64,
    last_requested_text: String,
    last_requested_exclude: String,
    last_requested_case: bool,

    /// What the current `filtered_indices` was actually computed for
    /// (only updated when results are received)
    indices_computed_for_text: String,
    indices_computed_for_exclude: String,
    indices_computed_for_case: bool,
    indices_computed_for_version: u64,

    /// Channel for receiving background filter results
    filter_result_rx: Receiver<FilterResult>,
    /// Sender kept to create new requests
    filter_result_tx: Sender<FilterResult>,
}

impl SearchState {
    /// Create a new search state with empty search text.
    pub fn new() -> Self {
        let (result_tx, result_rx) = channel();
        let id = NEXT_SEARCH_ID.fetch_add(1, Ordering::Relaxed);

        Self {
            id,
            search_text: String::new(),
            exclude_text: String::new(),
            last_requested_text: String::new(),
            last_requested_exclude: String::new(),
            case_sensitive: false,
            last_requested_case: false,
            indices_computed_for_text: String::new(),
            indices_computed_for_exclude: String::new(),
            indices_computed_for_case: false,
            indices_computed_for_version: 0,
            filtered_indices: Vec::new(),
            last_requested_version: 0,
            filter_result_rx: result_rx,
            filter_result_tx: result_tx,
        }
    }

    /// Get the unique identifier for this search.
    pub const fn id(&self) -> usize {
        self.id
    }

    pub fn get_filtered_indices_cached(&self) -> &Vec<StoreID> {
        profiling::scope!("SearchState::get_filtered_indices");
        &self.filtered_indices
    }

    pub fn get_regex(&self) -> Result<Regex, Box<Error>> {
        let pattern = if self.case_sensitive {
            &self.search_text
        } else {
            &format!("(?i){}", self.search_text)
        };
        Regex::new(pattern).map_err(Box::new)
    }

    pub fn get_exclude_regex(&self) -> Result<Option<Regex>, Box<Error>> {
        if self.exclude_text.is_empty() {
            return Ok(None);
        }
        let pattern = if self.case_sensitive {
            &self.exclude_text
        } else {
            &format!("(?i){}", self.exclude_text)
        };
        Regex::new(pattern).map(Some).map_err(Box::new)
    }

    /// Request a background filter update for the given store.
    fn request_filter_update(&self, store: Arc<LogStore>, worker: &FilterWorkerHandle) {
        if !self.search_text.is_empty() {
            log::trace!(
                "Search {}: requesting background filter for '{}'",
                self.id,
                self.search_text
            );
        }

        if let Ok(regex) = self.get_regex() {
            let exclude_regex = self.get_exclude_regex().ok().flatten();
            let request = FilterRequest {
                filter_id: self.id,
                regex,
                exclude_regex,
                store,
                result_tx: self.filter_result_tx.clone(),
                search_text: self.search_text.clone(),
                exclude_text: self.exclude_text.clone(),
                case_sensitive: self.case_sensitive,
            };

            worker.send_request(request);
        }
    }

    /// Check for completed filter results from background thread.
    /// Returns true if new results were received.
    pub fn check_filter_results(&mut self) -> bool {
        if let Ok(result) = self.filter_result_rx.try_recv() {
            self.filtered_indices = result.filtered_indices;
            // Track what these indices were computed for (from the result, not cached_for)
            self.indices_computed_for_text = result.search_text;
            self.indices_computed_for_exclude = result.exclude_text;
            self.indices_computed_for_case = result.case_sensitive;
            self.indices_computed_for_version = result.store_version;
            log::trace!(
                "Search {}: completed filtering ({} matches)",
                self.id,
                self.filtered_indices.len()
            );
            return true;
        }
        false
    }

    /// Get the search text that the current filtered indices were computed for.
    pub fn indices_computed_for(&self) -> (&str, &str, bool, u64) {
        (
            &self.indices_computed_for_text,
            &self.indices_computed_for_exclude,
            self.indices_computed_for_case,
            self.indices_computed_for_version,
        )
    }

    /// Check if cache is valid for the given store version, request update if not.
    pub fn ensure_cache_valid(&mut self, store: &Arc<LogStore>, worker: &FilterWorkerHandle) {
        if self.last_requested_version != store.version()
            || self.last_requested_text != self.search_text
            || self.last_requested_exclude != self.exclude_text
            || self.last_requested_case != self.case_sensitive
        {
            self.request_filter_update(Arc::clone(store), worker);
            self.last_requested_version = store.version();
            self.last_requested_text = self.search_text.clone();
            self.last_requested_exclude = self.exclude_text.clone();
            self.last_requested_case = self.case_sensitive;
        }
    }

    /// Find the row position of the closest line in filtered results to the target.
    /// Returns the index within the filtered list (for scrolling to that row).
    pub fn find_closest_row_position_in_cache(
        &self,
        target: StoreID,
        store: &Arc<LogStore>,
    ) -> Option<usize> {
        profiling::scope!("find_closest_row_position");
        let indices = {
            profiling::scope!("get_filtered_indices");
            self.get_filtered_indices_cached()
        };
        if indices.is_empty() {
            return None;
        }
        profiling::scope!("find_min_distance");
        Some(indices.partition_point(|other| other.cmp(&target, store) == std::cmp::Ordering::Less))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_exclude_regex_empty() {
        let state = SearchState::new();
        assert!(state.get_exclude_regex().unwrap().is_none());
    }

    #[test]
    fn test_get_exclude_regex_with_pattern() {
        let mut state = SearchState::new();
        state.exclude_text = "ERROR".to_string();
        let regex = state.get_exclude_regex().unwrap();
        assert!(regex.is_some());
        // Case insensitive by default
        assert!(regex.unwrap().is_match("error").unwrap());
    }

    #[test]
    fn test_get_exclude_regex_case_sensitive() {
        let mut state = SearchState::new();
        state.exclude_text = "ERROR".to_string();
        state.case_sensitive = true;
        let regex = state.get_exclude_regex().unwrap();
        assert!(regex.is_some());
        let regex = regex.unwrap();
        assert!(regex.is_match("ERROR").unwrap());
        assert!(!regex.is_match("error").unwrap());
    }

    #[test]
    fn test_get_exclude_regex_invalid() {
        let mut state = SearchState::new();
        state.exclude_text = "[invalid".to_string();
        assert!(state.get_exclude_regex().is_err());
    }
}
