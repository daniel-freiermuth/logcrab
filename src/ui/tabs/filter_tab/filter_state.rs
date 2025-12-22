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

use crate::core::log_store::StoreID;
use crate::core::{SavedFilter, SearchRule};
use crate::ui::tabs::filter_tab::histogram::HistogramCache;
use crate::ui::tabs::filter_tab::log_table::ColumnWidths;
use egui::Color32;

/// Represents a single filter view with its own search criteria and cached results.
///
/// Wraps a `SearchRule` (shared with highlights) and adds filter-tab-specific
/// UI state like scroll tracking, histogram cache, and column widths.
pub struct FilterState {
    /// Core search rule (name, color, search, enabled, `show_in_histogram`)
    pub rule: SearchRule,

    /// Last rendered selection for scroll tracking
    pub last_rendered_selection: Option<StoreID>,

    /// Histogram cache for expensive bucket computations
    pub histogram_cache: HistogramCache,

    /// Column widths for the log table
    pub column_widths: ColumnWidths,
}

impl FilterState {
    pub fn new(name: String, color: Color32) -> Self {
        let rule = SearchRule::new(name, color);
        let filter_id = rule.id();
        Self {
            rule,
            last_rendered_selection: None,
            histogram_cache: HistogramCache::new(filter_id),
            column_widths: ColumnWidths::default(),
        }
    }

    /// Get the unique filter ID
    pub fn get_id(&self) -> usize {
        self.rule.id()
    }
}

// ============================================================================
// Convenience accessors for common fields (reduces churn in calling code)
// ============================================================================

impl std::ops::Deref for FilterState {
    type Target = SearchRule;

    fn deref(&self) -> &Self::Target {
        &self.rule
    }
}

impl std::ops::DerefMut for FilterState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.rule
    }
}

// ============================================================================
// Conversion traits for session persistence
// ============================================================================

impl From<&SavedFilter> for FilterState {
    fn from(saved: &SavedFilter) -> Self {
        let rule = SearchRule::from(saved);
        let filter_id = rule.id();
        Self {
            rule,
            last_rendered_selection: None,
            histogram_cache: HistogramCache::new(filter_id),
            column_widths: ColumnWidths::default(),
        }
    }
}

impl From<&FilterState> for SavedFilter {
    fn from(filter: &FilterState) -> Self {
        SavedFilter::from(&filter.rule)
    }
}
