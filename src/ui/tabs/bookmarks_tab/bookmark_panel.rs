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

use crate::parser::line::LogLine;
use crate::ui::log_view::{FilterHighlight, LogViewState};
use chrono::DateTime;
use egui::{Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};

/// Bookmark data
#[derive(Debug, Clone)]
pub struct BookmarkData {
    pub line_index: usize,
    pub name: String,
    pub timestamp: DateTime<chrono::Local>,
}

/// Events emitted by the bookmark panel
#[derive(Debug, Clone)]
pub enum BookmarkPanelEvent {
    BookmarkClicked { line_index: usize },
    BookmarkDeleted { line_index: usize },
    BookmarkRenamed { line_index: usize, new_name: String },
    StartRenaming { line_index: usize },
    CancelRenaming,
}

/// Convert anomaly score to color
fn score_to_color(score: f64) -> Color32 {
    if score >= 80.0 {
        Color32::from_rgb(255, 100, 100)
    } else if score >= 60.0 {
        Color32::from_rgb(255, 180, 100)
    } else if score >= 30.0 {
        Color32::from_rgb(255, 200, 200)
    } else {
        Color32::LIGHT_GRAY
    }
}

/// Reusable bookmark panel component
pub struct BookmarkPanel;

impl BookmarkPanel {
    /// Render the bookmarks view
    ///
    /// Returns events that occurred (clicks, deletes, renames)
    pub fn render(
        ui: &mut Ui,
        log_view_state: &LogViewState,
        bookmarks: &[BookmarkData],
        editing_bookmark: Option<usize>,
        bookmark_name_input: &mut String,
        all_filter_highlights: &[FilterHighlight],
    ) -> Vec<BookmarkPanelEvent> {
        let mut events = Vec::new();

        if bookmarks.is_empty() {
            Self::render_empty_state(ui);
            return events;
        }

        egui::ScrollArea::horizontal()
            .id_salt("bookmarks_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                Self::render_bookmark_table(
                    ui,
                    log_view_state,
                    bookmarks,
                    editing_bookmark,
                    bookmark_name_input,
                    all_filter_highlights,
                    &mut events,
                );
            });

        events
    }

    fn render_empty_state(ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(50.0);
            ui.label("No bookmarks yet");
            ui.label("Right-click on any line to bookmark it");
        });
    }

    fn render_bookmark_table(
        ui: &mut Ui,
        log_view_state: &LogViewState,
        bookmarks: &[BookmarkData],
        editing_bookmark: Option<usize>,
        bookmark_name_input: &mut String,
        all_filter_highlights: &[FilterHighlight],
        events: &mut Vec<BookmarkPanelEvent>,
    ) {
        let available_height = ui.available_height();
        let header_height = ui.text_style_height(&egui::TextStyle::Heading);
        let body_height = available_height - header_height - 1.0;

        TableBuilder::new(ui)
            .striped(true)
            .resizable(false)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .vscroll(true)
            .min_scrolled_height(body_height)
            .max_scroll_height(body_height)
            .column(Column::initial(60.0).resizable(true).clip(true))
            .column(Column::initial(110.0).resizable(true).clip(true))
            .column(Column::initial(200.0).resizable(true).clip(true))
            .column(Column::remainder().resizable(true).clip(true))
            .column(Column::initial(40.0).resizable(false).clip(true))
            .header(header_height, |mut header| {
                header.col(|ui| {
                    ui.strong("Line");
                });
                header.col(|ui| {
                    ui.strong("Timestamp");
                });
                header.col(|ui| {
                    ui.strong("Name");
                });
                header.col(|ui| {
                    ui.strong("Message");
                });
                header.col(|ui| {
                    ui.strong("");
                });
            })
            .body(|body| {
                body.rows(18.0, bookmarks.len(), |mut row| {
                    Self::render_bookmark_row(
                        &mut row,
                        log_view_state,
                        bookmarks,
                        editing_bookmark,
                        bookmark_name_input,
                        all_filter_highlights,
                        events,
                    );
                });
            });
    }

    fn render_bookmark_row(
        row: &mut egui_extras::TableRow<'_, '_>,
        log_view_state: &LogViewState,
        bookmarks: &[BookmarkData],
        editing_bookmark: Option<usize>,
        bookmark_name_input: &mut String,
        all_filter_highlights: &[FilterHighlight],
        events: &mut Vec<BookmarkPanelEvent>,
    ) {
        let row_index = row.index();
        let bookmark = &bookmarks[row_index];
        let line_idx = bookmark.line_index;
        let lines = &log_view_state.lines;

        let is_selected = log_view_state.selected_line_index == line_idx;
        let color = if let Some(score) = &log_view_state.scores {
            score_to_color(score[line_idx])
        } else {
            Color32::WHITE
        };

        let mut row_clicked = false;

        // Line number column
        Self::render_line_column(row, line_idx, is_selected, color, lines, &mut row_clicked);

        // Timestamp column
        Self::render_timestamp_column(
            row,
            line_idx,
            is_selected,
            color,
            bookmark,
            &mut row_clicked,
        );

        // Name column
        Self::render_name_column(
            row,
            line_idx,
            is_selected,
            color,
            bookmark,
            editing_bookmark,
            bookmark_name_input,
            events,
            &mut row_clicked,
        );

        // Message column
        Self::render_message_column(
            row,
            line_idx,
            is_selected,
            color,
            lines,
            all_filter_highlights,
            &mut row_clicked,
        );

        // Delete button column
        Self::render_delete_column(row, line_idx, is_selected, events);

        if row_clicked {
            events.push(BookmarkPanelEvent::BookmarkClicked {
                line_index: line_idx,
            });
        }
    }

    fn render_line_column(
        row: &mut egui_extras::TableRow<'_, '_>,
        line_idx: usize,
        is_selected: bool,
        color: Color32,
        lines: &[LogLine],
        row_clicked: &mut bool,
    ) {
        row.col(|ui| {
            Self::paint_selection_background(ui, is_selected);

            let line_number = lines[line_idx].line_number;
            let text = if is_selected {
                RichText::new(format!("★ ▶ {line_number}"))
                    .color(color)
                    .strong()
            } else {
                RichText::new(format!("★ {line_number}")).color(color)
            };
            ui.label(text);

            let response = ui.interact(
                ui.max_rect(),
                ui.id().with(line_idx).with("bm_line"),
                egui::Sense::click(),
            );
            if response.clicked() {
                *row_clicked = true;
            }
        });
    }

    fn render_timestamp_column(
        row: &mut egui_extras::TableRow<'_, '_>,
        line_idx: usize,
        is_selected: bool,
        color: Color32,
        bookmark: &BookmarkData,
        row_clicked: &mut bool,
    ) {
        row.col(|ui| {
            Self::paint_selection_background(ui, is_selected);

            let timestamp_str = bookmark.timestamp.format("%H:%M:%S%.3f").to_string();
            ui.label(RichText::new(&timestamp_str).color(color));

            let response = ui.interact(
                ui.max_rect(),
                ui.id().with(line_idx).with("bm_ts"),
                egui::Sense::click(),
            );
            if response.clicked() {
                *row_clicked = true;
            }
        });
    }

    fn render_name_column(
        row: &mut egui_extras::TableRow<'_, '_>,
        line_idx: usize,
        is_selected: bool,
        color: Color32,
        bookmark: &BookmarkData,
        editing_bookmark: Option<usize>,
        bookmark_name_input: &mut String,
        events: &mut Vec<BookmarkPanelEvent>,
        row_clicked: &mut bool,
    ) {
        row.col(|ui| {
            Self::paint_selection_background(ui, is_selected);

            if editing_bookmark == Some(line_idx) {
                Self::render_name_editor(ui, line_idx, bookmark_name_input, events);
            } else {
                ui.label(RichText::new(&bookmark.name).color(color).strong());
                let response = ui.interact(
                    ui.max_rect(),
                    ui.id().with(line_idx).with("bm_name"),
                    egui::Sense::click(),
                );

                if response.double_clicked() {
                    events.push(BookmarkPanelEvent::StartRenaming {
                        line_index: line_idx,
                    });
                } else if response.clicked() {
                    *row_clicked = true;
                }
            }
        });
    }

    fn render_name_editor(
        ui: &mut Ui,
        line_idx: usize,
        bookmark_name_input: &mut String,
        events: &mut Vec<BookmarkPanelEvent>,
    ) {
        let text_edit =
            egui::TextEdit::singleline(bookmark_name_input).desired_width(ui.available_width());

        let id = ui.id().with("bm_edit").with(line_idx);
        let response = ui.add(text_edit.id(id));

        // Request focus and select all on first frame
        let was_focused = ui.memory(|mem| mem.has_focus(id));
        if !was_focused {
            response.request_focus();
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                let ccursor_start = egui::text::CCursor::new(0);
                let ccursor_end = egui::text::CCursor::new(bookmark_name_input.len());
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        ccursor_start,
                        ccursor_end,
                    )));
                state.store(ui.ctx(), id);
            }
        }

        // Save on Enter
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !bookmark_name_input.is_empty() {
            events.push(BookmarkPanelEvent::BookmarkRenamed {
                line_index: line_idx,
                new_name: bookmark_name_input.clone(),
            });
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            events.push(BookmarkPanelEvent::CancelRenaming);
        }
    }

    fn render_message_column(
        row: &mut egui_extras::TableRow<'_, '_>,
        line_idx: usize,
        is_selected: bool,
        color: Color32,
        lines: &[LogLine],
        all_filter_highlights: &[FilterHighlight],
        row_clicked: &mut bool,
    ) {
        row.col(|ui| {
            Self::paint_selection_background(ui, is_selected);

            let message = &lines[line_idx].message;
            let job =
                FilterHighlight::highlight_text_with_filters(message, color, all_filter_highlights);
            ui.label(job);

            let response = ui.interact(
                ui.max_rect(),
                ui.id().with(line_idx).with("bm_msg"),
                egui::Sense::click(),
            );
            if response.clicked() {
                *row_clicked = true;
            }
        });
    }

    fn render_delete_column(
        row: &mut egui_extras::TableRow<'_, '_>,
        line_idx: usize,
        is_selected: bool,
        events: &mut Vec<BookmarkPanelEvent>,
    ) {
        row.col(|ui| {
            Self::paint_selection_background(ui, is_selected);

            if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                events.push(BookmarkPanelEvent::BookmarkDeleted {
                    line_index: line_idx,
                });
            }
        });
    }

    fn paint_selection_background(ui: &mut Ui, is_selected: bool) {
        if is_selected {
            ui.painter().rect_filled(
                ui.available_rect_before_wrap(),
                0.0,
                Color32::from_rgb(100, 80, 30),
            );
        }
    }
}
