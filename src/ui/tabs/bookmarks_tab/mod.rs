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

pub mod bookmark_panel;

pub use bookmark_panel::{BookmarkData, BookmarkPanel, BookmarkPanelEvent};

use crate::{
    core::{log_store::StoreID, SavedFilter},
    input::ShortcutAction,
    ui::{
        filter_highlight::FilterHighlight,
        session_state::SessionState,
        tabs::{filter_tab::HistogramMarker, LogCrabTab},
        windows::SyncDltTimeWindow,
    },
};
use egui::Ui;

/// Orchestrates the bookmarks view UI using the `BookmarkPanel` component
#[derive(Default)]
pub struct BookmarksView {
    edited_store_id: Option<StoreID>,
    bookmark_name_input: String,
    enter_pressed_this_frame: bool,
    last_selected_line: Option<StoreID>,
    closest_bookmark_index: Option<usize>,
    sync_dlt_time_window: Option<(StoreID, SyncDltTimeWindow, Option<String>, Option<String>)>,
}

impl BookmarksView {
    /// Calculate scroll position and closest bookmark index
    fn calculate_scroll_and_closest(
        &mut self,
        selected_line_index: Option<StoreID>,
        bookmarks: &[BookmarkData],
        store: &crate::core::LogStore,
    ) -> (Option<usize>, Option<usize>) {
        // Check if selection changed
        if self.last_selected_line == selected_line_index {
            return (None, self.closest_bookmark_index);
        }

        self.last_selected_line = selected_line_index;

        let Some(selected) = selected_line_index else {
            self.closest_bookmark_index = None;
            return (None, None);
        };

        // Find closest bookmark by timestamp
        if bookmarks.is_empty() {
            self.closest_bookmark_index = None;
            return (None, None);
        }

        let closest_idx = bookmarks.partition_point(|bookmark| {
            bookmark.store_id.cmp(&selected, store) == std::cmp::Ordering::Less
        });

        let closest_idx = closest_idx.min(bookmarks.len() - 1);
        self.closest_bookmark_index = Some(closest_idx);

        (Some(closest_idx), Some(closest_idx))
    }

    /// Render the bookmarks view
    ///
    /// Returns events that occurred during rendering
    fn render(
        ui: &mut Ui,
        session_state: &SessionState,
        bookmarks: &[BookmarkData],
        editing_bookmark: Option<&StoreID>,
        bookmark_name_input: &mut String,
        scroll_to_row: Option<usize>,
        closest_bookmark_index: Option<usize>,
        all_filter_highlights: &[FilterHighlight],
    ) -> Vec<BookmarkPanelEvent> {
        BookmarkPanel::render(
            ui,
            session_state,
            bookmarks,
            editing_bookmark,
            bookmark_name_input,
            scroll_to_row,
            closest_bookmark_index,
            all_filter_highlights,
        )
    }

    fn start_renaming_bookmark(&mut self, store_id: StoreID, data_state: &SessionState) {
        if let Some(bookmark) = data_state.get_bookmark(&store_id) {
            self.edited_store_id = Some(store_id);
            self.bookmark_name_input.clone_from(&bookmark.name);
        }
    }

    fn sort_bookmarks_by_timestamp(bookmarks: &mut [BookmarkData], data_state: &SessionState) {
        bookmarks.sort_by(|b1, b2| b1.store_id.cmp(&b2.store_id, &data_state.store));
    }

    pub fn render_bookmarks(
        &mut self,
        ui: &mut Ui,
        data_state: &mut SessionState,
        all_filter_highlights: &[FilterHighlight],
    ) {
        // Check if Enter was pressed this frame (when not editing)
        if self.edited_store_id.is_none() {
            self.enter_pressed_this_frame = ui.input(|i| i.key_pressed(egui::Key::Enter));
        } else {
            self.enter_pressed_this_frame = false;
        }

        // Convert bookmarks to BookmarkData format
        let mut bookmarks: Vec<BookmarkData> = data_state.get_all_bookmarks();
        Self::sort_bookmarks_by_timestamp(&mut bookmarks, data_state);

        // Calculate scroll position and closest bookmark for highlighting
        let (scroll_to_row, closest_bookmark_index) = self.calculate_scroll_and_closest(
            data_state.selected_line_index,
            &bookmarks,
            &data_state.store,
        );

        // Render using BookmarksView
        let events = Self::render(
            ui,
            data_state,
            &bookmarks,
            self.edited_store_id.as_ref(),
            &mut self.bookmark_name_input,
            scroll_to_row,
            closest_bookmark_index,
            all_filter_highlights,
        );

        // Handle events
        for event in events {
            match event {
                BookmarkPanelEvent::BookmarkClicked { store_id } => {
                    data_state.selected_line_index = Some(store_id);
                }
                BookmarkPanelEvent::BookmarkDeleted { store_id } => {
                    data_state.remove_bookmark(&store_id);
                }
                BookmarkPanelEvent::BookmarkRenamed { store_id, new_name } => {
                    data_state.rename_bookmark(&store_id, new_name);
                    self.edited_store_id = None;
                }
                BookmarkPanelEvent::StartRenaming { store_id } => {
                    self.start_renaming_bookmark(store_id, data_state);
                }
                BookmarkPanelEvent::CancelRenaming => {
                    self.edited_store_id = None;
                }
                BookmarkPanelEvent::SyncTime {
                    line_index,
                    calculated_time,
                    storage_time,
                    ecu_id,
                    app_id,
                } => {
                    let is_dlt = ecu_id.is_some() || app_id.is_some();
                    self.sync_dlt_time_window = Some((
                        line_index,
                        SyncDltTimeWindow::new(
                            calculated_time,
                            is_dlt,
                            if is_dlt { Some(calculated_time) } else { None },
                            storage_time,
                        ),
                        ecu_id,
                        app_id,
                    ));
                }
            }
        }

        // Handle time sync dialog (DLT calibration or file offset)
        if let Some((store_id, ref mut window, ref ecu_id, ref app_id)) = self.sync_dlt_time_window
        {
            match window.render(ui) {
                Ok(Some(target_time)) => {
                    // User confirmed - perform the sync
                    let is_dlt = ecu_id.is_some() || app_id.is_some();

                    let result = if is_dlt {
                        // DLT calibration (per ECU, per App)
                        data_state.store.resync_dlt_time_to_target(
                            &store_id,
                            target_time,
                            ecu_id.as_ref(),
                            app_id.as_ref(),
                        )
                    } else {
                        // Non-DLT file offset
                        data_state
                            .store
                            .set_time_offset_to_target(&store_id, target_time)
                    };

                    match result {
                        Ok(()) => {
                            let sync_type = if is_dlt {
                                "DLT timestamps"
                            } else {
                                "file time offset"
                            };
                            log::info!("Successfully synced {sync_type} to target: {target_time}");
                            data_state.modified = true;
                        }
                        Err(e) => {
                            log::error!("Failed to sync time: {e}");
                        }
                    }
                    self.sync_dlt_time_window = None;
                }
                Ok(None) => {
                    // Still editing
                }
                Err(()) => {
                    // Cancelled
                    self.sync_dlt_time_window = None;
                }
            }
        }
    }

    /// Move selection in bookmarks view
    pub fn move_selection_in_bookmarks(delta: i32, data_state: &mut SessionState) {
        let mut bookmarks = data_state.get_all_bookmarks();
        if bookmarks.is_empty() {
            return;
        }

        // Get sorted list of bookmark store IDs
        // TODO: We shouldn't sort this every time - maybe sort by timestamp?
        Self::sort_bookmarks_by_timestamp(&mut bookmarks, data_state);

        let bookmark_ids: Vec<StoreID> = bookmarks.into_iter().map(|id| id.store_id).collect();

        // Find current position in bookmark list
        let current_pos = data_state
            .selected_line_index
            .as_ref()
            .and_then(|selected| bookmark_ids.iter().position(|id| id == selected))
            .unwrap_or(if delta >= 0 {
                0
            } else {
                bookmark_ids.len() - 1
            });

        let new_pos = if delta < 0 {
            current_pos.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (current_pos + delta as usize).min(bookmark_ids.len() - 1)
        };

        data_state.selected_line_index = Some(bookmark_ids[new_pos]);
    }

    /// Jump to the first bookmark (Vim-style gg)
    pub fn jump_to_top_in_bookmarks(data_state: &mut SessionState) {
        let bookmarks = data_state.get_all_bookmarks();
        if bookmarks.is_empty() {
            return;
        }

        // Get the first bookmark by some ordering
        if let Some(bookmark_data) = bookmarks.into_iter().next() {
            data_state.selected_line_index = Some(bookmark_data.store_id);
        }
    }

    /// Jump to the last bookmark (Vim-style G)
    pub fn jump_to_bottom_in_bookmarks(data_state: &mut SessionState) {
        let bookmarks = data_state.get_all_bookmarks();
        if bookmarks.is_empty() {
            return;
        }

        // Get the last bookmark by some ordering
        if let Some(bookmark_data) = bookmarks.into_iter().last() {
            data_state.selected_line_index = Some(bookmark_data.store_id);
        }
    }

    /// Move selection up by one page in bookmarks view
    pub fn page_up_in_bookmarks(data_state: &mut SessionState) {
        const PAGE_SIZE: i32 = 25;
        Self::move_selection_in_bookmarks(-PAGE_SIZE, data_state);
    }

    /// Move selection down by one page in bookmarks view
    pub fn page_down_in_bookmarks(data_state: &mut SessionState) {
        const PAGE_SIZE: i32 = 25;
        Self::move_selection_in_bookmarks(PAGE_SIZE, data_state);
    }
}

impl LogCrabTab for BookmarksView {
    fn title(&mut self) -> egui::WidgetText {
        "Bookmarks".into()
    }

    fn render(
        &mut self,
        ui: &mut egui::Ui,
        data_state: &mut SessionState,
        global_config: &mut crate::config::GlobalConfig,
        all_filter_highlights: &[FilterHighlight],
        _histogram_markers: &[HistogramMarker],
    ) {
        // Add timeline toggle button at the top
        ui.horizontal(|ui| {
            if ui
                .toggle_value(&mut global_config.show_bookmarks_in_timeline, "📊")
                .on_hover_text("Show bookmarks as markers in timeline")
                .changed()
            {
                // Save config when changed
                if let Err(e) = global_config.save() {
                    log::error!("Failed to save config: {e}");
                }
            }
            ui.label("Show in Timeline");
        });

        ui.separator();

        self.render_bookmarks(ui, data_state, all_filter_highlights);
    }

    fn process_events(
        &mut self,
        actions: &[ShortcutAction],
        data_state: &mut SessionState,
    ) -> bool {
        // Handle Enter key for starting bookmark rename (when not already editing)
        // enter_pressed_this_frame is set during render when we have UI context
        if self.enter_pressed_this_frame && self.edited_store_id.is_none() {
            if let Some(selected) = data_state.selected_line_index {
                self.start_renaming_bookmark(selected, data_state);
            }
        }

        for action in actions {
            match action {
                ShortcutAction::MoveDown => Self::move_selection_in_bookmarks(1, data_state),
                ShortcutAction::MoveUp => Self::move_selection_in_bookmarks(-1, data_state),
                ShortcutAction::ToggleBookmark => {}
                ShortcutAction::JumpToTop => {
                    Self::jump_to_top_in_bookmarks(data_state);
                }
                ShortcutAction::JumpToBottom => {
                    Self::jump_to_bottom_in_bookmarks(data_state);
                }
                ShortcutAction::PageUp => {
                    Self::page_up_in_bookmarks(data_state);
                }
                ShortcutAction::PageDown => {
                    Self::page_down_in_bookmarks(data_state);
                }
                ShortcutAction::FocusSearch => {}
                ShortcutAction::NewFilterTab => {}
                ShortcutAction::NewBookmarksTab => {}
                ShortcutAction::ReverseCycleTab => {}
                ShortcutAction::OpenFile => {}
                ShortcutAction::RenameFilter => {}
                ShortcutAction::CloseTab => {}
                ShortcutAction::CycleTab => {}
                ShortcutAction::FocusPaneLeft => {}
                ShortcutAction::FocusPaneDown => {}
                ShortcutAction::FocusPaneUp => {}
                ShortcutAction::FocusPaneRight => {}
            }
        }
        false
    }

    fn try_into_stored_filter(&self) -> Option<SavedFilter> {
        None
    }

    fn get_filter_highlight(&self) -> Option<FilterHighlight> {
        None
    }

    fn get_histogram_marker(&mut self) -> Option<HistogramMarker> {
        None
    }
}
