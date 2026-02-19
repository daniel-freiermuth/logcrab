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

use crate::core::log_store::StoreID;
use crate::core::LogStore;
use crate::parser::line::{LogLine, LogLineCore};
use crate::parser::logline_types::format_time_diff;
use crate::ui::filter_highlight::FilterHighlight;
use crate::ui::session_state::SessionState;
use crate::ui::tabs::filter_tab::log_table::{
    score_to_color, scrolled_to_row_color, selected_row_color,
};
use chrono::Local;
use egui::{Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};

/// Bookmark data
#[derive(Debug, Clone)]
pub struct BookmarkData {
    pub store_id: StoreID,
    pub name: String,
}

/// Events emitted by the bookmark panel
#[derive(Debug, Clone)]
pub enum BookmarkPanelEvent {
    BookmarkClicked {
        store_id: StoreID,
    },
    BookmarkDeleted {
        store_id: StoreID,
    },
    BookmarkRenamed {
        store_id: StoreID,
        new_name: String,
    },
    StartRenaming {
        store_id: StoreID,
    },
    CancelRenaming,
    SyncTime {
        line_index: StoreID,
        calculated_time: chrono::DateTime<chrono::Local>,
        storage_time: Option<chrono::DateTime<chrono::Local>>,
        ecu_id: Option<String>,
        app_id: Option<String>,
    },
}

/// Reusable bookmark panel component
pub struct BookmarkPanel;

impl BookmarkPanel {
    /// Show context menu for a log line
    fn show_line_context_menu(
        response: &egui::Response,
        store: &LogStore,
        line_idx: StoreID,
        events: &mut Vec<BookmarkPanelEvent>,
    ) {
        use crate::parser::line::{LogLineCore, LogLineVariant};
        response.context_menu(|ui| {
            if ui.button("📑 Remove Bookmark").clicked() {
                events.push(BookmarkPanelEvent::BookmarkDeleted { store_id: line_idx });
                ui.close();
            }

            if ui.button("🎯 Jump to Line").clicked() {
                events.push(BookmarkPanelEvent::BookmarkClicked { store_id: line_idx });
                ui.close();
            }

            // Time synchronization option (DLT-specific calibration or general file offset)
            let Some(line) = store.get_by_id(&line_idx) else {
                return;
            };

            // Determine if we should show the sync/calibrate option
            let show_sync_option = if let LogLineVariant::Dlt(ref dlt_line) = line {
                // For DLT: only show in CalibratedMonotonic mode (when boot_time is set)
                dlt_line.boot_time.is_some()
            } else {
                // For non-DLT files: show calibrate option
                true
            };

            if show_sync_option && ui.button("⏱ Calibrate Time Here").clicked() {
                // Get current timestamp (with any offset applied) and original timestamp
                let offset_ms = store.get_time_offset_ms(&line_idx).unwrap_or(0);
                let original_time = line.timestamp();
                let calculated_time = if offset_ms != 0 {
                    original_time + chrono::Duration::milliseconds(offset_ms)
                } else {
                    original_time
                };

                // For DLT files, extract storage time and ECU/App IDs
                // For non-DLT files, use original timestamp (without offset)
                let (storage_time, ecu_id, app_id) = if let LogLineVariant::Dlt(ref dlt_line) = line
                {
                    let stor_time = dlt_line.dlt_message.storage_header.as_ref().and_then(|sh| {
                        use chrono::TimeZone;
                        let secs = i64::from(sh.timestamp.seconds);
                        let nsecs = sh.timestamp.microseconds * 1000;
                        chrono::Local.timestamp_opt(secs, nsecs).single()
                    });

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
                    // For non-DLT files, use original timestamp (line.timestamp() without offset)
                    (Some(original_time), None, None)
                };

                events.push(BookmarkPanelEvent::SyncTime {
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

    /// Render the bookmarks view
    ///
    /// Returns events that occurred (clicks, deletes, renames)
    pub fn render(
        ui: &mut Ui,
        log_view_state: &SessionState,
        bookmarks: &[BookmarkData],
        editing_bookmark: Option<&StoreID>,
        bookmark_name_input: &mut String,
        scroll_to_row: Option<usize>,
        closest_bookmark_index: Option<usize>,
        all_filter_highlights: &[FilterHighlight],
    ) -> Vec<BookmarkPanelEvent> {
        let mut events = Vec::new();

        if bookmarks.is_empty() {
            Self::render_empty_state(ui);
            return events;
        }

        let available_width = ui.available_width();
        egui::ScrollArea::horizontal()
            .id_salt("bookmarks_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(available_width);
                Self::render_bookmark_table(
                    ui,
                    log_view_state,
                    bookmarks,
                    editing_bookmark,
                    bookmark_name_input,
                    scroll_to_row,
                    closest_bookmark_index,
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
        log_view_state: &SessionState,
        bookmarks: &[BookmarkData],
        editing_bookmark: Option<&StoreID>,
        bookmark_name_input: &mut String,
        scroll_to_row: Option<usize>,
        closest_bookmark_index: Option<usize>,
        all_filter_highlights: &[FilterHighlight],
        events: &mut Vec<BookmarkPanelEvent>,
    ) {
        let available_height = ui.available_height();
        let header_height = ui.text_style_height(&egui::TextStyle::Heading);
        let body_height = available_height - header_height - 1.0;
        let dark_mode = ui.visuals().dark_mode;

        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(false)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .vscroll(true)
            .min_scrolled_height(body_height)
            .drag_to_scroll(false)
            .max_scroll_height(body_height)
            .column(Column::initial(150.0).resizable(true).clip(true))
            .column(Column::initial(175.0).resizable(true).clip(true))
            .column(Column::initial(200.0).resizable(true).clip(true))
            .column(Column::remainder().resizable(true).clip(true))
            .column(Column::initial(40.0).resizable(false).clip(true));

        if let Some(row_idx) = scroll_to_row {
            table = table.scroll_to_row(row_idx, Some(egui::Align::Center));
        }

        table
            .header(header_height, |mut header| {
                header.col(|ui| {
                    ui.strong("Annotation");
                });
                header.col(|ui| {
                    ui.strong("Line");
                });
                header.col(|ui| {
                    let now = Local::now();
                    let offset = now.offset();
                    ui.strong(format!("Timestamp (UTC{offset})"));
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
                        closest_bookmark_index,
                        all_filter_highlights,
                        events,
                        dark_mode,
                    );
                });
            });
    }

    fn render_bookmark_row(
        row: &mut egui_extras::TableRow<'_, '_>,
        log_view_state: &SessionState,
        bookmarks: &[BookmarkData],
        editing_bookmark: Option<&StoreID>,
        bookmark_name_input: &mut String,
        closest_bookmark_index: Option<usize>,
        all_filter_highlights: &[FilterHighlight],
        events: &mut Vec<BookmarkPanelEvent>,
        dark_mode: bool,
    ) {
        let row_index = row.index();
        let bookmark = &bookmarks[row_index];
        let store_id = &bookmark.store_id;

        let is_selected = log_view_state
            .selected_line_index
            .as_ref()
            .is_some_and(|s| s == store_id);

        let is_closest = !is_selected
            && closest_bookmark_index.is_some_and(|idx| idx == row_index)
            && log_view_state.selected_line_index.is_some();

        let Some(line) = log_view_state.store.get_by_id(store_id) else {
            row.col(|ui| {
                ui.label("Loading...");
            });
            row.col(|_| {});
            row.col(|_| {});
            row.col(|_| {});
            row.col(|_| {});
            return;
        };

        let color = score_to_color(line.anomaly_score(), dark_mode);

        let mut row_clicked = false;

        // Annotation column
        Self::render_name_column(
            row,
            store_id,
            is_selected,
            is_closest,
            color,
            bookmark,
            editing_bookmark,
            bookmark_name_input,
            events,
            &mut row_clicked,
            dark_mode,
        );

        // Line number column
        Self::render_line_column(
            row,
            store_id,
            is_selected,
            is_closest,
            color,
            &mut row_clicked,
            &line,
            dark_mode,
        );

        // Timestamp column
        Self::render_timestamp_column(
            row,
            &log_view_state.store,
            store_id,
            is_selected,
            is_closest,
            color,
            &line,
            &mut row_clicked,
            events,
            dark_mode,
        );

        // Message column
        Self::render_message_column(
            row,
            &log_view_state.store,
            store_id,
            is_selected,
            is_closest,
            color,
            &line,
            all_filter_highlights,
            &mut row_clicked,
            events,
            dark_mode,
        );

        // Delete button column
        Self::render_delete_column(row, store_id, is_selected, is_closest, events, dark_mode);

        if row_clicked {
            events.push(BookmarkPanelEvent::BookmarkClicked {
                store_id: *store_id,
            });
        }
    }

    fn render_line_column(
        row: &mut egui_extras::TableRow<'_, '_>,
        store_id: &StoreID,
        is_selected: bool,
        is_closest: bool,
        color: Color32,
        row_clicked: &mut bool,
        line: &LogLine,
        dark_mode: bool,
    ) {
        row.col(|ui| {
            Self::paint_selection_background(ui, is_selected, is_closest, dark_mode);

            let line_number = line.line_number();
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
                ui.id().with(store_id).with("bm_line"),
                egui::Sense::click(),
            );
            if response.clicked() {
                *row_clicked = true;
            }
        });
    }

    fn render_timestamp_column(
        row: &mut egui_extras::TableRow<'_, '_>,
        store: &LogStore,
        store_id: &StoreID,
        is_selected: bool,
        is_closest: bool,
        color: Color32,
        line: &LogLine,
        row_clicked: &mut bool,
        events: &mut Vec<BookmarkPanelEvent>,
        dark_mode: bool,
    ) {
        row.col(|ui| {
            Self::paint_selection_background(ui, is_selected, is_closest, dark_mode);

            // Apply time offset if present
            let base_time = line.timestamp();
            let offset_ms = store.get_time_offset_ms(store_id).unwrap_or(0);
            let display_time = if offset_ms != 0 {
                base_time + chrono::Duration::milliseconds(offset_ms)
            } else {
                base_time
            };

            let timestamp_str = display_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
            ui.label(RichText::new(&timestamp_str).color(color));

            let response = ui.interact(
                ui.max_rect(),
                ui.id().with(store_id).with("bm_ts"),
                egui::Sense::click(),
            );
            if response.clicked() {
                *row_clicked = true;
            }

            // Show context menu on right-click
            Self::show_line_context_menu(&response, store, *store_id, events);
        });
    }

    fn render_name_column(
        row: &mut egui_extras::TableRow<'_, '_>,
        store_id: &StoreID,
        is_selected: bool,
        is_closest: bool,
        color: Color32,
        bookmark: &BookmarkData,
        editing_bookmark: Option<&StoreID>,
        bookmark_name_input: &mut String,
        events: &mut Vec<BookmarkPanelEvent>,
        row_clicked: &mut bool,
        dark_mode: bool,
    ) {
        row.col(|ui| {
            Self::paint_selection_background(ui, is_selected, is_closest, dark_mode);

            if editing_bookmark == Some(store_id) {
                Self::render_name_editor(ui, store_id, bookmark_name_input, events);
            } else {
                let text = if bookmark.name.is_empty() {
                    RichText::new("Double-click to annotate")
                        .color(ui.visuals().weak_text_color())
                        .italics()
                } else {
                    RichText::new(&bookmark.name).color(color).strong()
                };
                ui.label(text);
                let response = ui.interact(
                    ui.max_rect(),
                    ui.id().with(store_id).with("bm_name"),
                    egui::Sense::click(),
                );

                if response.double_clicked() {
                    events.push(BookmarkPanelEvent::StartRenaming {
                        store_id: *store_id,
                    });
                } else if response.clicked() {
                    *row_clicked = true;
                }
            }
        });
    }

    fn render_name_editor(
        ui: &mut Ui,
        store_id: &StoreID,
        bookmark_name_input: &mut String,
        events: &mut Vec<BookmarkPanelEvent>,
    ) {
        let text_edit =
            egui::TextEdit::singleline(bookmark_name_input).desired_width(ui.available_width());

        let id = ui.id().with("bm_edit").with(store_id);

        // Track if we've initialized focus for this editing session
        let init_id = id.with("initialized");
        let already_initialized = ui.data(|d| d.get_temp::<bool>(init_id).unwrap_or(false));

        let response = ui.add(text_edit.id(id));

        // Request focus and select all only on first frame (not every frame)
        if !already_initialized {
            ui.data_mut(|d| d.insert_temp(init_id, true));
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
        } else if response.lost_focus() {
            // Close editor when clicking outside (focus lost) and clean up temp data
            ui.data_mut(|d| d.remove::<bool>(init_id));
            events.push(BookmarkPanelEvent::CancelRenaming);
        }

        // Save on Enter
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            ui.data_mut(|d| d.remove::<bool>(init_id));
            events.push(BookmarkPanelEvent::BookmarkRenamed {
                store_id: *store_id,
                new_name: bookmark_name_input.clone(),
            });
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            ui.data_mut(|d| d.remove::<bool>(init_id));
            events.push(BookmarkPanelEvent::CancelRenaming);
        }
    }

    fn render_message_column(
        row: &mut egui_extras::TableRow<'_, '_>,
        store: &LogStore,
        store_id: &StoreID,
        is_selected: bool,
        is_closest: bool,
        color: Color32,
        line: &LogLine,
        all_filter_highlights: &[FilterHighlight],
        row_clicked: &mut bool,
        events: &mut Vec<BookmarkPanelEvent>,
        dark_mode: bool,
    ) {
        row.col(|ui| {
            Self::paint_selection_background(ui, is_selected, is_closest, dark_mode);

            // Add time offset prefix if present
            let offset_ms = store.get_time_offset_ms(store_id).unwrap_or(0);
            let prefix = if offset_ms != 0 {
                let offset_duration = chrono::Duration::milliseconds(offset_ms);
                format!("[{}] ", format_time_diff(offset_duration))
            } else {
                String::new()
            };

            let message = format!("{}{}", prefix, line.message());
            let job = FilterHighlight::highlight_text_with_filters(
                &message,
                color,
                all_filter_highlights,
                dark_mode,
            );

            let response = ui.add(egui::Label::new(job).selectable(true).extend());

            if response.clicked() {
                *row_clicked = true;
            }

            // Show context menu on right-click
            Self::show_line_context_menu(&response, store, *store_id, events);
        });
    }

    fn render_delete_column(
        row: &mut egui_extras::TableRow<'_, '_>,
        store_id: &StoreID,
        is_selected: bool,
        is_closest: bool,
        events: &mut Vec<BookmarkPanelEvent>,
        dark_mode: bool,
    ) {
        row.col(|ui| {
            Self::paint_selection_background(ui, is_selected, is_closest, dark_mode);

            if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                events.push(BookmarkPanelEvent::BookmarkDeleted {
                    store_id: *store_id,
                });
            }
        });
    }

    fn paint_selection_background(ui: &Ui, is_selected: bool, is_closest: bool, dark_mode: bool) {
        // Paint background for selected or closest rows
        if is_selected {
            let bg_color = selected_row_color(dark_mode);
            ui.painter()
                .rect_filled(ui.available_rect_before_wrap(), 0.0, bg_color);
        } else if is_closest {
            let bg_color = scrolled_to_row_color(dark_mode);
            ui.painter()
                .rect_filled(ui.available_rect_before_wrap(), 0.0, bg_color);
        }
    }
}
