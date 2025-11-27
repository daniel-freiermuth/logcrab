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

pub use bookmark_panel::{BookmarkData, BookmarkPanel, BookmarkPanelEvent};

use crate::{
    input::InputAction,
    parser::line::LogLine,
    ui::{tabs::LogCrabTab, LogView},
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

/// Orchestrates the bookmarks view UI using the BookmarkPanel component
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BookmarksView {
    edited_line_index: Option<usize>,
    bookmark_name_input: String,
}

impl BookmarksView {
    /// Render the bookmarks view
    ///
    /// Returns events that occurred during rendering
    pub fn render(
        ui: &mut Ui,
        lines: &[LogLine],
        bookmarks: Vec<BookmarkData>,
        selected_line_index: Option<usize>,
        editing_bookmark: Option<usize>,
        bookmark_name_input: &mut String,
    ) -> Vec<BookmarksViewEvent> {
        let panel_events = BookmarkPanel::render(
            ui,
            lines,
            &bookmarks,
            selected_line_index,
            editing_bookmark,
            bookmark_name_input,
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

    pub fn render_bookmarks(&mut self, ui: &mut Ui, data_state: &mut LogView) {
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
        let events = BookmarksView::render(
            ui,
            &data_state.lines,
            bookmarks,
            data_state.selected_line_index,
            self.edited_line_index,
            &mut self.bookmark_name_input,
        );

        // Handle events
        let mut should_save = false;
        for event in events {
            match event {
                BookmarksViewEvent::BookmarkClicked { line_index } => {
                    data_state.selected_line_index = Some(line_index);
                }
                BookmarksViewEvent::BookmarkDeleted { line_index } => {
                    data_state.bookmarks.remove(&line_index);
                    should_save = true;
                }
                BookmarksViewEvent::BookmarkRenamed {
                    line_index,
                    new_name,
                } => {
                    if let Some(b) = data_state.bookmarks.get_mut(&line_index) {
                        b.name = new_name;
                        should_save = true;
                    }
                    self.edited_line_index = None;
                }
                BookmarksViewEvent::StartRenaming { line_index } => {
                    if let Some(bookmark) = data_state.bookmarks.get(&line_index) {
                        self.edited_line_index = Some(line_index);
                        self.bookmark_name_input = bookmark.name.clone();
                    }
                }
                BookmarksViewEvent::CancelRenaming => {
                    self.edited_line_index = None;
                }
            }
        }

        if should_save {
            data_state.save_crab_file();
        }
    }

    /// Move selection in bookmarks view
    pub fn move_selection_in_bookmarks(&mut self, delta: i32, data_state: &mut LogView) {
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
        let current_pos = if let Some(sel) = data_state.selected_line_index {
            bookmark_indices
                .iter()
                .position(|&idx| idx == sel)
                .unwrap_or(if delta >= 0 {
                    0
                } else {
                    bookmark_indices.len() - 1
                })
        } else if delta >= 0 {
            0
        } else {
            bookmark_indices.len() - 1
        };

        let new_pos = if delta < 0 {
            current_pos.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (current_pos + delta as usize).min(bookmark_indices.len() - 1)
        };

        let new_line_index = bookmark_indices[new_pos];
        data_state.selected_line_index = Some(new_line_index);
    }

    /// Jump to the first bookmark (Vim-style gg)
    pub fn jump_to_top_in_bookmarks(&mut self, data_state: &mut LogView) {
        if data_state.bookmarks.is_empty() {
            return;
        }

        let first_index = *data_state.bookmarks.keys().min().unwrap();
        data_state.selected_line_index = Some(first_index);
    }

    /// Jump to the last bookmark (Vim-style G)
    pub fn jump_to_bottom_in_bookmarks(&mut self, data_state: &mut LogView) {
        if data_state.bookmarks.is_empty() {
            return;
        }

        // TODO
        let last_index = data_state.bookmarks.keys().max().unwrap();
        data_state.selected_line_index = Some(*last_index);
    }

    /// Move selection up by one page in bookmarks view
    pub fn page_up_in_bookmarks(&mut self, data_state: &mut LogView) {
        const PAGE_SIZE: i32 = 25;
        self.move_selection_in_bookmarks(-PAGE_SIZE, data_state);
    }

    /// Move selection down by one page in bookmarks view
    pub fn page_down_in_bookmarks(&mut self, data_state: &mut LogView) {
        const PAGE_SIZE: i32 = 25;
        self.move_selection_in_bookmarks(PAGE_SIZE, data_state);
    }
}

impl LogCrabTab for BookmarksView {
    fn title(&mut self) -> egui::WidgetText {
        "Bookmarks".into()
    }

    fn render(
        &mut self,
        ui: &mut egui::Ui,
        data_state: &mut LogView,
        _global_config: &mut crate::config::GlobalConfig,
    ) {
        self.render_bookmarks(ui, data_state);
    }

    fn process_events(&mut self, actions: &[InputAction], data_state: &mut LogView) {
        for action in actions {
            match action {
                InputAction::MoveSelection(delta) => {
                    self.move_selection_in_bookmarks(*delta, data_state)
                }
                InputAction::ToggleBookmark => data_state.toggle_bookmark_for_selected(), // TODO: should be noop
                InputAction::JumpToTop => {
                    self.jump_to_top_in_bookmarks(data_state);
                }
                InputAction::JumpToBottom => {
                    self.jump_to_bottom_in_bookmarks(data_state);
                }
                InputAction::PageUp => {
                    self.page_up_in_bookmarks(data_state);
                }
                InputAction::PageDown => {
                    self.page_down_in_bookmarks(data_state);
                }
                InputAction::FocusSearch(_idx) => {}
                InputAction::NewFilterTab => {}
                InputAction::NewBookmarksTab => {}
                InputAction::ReverseCycleTab => {}
                InputAction::OpenFile => {}
                InputAction::NavigatePane(_direction) => {}
                InputAction::RenameFilter(_idx) => {}
                InputAction::CloseTab => todo!(),
                InputAction::CycleTab => {}
            }
        }
        todo!()
    }
}
