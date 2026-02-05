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

//! A colored search rule used for both filters and highlights.
//!
//! This module provides the core data structure for regex-based search rules
//! that can be displayed with a name and color. Both filter tabs and
//! highlight rules use this as their foundation.

use egui::Color32;

use crate::core::{SavedSearch, SearchState};

/// A colored search rule that can filter/highlight log lines.
///
/// This is the shared foundation for both filter tabs and highlight rules.
/// It combines a `SearchState` (for regex matching) with display properties.
pub struct SearchRule {
    /// Core search state (handles regex, filtering, caching)
    pub search: SearchState,

    /// Display name for this rule
    pub name: String,

    /// Color used for highlighting matches
    pub color: Color32,

    /// Whether this rule is active/visible
    pub enabled: bool,

    /// Whether to show matches as markers in the histogram
    pub show_in_histogram: bool,
}

impl SearchRule {
    /// Create a new search rule with the given name and color.
    pub fn new(name: String, color: Color32) -> Self {
        Self {
            search: SearchState::new(),
            name,
            color,
            enabled: true,
            show_in_histogram: false,
        }
    }

    /// Get the unique identifier for this rule (delegates to `SearchState`).
    pub const fn id(&self) -> usize {
        self.search.id()
    }

    /// Check if this rule matches a favorite filter's search criteria.
    pub fn matches_search(&self, search_text: &str, case_sensitive: bool) -> bool {
        self.search.search_text == search_text && self.search.case_sensitive == case_sensitive
    }
}

// ============================================================================
// Conversion traits for session persistence
// ============================================================================

impl From<&SavedSearch> for SearchRule {
    fn from(saved: &SavedSearch) -> Self {
        let mut rule = Self::new(saved.name.clone(), saved.color);
        rule.search.search_text.clone_from(&saved.search_text);
        rule.search.exclude_text.clone_from(&saved.exclude_text);
        rule.search.case_sensitive = saved.case_sensitive;
        rule.enabled = saved.enabled;
        rule.show_in_histogram = saved.show_in_histogram;
        rule
    }
}

impl From<&SearchRule> for SavedSearch {
    fn from(rule: &SearchRule) -> Self {
        Self {
            name: rule.name.clone(),
            search_text: rule.search.search_text.clone(),
            exclude_text: rule.search.exclude_text.clone(),
            case_sensitive: rule.search.case_sensitive,
            color: rule.color,
            enabled: rule.enabled,
            show_in_histogram: rule.show_in_histogram,
        }
    }
}
