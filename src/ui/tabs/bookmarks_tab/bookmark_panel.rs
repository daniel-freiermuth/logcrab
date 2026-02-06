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

use crate::core::log_store::StoreID;
use crate::parser::line::{LogLine, LogLineCore};
use crate::ui::filter_highlight::FilterHighlight;
use crate::ui::session_state::SessionState;
use crate::ui::tabs::filter_tab::log_table::{
    bookmarked_row_color, score_to_color, selected_row_color,
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
    BookmarkClicked { store_id: StoreID },
    BookmarkDeleted { store_id: StoreID },
    BookmarkRenamed { store_id: StoreID, new_name: String },
    StartRenaming { store_id: StoreID },
    CancelRenaming,
}

/// Reusable bookmark panel component
pub struct BookmarkPanel;

impl BookmarkPanel {
    /// Render the bookmarks view
    ///
    /// Returns events that occurred (clicks, deletes, renames)
    pub fn render(
        ui: &mut Ui,
        log_view_state: &SessionState,
        bookmarks: &[BookmarkData],
        editing_bookmark: Option<&StoreID>,
        bookmark_name_input: &mut String,
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
        all_filter_highlights: &[FilterHighlight],
        events: &mut Vec<BookmarkPanelEvent>,
    ) {
        let available_height = ui.available_height();
        let header_height = ui.text_style_height(&egui::TextStyle::Heading);
        let body_height = available_height - header_height - 1.0;
        let dark_mode = ui.visuals().dark_mode;

        TableBuilder::new(ui)
            .striped(true)
            .resizable(false)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .vscroll(true)
            .min_scrolled_height(body_height)
            .drag_to_scroll(false)
            .max_scroll_height(body_height)
            .column(Column::initial(60.0).resizable(true).clip(true))
            .column(Column::initial(175.0).resizable(true).clip(true))
            .column(Column::initial(200.0).resizable(true).clip(true))
            .column(Column::remainder().resizable(true).clip(true))
            .column(Column::initial(40.0).resizable(false).clip(true))
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
            color,
            &mut row_clicked,
            &line,
            dark_mode,
        );

        // Timestamp column
        Self::render_timestamp_column(
            row,
            store_id,
            is_selected,
            color,
            &line,
            &mut row_clicked,
            dark_mode,
        );

        // Message column
        Self::render_message_column(
            row,
            is_selected,
            color,
            &line,
            all_filter_highlights,
            &mut row_clicked,
            dark_mode,
        );

        // Delete button column
        Self::render_delete_column(row, store_id, is_selected, events, dark_mode);

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
        color: Color32,
        row_clicked: &mut bool,
        line: &LogLine,
        dark_mode: bool,
    ) {
        row.col(|ui| {
            Self::paint_selection_background(ui, is_selected, dark_mode);

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
        store_id: &StoreID,
        is_selected: bool,
        color: Color32,
        line: &LogLine,
        row_clicked: &mut bool,
        dark_mode: bool,
    ) {
        row.col(|ui| {
            Self::paint_selection_background(ui, is_selected, dark_mode);

            let timestamp_str = line.timestamp().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
            ui.label(RichText::new(&timestamp_str).color(color));

            let response = ui.interact(
                ui.max_rect(),
                ui.id().with(store_id).with("bm_ts"),
                egui::Sense::click(),
            );
            if response.clicked() {
                *row_clicked = true;
            }
        });
    }

    fn render_name_column(
        row: &mut egui_extras::TableRow<'_, '_>,
        store_id: &StoreID,
        is_selected: bool,
        color: Color32,
        bookmark: &BookmarkData,
        editing_bookmark: Option<&StoreID>,
        bookmark_name_input: &mut String,
        events: &mut Vec<BookmarkPanelEvent>,
        row_clicked: &mut bool,
        dark_mode: bool,
    ) {
        row.col(|ui| {
            Self::paint_selection_background(ui, is_selected, dark_mode);

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
                store_id: *store_id,
                new_name: bookmark_name_input.clone(),
            });
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            events.push(BookmarkPanelEvent::CancelRenaming);
        }
    }

    fn render_message_column(
        row: &mut egui_extras::TableRow<'_, '_>,
        is_selected: bool,
        color: Color32,
        line: &LogLine,
        all_filter_highlights: &[FilterHighlight],
        row_clicked: &mut bool,
        dark_mode: bool,
    ) {
        row.col(|ui| {
            Self::paint_selection_background(ui, is_selected, dark_mode);

            let message = line.message();
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
        });
    }

    fn render_delete_column(
        row: &mut egui_extras::TableRow<'_, '_>,
        store_id: &StoreID,
        is_selected: bool,
        events: &mut Vec<BookmarkPanelEvent>,
        dark_mode: bool,
    ) {
        row.col(|ui| {
            Self::paint_selection_background(ui, is_selected, dark_mode);

            if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                events.push(BookmarkPanelEvent::BookmarkDeleted {
                    store_id: *store_id,
                });
            }
        });
    }

    fn paint_selection_background(ui: &Ui, is_selected: bool, dark_mode: bool) {
        // All bookmark rows have the bookmark background
        let bg_color = if is_selected {
            selected_row_color(dark_mode)
        } else {
            bookmarked_row_color(dark_mode)
        };
        ui.painter()
            .rect_filled(ui.available_rect_before_wrap(), 0.0, bg_color);
    }
}
