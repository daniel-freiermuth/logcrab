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
use chrono::DateTime;
use egui::{Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};

/// Bookmark data
#[derive(Debug, Clone)]
pub struct BookmarkData {
    pub line_index: usize,
    pub name: String,
    pub timestamp: Option<DateTime<chrono::Local>>,
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
        lines: &[LogLine],
        bookmarks: &[BookmarkData],
        selected_line_index: Option<usize>,
        editing_bookmark: Option<usize>,
        bookmark_name_input: &mut String,
    ) -> Vec<BookmarkPanelEvent> {
        let mut events = Vec::new();

        if bookmarks.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(50.0);
                ui.label("No bookmarks yet");
                ui.label("Right-click on any line to bookmark it");
            });
            return events;
        }

        egui::ScrollArea::horizontal()
            .id_salt("bookmarks_scroll")
            .auto_shrink([false, false]) // Don't shrink, take all available space
            .show(ui, |ui| {
                // Calculate available height to make table fill the pane
                let available_height = ui.available_height();
                let header_height = ui.text_style_height(&egui::TextStyle::Heading);
                let body_height = available_height - header_height - 1.0;

                let table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(false)
                    .sense(egui::Sense::click())
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .vscroll(true)
                    .min_scrolled_height(body_height) // Force table to fill available space minus header
                    .max_scroll_height(body_height) // Don't exceed available space
                    .column(Column::initial(60.0).resizable(true).clip(true))
                    .column(Column::initial(110.0).resizable(true).clip(true))
                    .column(Column::initial(200.0).resizable(true).clip(true))
                    .column(Column::remainder().resizable(true).clip(true))
                    .column(Column::initial(40.0).resizable(false).clip(true));

                table
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
                            let row_index = row.index();
                            let bookmark = &bookmarks[row_index];
                            let line_idx = bookmark.line_index;

                            let is_selected = selected_line_index == Some(line_idx);
                            let color = if line_idx < lines.len() {
                                score_to_color(lines[line_idx].anomaly_score)
                            } else {
                                Color32::WHITE
                            };

                            let line_number = if line_idx < lines.len() {
                                lines[line_idx].line_number
                            } else {
                                line_idx
                            };

                            let timestamp_str = if let Some(ts) = bookmark.timestamp {
                                ts.format("%H:%M:%S%.3f").to_string()
                            } else {
                                "-".to_string()
                            };

                            let message = if line_idx < lines.len() {
                                lines[line_idx].message.clone()
                            } else {
                                String::new()
                            };

                            let mut row_clicked = false;

                            // Line number
                            row.col(|ui| {
                                if is_selected {
                                    ui.painter().rect_filled(
                                        ui.available_rect_before_wrap(),
                                        0.0,
                                        Color32::from_rgb(100, 80, 30),
                                    );
                                }
                                let text = if is_selected {
                                    RichText::new(format!("★ ▶ {}", line_number))
                                        .color(color)
                                        .strong()
                                } else {
                                    RichText::new(format!("★ {}", line_number)).color(color)
                                };
                                ui.label(text);
                                let response = ui.interact(
                                    ui.max_rect(),
                                    ui.id().with(line_idx).with("bm_line"),
                                    egui::Sense::click(),
                                );
                                if response.clicked() {
                                    row_clicked = true;
                                }
                            });

                            // Timestamp
                            row.col(|ui| {
                                if is_selected {
                                    ui.painter().rect_filled(
                                        ui.available_rect_before_wrap(),
                                        0.0,
                                        Color32::from_rgb(100, 80, 30),
                                    );
                                }
                                ui.label(RichText::new(&timestamp_str).color(color));
                                let response = ui.interact(
                                    ui.max_rect(),
                                    ui.id().with(line_idx).with("bm_ts"),
                                    egui::Sense::click(),
                                );
                                if response.clicked() {
                                    row_clicked = true;
                                }
                            });

                            // Name
                            row.col(|ui| {
                                if is_selected {
                                    ui.painter().rect_filled(
                                        ui.available_rect_before_wrap(),
                                        0.0,
                                        Color32::from_rgb(100, 80, 30),
                                    );
                                }

                                // Editable name field
                                if editing_bookmark == Some(line_idx) {
                                    let text_edit = egui::TextEdit::singleline(bookmark_name_input)
                                        .desired_width(ui.available_width());

                                    let id = ui.id().with("bm_edit").with(line_idx);
                                    let response = ui.add(text_edit.id(id));

                                    // Request focus and select all on first frame
                                    let was_focused = ui.memory(|mem| mem.has_focus(id));
                                    if !was_focused {
                                        response.request_focus();
                                        // Select all text by setting cursor range
                                        if let Some(mut state) =
                                            egui::TextEdit::load_state(ui.ctx(), id)
                                        {
                                            let ccursor_start = egui::text::CCursor::new(0);
                                            let ccursor_end =
                                                egui::text::CCursor::new(bookmark_name_input.len());
                                            state.cursor.set_char_range(Some(
                                                egui::text::CCursorRange::two(
                                                    ccursor_start,
                                                    ccursor_end,
                                                ),
                                            ));
                                            state.store(ui.ctx(), id);
                                        }
                                    }

                                    // Save on Enter
                                    let enter_pressed =
                                        ui.input(|i| i.key_pressed(egui::Key::Enter));
                                    if enter_pressed && !bookmark_name_input.is_empty() {
                                        events.push(BookmarkPanelEvent::BookmarkRenamed {
                                            line_index: line_idx,
                                            new_name: bookmark_name_input.clone(),
                                        });
                                    }
                                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                        // Signal to cancel editing
                                        events.push(BookmarkPanelEvent::CancelRenaming);
                                    }
                                } else {
                                    ui.label(RichText::new(&bookmark.name).color(color).strong());
                                    let response = ui.interact(
                                        ui.max_rect(),
                                        ui.id().with(line_idx).with("bm_name"),
                                        egui::Sense::click(),
                                    );

                                    // Start editing on double-click or Enter key
                                    let enter_pressed =
                                        ui.input(|i| i.key_pressed(egui::Key::Enter));
                                    if response.double_clicked() || (is_selected && enter_pressed) {
                                        events.push(BookmarkPanelEvent::StartRenaming {
                                            line_index: line_idx,
                                        });
                                    } else if response.clicked() {
                                        row_clicked = true;
                                    }
                                }
                            });

                            // Message
                            row.col(|ui| {
                                if is_selected {
                                    ui.painter().rect_filled(
                                        ui.available_rect_before_wrap(),
                                        0.0,
                                        Color32::from_rgb(100, 80, 30),
                                    );
                                }
                                ui.label(RichText::new(&message).color(color));
                                let response = ui.interact(
                                    ui.max_rect(),
                                    ui.id().with(line_idx).with("bm_msg"),
                                    egui::Sense::click(),
                                );
                                if response.clicked() {
                                    row_clicked = true;
                                }
                            });

                            // Delete button
                            row.col(|ui| {
                                if is_selected {
                                    ui.painter().rect_filled(
                                        ui.available_rect_before_wrap(),
                                        0.0,
                                        Color32::from_rgb(100, 80, 30),
                                    );
                                }
                                if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                                    events.push(BookmarkPanelEvent::BookmarkDeleted {
                                        line_index: line_idx,
                                    });
                                }
                            });

                            if row_clicked {
                                events.push(BookmarkPanelEvent::BookmarkClicked {
                                    line_index: line_idx,
                                });
                            }
                        });
                    });
            });

        events
    }
}
