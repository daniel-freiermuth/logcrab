// LogCrab - GPL-3.0-or-later
// This file is part of LogCrab.
//
// Copyright (C) 2026 Daniel Freiermuth
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

use std::sync::Arc;

use crate::{
    core::{log_store::StoreID, LogStore},
    parser::line::{LogLine, LogLineCore},
    ui::{filter_highlight::FilterHighlight, tabs::filter_tab::filter_state::FilterState},
};
use chrono::{DateTime, Local};
use egui::{Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};

/// Events emitted by the log table
#[derive(Clone)]
pub enum LogTableEvent {
    LineClicked {
        line_index: StoreID,
    },
    BookmarkToggled {
        line_index: StoreID,
    },
    SyncDltTime {
        line_index: StoreID,
        storage_time: DateTime<Local>,
        ecu_id: Option<String>,
        app_id: Option<String>,
    },
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
            let gray = t.mul_add(105.0, 150.0) as u8; // 150 -> 255
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
            let g = (255.0 * t.mul_add(-0.4, 1.0)) as u8; // 255 -> 153
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
            let gray = t.mul_add(-40.0, 140.0) as u8; // 140 -> 100
            Color32::from_rgb(gray, gray, gray)
        } else if normalized < 0.6 {
            // Medium-low scores: dark gray to dark yellow/olive
            let t = (normalized - 0.3) / 0.3;
            let r = t.mul_add(80.0, 100.0) as u8; // 100 -> 180
            let g = t.mul_add(60.0, 100.0) as u8; // 100 -> 160
            let b = (100.0 * (1.0 - t)) as u8; // 100 -> 0
            Color32::from_rgb(r, g, b)
        } else if normalized < 0.8 {
            // Medium-high scores: dark yellow to dark orange
            let t = (normalized - 0.6) / 0.2;
            let r = t.mul_add(20.0, 180.0) as u8; // 180 -> 200
            let g = t.mul_add(-60.0, 160.0) as u8; // 160 -> 100
            let b = 0;
            Color32::from_rgb(r, g, b)
        } else {
            // High scores: dark orange to dark red
            let t = (normalized - 0.8) / 0.2;
            let r = t.mul_add(20.0, 200.0) as u8; // 200 -> 220
            let g = t.mul_add(-70.0, 100.0) as u8; // 100 -> 30
            let b = (t * 30.0) as u8; // 0 -> 30
            Color32::from_rgb(r, g, b)
        }
    }
}

/// Get the background color for a selected row
pub const fn selected_row_color(dark_mode: bool) -> Color32 {
    if dark_mode {
        Color32::from_rgb(60, 60, 80) // Dark blue-gray
    } else {
        Color32::from_rgb(200, 210, 230) // Light blue-gray
    }
}

/// Get the background color for a bookmarked row
pub const fn bookmarked_row_color(dark_mode: bool) -> Color32 {
    if dark_mode {
        Color32::from_rgb(100, 80, 30) // Dark golden/brown
    } else {
        Color32::from_rgb(255, 240, 180) // Light golden/yellow
    }
}

/// Get the background color for a row that is both selected and bookmarked
pub const fn selected_bookmarked_row_color(dark_mode: bool) -> Color32 {
    if dark_mode {
        Color32::from_rgb(140, 100, 60) // Brighter golden/orange
    } else {
        Color32::from_rgb(255, 220, 150) // Bright golden
    }
}

/// Reusable log table component
pub struct LogTable;

impl LogTable {
    /// Show context menu for a log line
    fn show_line_context_menu(
        response: &egui::Response,
        store: &LogStore,
        line_idx: StoreID,
        events: &mut Vec<LogTableEvent>,
    ) {
        use crate::parser::line::{LogLineCore, LogLineVariant};
        response.context_menu(|ui| {
            if ui.button("📑 Toggle Bookmark").clicked() {
                events.push(LogTableEvent::BookmarkToggled {
                    line_index: line_idx,
                });
                ui.close();
            }

            if ui.button("🎯 Jump to Line").clicked() {
                events.push(LogTableEvent::LineClicked {
                    line_index: line_idx,
                });
                ui.close();
            }

            // DLT-specific: Sync time option
            if let Some(LogLineVariant::Dlt(dlt_line)) = store.get_by_id(&line_idx) {
                if ui.button("⏱ Calibrate Time Here").clicked() {
                    // Extract storage timestamp from the DLT message
                    if let Some(ref storage_header) = dlt_line.dlt_message.storage_header {
                        if let Some(storage_time) =
                            crate::parser::dlt::storage_time_to_datetime(&storage_header.timestamp)
                        {
                            // Extract ECU ID and App ID
                            let ecu_id = dlt_line
                                .dlt_message
                                .header
                                .ecu_id
                                .as_ref()
                                .map(std::string::ToString::to_string);
                            let app_id = dlt_line
                                .dlt_message
                                .extended_header
                                .as_ref()
                                .map(|ext| ext.application_id.clone());

                            events.push(LogTableEvent::SyncDltTime {
                                line_index: line_idx,
                                storage_time,
                                ecu_id,
                                app_id,
                            });
                            ui.close();
                        }
                    }
                }
            }

            ui.separator();

            if let Some(line) = store.get_by_id(&line_idx) {
                if ui.button("📋 Copy Message").clicked() {
                    ui.ctx().copy_text(line.message());
                    ui.close();
                }

                if ui.button("📋 Copy Full Line").clicked() {
                    ui.ctx().copy_text(line.raw());
                    ui.close();
                }
            }
        });
    }
}

/// Stored column widths for the log table
#[derive(Clone, Debug)]
pub struct ColumnWidths {
    pub source: f32,
    pub line: f32,
    pub timestamp: f32,
    pub message: f32,
    pub score: f32,
}

impl Default for ColumnWidths {
    fn default() -> Self {
        Self {
            source: 120.0,
            line: 60.0,
            timestamp: 175.0,
            message: 0.0, // Will be calculated
            score: 70.0,
        }
    }
}

impl LogTable {
    /// Render a table of log lines
    ///
    /// Returns events that occurred (line clicks, bookmark toggles)
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        ui: &mut Ui,
        store: &Arc<LogStore>,
        filter: &mut FilterState,
        selected_line_index: Option<StoreID>,
        bookmarked_lines: &std::collections::HashMap<StoreID, String>,
        scroll_to_row: Option<usize>,
        all_filter_highlights: &[FilterHighlight],
    ) -> Vec<LogTableEvent> {
        profiling::scope!("LogTable::render");

        let mut events = Vec::new();
        let dark_mode = ui.visuals().dark_mode;

        // Get filtered indices first to avoid borrow conflicts
        let filtered_indices = filter.search.get_filtered_indices_cached().clone();
        let filter_id = filter.get_id();

        let available_width = ui.available_width();
        egui::ScrollArea::horizontal()
            .id_salt(format!("filtered_scroll_{filter_id}"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                profiling::scope!("filtered_table");
                ui.set_min_width(available_width);

                let table = Self::create_table(ui, scroll_to_row, &filter.column_widths);

                Self::render_table_with_header(
                    table,
                    store,
                    &filtered_indices,
                    selected_line_index,
                    bookmarked_lines,
                    all_filter_highlights,
                    &mut events,
                    dark_mode,
                    &mut filter.column_widths,
                );
            });

        events
    }

    const MIN_MESSAGE_WIDTH: f32 = 100.0;

    fn create_table<'a>(
        ui: &'a mut Ui,
        scroll_to_row: Option<usize>,
        column_widths: &ColumnWidths,
    ) -> TableBuilder<'a> {
        let available_height = ui.available_height();
        let available_width = ui.available_width();
        let header_height = ui.text_style_height(&egui::TextStyle::Heading);
        let body_height = available_height - header_height - 1.0;

        // Calculate minimum message column width to fill remaining space
        let other_cols_width = column_widths.source + column_widths.line + column_widths.timestamp;
        let remainder = (available_width - other_cols_width).max(Self::MIN_MESSAGE_WIDTH);

        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(false)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .vscroll(true)
            .drag_to_scroll(false)
            .min_scrolled_height(body_height)
            .max_scroll_height(body_height)
            .column(Column::initial(120.0).resizable(true).clip(true)) // Source
            .column(Column::initial(60.0).resizable(true).clip(true)) // Line
            .column(Column::initial(175.0).resizable(true).clip(true)) // Timestamp
            .column(
                Column::initial(remainder)
                    .at_least(remainder)
                    .resizable(true)
                    .clip(true),
            ) // Message
            .column(Column::auto().clip(true)); // Score

        if let Some(row_idx) = scroll_to_row {
            table = table.scroll_to_row(row_idx, Some(egui::Align::Center));
        }

        table
    }

    #[allow(clippy::too_many_arguments)]
    fn render_table_with_header(
        table: TableBuilder,
        store: &LogStore,
        filtered_indices: &[StoreID],
        selected_line_index: Option<StoreID>,
        bookmarked_lines: &std::collections::HashMap<StoreID, String>,
        all_filter_highlights: &[FilterHighlight],
        events: &mut Vec<LogTableEvent>,
        dark_mode: bool,
        column_widths: &mut ColumnWidths,
    ) {
        table
            .header(20.0, |mut header| {
                Self::render_header(&mut header, column_widths);
            })
            .body(|body| {
                profiling::scope!("LogTable::body");
                Self::render_table_body(
                    body,
                    store,
                    filtered_indices,
                    selected_line_index,
                    bookmarked_lines,
                    all_filter_highlights,
                    events,
                    dark_mode,
                );
            });
    }

    fn render_header(header: &mut egui_extras::TableRow, column_widths: &mut ColumnWidths) {
        header.col(|ui| {
            column_widths.source = ui.available_width();
            ui.strong("Source");
        });
        header.col(|ui| {
            column_widths.line = ui.available_width();
            ui.strong("Line");
        });
        header.col(|ui| {
            column_widths.timestamp = ui.available_width();
            let now = Local::now();
            let offset = now.offset();
            ui.strong(format!("Timestamp (UTC{offset})"));
        });
        header.col(|ui| {
            column_widths.message = ui.available_width();
            ui.strong("Message");
        });
        header.col(|ui| {
            column_widths.score = ui.available_width();
            ui.strong("Score");
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn render_table_body(
        body: egui_extras::TableBody,
        store: &LogStore,
        filtered_indices: &[StoreID],
        selected_line_index: Option<StoreID>,
        bookmarked_lines: &std::collections::HashMap<StoreID, String>,
        all_filter_highlights: &[FilterHighlight],
        events: &mut Vec<LogTableEvent>,
        dark_mode: bool,
    ) {
        let visible_lines = filtered_indices.len();

        body.rows(18.0, visible_lines, |mut row| {
            let event = Self::render_table_row(
                &mut row,
                store,
                filtered_indices,
                selected_line_index,
                bookmarked_lines,
                all_filter_highlights,
                events,
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
        filtered_indices: &[StoreID],
        selected_line_index: Option<StoreID>,
        bookmarked_lines: &std::collections::HashMap<StoreID, String>,
        all_filter_highlights: &[FilterHighlight],
        events: &mut Vec<LogTableEvent>,
        dark_mode: bool,
    ) -> Option<LogTableEvent> {
        let row_index = row.index();
        let line_idx = filtered_indices[row_index];
        let line = store
            .get_by_id(&line_idx)
            .expect("filtered index must exist in store");

        let is_selected = selected_line_index.as_ref() == Some(&line_idx);
        let is_bookmarked = bookmarked_lines.contains_key(&line_idx);
        let color = score_to_color(line.anomaly_score(), dark_mode);
        let source_name = store.get_source_name(&line_idx);

        let mut row_clicked = false;
        let mut row_middle_clicked = false;

        Self::render_all_columns(
            row,
            store,
            &line,
            line_idx,
            is_selected,
            is_bookmarked,
            color,
            source_name.as_deref(),
            bookmarked_lines,
            all_filter_highlights,
            &mut row_clicked,
            &mut row_middle_clicked,
            events,
            dark_mode,
        );

        if row_middle_clicked {
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
        store: &LogStore,
        line: &LogLine,
        line_idx: StoreID,
        is_selected: bool,
        is_bookmarked: bool,
        color: Color32,
        source_name: Option<&str>,
        bookmarked_lines: &std::collections::HashMap<StoreID, String>,
        all_filter_highlights: &[FilterHighlight],
        row_clicked: &mut bool,
        row_middle_clicked: &mut bool,
        events: &mut Vec<LogTableEvent>,
        dark_mode: bool,
    ) {
        Self::render_source_column(
            row,
            store,
            line_idx,
            is_selected,
            is_bookmarked,
            color,
            source_name,
            row_clicked,
            row_middle_clicked,
            events,
            dark_mode,
        );

        Self::render_line_column(
            row,
            store,
            line,
            line_idx,
            is_selected,
            is_bookmarked,
            color,
            bookmarked_lines
                .get(&line_idx)
                .map(std::string::String::as_str),
            row_clicked,
            row_middle_clicked,
            events,
            dark_mode,
        );

        Self::render_timestamp_column(
            row,
            store,
            line,
            line_idx,
            is_selected,
            is_bookmarked,
            color,
            row_clicked,
            row_middle_clicked,
            events,
            dark_mode,
        );

        Self::render_message_column(
            row,
            store,
            line,
            line_idx,
            is_selected,
            is_bookmarked,
            color,
            all_filter_highlights,
            row_clicked,
            row_middle_clicked,
            events,
            dark_mode,
        );

        Self::render_score_column(
            row,
            store,
            line,
            line_idx,
            is_selected,
            is_bookmarked,
            color,
            row_clicked,
            row_middle_clicked,
            events,
            dark_mode,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn render_source_column(
        row: &mut egui_extras::TableRow,
        store: &LogStore,
        line_idx: StoreID,
        is_selected: bool,
        is_bookmarked: bool,
        color: Color32,
        source_name: Option<&str>,
        row_clicked: &mut bool,
        row_middle_clicked: &mut bool,
        events: &mut Vec<LogTableEvent>,
        dark_mode: bool,
    ) {
        row.col(|ui| {
            // Background highlight for selected/bookmarked rows
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

            // Display source name (truncated if needed)
            let display_name = source_name.unwrap_or("stdin");
            let text = RichText::new(display_name).color(color);
            let label_response = ui.add(egui::Label::new(text).truncate());

            // Tooltip with full source name
            if let Some(name) = source_name {
                label_response.on_hover_text(name);
            }

            let response = ui.interact(
                ui.max_rect(),
                ui.id().with(line_idx).with("source"),
                egui::Sense::click(),
            );

            if response.clicked() {
                *row_clicked = true;
            }
            if response.middle_clicked() {
                *row_middle_clicked = true;
            }

            // Show context menu on right-click
            Self::show_line_context_menu(&response, store, line_idx, events);
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn render_line_column(
        row: &mut egui_extras::TableRow,
        store: &LogStore,
        line: &LogLine,
        line_idx: StoreID,
        is_selected: bool,
        is_bookmarked: bool,
        color: Color32,
        bookmark_name: Option<&str>,
        row_clicked: &mut bool,
        row_middle_clicked: &mut bool,
        events: &mut Vec<LogTableEvent>,
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
                format!("▶ {}{}", bookmark_icon, line.line_number())
            } else {
                format!("{}{}", bookmark_icon, line.line_number())
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
                ui.id().with(line_idx).with("line"),
                egui::Sense::click(),
            );

            if response.clicked() {
                *row_clicked = true;
            }
            if response.middle_clicked() {
                *row_middle_clicked = true;
            }

            // Show context menu on right-click
            Self::show_line_context_menu(&response, store, line_idx, events);
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn render_timestamp_column(
        row: &mut egui_extras::TableRow,
        store: &LogStore,
        line: &LogLine,
        line_idx: StoreID,
        is_selected: bool,
        is_bookmarked: bool,
        color: Color32,
        row_clicked: &mut bool,
        row_middle_clicked: &mut bool,
        events: &mut Vec<LogTableEvent>,
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

            let timestamp_str = line.timestamp().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
            let text = RichText::new(timestamp_str).color(color);
            ui.label(text);

            let response = ui.interact(
                ui.max_rect(),
                ui.id().with(line_idx).with("ts"),
                egui::Sense::click(),
            );
            if response.clicked() {
                *row_clicked = true;
            }
            if response.middle_clicked() {
                *row_middle_clicked = true;
            }

            // Show context menu on right-click
            Self::show_line_context_menu(&response, store, line_idx, events);
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn render_message_column(
        row: &mut egui_extras::TableRow,
        store: &LogStore,
        line: &LogLine,
        line_idx: StoreID,
        is_selected: bool,
        is_bookmarked: bool,
        bg_color: Color32,
        all_filter_highlights: &[FilterHighlight],
        row_clicked: &mut bool,
        row_middle_clicked: &mut bool,
        events: &mut Vec<LogTableEvent>,
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

            let message = line.message();
            let job = FilterHighlight::highlight_text_with_filters(
                &message,
                bg_color,
                all_filter_highlights,
                dark_mode,
            );

            // Layout the text to check if it would be clipped
            let available_width = ui.available_width();
            let galley = ui.painter().layout_job(job.clone());
            let text_width = galley.size().x;
            let is_clipped = text_width > available_width;

            let response = ui.add(egui::Label::new(job).selectable(true).extend());

            // Only show hover tooltip if text was clipped
            let response = if is_clipped {
                response.on_hover_text(line.raw())
            } else {
                response
            };

            if response.clicked() {
                *row_clicked = true;
            }
            if response.middle_clicked() {
                *row_middle_clicked = true;
            }

            // Show context menu on right-click
            Self::show_line_context_menu(&response, store, line_idx, events);
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn render_score_column(
        row: &mut egui_extras::TableRow,
        store: &LogStore,
        line: &LogLine,
        line_idx: StoreID,
        is_selected: bool,
        is_bookmarked: bool,
        color: Color32,
        row_clicked: &mut bool,
        row_middle_clicked: &mut bool,
        events: &mut Vec<LogTableEvent>,
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

            let anomaly_str = format!("{:.1}", line.anomaly_score());
            let text = RichText::new(anomaly_str).strong().color(color);
            ui.label(text);

            let response = ui.interact(
                ui.max_rect(),
                ui.id().with(line_idx).with("score"),
                egui::Sense::click(),
            );
            if response.clicked() {
                *row_clicked = true;
            }
            if response.middle_clicked() {
                *row_middle_clicked = true;
            }

            // Show context menu on right-click
            Self::show_line_context_menu(&response, store, line_idx, events);
        });
    }
}
