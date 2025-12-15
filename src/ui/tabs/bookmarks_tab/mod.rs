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

pub mod bookmark_panel;

use std::sync::Arc;

pub use bookmark_panel::{BookmarkData, BookmarkPanel, BookmarkPanelEvent};

use crate::{
    core::LogStore,
    input::ShortcutAction,
    ui::{
        filter_highlight::FilterHighlight,
        log_view::{LogViewState, SavedFilter},
        tabs::{filter_tab::HistogramMarker, LogCrabTab},
    },
};
use egui::Ui;

/// Events that can be emitted by the bookmarks view
#[derive(Debug, Clone)]
pub enum BookmarksViewEvent {
    BookmarkClicked { line_index: usize },
    BookmarkDeleted { line_index: usize },
    BookmarkRenamed { line_index: usize, new_name: String },
    StartRenaming { line_index: usize },
    CancelRenaming,
}

/// Orchestrates the bookmarks view UI using the `BookmarkPanel` component
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BookmarksView {
    edited_line_index: Option<usize>,
    bookmark_name_input: String,
    enter_pressed_this_frame: bool,
}

impl BookmarksView {
    /// Render the bookmarks view
    ///
    /// Returns events that occurred during rendering
    pub fn render(
        ui: &mut Ui,
        log_view_state: &LogViewState,
        bookmarks: &[BookmarkData],
        editing_bookmark: Option<usize>,
        bookmark_name_input: &mut String,
        all_filter_highlights: &[FilterHighlight],
    ) -> Vec<BookmarksViewEvent> {
        let panel_events = BookmarkPanel::render(
            ui,
            log_view_state,
            bookmarks,
            editing_bookmark,
            bookmark_name_input,
            all_filter_highlights,
        );

        // Transform panel events to view events
        panel_events
            .into_iter()
            .map(|event| match event {
                BookmarkPanelEvent::BookmarkClicked { line_index } => {
                    BookmarksViewEvent::BookmarkClicked { line_index }
                }
                BookmarkPanelEvent::BookmarkDeleted { line_index } => {
                    BookmarksViewEvent::BookmarkDeleted { line_index }
                }
                BookmarkPanelEvent::BookmarkRenamed {
                    line_index,
                    new_name,
                } => BookmarksViewEvent::BookmarkRenamed {
                    line_index,
                    new_name,
                },
                BookmarkPanelEvent::StartRenaming { line_index } => {
                    BookmarksViewEvent::StartRenaming { line_index }
                }
                BookmarkPanelEvent::CancelRenaming => BookmarksViewEvent::CancelRenaming,
            })
            .collect()
    }

    fn start_renaming_bookmark(&mut self, line_index: usize, data_state: &LogViewState) {
        if let Some(bookmark) = data_state.bookmarks.get(&line_index) {
            self.edited_line_index = Some(line_index);
            self.bookmark_name_input.clone_from(&bookmark.name);
        }
    }

    pub fn render_bookmarks(
        &mut self,
        ui: &mut Ui,
        data_state: &mut LogViewState,
        all_filter_highlights: &[FilterHighlight],
    ) {
        // Check if Enter was pressed this frame (when not editing)
        if self.edited_line_index.is_none() {
            self.enter_pressed_this_frame = ui.input(|i| i.key_pressed(egui::Key::Enter));
        } else {
            self.enter_pressed_this_frame = false;
        }

        // Convert bookmarks to BookmarkData format
        let mut bookmarks: Vec<BookmarkData> = data_state
            .bookmarks
            .values()
            .map(|b| BookmarkData {
                line_index: b.line_index,
                name: b.name.clone(),
                timestamp: b.timestamp,
            })
            .collect();
        bookmarks.sort_by_key(|b| b.line_index);

        // Render using BookmarksView
        let events = Self::render(
            ui,
            data_state,
            &bookmarks,
            self.edited_line_index,
            &mut self.bookmark_name_input,
            all_filter_highlights,
        );

        // Handle events
        for event in events {
            match event {
                BookmarksViewEvent::BookmarkClicked { line_index } => {
                    data_state.selected_line_index = line_index;
                }
                BookmarksViewEvent::BookmarkDeleted { line_index } => {
                    data_state.bookmarks.remove(&line_index);
                    data_state.modified = true;
                }
                BookmarksViewEvent::BookmarkRenamed {
                    line_index,
                    new_name,
                } => {
                    if let Some(b) = data_state.bookmarks.get_mut(&line_index) {
                        b.name = new_name;
                    }
                    data_state.modified = true;
                    self.edited_line_index = None;
                }
                BookmarksViewEvent::StartRenaming { line_index } => {
                    self.start_renaming_bookmark(line_index, data_state);
                }
                BookmarksViewEvent::CancelRenaming => {
                    self.edited_line_index = None;
                }
            }
        }
    }

    /// Move selection in bookmarks view
    pub fn move_selection_in_bookmarks(delta: i32, data_state: &mut LogViewState) {
        if data_state.bookmarks.is_empty() {
            return;
        }

        // Get sorted list of bookmark indices
        let mut bookmark_indices: Vec<usize> = data_state.bookmarks.keys().copied().collect();
        // TODO: We shouldn't sort this every time
        bookmark_indices.sort_unstable();

        // Find current position in bookmark list
        // TODO: here we should start from the current selected line, even if not a bookmark
        // TODO: optimize with option-chaining
        let current_pos = bookmark_indices
            .iter()
            .position(|&idx| idx == data_state.selected_line_index)
            .unwrap_or(if delta >= 0 {
                0
            } else {
                bookmark_indices.len() - 1
            });

        let new_pos = if delta < 0 {
            current_pos.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (current_pos + delta as usize).min(bookmark_indices.len() - 1)
        };

        let new_line_index = bookmark_indices[new_pos];
        data_state.selected_line_index = new_line_index;
    }

    /// Jump to the first bookmark (Vim-style gg)
    pub fn jump_to_top_in_bookmarks(data_state: &mut LogViewState) {
        if data_state.bookmarks.is_empty() {
            return;
        }

        let first_index = *data_state.bookmarks.keys().min().unwrap();
        data_state.selected_line_index = first_index;
    }

    /// Jump to the last bookmark (Vim-style G)
    pub fn jump_to_bottom_in_bookmarks(data_state: &mut LogViewState) {
        if data_state.bookmarks.is_empty() {
            return;
        }

        // TODO
        let last_index = data_state.bookmarks.keys().max().unwrap();
        data_state.selected_line_index = *last_index;
    }

    /// Move selection up by one page in bookmarks view
    pub fn page_up_in_bookmarks(data_state: &mut LogViewState) {
        const PAGE_SIZE: i32 = 25;
        Self::move_selection_in_bookmarks(-PAGE_SIZE, data_state);
    }

    /// Move selection down by one page in bookmarks view
    pub fn page_down_in_bookmarks(data_state: &mut LogViewState) {
        const PAGE_SIZE: i32 = 25;
        Self::move_selection_in_bookmarks(PAGE_SIZE, data_state);
    }
}

impl LogCrabTab for BookmarksView {
    fn title(&mut self) -> egui::WidgetText {
        "Bookmarks".into()
    }

    fn render(
        &mut self,
        ui: &mut egui::Ui,
        data_state: &mut LogViewState,
        _global_config: &mut crate::config::GlobalConfig,
        all_filter_highlights: &[FilterHighlight],
        _histogram_markers: &[HistogramMarker],
    ) {
        self.render_bookmarks(ui, data_state, all_filter_highlights);
    }

    fn process_events(
        &mut self,
        actions: &[ShortcutAction],
        data_state: &mut LogViewState,
    ) -> bool {
        // Handle Enter key for starting bookmark rename (when not already editing)
        // enter_pressed_this_frame is set during render when we have UI context
        if self.enter_pressed_this_frame && self.edited_line_index.is_none() {
            let selected_line_index = data_state.selected_line_index;
            self.start_renaming_bookmark(selected_line_index, data_state);
        }

        for action in actions {
            match action {
                ShortcutAction::MoveDown => Self::move_selection_in_bookmarks(1, data_state),
                ShortcutAction::MoveUp => Self::move_selection_in_bookmarks(-1, data_state),
                ShortcutAction::ToggleBookmark => {}
                ShortcutAction::JumpToTop => {
                    Self::jump_to_top_in_bookmarks(data_state);
                }
                ShortcutAction::JumpToBottom => {
                    Self::jump_to_bottom_in_bookmarks(data_state);
                }
                ShortcutAction::PageUp => {
                    Self::page_up_in_bookmarks(data_state);
                }
                ShortcutAction::PageDown => {
                    Self::page_down_in_bookmarks(data_state);
                }
                ShortcutAction::FocusSearch => {}
                ShortcutAction::NewFilterTab => {}
                ShortcutAction::NewBookmarksTab => {}
                ShortcutAction::ReverseCycleTab => {}
                ShortcutAction::OpenFile => {}
                ShortcutAction::RenameFilter => {}
                ShortcutAction::CloseTab => {}
                ShortcutAction::CycleTab => {}
                ShortcutAction::FocusPaneLeft => {}
                ShortcutAction::FocusPaneDown => {}
                ShortcutAction::FocusPaneUp => {}
                ShortcutAction::FocusPaneRight => {}
            }
        }
        false
    }

    fn try_into_stored_filter(&self) -> Option<SavedFilter> {
        None
    }

    fn get_filter_highlight(&self) -> Option<FilterHighlight> {
        None
    }

    fn get_histogram_marker(&mut self, _store: &Arc<LogStore>) -> Option<HistogramMarker> {
        None
    }
}
