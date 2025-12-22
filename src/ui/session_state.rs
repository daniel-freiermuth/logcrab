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

//! Shared session state passed to all tabs.
//!
//! This module contains the state that is shared across all tabs in a session,
//! including bookmarks, highlights, selection state, and filter history.

use std::sync::Arc;

use chrono::{DateTime, Local};
use egui::Color32;

use crate::core::log_store::StoreID;
use crate::core::{FilterWorkerHandle, LogStore, SearchRule};
use crate::ui::tabs::bookmarks_tab::BookmarkData;

/// Shared state for a log viewing session.
///
/// This state is passed to all tabs and contains:
/// - The log store with all parsed lines (including bookmarks)
/// - Current selection
/// - Highlights (which apply across all tabs)
/// - Filter history
/// - Pending conversion requests between filters and highlights
pub struct SessionState {
    pub store: Arc<LogStore>,

    /// Handle to send filter requests to the background worker
    pub filter_worker: FilterWorkerHandle,

    /// Currently selected line index
    pub selected_line_index: Option<StoreID>,

    /// Whether the session has unsaved modifications
    pub modified: bool,

    /// Last time the session was auto-saved
    pub(crate) last_saved: Option<DateTime<Local>>,

    /// Global filter history (shared across all filter tabs)
    pub filter_history: Vec<String>,

    /// Highlight rules that apply across all tabs
    pub highlights: Vec<SearchRule>,

    /// Pending conversion request: highlight index to convert to filter
    pub pending_highlight_to_filter: Option<usize>,

    /// Pending conversion request: filter data to convert to highlight
    pub pending_filter_to_highlight: Option<FilterToHighlightData>,
}

/// Data needed to convert a filter to a highlight
#[derive(Debug, Clone)]
pub struct FilterToHighlightData {
    pub filter_uuid: usize,
    pub name: String,
    pub search_text: String,
    pub case_sensitive: bool,
    pub color: Color32,
    pub enabled: bool,
    pub show_in_histogram: bool,
}

impl SessionState {
    /// Create a new session state with the given log store and filter worker handle.
    pub fn new(store: Arc<LogStore>, filter_worker: FilterWorkerHandle) -> Self {
        Self {
            store,
            filter_worker,
            selected_line_index: None,
            modified: false,
            last_saved: None,
            filter_history: Vec::new(),
            highlights: Vec::new(),
            pending_highlight_to_filter: None,
            pending_filter_to_highlight: None,
        }
    }

    /// Add a filter pattern to the global history (called when filter is committed)
    pub fn add_to_filter_history(&mut self, pattern: String) {
        if pattern.is_empty() {
            return;
        }
        // Remove if already exists to avoid duplicates
        self.filter_history.retain(|p| p != &pattern);
        // Add to front (most recent first)
        self.filter_history.insert(0, pattern);
        // Keep only last 50 entries
        if self.filter_history.len() > 50 {
            self.filter_history.truncate(50);
        }
    }

    // ========================================================================
    // Bookmark Management (delegates to LogStore)
    // ========================================================================

    /// Get a bookmark by `StoreID`
    pub fn get_bookmark(&self, id: &StoreID) -> Option<BookmarkData> {
        self.store.get_bookmark(id)
    }

    /// Get all bookmarks
    pub fn get_all_bookmarks(&self) -> Vec<BookmarkData> {
        self.store.get_all_bookmarks()
    }

    /// Toggle bookmark at the given line index
    pub fn toggle_bookmark(&mut self, line_index: StoreID) {
        if self.store.has_bookmark(&line_index) {
            log::debug!("Removing bookmark at line {line_index:?}");
            self.store.remove_bookmark(&line_index);
        } else if let Some(line) = self.store.get_by_id(&line_index) {
            let bookmark_name = format!("Line {}", line.line_number);
            log::debug!("Adding bookmark: {bookmark_name}");
            self.store.set_bookmark(&line_index, bookmark_name);
        }
        self.modified = true;
    }

    /// Toggle bookmark for the currently selected line
    pub fn toggle_bookmark_for_selected(&mut self) {
        if let Some(line_index) = self.selected_line_index {
            self.toggle_bookmark(line_index);
        }
    }

    /// Rename a bookmark
    pub fn rename_bookmark(&mut self, id: &StoreID, new_name: String) {
        self.store.set_bookmark(id, new_name);
        self.modified = true;
    }

    /// Remove a bookmark
    pub fn remove_bookmark(&mut self, id: &StoreID) {
        self.store.remove_bookmark(id);
        self.modified = true;
    }
}
