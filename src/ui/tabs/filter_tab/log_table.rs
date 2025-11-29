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

use crate::{parser::line::LogLine, ui::tabs::filter_tab::filter_state::FilterState};
use egui::{Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};

/// Events emitted by the log table
#[derive(Debug, Clone)]
pub enum LogTableEvent {
    LineClicked { line_index: usize },
    BookmarkToggled { line_index: usize },
}

/// Convert anomaly score to color with continuous gradient
pub fn score_to_color(score: f64) -> Color32 {
    // Normalize score to 0.0-1.0 range
    let normalized = (score / 100.0).clamp(0.0, 1.0);

    if normalized < 0.3 {
        // Low scores: light gray to white
        let t = normalized / 0.3;
        let gray = (150.0 + t * 105.0) as u8; // 150 -> 255
        Color32::from_rgb(gray, gray, gray)
    } else if normalized < 0.6 {
        // Medium-low scores: white to yellow
        let t = (normalized - 0.3) / 0.3;
        let r = 255;
        let g = 255;
        let b = (255.0 * (1.0 - t)) as u8; // 255 -> 0
        Color32::from_rgb(r, g, b)
    } else if normalized < 0.8 {
        // Medium-high scores: yellow to orange
        let t = (normalized - 0.6) / 0.2;
        let r = 255;
        let g = (255.0 * (1.0 - t * 0.4)) as u8; // 255 -> 153
        let b = 0;
        Color32::from_rgb(r, g, b)
    } else {
        // High scores: orange to red
        let t = (normalized - 0.8) / 0.2;
        let r = 255;
        let g = (153.0 * (1.0 - t)) as u8; // 153 -> 0
        let b = 0;
        Color32::from_rgb(r, g, b)
    }
}

/// Reusable log table component
pub struct LogTable;

impl LogTable {
    /// Render a table of log lines
    ///
    /// Returns events that occurred (line clicks, bookmark toggles)
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        ui: &mut Ui,
        lines: &[LogLine],
        filter: &FilterState,
        ui_salt: usize,
        selected_line_index: Option<usize>,
        bookmarked_lines: &std::collections::HashMap<usize, String>,
        scroll_to_row: Option<usize>,
        highlight_color: Color32,
    ) -> Vec<LogTableEvent> {
        let mut events = Vec::new();
        let visible_lines = filter.filtered_indices.len();

        egui::ScrollArea::horizontal()
            .id_salt(format!("filtered_scroll_{}", ui_salt))
            .auto_shrink([false, false]) // Don't shrink, take all available space
            .show(ui, |ui| {
                #[cfg(feature = "cpu-profiling")]
                puffin::profile_scope!("filtered_table");

                // Calculate available height to make table fill the pane
                let available_height = ui.available_height();
                let header_height = ui.text_style_height(&egui::TextStyle::Heading);
                let body_height = available_height - header_height - 1.0;

                let mut table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(false)
                    .sense(egui::Sense::click())
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .vscroll(true)
                    .min_scrolled_height(body_height) // Force table to fill available space minus header
                    .max_scroll_height(body_height) // Don't exceed available space
                    .column(Column::initial(60.0).resizable(true).clip(true))
                    .column(Column::initial(110.0).resizable(true).clip(true))
                    .column(Column::remainder().resizable(true).clip(true))
                    .column(Column::initial(70.0).resizable(true).clip(true));

                if let Some(row_idx) = scroll_to_row {
                    table = table.scroll_to_row(row_idx, Some(egui::Align::Center));
                }

                table
                    .header(header_height, |mut header| {
                        header.col(|ui| {
                            ui.strong("Line");
                        });
                        header.col(|ui| {
                            ui.strong("Timestamp");
                        });
                        header.col(|ui| {
                            ui.strong("Message");
                        });
                        header.col(|ui| {
                            ui.strong("Score");
                        });
                    })
                    .body(|body| {
                        body.rows(18.0, visible_lines, |mut row| {
                            let row_index = row.index();
                            let line_idx = filter.filtered_indices[row_index];
                            let line = &lines[line_idx];

                            let is_selected = selected_line_index == Some(line_idx);
                            let is_bookmarked = bookmarked_lines.contains_key(&line_idx);
                            let color = score_to_color(line.anomaly_score);

                            let mut row_clicked = false;
                            let mut row_right_clicked = false;

                            // Line number column
                            Self::render_line_column(
                                &mut row,
                                line,
                                line_idx,
                                ui_salt,
                                is_selected,
                                is_bookmarked,
                                color,
                                bookmarked_lines.get(&line_idx).map(|s| s.as_str()),
                                &mut row_clicked,
                                &mut row_right_clicked,
                            );

                            // Timestamp column
                            Self::render_timestamp_column(
                                &mut row,
                                line,
                                line_idx,
                                ui_salt,
                                filter,
                                is_selected,
                                is_bookmarked,
                                color,
                                highlight_color,
                                &mut row_clicked,
                                &mut row_right_clicked,
                            );

                            // Message column
                            Self::render_message_column(
                                &mut row,
                                line,
                                line_idx,
                                ui_salt,
                                filter,
                                is_selected,
                                is_bookmarked,
                                color,
                                highlight_color,
                                &mut row_clicked,
                                &mut row_right_clicked,
                            );

                            // Score column
                            Self::render_score_column(
                                &mut row,
                                line,
                                line_idx,
                                ui_salt,
                                is_selected,
                                is_bookmarked,
                                color,
                                &mut row_clicked,
                                &mut row_right_clicked,
                            );

                            // Handle interaction
                            if row_right_clicked {
                                events.push(LogTableEvent::BookmarkToggled {
                                    line_index: line_idx,
                                });
                            } else if row_clicked {
                                events.push(LogTableEvent::LineClicked {
                                    line_index: line_idx,
                                });
                            }
                        });
                    });
            });

        events
    }

    #[allow(clippy::too_many_arguments)]
    fn render_line_column(
        row: &mut egui_extras::TableRow,
        line: &LogLine,
        line_idx: usize,
        ui_salt: usize,
        is_selected: bool,
        is_bookmarked: bool,
        color: Color32,
        bookmark_name: Option<&str>,
        row_clicked: &mut bool,
        row_right_clicked: &mut bool,
    ) {
        row.col(|ui| {
            if is_selected && is_bookmarked {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    Color32::from_rgb(140, 100, 60),
                );
            } else if is_bookmarked {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    Color32::from_rgb(100, 80, 30),
                );
            } else if is_selected {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    Color32::from_rgb(60, 60, 80),
                );
            }

            let bookmark_icon = if is_bookmarked { "★ " } else { "" };
            let line_text = if is_selected {
                format!("▶ {}{}", bookmark_icon, line.line_number)
            } else {
                format!("{}{}", bookmark_icon, line.line_number)
            };

            let text = if is_selected {
                RichText::new(line_text).color(color).strong()
            } else {
                RichText::new(line_text).color(color)
            };
            let label_response = ui.label(text);

            // Show tooltip with bookmark name if bookmarked
            if is_bookmarked {
                if let Some(name) = bookmark_name {
                    label_response.on_hover_text(format!("📑 Bookmark: {}", name));
                }
            }

            let response = ui.interact(
                ui.max_rect(),
                ui.id().with(line_idx).with(ui_salt).with("line"),
                egui::Sense::click(),
            );

            if response.clicked() {
                *row_clicked = true;
            }
            if response.secondary_clicked() {
                *row_right_clicked = true;
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn render_timestamp_column(
        row: &mut egui_extras::TableRow,
        line: &LogLine,
        line_idx: usize,
        ui_salt: usize,
        filter: &FilterState,
        is_selected: bool,
        is_bookmarked: bool,
        bg_color: Color32,
        highlight_color: Color32,
        row_clicked: &mut bool,
        row_right_clicked: &mut bool,
    ) {
        row.col(|ui| {
            if is_selected && is_bookmarked {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    Color32::from_rgb(140, 100, 60),
                );
            } else if is_bookmarked {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    Color32::from_rgb(100, 80, 30),
                );
            } else if is_selected {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    Color32::from_rgb(60, 60, 80),
                );
            }

            let timestamp_str = if let Some(ts) = line.timestamp {
                ts.format("%H:%M:%S%.3f").to_string()
            } else {
                "-".to_string()
            };

            let job = filter.highlight_matches(&timestamp_str, bg_color, highlight_color);
            ui.label(job);

            let response = ui.interact(
                ui.max_rect(),
                ui.id().with(line_idx).with(ui_salt).with("ts"),
                egui::Sense::click(),
            );
            if response.clicked() {
                *row_clicked = true;
            }
            if response.secondary_clicked() {
                *row_right_clicked = true;
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn render_message_column(
        row: &mut egui_extras::TableRow,
        line: &LogLine,
        line_idx: usize,
        ui_salt: usize,
        filter: &FilterState,
        is_selected: bool,
        is_bookmarked: bool,
        bg_color: Color32,
        highlight_color: Color32,
        row_clicked: &mut bool,
        row_right_clicked: &mut bool,
    ) {
        row.col(|ui| {
            if is_selected && is_bookmarked {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    Color32::from_rgb(140, 100, 60),
                );
            } else if is_bookmarked {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    Color32::from_rgb(100, 80, 30),
                );
            } else if is_selected {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    Color32::from_rgb(60, 60, 80),
                );
            }

            let job = filter.highlight_matches(&line.message, bg_color, highlight_color);
            ui.label(job);

            let response = ui.interact(
                ui.max_rect(),
                ui.id().with(line_idx).with(ui_salt).with("msg"),
                egui::Sense::click(),
            );
            let response = response.on_hover_text(&line.raw);
            if response.clicked() {
                *row_clicked = true;
            }
            if response.secondary_clicked() {
                *row_right_clicked = true;
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn render_score_column(
        row: &mut egui_extras::TableRow,
        line: &LogLine,
        line_idx: usize,
        ui_salt: usize,
        is_selected: bool,
        is_bookmarked: bool,
        color: Color32,
        row_clicked: &mut bool,
        row_right_clicked: &mut bool,
    ) {
        row.col(|ui| {
            if is_selected && is_bookmarked {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    Color32::from_rgb(140, 100, 60),
                );
            } else if is_bookmarked {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    Color32::from_rgb(100, 80, 30),
                );
            } else if is_selected {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    Color32::from_rgb(60, 60, 80),
                );
            }

            let text = RichText::new(format!("{:.1}", line.anomaly_score))
                .strong()
                .color(color);
            ui.label(text);

            let response = ui.interact(
                ui.max_rect(),
                ui.id().with(line_idx).with(ui_salt).with("score"),
                egui::Sense::click(),
            );
            if response.clicked() {
                *row_clicked = true;
            }
            if response.secondary_clicked() {
                *row_right_clicked = true;
            }
        });
    }
}
