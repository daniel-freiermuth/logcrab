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

use crate::{parser::line::LogLine, ui::LogView};
use chrono::DateTime;
use egui::Ui;

/// Events that can be emitted by the bookmarks view
#[derive(Debug, Clone)]
pub enum BookmarksViewEvent {
    BookmarkClicked {
        line_index: usize,
        timestamp: Option<DateTime<chrono::Local>>,
    },
    BookmarkDeleted {
        line_index: usize,
    },
    BookmarkRenamed {
        line_index: usize,
        new_name: String,
    },
    StartRenaming {
        line_index: usize,
    },
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
                BookmarkPanelEvent::BookmarkClicked {
                    line_index,
                    timestamp,
                } => BookmarksViewEvent::BookmarkClicked {
                    line_index,
                    timestamp,
                },
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
                BookmarksViewEvent::BookmarkClicked {
                    line_index,
                    timestamp,
                } => {
                    data_state.selected_line_index = Some(line_index);
                    data_state.selected_timestamp = timestamp;
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
}
