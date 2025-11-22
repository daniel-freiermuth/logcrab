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

use egui::{Color32, RichText, Ui};
use egui_extras::{TableBuilder, Column};
use crate::parser::line::LogLine;
use chrono::DateTime;

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
    BookmarkClicked { 
        line_index: usize,
        timestamp: Option<DateTime<chrono::Local>>,
    },
    BookmarkDeleted { line_index: usize },
    BookmarkRenamed { line_index: usize, new_name: String },
    StartRenaming { line_index: usize },
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
        
        ui.heading("Bookmarks");
        ui.separator();
        
        if bookmarks.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(50.0);
                ui.label("No bookmarks yet");
                ui.label("Right-click on any line to bookmark it");
            });
            return events;
        }
        
        ui.label(format!("Total bookmarks: {}", bookmarks.len()));
        ui.separator();
        
        egui::ScrollArea::horizontal()
            .id_source("bookmarks_scroll")
            .show(ui, |ui| {
                let table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(false)
                    .sense(egui::Sense::click())
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .vscroll(true)
                    .max_scroll_height(f32::INFINITY)
                    .column(Column::initial(60.0).resizable(true).clip(true))
                    .column(Column::initial(110.0).resizable(true).clip(true))
                    .column(Column::initial(200.0).resizable(true).clip(true))
                    .column(Column::remainder().resizable(true).clip(true))
                    .column(Column::initial(80.0).resizable(true).clip(true));
                
                table.header(20.0, |mut header| {
                    header.col(|ui| { ui.strong("Line"); });
                    header.col(|ui| { ui.strong("Timestamp"); });
                    header.col(|ui| { ui.strong("Name"); });
                    header.col(|ui| { ui.strong("Message"); });
                    header.col(|ui| { ui.strong("Actions"); });
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
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            }
                            let text = if is_selected {
                                RichText::new(format!("★ ▶ {}", line_number)).color(color).strong()
                            } else {
                                RichText::new(format!("★ {}", line_number)).color(color)
                            };
                            ui.label(text);
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with("bm_line"), egui::Sense::click());
                            if response.clicked() { row_clicked = true; }
                        });
                        
                        // Timestamp
                        row.col(|ui| {
                            if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            }
                            ui.label(RichText::new(&timestamp_str).color(color));
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with("bm_ts"), egui::Sense::click());
                            if response.clicked() { row_clicked = true; }
                        });
                        
                        // Name
                        row.col(|ui| {
                            if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            }
                            
                            // Editable name field
                            if editing_bookmark == Some(line_idx) {
                                let response = ui.add(
                                    egui::TextEdit::singleline(bookmark_name_input)
                                        .desired_width(ui.available_width() - 50.0)
                                );
                                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    if !bookmark_name_input.is_empty() {
                                        events.push(BookmarkPanelEvent::BookmarkRenamed {
                                            line_index: line_idx,
                                            new_name: bookmark_name_input.clone(),
                                        });
                                    }
                                }
                                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    // Clear editing state - will be handled by parent
                                    bookmark_name_input.clear();
                                }
                            } else {
                                ui.label(RichText::new(&bookmark.name).color(color).strong());
                                let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with("bm_name"), egui::Sense::click());
                                if response.double_clicked() {
                                    events.push(BookmarkPanelEvent::StartRenaming { line_index: line_idx });
                                } else if response.clicked() {
                                    row_clicked = true;
                                }
                            }
                        });
                        
                        // Message
                        row.col(|ui| {
                            if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            }
                            ui.label(RichText::new(&message).color(color));
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with("bm_msg"), egui::Sense::click());
                            if response.clicked() { row_clicked = true; }
                        });
                        
                        // Actions
                        row.col(|ui| {
                            if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            }
                            ui.horizontal(|ui| {
                                if ui.small_button("✏").on_hover_text("Rename").clicked() {
                                    events.push(BookmarkPanelEvent::StartRenaming { line_index: line_idx });
                                }
                                if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                                    events.push(BookmarkPanelEvent::BookmarkDeleted { line_index: line_idx });
                                }
                            });
                        });
                        
                        if row_clicked {
                            events.push(BookmarkPanelEvent::BookmarkClicked {
                                line_index: line_idx,
                                timestamp: bookmark.timestamp,
                            });
                        }
                    });
                });
            });
        
        events
    }
}
