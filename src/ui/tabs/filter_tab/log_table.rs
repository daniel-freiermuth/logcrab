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

use crate::{
    core::LogStore,
    parser::line::LogLine,
    ui::{log_view::FilterHighlight, tabs::filter_tab::filter_state::FilterState},
};
use egui::{Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};

/// Events emitted by the log table
#[derive(Debug, Clone)]
pub enum LogTableEvent {
    LineClicked { line_index: usize },
    BookmarkToggled { line_index: usize },
}

/// Convert anomaly score to color with continuous gradient
/// In dark mode: light gray -> white -> yellow -> orange -> red
/// In light mode: dark gray -> darker variants of same progression
pub fn score_to_color(score: f64, dark_mode: bool) -> Color32 {
    // Normalize score to 0.0-1.0 range
    let normalized = (score / 100.0).clamp(0.0, 1.0);

    if dark_mode {
        // Dark mode: bright colors on dark background
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
    } else {
        // Light mode: darker colors on light background
        if normalized < 0.3 {
            // Low scores: medium gray to dark gray
            let t = normalized / 0.3;
            let gray = (140.0 - t * 40.0) as u8; // 140 -> 100
            Color32::from_rgb(gray, gray, gray)
        } else if normalized < 0.6 {
            // Medium-low scores: dark gray to dark yellow/olive
            let t = (normalized - 0.3) / 0.3;
            let r = (100.0 + t * 80.0) as u8; // 100 -> 180
            let g = (100.0 + t * 60.0) as u8; // 100 -> 160
            let b = (100.0 * (1.0 - t)) as u8; // 100 -> 0
            Color32::from_rgb(r, g, b)
        } else if normalized < 0.8 {
            // Medium-high scores: dark yellow to dark orange
            let t = (normalized - 0.6) / 0.2;
            let r = (180.0 + t * 20.0) as u8; // 180 -> 200
            let g = (160.0 - t * 60.0) as u8; // 160 -> 100
            let b = 0;
            Color32::from_rgb(r, g, b)
        } else {
            // High scores: dark orange to dark red
            let t = (normalized - 0.8) / 0.2;
            let r = (200.0 + t * 20.0) as u8; // 200 -> 220
            let g = (100.0 - t * 70.0) as u8; // 100 -> 30
            let b = (t * 30.0) as u8; // 0 -> 30
            Color32::from_rgb(r, g, b)
        }
    }
}

/// Get the background color for a selected row
pub fn selected_row_color(dark_mode: bool) -> Color32 {
    if dark_mode {
        Color32::from_rgb(60, 60, 80) // Dark blue-gray
    } else {
        Color32::from_rgb(200, 210, 230) // Light blue-gray
    }
}

/// Get the background color for a bookmarked row
pub fn bookmarked_row_color(dark_mode: bool) -> Color32 {
    if dark_mode {
        Color32::from_rgb(100, 80, 30) // Dark golden/brown
    } else {
        Color32::from_rgb(255, 240, 180) // Light golden/yellow
    }
}

/// Get the background color for a row that is both selected and bookmarked
pub fn selected_bookmarked_row_color(dark_mode: bool) -> Color32 {
    if dark_mode {
        Color32::from_rgb(140, 100, 60) // Brighter golden/orange
    } else {
        Color32::from_rgb(255, 220, 150) // Bright golden
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
        store: &LogStore,
        filter: &FilterState,
        ui_salt: usize,
        selected_line_index: usize,
        bookmarked_lines: &std::collections::HashMap<usize, String>,
        scroll_to_row: Option<usize>,
        all_filter_highlights: &[FilterHighlight],
    ) -> Vec<LogTableEvent> {
        let mut events = Vec::new();
        let dark_mode = ui.visuals().dark_mode;

        egui::ScrollArea::horizontal()
            .id_salt(format!("filtered_scroll_{ui_salt}"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                #[cfg(feature = "cpu-profiling")]
                puffin::profile_scope!("filtered_table");

                let table = Self::create_table(ui, scroll_to_row);

                Self::render_table_with_header(
                    table,
                    store,
                    filter,
                    ui_salt,
                    selected_line_index,
                    bookmarked_lines,
                    all_filter_highlights,
                    &mut events,
                    dark_mode,
                );
            });

        events
    }

    fn create_table<'a>(ui: &'a mut Ui, scroll_to_row: Option<usize>) -> TableBuilder<'a> {
        let available_height = ui.available_height();
        let header_height = ui.text_style_height(&egui::TextStyle::Heading);
        let body_height = available_height - header_height - 1.0;

        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(false)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .vscroll(true)
            .min_scrolled_height(body_height)
            .max_scroll_height(body_height)
            .column(Column::initial(60.0).resizable(true).clip(true))
            .column(Column::initial(110.0).resizable(true).clip(true))
            .column(Column::remainder().resizable(true).clip(true))
            .column(Column::initial(70.0).resizable(true).clip(true));

        if let Some(row_idx) = scroll_to_row {
            table = table.scroll_to_row(row_idx, Some(egui::Align::Center));
        }

        table
    }

    #[allow(clippy::too_many_arguments)]
    fn render_table_with_header(
        table: TableBuilder,
        store: &LogStore,
        filter: &FilterState,
        ui_salt: usize,
        selected_line_index: usize,
        bookmarked_lines: &std::collections::HashMap<usize, String>,
        all_filter_highlights: &[FilterHighlight],
        events: &mut Vec<LogTableEvent>,
        dark_mode: bool,
    ) {
        table
            .header(20.0, |mut header| {
                Self::render_header(&mut header);
            })
            .body(|body| {
                Self::render_table_body(
                    body,
                    store,
                    filter,
                    ui_salt,
                    selected_line_index,
                    bookmarked_lines,
                    all_filter_highlights,
                    events,
                    dark_mode,
                );
            });
    }

    fn render_header(header: &mut egui_extras::TableRow) {
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
    }

    #[allow(clippy::too_many_arguments)]
    fn render_table_body(
        body: egui_extras::TableBody,
        store: &LogStore,
        filter: &FilterState,
        ui_salt: usize,
        selected_line_index: usize,
        bookmarked_lines: &std::collections::HashMap<usize, String>,
        all_filter_highlights: &[FilterHighlight],
        events: &mut Vec<LogTableEvent>,
        dark_mode: bool,
    ) {
        let visible_lines = filter.filtered_indices.len();

        body.rows(18.0, visible_lines, |mut row| {
            let event = Self::render_table_row(
                &mut row,
                store,
                filter,
                ui_salt,
                selected_line_index,
                bookmarked_lines,
                all_filter_highlights,
                dark_mode,
            );

            if let Some(evt) = event {
                events.push(evt);
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn render_table_row(
        row: &mut egui_extras::TableRow,
        store: &LogStore,
        filter: &FilterState,
        ui_salt: usize,
        selected_line_index: usize,
        bookmarked_lines: &std::collections::HashMap<usize, String>,
        all_filter_highlights: &[FilterHighlight],
        dark_mode: bool,
    ) -> Option<LogTableEvent> {
        let row_index = row.index();
        let line_idx = filter.filtered_indices[row_index];
        let line = store.get_by_id(line_idx).unwrap();

        let is_selected = selected_line_index == line_idx;
        let is_bookmarked = bookmarked_lines.contains_key(&line_idx);
        let color = score_to_color(line.anomaly_score, dark_mode);

        let mut row_clicked = false;
        let mut row_right_clicked = false;

        Self::render_all_columns(
            row,
            &line,
            line_idx,
            ui_salt,
            is_selected,
            is_bookmarked,
            color,
            bookmarked_lines,
            all_filter_highlights,
            &mut row_clicked,
            &mut row_right_clicked,
            dark_mode,
        );

        if row_right_clicked {
            Some(LogTableEvent::BookmarkToggled {
                line_index: line_idx,
            })
        } else if row_clicked {
            Some(LogTableEvent::LineClicked {
                line_index: line_idx,
            })
        } else {
            None
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_all_columns(
        row: &mut egui_extras::TableRow,
        line: &LogLine,
        line_idx: usize,
        ui_salt: usize,
        is_selected: bool,
        is_bookmarked: bool,
        color: Color32,
        bookmarked_lines: &std::collections::HashMap<usize, String>,
        all_filter_highlights: &[FilterHighlight],
        row_clicked: &mut bool,
        row_right_clicked: &mut bool,
        dark_mode: bool,
    ) {
        Self::render_line_column(
            row,
            line,
            line_idx,
            ui_salt,
            is_selected,
            is_bookmarked,
            color,
            bookmarked_lines
                .get(&line_idx)
                .map(std::string::String::as_str),
            row_clicked,
            row_right_clicked,
            dark_mode,
        );

        Self::render_timestamp_column(
            row,
            line,
            line_idx,
            ui_salt,
            is_selected,
            is_bookmarked,
            color,
            all_filter_highlights,
            row_clicked,
            row_right_clicked,
            dark_mode,
        );

        Self::render_message_column(
            row,
            line,
            line_idx,
            ui_salt,
            is_selected,
            is_bookmarked,
            color,
            all_filter_highlights,
            row_clicked,
            row_right_clicked,
            dark_mode,
        );

        Self::render_score_column(
            row,
            line,
            line_idx,
            ui_salt,
            is_selected,
            is_bookmarked,
            color,
            row_clicked,
            row_right_clicked,
            dark_mode,
        );
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
        dark_mode: bool,
    ) {
        row.col(|ui| {
            if is_selected && is_bookmarked {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    selected_bookmarked_row_color(dark_mode),
                );
            } else if is_bookmarked {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    bookmarked_row_color(dark_mode),
                );
            } else if is_selected {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    selected_row_color(dark_mode),
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
                    label_response.on_hover_text(format!("📑 Bookmark: {name}"));
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
        is_selected: bool,
        is_bookmarked: bool,
        bg_color: Color32,
        all_filter_highlights: &[FilterHighlight],
        row_clicked: &mut bool,
        row_right_clicked: &mut bool,
        dark_mode: bool,
    ) {
        row.col(|ui| {
            if is_selected && is_bookmarked {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    selected_bookmarked_row_color(dark_mode),
                );
            } else if is_bookmarked {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    bookmarked_row_color(dark_mode),
                );
            } else if is_selected {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    selected_row_color(dark_mode),
                );
            }

            let timestamp_str = line.timestamp.format("%H:%M:%S%.3f").to_string();

            let job = FilterHighlight::highlight_text_with_filters(
                &timestamp_str,
                bg_color,
                all_filter_highlights,
                dark_mode,
            );
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
        is_selected: bool,
        is_bookmarked: bool,
        bg_color: Color32,
        all_filter_highlights: &[FilterHighlight],
        row_clicked: &mut bool,
        row_right_clicked: &mut bool,
        dark_mode: bool,
    ) {
        row.col(|ui| {
            if is_selected && is_bookmarked {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    selected_bookmarked_row_color(dark_mode),
                );
            } else if is_bookmarked {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    bookmarked_row_color(dark_mode),
                );
            } else if is_selected {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    selected_row_color(dark_mode),
                );
            }

            let job = FilterHighlight::highlight_text_with_filters(
                &line.message,
                bg_color,
                all_filter_highlights,
                dark_mode,
            );

            // Layout the text and check if it would be clipped
            let available_width = ui.available_width();
            let galley = ui.painter().layout_job(job);
            let text_width = galley.size().x;
            let is_clipped = text_width > available_width;

            ui.label(galley);

            let response = ui.interact(
                ui.max_rect(),
                ui.id().with(line_idx).with(ui_salt).with("msg"),
                egui::Sense::click(),
            );

            // Only show hover tooltip if text was clipped
            let response = if is_clipped {
                response.on_hover_text(&line.raw)
            } else {
                response
            };

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
        dark_mode: bool,
    ) {
        row.col(|ui| {
            if is_selected && is_bookmarked {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    selected_bookmarked_row_color(dark_mode),
                );
            } else if is_bookmarked {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    bookmarked_row_color(dark_mode),
                );
            } else if is_selected {
                ui.painter().rect_filled(
                    ui.available_rect_before_wrap(),
                    0.0,
                    selected_row_color(dark_mode),
                );
            }

            let anomaly_str = format!("{:.1}", line.anomaly_score);
            let text = RichText::new(anomaly_str).strong().color(color);
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
