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
use crate::ui::tabs::filter_tab::filter_state::{FilterRequest, FilterResult, GlobalFilterWorker};
use egui::Color32;
use fancy_regex::{Error, Regex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Global counter for assigning unique highlight IDs
/// Starts high to avoid collision with filter IDs
static NEXT_HIGHLIGHT_ID: AtomicUsize = AtomicUsize::new(1_000_000);

/// State for a single highlight rule
pub struct HighlightState {
    highlight_id: usize,
    pub name: String,
    pub search_text: String,
    pub search_regex: Result<Regex, Error>,
    pub case_sensitive: bool,
    pub color: Color32,
    pub enabled: bool,
    pub show_in_histogram: bool,

    /// Cached matching line indices
    pub filtered_indices: Vec<usize>,
    /// Version of LogStore this cache was computed for
    pub cached_for_version: u64,

    /// Channel for receiving background filter results
    filter_result_rx: Receiver<FilterResult>,
    filter_result_tx: Sender<FilterResult>,
}

impl std::fmt::Debug for HighlightState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HighlightState")
            .field("highlight_id", &self.highlight_id)
            .field("name", &self.name)
            .field("search_text", &self.search_text)
            .field("case_sensitive", &self.case_sensitive)
            .field("color", &self.color)
            .field("enabled", &self.enabled)
            .field("show_in_histogram", &self.show_in_histogram)
            .field("filtered_indices_len", &self.filtered_indices.len())
            .field("cached_for_version", &self.cached_for_version)
            .finish()
    }
}

impl HighlightState {
    pub fn new(name: String, color: Color32) -> Self {
        let (result_tx, result_rx) = channel();
        let highlight_id = NEXT_HIGHLIGHT_ID.fetch_add(1, Ordering::Relaxed);

        Self {
            highlight_id,
            name,
            search_text: String::new(),
            search_regex: Regex::new(""),
            case_sensitive: false,
            color,
            enabled: true,
            show_in_histogram: false,
            filtered_indices: Vec::new(),
            cached_for_version: 0,
            filter_result_rx: result_rx,
            filter_result_tx: result_tx,
        }
    }

    /// Update the search regex based on current search text and case sensitivity
    pub fn update_search_regex(&mut self) {
        let pattern = if !self.case_sensitive {
            format!("(?i){}", self.search_text)
        } else {
            self.search_text.clone()
        };
        self.search_regex = Regex::new(&pattern);
    }

    /// Request background filtering for this highlight
    pub fn request_filter_update(&mut self, store: Arc<LogStore>) {
        self.update_search_regex();
        self.cached_for_version = store.version();

        if !self.search_text.is_empty() {
            log::debug!(
                "Highlight {}: requesting background filter for '{}'",
                self.highlight_id,
                self.search_text
            );
        }

        // Send request to global filter worker (reuses same infrastructure as filters)
        let request = FilterRequest {
            filter_id: self.highlight_id,
            regex: self.search_regex.as_ref().ok().cloned(),
            store,
            result_tx: self.filter_result_tx.clone(),
        };

        GlobalFilterWorker::send_request(request);
    }

    /// Check for completed filter results from background thread
    pub fn check_filter_results(&mut self) -> bool {
        if let Ok(result) = self.filter_result_rx.try_recv() {
            self.filtered_indices = result.filtered_indices;
            log::debug!(
                "Highlight {}: completed filtering ({} matches)",
                self.highlight_id,
                self.filtered_indices.len()
            );
            return true;
        }
        false
    }

    /// Check if cache needs update and request it if so
    pub fn ensure_cache_valid(&mut self, store: &Arc<LogStore>) {
        if self.cached_for_version != store.version() {
            self.request_filter_update(Arc::clone(store));
        }
    }
}
