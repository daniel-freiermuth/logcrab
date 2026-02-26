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
    parser::logline_types::format_time_diff,
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
    SyncTime {
        line_index: StoreID,
        calculated_time: DateTime<Local>,
        storage_time: Option<DateTime<Local>>,
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

/// Get the background color for a row that is scrolled-to (closest to selected, but not exact match)
pub const fn scrolled_to_row_color(dark_mode: bool) -> Color32 {
    if dark_mode {
        Color32::from_rgb(50, 55, 65) // Subtle darker blue-gray
    } else {
        Color32::from_rgb(220, 225, 235) // Subtle lighter blue-gray
    }
}

/// Blend two colors together using weighted average
fn blend_colors(base: Color32, overlay: Color32, overlay_weight: f32) -> Color32 {
    let base_weight = 1.0 - overlay_weight;
    let r = f32::from(base.r()).mul_add(base_weight, f32::from(overlay.r()) * overlay_weight) as u8;
    let g = f32::from(base.g()).mul_add(base_weight, f32::from(overlay.g()) * overlay_weight) as u8;
    let b = f32::from(base.b()).mul_add(base_weight, f32::from(overlay.b()) * overlay_weight) as u8;
    Color32::from_rgb(r, g, b)
}

/// Compute the background color for a row based on selection state and bookmark status
#[allow(clippy::fn_params_excessive_bools)]
fn compute_row_background_color(
    is_selected: bool,
    is_scrolled_to_closest: bool,
    is_bookmarked: bool,
    dark_mode: bool,
) -> Option<Color32> {
    // First determine base color from selection state
    let base_color = if is_selected {
        Some(selected_row_color(dark_mode))
    } else if is_scrolled_to_closest {
        Some(scrolled_to_row_color(dark_mode))
    } else {
        None
    };

    // Blend with bookmark color if bookmarked
    if is_bookmarked {
        let bookmark_color = bookmarked_row_color(dark_mode);
        Some(base_color.map_or(bookmark_color, |base| {
            blend_colors(base, bookmark_color, 0.6)
        }))
    } else {
        base_color
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

            // Time synchronization option (DLT-specific calibration or general file offset)
            let Some(line) = store.get_by_id(&line_idx) else {
                return;
            };

            // Determine if we should show the sync/calibrate option
            // Always show for all file types (DLT in both StorageTime and CalibratedMonotonic modes)
            let show_sync_option = true;

            if show_sync_option && ui.button("⏱ Calibrate Time Here").clicked() {
                // Get current timestamp (with any offset applied) and original timestamp
                let offset_ms = store.get_time_offset_ms(&line_idx).unwrap_or(0);
                let original_time = line.uncalibrated_timestamp();
                let calculated_time = if offset_ms != 0 {
                    original_time + chrono::Duration::milliseconds(offset_ms)
                } else {
                    original_time
                };

                // For DLT files in CalibratedMonotonic mode, extract storage time and ECU/App IDs
                // For DLT files in StorageTime mode or non-DLT files, use generic offset mechanism
                let (storage_time, ecu_id, app_id) = if let LogLineVariant::Dlt(ref dlt_line) = line
                {
                    let stor_time = dlt_line.dlt_message.storage_header.as_ref().and_then(|sh| {
                        use chrono::TimeZone;
                        let secs = i64::from(sh.timestamp.seconds);
                        let nsecs = sh.timestamp.microseconds * 1000;
                        chrono::Local.timestamp_opt(secs, nsecs).single()
                    });

                    // Only pass ECU/App IDs if in CalibratedMonotonic mode (boot_time is set)
                    // This determines whether to use resync_dlt_time_to_target or set_time_offset_to_target
                    if dlt_line.boot_time.is_some() {
                        let ecu = dlt_line
                            .dlt_message
                            .header
                            .ecu_id
                            .as_ref()
                            .map(std::string::ToString::to_string);
                        let app = dlt_line
                            .dlt_message
                            .extended_header
                            .as_ref()
                            .map(|ext| ext.application_id.clone());
                        (stor_time, ecu, app)
                    } else {
                        // StorageTime mode: use generic offset
                        (stor_time, None, None)
                    }
                } else {
                    // For non-DLT files, use original timestamp (line.timestamp() without offset)
                    (Some(original_time), None, None)
                };

                events.push(LogTableEvent::SyncTime {
                    line_index: line_idx,
                    calculated_time,
                    storage_time,
                    ecu_id,
                    app_id,
                });
                ui.close();
            }

            ui.separator();

            if ui.button("📋 Copy Message").clicked() {
                ui.ctx().copy_text(line.message());
                ui.close();
            }

            if ui.button("📋 Copy Full Line").clicked() {
                ui.ctx().copy_text(line.raw());
                ui.close();
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
        closest_row_index: Option<usize>,
        all_filter_highlights: &[FilterHighlight],
    ) -> Vec<LogTableEvent> {
        profiling::scope!("LogTable::render");

        let mut events = Vec::new();
        let dark_mode = ui.visuals().dark_mode;

        // Get filtered indices first to avoid borrow conflicts
        let filtered_indices = filter.search.get_filtered_indices_cached();
        let filter_id = filter.get_id();

        let available_width = ui.available_width();
        let ctx = ui.ctx().clone();
        egui::ScrollArea::horizontal()
            .id_salt(format!("filtered_scroll_{filter_id}"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                profiling::scope!("filtered_table");
                ui.set_min_width(available_width);

                let table = Self::create_table(ui, scroll_to_row, &filter.column_widths);

                Self::render_table_with_header(
                    table,
                    &ctx,
                    store,
                    &filtered_indices,
                    selected_line_index,
                    bookmarked_lines,
                    closest_row_index,
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
        ctx: &egui::Context,
        store: &LogStore,
        filtered_indices: &[StoreID],
        selected_line_index: Option<StoreID>,
        bookmarked_lines: &std::collections::HashMap<StoreID, String>,
        closest_row_index: Option<usize>,
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
                    ctx,
                    store,
                    filtered_indices,
                    selected_line_index,
                    bookmarked_lines,
                    closest_row_index,
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
        ctx: &egui::Context,
        store: &LogStore,
        filtered_indices: &[StoreID],
        selected_line_index: Option<StoreID>,
        bookmarked_lines: &std::collections::HashMap<StoreID, String>,
        closest_row_index: Option<usize>,
        all_filter_highlights: &[FilterHighlight],
        events: &mut Vec<LogTableEvent>,
        dark_mode: bool,
    ) {
        let visible_lines = filtered_indices.len();

        // One-frame delay hover: read which row was hovered last frame
        let hover_storage_id = egui::Id::new("log_table_row_hover");
        let last_frame_hovered: Option<usize> =
            ctx.data(|d| d.get_temp(hover_storage_id)).flatten();

        let mut current_hovered_row: Option<usize> = None;

        body.rows(18.0, visible_lines, |mut row| {
            let row_index = row.index();

            // Apply hover state from last frame (before any col() calls)
            if last_frame_hovered == Some(row_index) {
                row.set_hovered(true);
            }

            let event = Self::render_table_row(
                &mut row,
                store,
                filtered_indices,
                selected_line_index,
                bookmarked_lines,
                closest_row_index,
                all_filter_highlights,
                events,
                dark_mode,
            );

            // Check if pointer is over this row for next frame
            if row.response().contains_pointer() {
                current_hovered_row = Some(row_index);
            }

            if let Some(evt) = event {
                events.push(evt);
            }
        });

        // Store for next frame
        ctx.data_mut(|d| d.insert_temp(hover_storage_id, current_hovered_row));
    }

    #[allow(clippy::too_many_arguments)]
    fn render_table_row(
        row: &mut egui_extras::TableRow,
        store: &LogStore,
        filtered_indices: &[StoreID],
        selected_line_index: Option<StoreID>,
        bookmarked_lines: &std::collections::HashMap<StoreID, String>,
        closest_row_index: Option<usize>,
        all_filter_highlights: &[FilterHighlight],
        events: &mut Vec<LogTableEvent>,
        dark_mode: bool,
    ) -> Option<LogTableEvent> {
        let row_index = row.index();
        let line_idx = filtered_indices[row_index];

        // Handle stale indices gracefully (can happen briefly after source removal)
        let Some(line) = store.get_by_id(&line_idx) else {
            // Render empty placeholder row
            row.col(|_| {});
            row.col(|_| {});
            row.col(|_| {});
            row.col(|_| {});
            row.col(|_| {}); // Score column
            return None;
        };

        let is_selected = selected_line_index.as_ref() == Some(&line_idx);
        let is_bookmarked = bookmarked_lines.contains_key(&line_idx);
        // Check if this is the scrolled-to row when the selected line is not in filtered results
        let is_scrolled_to_closest = !is_selected
            && closest_row_index.is_some_and(|closest_row| closest_row == row_index)
            && selected_line_index.is_some();
        let color = score_to_color(line.anomaly_score(), dark_mode);
        let source_name = store.get_source_name(&line_idx);

        let column_response = Self::render_all_columns(
            row,
            store,
            &line,
            line_idx,
            is_selected,
            is_scrolled_to_closest,
            is_bookmarked,
            color,
            source_name.as_deref(),
            bookmarked_lines,
            all_filter_highlights,
            dark_mode,
        );

        // Row-level interaction handling (union column and row responses)
        let merged = column_response.union(row.response());
        let row_clicked = merged.clicked();
        let row_middle_clicked = merged.middle_clicked();

        Self::show_line_context_menu(&merged, store, line_idx, events);

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
    #[allow(clippy::fn_params_excessive_bools)]
    fn render_all_columns(
        row: &mut egui_extras::TableRow,
        store: &LogStore,
        line: &LogLine,
        line_idx: StoreID,
        is_selected: bool,
        is_scrolled_to_closest: bool,
        is_bookmarked: bool,
        color: Color32,
        source_name: Option<&str>,
        bookmarked_lines: &std::collections::HashMap<StoreID, String>,
        all_filter_highlights: &[FilterHighlight],
        dark_mode: bool,
    ) -> egui::Response {
        let responses = [
            Self::render_source_column(
                row,
                is_selected,
                is_scrolled_to_closest,
                is_bookmarked,
                color,
                source_name,
                dark_mode,
            ),
            Self::render_line_column(
                row,
                line,
                is_selected,
                is_scrolled_to_closest,
                is_bookmarked,
                color,
                bookmarked_lines
                    .get(&line_idx)
                    .map(std::string::String::as_str),
                dark_mode,
            ),
            Self::render_timestamp_column(
                row,
                store,
                line,
                line_idx,
                is_selected,
                is_scrolled_to_closest,
                is_bookmarked,
                color,
                dark_mode,
            ),
            Self::render_message_column(
                row,
                store,
                line,
                line_idx,
                is_selected,
                is_scrolled_to_closest,
                is_bookmarked,
                color,
                all_filter_highlights,
                dark_mode,
            ),
            Self::render_score_column(
                row,
                line,
                is_selected,
                is_scrolled_to_closest,
                is_bookmarked,
                color,
                dark_mode,
            ),
        ];

        responses
            .into_iter()
            .reduce(|a, b| a.union(b))
            .expect("array is non-empty")
    }

    #[allow(clippy::fn_params_excessive_bools)]
    fn render_source_column(
        row: &mut egui_extras::TableRow,
        is_selected: bool,
        is_scrolled_to_closest: bool,
        is_bookmarked: bool,
        color: Color32,
        source_name: Option<&str>,
        dark_mode: bool,
    ) -> egui::Response {
        let mut response: Option<egui::Response> = None;
        row.col(|ui| {
            // Background highlight for selected/bookmarked rows
            if let Some(bg_color) = compute_row_background_color(
                is_selected,
                is_scrolled_to_closest,
                is_bookmarked,
                dark_mode,
            ) {
                ui.painter()
                    .rect_filled(ui.available_rect_before_wrap(), 0.0, bg_color);
            }

            // Display source name (truncated if needed)
            let display_name = source_name.unwrap_or("stdin");
            let text = RichText::new(display_name).color(color);
            let label_response = ui.add(
                egui::Label::new(text)
                    .truncate()
                    .sense(egui::Sense::click()),
            );

            // Tooltip with full source name
            if let Some(name) = source_name {
                label_response.clone().on_hover_text(name);
            }
            response = Some(label_response);
        });
        response.expect("column always renders")
    }

    #[allow(clippy::fn_params_excessive_bools)]
    fn render_line_column(
        row: &mut egui_extras::TableRow,
        line: &LogLine,
        is_selected: bool,
        is_scrolled_to_closest: bool,
        is_bookmarked: bool,
        color: Color32,
        bookmark_name: Option<&str>,
        dark_mode: bool,
    ) -> egui::Response {
        let mut response: Option<egui::Response> = None;
        row.col(|ui| {
            if let Some(bg_color) = compute_row_background_color(
                is_selected,
                is_scrolled_to_closest,
                is_bookmarked,
                dark_mode,
            ) {
                ui.painter()
                    .rect_filled(ui.available_rect_before_wrap(), 0.0, bg_color);
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
            let label_response = ui.add(egui::Label::new(text).sense(egui::Sense::click()));

            // Show tooltip with bookmark name if bookmarked
            if is_bookmarked {
                if let Some(name) = bookmark_name {
                    label_response
                        .clone()
                        .on_hover_text(format!("📑 Bookmark: {name}"));
                }
            }
            response = Some(label_response);
        });
        response.expect("column always renders")
    }

    #[allow(clippy::fn_params_excessive_bools)]
    fn render_timestamp_column(
        row: &mut egui_extras::TableRow,
        store: &LogStore,
        line: &LogLine,
        line_idx: StoreID,
        is_selected: bool,
        is_scrolled_to_closest: bool,
        is_bookmarked: bool,
        color: Color32,
        dark_mode: bool,
    ) -> egui::Response {
        let mut response: Option<egui::Response> = None;
        row.col(|ui| {
            if let Some(bg_color) = compute_row_background_color(
                is_selected,
                is_scrolled_to_closest,
                is_bookmarked,
                dark_mode,
            ) {
                ui.painter()
                    .rect_filled(ui.available_rect_before_wrap(), 0.0, bg_color);
            }

            // Apply time offset if present
            let base_time = line.uncalibrated_timestamp();
            let offset_ms = store.get_time_offset_ms(&line_idx).unwrap_or(0);
            let display_time = if offset_ms != 0 {
                base_time + chrono::Duration::milliseconds(offset_ms)
            } else {
                base_time
            };

            let timestamp_str = display_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
            let text = RichText::new(timestamp_str).color(color);
            response = Some(ui.add(egui::Label::new(text).sense(egui::Sense::click())));
        });
        response.expect("column always renders")
    }

    #[allow(clippy::fn_params_excessive_bools)]
    fn render_message_column(
        row: &mut egui_extras::TableRow,
        store: &LogStore,
        line: &LogLine,
        line_idx: StoreID,
        is_selected: bool,
        is_scrolled_to_closest: bool,
        is_bookmarked: bool,
        bg_color: Color32,
        all_filter_highlights: &[FilterHighlight],
        dark_mode: bool,
    ) -> egui::Response {
        let mut response: Option<egui::Response> = None;
        row.col(|ui| {
            if let Some(bg_color) = compute_row_background_color(
                is_selected,
                is_scrolled_to_closest,
                is_bookmarked,
                dark_mode,
            ) {
                ui.painter()
                    .rect_filled(ui.available_rect_before_wrap(), 0.0, bg_color);
            }

            // Add time offset prefix if present
            let offset_ms = store.get_time_offset_ms(&line_idx).unwrap_or(0);
            let prefix = if offset_ms != 0 {
                let offset_duration = chrono::Duration::milliseconds(offset_ms);
                format!("[{}] ", format_time_diff(offset_duration))
            } else {
                String::new()
            };

            let message = format!("{}{}", prefix, line.message());
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

            let label_response = ui.add(egui::Label::new(job).selectable(true).extend());

            // Only show hover tooltip if text was clipped
            if is_clipped {
                label_response.clone().on_hover_text(line.raw());
            }
            response = Some(label_response);
        });
        response.expect("column always renders")
    }

    #[allow(clippy::fn_params_excessive_bools)]
    fn render_score_column(
        row: &mut egui_extras::TableRow,
        line: &LogLine,
        is_selected: bool,
        is_scrolled_to_closest: bool,
        is_bookmarked: bool,
        color: Color32,
        dark_mode: bool,
    ) -> egui::Response {
        let mut response: Option<egui::Response> = None;
        row.col(|ui| {
            if let Some(bg_color) = compute_row_background_color(
                is_selected,
                is_scrolled_to_closest,
                is_bookmarked,
                dark_mode,
            ) {
                ui.painter()
                    .rect_filled(ui.available_rect_before_wrap(), 0.0, bg_color);
            }

            let anomaly_str = format!("{:.1}", line.anomaly_score());
            let text = RichText::new(anomaly_str).strong().color(color);
            response = Some(ui.add(egui::Label::new(text).sense(egui::Sense::click())));
        });
        response.expect("column always renders")
    }
}
