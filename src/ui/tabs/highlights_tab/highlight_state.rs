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

use crate::core::SearchState;
use egui::Color32;

/// State for a single highlight rule.
///
/// Uses `SearchState` for the core search functionality, adding highlight-specific
/// features like enabled state and display settings.
pub struct HighlightState {
    /// Core search state (handles regex, filtering, caching)
    pub search: SearchState,

    /// Display name for this highlight
    pub name: String,

    /// Color used for highlighting matches
    pub color: Color32,

    /// Whether this highlight is currently active
    pub enabled: bool,

    /// Whether to show matches as markers in the histogram
    pub show_in_histogram: bool,
}

impl HighlightState {
    pub fn new(name: String, color: Color32) -> Self {
        Self {
            search: SearchState::new(),
            name,
            color,
            enabled: true,
            show_in_histogram: false,
        }
    }
}
