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

pub mod filter_bar;
pub mod histogram;
pub mod log_table;

use egui_dock::tab_viewer::OnCloseResponse;
pub use filter_bar::{FavoriteFilter, FilterBar, FilterInternalEvent};
pub use histogram::Histogram;
pub use log_table::{LogTable, LogTableEvent};

use crate::config::GlobalConfig;
use crate::input::InputAction;
use crate::parser::line::LogLine;
use crate::state::FilterState;
use crate::ui::tabs::LogCrabTab;
use crate::ui::windows::ChangeFilternameWindow;
use crate::ui::LogView;
use chrono::DateTime;
use egui::{Color32, Ui};
use std::collections::HashMap;
use std::sync::Arc;

/// Events that can be emitted by the filter view
#[derive(Debug, Clone)]
pub enum FilterViewEvent {
    LineSelected {
        line_index: usize,
        timestamp: Option<DateTime<chrono::Local>>,
    },
    BookmarkToggled {
        line_index: usize,
    },
    FilterNameEditRequested,
    FavoriteToggled,
    FilterModified, // Filter search text or case sensitivity changed
}

/// Orchestrates the filter view UI using reusable components
pub struct FilterView {
    name: String,
    index: usize,
    should_focus_search: bool,
}

impl FilterView {
    pub fn new(name: String, index: usize) -> Self {
        Self {
            name,
            index,
            should_focus_search: false,
        }
    }

    pub fn focus_search_next_frame(&mut self) {
        self.should_focus_search = true;
    }
    /// Render a complete filter view
    ///
    /// Returns events that occurred during rendering
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        ui: &mut Ui,
        lines: &Arc<Vec<LogLine>>,
        filter: &mut FilterState,
        filter_index: usize,
        all_filters: &[FilterState],
        selected_line_index: Option<usize>,
        selected_timestamp: Option<DateTime<chrono::Local>>,
        bookmarked_lines: &HashMap<usize, String>,
        min_score_filter: f64,
    ) -> Vec<FilterViewEvent> {
        let mut events = Vec::new();

        // Collect favorite filters from all filters
        let favorites: Vec<FavoriteFilter> = all_filters
            .iter()
            .filter(|f| f.is_favorite && !f.search_text.is_empty())
            .map(|f| FavoriteFilter {
                search_text: f.search_text.clone(),
                case_insensitive: f.case_insensitive,
            })
            .collect();

        // Render filter bar
        let filter_bar_events = FilterBar::render(
            ui,
            filter,
            filter_index,
            &favorites,
            self.should_focus_search,
        );
        self.should_focus_search = false;

        // Handle filter bar events
        for event in filter_bar_events {
            match event {
                FilterInternalEvent::SearchChanged => {
                    filter.update_search_regex();
                    filter.request_filter_update(Arc::clone(lines), min_score_filter);
                    events.push(FilterViewEvent::FilterModified);
                }
                FilterInternalEvent::CaseInsensitiveToggled => {
                    filter.update_search_regex();
                    filter.request_filter_update(Arc::clone(lines), min_score_filter);
                    events.push(FilterViewEvent::FilterModified);
                }
                FilterInternalEvent::FavoriteSelected {
                    search_text,
                    case_insensitive,
                } => {
                    filter.search_text = search_text;
                    filter.case_insensitive = case_insensitive;
                    filter.update_search_regex();
                    filter.request_filter_update(Arc::clone(lines), min_score_filter);
                    events.push(FilterViewEvent::FilterModified);
                }
                FilterInternalEvent::FilterNameEditRequested => {
                    events.push(FilterViewEvent::FilterNameEditRequested)
                }
                FilterInternalEvent::FavoriteToggled => {
                    events.push(FilterViewEvent::FavoriteToggled)
                }
            }
        }

        ui.separator();

        // Check for completed filter results from background thread
        filter.check_filter_results();

        // Rebuild filtered indices synchronously ONLY if:
        // - filter_dirty is true (needs filtering)
        // - AND we're NOT currently waiting for a background result
        let mut scroll_to_row = if filter.filter_dirty && !filter.is_filtering {
            filter.rebuild_filtered_indices(
                lines,
                min_score_filter,
                selected_line_index,
                selected_timestamp,
            )
        } else {
            None
        };

        // Check if selection changed
        if scroll_to_row.is_none()
            && selected_line_index.is_some()
            && filter.last_rendered_selection != selected_line_index
        {
            if let Some(selected_idx) = selected_line_index {
                if let Some(position) = filter
                    .filtered_indices
                    .iter()
                    .position(|&idx| idx == selected_idx)
                {
                    scroll_to_row = Some(position);
                } else {
                    // Line not in filtered results - try to find closest by timestamp
                    if let Some(selected_ts) = selected_timestamp {
                        if let Some(closest_pos) =
                            filter.find_closest_timestamp_index(lines, selected_ts)
                        {
                            scroll_to_row = Some(closest_pos);
                        }
                    }
                }
                // Mark as processed so we don't keep checking on every render
                filter.last_rendered_selection = selected_line_index;
            }
        }

        // Render histogram
        if let Some(hist_event) =
            Histogram::render(ui, lines, &filter.filtered_indices, selected_line_index)
        {
            events.push(FilterViewEvent::LineSelected {
                line_index: hist_event.line_index,
                timestamp: hist_event.timestamp,
            });
        }

        ui.separator();

        // Render log table
        let table_events = LogTable::render(
            ui,
            lines,
            filter,
            filter_index,
            selected_line_index,
            bookmarked_lines,
            scroll_to_row,
        );

        // Handle table events
        for event in table_events {
            match event {
                LogTableEvent::LineClicked {
                    line_index,
                    timestamp,
                } => {
                    events.push(FilterViewEvent::LineSelected {
                        line_index,
                        timestamp,
                    });
                }
                LogTableEvent::BookmarkToggled { line_index } => {
                    events.push(FilterViewEvent::BookmarkToggled { line_index });
                }
            }
        }

        events
    }

    /// Render a specific filter view
    pub fn render_filter(
        &mut self,
        ui: &mut Ui,
        filter_index: usize,
        data_state: &mut LogView,
        global_config: &mut GlobalConfig,
    ) {
        if filter_index >= data_state.filters.len() {
            ui.label("Invalid filter index");
            return;
        }

        // Convert bookmarks HashMap to simple HashMap<usize, String> for the component
        let bookmarked_lines: HashMap<usize, String> = data_state
            .bookmarks
            .iter()
            .map(|(&idx, bookmark)| (idx, bookmark.name.clone()))
            .collect();

        // Get current filter's search text for favorite checking
        let current_search = data_state.filters[filter_index].search_text.clone();
        let current_case_insensitive = data_state.filters[filter_index].case_insensitive;

        // Check if current filter matches any global favorite
        let is_favorite = global_config.favorite_filters.iter().any(|f| {
            f.search_text == current_search && f.case_insensitive == current_case_insensitive
        });

        // Update the filter's favorite status (for UI display only, not saved to .crab)
        data_state.filters[filter_index].is_favorite = is_favorite;

        // Temporarily take out the filter we're rendering
        // TODO
        let mut current_filter = std::mem::replace(
            &mut data_state.filters[filter_index],
            FilterState::new(Color32::YELLOW),
        );

        // Use global favorites instead of per-file favorites
        let temp_filters: Vec<FilterState> = global_config
            .favorite_filters
            .iter()
            .map(|fav| {
                let mut f = FilterState::new(Color32::YELLOW);
                f.search_text = fav.search_text.clone();
                f.case_insensitive = fav.case_insensitive;
                f.is_favorite = true;
                f
            })
            .collect();

        // Render using FilterView
        let events = self.render(
            ui,
            &data_state.lines,
            &mut current_filter,
            filter_index,
            &temp_filters,
            data_state.selected_line_index,
            data_state.selected_timestamp,
            &bookmarked_lines,
            data_state.min_score_filter,
        );

        // Put the filter back
        data_state.filters[filter_index] = current_filter;

        // Handle events
        for event in events {
            match event {
                FilterViewEvent::LineSelected {
                    line_index,
                    timestamp,
                } => {
                    data_state.selected_line_index = Some(line_index);
                    data_state.selected_timestamp = timestamp;
                }
                FilterViewEvent::BookmarkToggled { line_index } => {
                    data_state.toggle_bookmark(line_index);
                }
                FilterViewEvent::FilterNameEditRequested => {
                    // Prompt for new name
                    let starting_name =
                        if let Some(current_name) = &data_state.filters[filter_index].name {
                            current_name.clone()
                        } else {
                            format!("Filter {}", filter_index + 1)
                        };
                    data_state.change_filtername_window =
                        Some(ChangeFilternameWindow::new(starting_name));
                }
                FilterViewEvent::FavoriteToggled => {
                    let search_text = data_state.filters[filter_index].search_text.clone();
                    let case_insensitive = data_state.filters[filter_index].case_insensitive;

                    // Check if this filter is already a favorite
                    if let Some(pos) = global_config.favorite_filters.iter().position(|f| {
                        f.search_text == search_text && f.case_insensitive == case_insensitive
                    }) {
                        // Remove from favorites
                        global_config.favorite_filters.remove(pos);
                        data_state.filters[filter_index].is_favorite = false;
                        log::info!("Removed favorite: '{}'", search_text);
                    } else {
                        // Add to favorites
                        let name = data_state.filters[filter_index]
                            .name
                            .clone()
                            .unwrap_or_else(|| search_text.clone());
                        global_config
                            .favorite_filters
                            .push(crate::config::FavoriteFilter {
                                name,
                                search_text,
                                case_insensitive,
                            });
                        data_state.filters[filter_index].is_favorite = true;
                        log::info!(
                            "Added favorite: '{}'",
                            data_state.filters[filter_index].search_text
                        );
                    }

                    // Save global config
                    let _ = global_config.save();
                }
                FilterViewEvent::FilterModified => {
                    // Filter search text or case sensitivity changed, save to .crab file
                    data_state.save_crab_file();
                }
            }
        }

        // Handle filter name editing dialog
        if let Some(ref mut window) = data_state.change_filtername_window {
            match window.render(ui) {
                Ok(Some(new_name)) => {
                    data_state.set_filter_name(filter_index, Some(new_name));
                    data_state.change_filtername_window = None;
                }
                Ok(None) => {
                    // Still editing
                }
                Err(_) => {
                    // Cancelled
                    data_state.change_filtername_window = None;
                }
            }
        }
    }

    /// Move selection within a filtered view (only through matched indices)
    pub fn move_selection_in_filter(&mut self, delta: i32, data_state: &mut LogView) {
        let filter = &data_state.filters[self.index];
        if filter.filtered_indices.is_empty() {
            return;
        }

        // Determine current position within filtered list
        // TODO: start from current selected line even if not in filtered list
        let current_pos = if let Some(sel) = data_state.selected_line_index {
            filter
                .filtered_indices
                .iter()
                .position(|&idx| idx == sel)
                .unwrap_or({
                    // Fallback: choose nearest by timestamp if available later improvements; for now start at beginning
                    0
                })
        } else if delta >= 0 {
            0
        } else {
            filter.filtered_indices.len() - 1
        };

        let new_pos = if delta < 0 {
            current_pos.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (current_pos + delta as usize).min(filter.filtered_indices.len() - 1)
        };

        let new_line_index = filter.filtered_indices[new_pos];
        data_state.selected_line_index = Some(new_line_index);
        data_state.selected_timestamp = data_state.lines[new_line_index].timestamp;
    }

    /// Jump to the first line in a filtered view (Vim-style gg)
    pub fn jump_to_top_in_filter(&mut self, data_state: &mut LogView) {
        let filter = &data_state.filters[self.index];
        if filter.filtered_indices.is_empty() {
            return;
        }

        let first_line_index = filter.filtered_indices[0];
        data_state.selected_line_index = Some(first_line_index);
        data_state.selected_timestamp = data_state.lines[first_line_index].timestamp;
    }

    /// Jump to the last line in a filtered view (Vim-style G)
    pub fn jump_to_bottom_in_filter(&mut self, data_state: &mut LogView) {
        let filter = &data_state.filters[self.index];
        if filter.filtered_indices.is_empty() {
            return;
        }

        let last_pos = filter.filtered_indices.len() - 1;
        let last_line_index = filter.filtered_indices[last_pos];
        data_state.selected_line_index = Some(last_line_index);
        data_state.selected_timestamp = data_state.lines[last_line_index].timestamp;
    }

    /// Move selection up by one page in a filtered view
    pub fn page_up_in_filter(&mut self, data_state: &mut LogView) {
        // A page is approximately 20-30 lines in typical terminal views
        const PAGE_SIZE: i32 = 25;
        self.move_selection_in_filter(-PAGE_SIZE, data_state);
    }

    /// Move selection down by one page in a filtered view
    pub fn page_down_in_filter(&mut self, data_state: &mut LogView) {
        const PAGE_SIZE: i32 = 25;
        self.move_selection_in_filter(PAGE_SIZE, data_state);
    }
}

impl LogCrabTab for FilterView {
    fn title(&mut self) -> egui::WidgetText {
        self.name.clone().into()
    }

    fn render(
        &mut self,
        ui: &mut egui::Ui,
        data_state: &mut LogView,
        global_config: &mut GlobalConfig,
    ) {
        if let Some(custom_name) = data_state.get_filter_name(self.index) {
            self.name = custom_name.clone();
        }

        self.render_filter(ui, self.index, data_state, global_config);
    }

    fn process_events(&mut self, actions: &[InputAction], data_state: &mut LogView) {
        for action in actions {
            match action {
                InputAction::MoveSelection(delta) => {
                    self.move_selection_in_filter(*delta, data_state);
                }
                InputAction::ToggleBookmark => {
                    data_state.toggle_bookmark_for_selected();
                }
                InputAction::JumpToTop => {
                    self.jump_to_top_in_filter(data_state);
                }
                InputAction::JumpToBottom => {
                    self.jump_to_bottom_in_filter(data_state);
                }
                InputAction::PageUp => {
                    self.page_up_in_filter(data_state);
                }
                InputAction::PageDown => {
                    self.page_down_in_filter(data_state);
                }
                InputAction::FocusSearch(_idx) => {
                    self.focus_search_next_frame();
                }
                InputAction::NewFilterTab => {}
                InputAction::NewBookmarksTab => {}
                InputAction::CloseTab => {}
                InputAction::CycleTab => {}
                InputAction::ReverseCycleTab => {}
                InputAction::OpenFile => {}
                InputAction::NavigatePane(_direction) => {}
                InputAction::RenameFilter(_idx) => {}
            }
        }
    }

    fn on_close(&mut self, filter_to_remove: &mut Option<usize>) -> OnCloseResponse {
        // When closing a filter tab, mark it for removal
        // We can't remove it here because we need to update all other tabs' indices
        *filter_to_remove = Some(self.index);
        OnCloseResponse::Close
    }
    fn filter_got_removed(&mut self, filter_index: usize) {
        // If a filter was removed, and its index is less than this filter's index,
        // we need to decrement our index to stay in sync
        if filter_index < self.index {
            self.index -= 1;
        }
    }
    fn get_filter_index(&self) -> Option<usize> {
        Some(self.index)
    }
}
