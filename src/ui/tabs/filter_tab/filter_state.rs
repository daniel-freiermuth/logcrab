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

use crate::core::{SavedFilter, SearchState};
use crate::ui::tabs::filter_tab::histogram::HistogramCache;
use crate::ui::tabs::filter_tab::log_table::ColumnWidths;
use egui::Color32;

/// Represents a single filter view with its own search criteria and cached results.
///
/// Uses `SearchState` for the core search functionality, adding filter-specific
/// features like display settings and histogram caching.
pub struct FilterState {
    /// Core search state (handles regex, filtering, caching)
    pub search: SearchState,

    /// Last rendered selection for scroll tracking
    pub last_rendered_selection: Option<usize>,

    /// Display name for this filter
    pub name: String,

    /// Color used for highlighting matches
    pub color: Color32,

    /// Whether this filter's highlights should be shown in all tabs
    pub globally_visible: bool,

    /// Whether to show vertical markers in the histogram
    pub show_in_histogram: bool,

    /// Histogram cache for expensive bucket computations
    pub histogram_cache: HistogramCache,

    /// Column widths for the log table
    pub column_widths: ColumnWidths,
}

impl FilterState {
    pub fn new(name: String, color: Color32) -> Self {
        Self {
            search: SearchState::new(),
            last_rendered_selection: None,
            name,
            color,
            globally_visible: true,
            show_in_histogram: false,
            histogram_cache: HistogramCache::default(),
            column_widths: ColumnWidths::default(),
        }
    }

    /// Get the unique filter ID
    pub fn get_id(&self) -> usize {
        self.search.id()
    }
}

// ============================================================================
// Conversion traits for session persistence
// ============================================================================

impl From<&SavedFilter> for FilterState {
    fn from(saved: &SavedFilter) -> Self {
        let mut filter = Self::new(saved.name.clone(), saved.color);
        filter.search.search_text.clone_from(&saved.search_text);
        filter.search.case_sensitive = saved.case_sensitive;
        filter.globally_visible = saved.enabled;
        filter.show_in_histogram = saved.show_in_histogram;
        filter
    }
}

impl From<&FilterState> for SavedFilter {
    fn from(filter: &FilterState) -> Self {
        Self {
            search_text: filter.search.search_text.clone(),
            case_sensitive: filter.search.case_sensitive,
            name: filter.name.clone(),
            color: filter.color,
            enabled: filter.globally_visible,
            show_in_histogram: filter.show_in_histogram,
        }
    }
}
