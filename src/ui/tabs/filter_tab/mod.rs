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
pub mod filter_state;
pub mod histogram;
pub mod log_table;

pub use filter_bar::{FavoriteFilter, FilterBar, FilterInternalEvent};
pub use histogram::Histogram;
pub use log_table::{LogTable, LogTableEvent};

use crate::config::GlobalConfig;
use crate::input::ShortcutAction;
use crate::parser::line::LogLine;
use crate::ui::log_view::{LogViewState, SavedFilter};
use crate::ui::tabs::filter_tab::filter_state::FilterState;
use crate::ui::tabs::LogCrabTab;
use crate::ui::windows::ChangeFilternameWindow;
use egui::{Color32, Ui};
use std::collections::HashMap;
use std::sync::Arc;

/// Events that can be emitted by the filter view
#[derive(Debug, Clone)]
pub enum FilterViewEvent {
    LineSelected { line_index: usize },
    BookmarkToggled { line_index: usize },
    FilterNameEditRequested,
    FavoriteToggled,
    FilterModified, // Filter search text or case sensitivity changed
}

/// Orchestrates the filter view UI using reusable components
pub struct FilterView {
    uuid: usize,
    should_focus_search: bool,
    state: FilterState,
    change_filtername_window: Option<ChangeFilternameWindow>,
}

impl FilterView {
    pub fn new(uuid: usize, state: FilterState) -> Self {
        Self {
            uuid,
            should_focus_search: false,
            state,
            change_filtername_window: None,
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
        all_filters: &[FilterState],
        selected_line_index: Option<usize>,
        bookmarked_lines: &HashMap<usize, String>,
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
            &mut self.state,
            self.uuid,
            &favorites,
            self.should_focus_search,
        );
        self.should_focus_search = false;

        // Handle filter bar events
        for event in filter_bar_events {
            match event {
                FilterInternalEvent::SearchChanged => {
                    self.state.update_search_regex();
                    self.state.request_filter_update(Arc::clone(lines));
                    events.push(FilterViewEvent::FilterModified);
                }
                FilterInternalEvent::CaseInsensitiveToggled => {
                    self.state.update_search_regex();
                    self.state.request_filter_update(Arc::clone(lines));
                    events.push(FilterViewEvent::FilterModified);
                }
                FilterInternalEvent::FavoriteSelected {
                    search_text,
                    case_insensitive,
                } => {
                    self.state.search_text = search_text;
                    self.state.case_insensitive = case_insensitive;
                    self.state.update_search_regex();
                    self.state.request_filter_update(Arc::clone(lines));
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
        self.state.check_filter_results();

        // Rebuild filtered indices synchronously ONLY if:
        // - filter_dirty is true (needs filtering)
        // - AND we're NOT currently waiting for a background result
        let mut scroll_to_row = if self.state.filter_dirty && !self.state.is_filtering {
            self.state
                .rebuild_filtered_indices(lines, selected_line_index)
        } else {
            None
        };

        // Check if selection changed
        if scroll_to_row.is_none()
            && selected_line_index.is_some()
            && self.state.last_rendered_selection != selected_line_index
        {
            if let Some(selected_idx) = selected_line_index {
                if let Some(position) = self
                    .state
                    .filtered_indices
                    .iter()
                    .position(|&idx| idx == selected_idx)
                {
                    scroll_to_row = Some(position);
                } else {
                    // Line not in filtered results - try to find closest by timestamp
                    if let Some(closest_pos) = self.state.find_closest_timestamp_index(selected_idx)
                    {
                        scroll_to_row = Some(closest_pos);
                    }
                }
                // Mark as processed so we don't keep checking on every render
                self.state.last_rendered_selection = selected_line_index;
            }
        }

        // Render histogram
        if let Some(hist_event) =
            Histogram::render(ui, lines, &self.state.filtered_indices, selected_line_index)
        {
            events.push(FilterViewEvent::LineSelected {
                line_index: hist_event.line_index,
            });
        }

        ui.separator();

        // Render log table
        let table_events = LogTable::render(
            ui,
            lines,
            &self.state,
            self.uuid,
            selected_line_index,
            bookmarked_lines,
            scroll_to_row,
        );

        // Handle table events
        for event in table_events {
            match event {
                LogTableEvent::LineClicked { line_index } => {
                    events.push(FilterViewEvent::LineSelected { line_index });
                }
                LogTableEvent::BookmarkToggled { line_index } => {
                    events.push(FilterViewEvent::BookmarkToggled { line_index });
                }
            }
        }

        events
    }

    /// Render a specific filter view
    fn render_filter(
        &mut self,
        ui: &mut Ui,
        data_state: &mut LogViewState,
        global_config: &mut GlobalConfig,
    ) -> bool {
        // Convert bookmarks HashMap to simple HashMap<usize, String> for the component
        let bookmarked_lines: HashMap<usize, String> = data_state
            .bookmarks
            .iter()
            .map(|(&idx, bookmark)| (idx, bookmark.name.clone()))
            .collect();

        // Get current filter's search text for favorite checking
        let current_search = self.state.search_text.clone();
        let current_case_insensitive = self.state.case_insensitive;

        // Check if current filter matches any global favorite
        let is_favorite = global_config.favorite_filters.iter().any(|f| {
            f.search_text == current_search && f.case_insensitive == current_case_insensitive
        });

        // Update the filter's favorite status (for UI display only, not saved to .crab)
        self.state.is_favorite = is_favorite;

        // Use global favorites instead of per-file favorites
        let temp_filters: Vec<FilterState> = global_config
            .favorite_filters
            .iter()
            .map(|fav| {
                let mut f = FilterState::new(self.state.name.clone(), Color32::YELLOW);
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
            &temp_filters,
            data_state.selected_line_index,
            &bookmarked_lines,
        );

        // Handle events
        let mut should_save = false;
        for event in events {
            match event {
                FilterViewEvent::LineSelected { line_index } => {
                    data_state.selected_line_index = Some(line_index);
                }
                FilterViewEvent::BookmarkToggled { line_index } => {
                    data_state.toggle_bookmark(line_index);
                    should_save = true;
                }
                FilterViewEvent::FilterNameEditRequested => {
                    // Prompt for new name
                    self.change_filtername_window =
                        Some(ChangeFilternameWindow::new(self.state.name.clone()));
                }
                FilterViewEvent::FavoriteToggled => {
                    let search_text = self.state.search_text.clone();
                    let case_insensitive = self.state.case_insensitive;

                    // Check if this filter is already a favorite
                    if let Some(pos) = global_config.favorite_filters.iter().position(|f| {
                        f.search_text == search_text && f.case_insensitive == case_insensitive
                    }) {
                        // Remove from favorites
                        global_config.favorite_filters.remove(pos);
                        self.state.is_favorite = false;
                        log::info!("Removed favorite: '{}'", search_text);
                    } else {
                        // Add to favorites
                        let name = self.state.name.clone();
                        global_config
                            .favorite_filters
                            .push(crate::config::FavoriteFilter {
                                name,
                                search_text,
                                case_insensitive,
                            });
                        self.state.is_favorite = true;
                        log::info!("Added favorite: '{}'", self.state.search_text);
                    }

                    // Save global config
                    let _ = global_config.save();
                }
                FilterViewEvent::FilterModified => {
                    // Filter search text or case sensitivity changed, save to .crab file
                    should_save = true;
                }
            }
        }

        // Handle filter name editing dialog
        if let Some(ref mut window) = self.change_filtername_window {
            match window.render(ui) {
                Ok(Some(new_name)) => {
                    self.state.name = new_name;
                    self.change_filtername_window = None;
                    should_save = true;
                }
                Ok(None) => {
                    // Still editing
                }
                Err(_) => {
                    // Cancelled
                    self.change_filtername_window = None;
                }
            }
        }
        should_save
    }

    /// Move selection within a filtered view (only through matched indices)
    pub fn move_selection_in_filter(&mut self, delta: i32, data_state: &mut LogViewState) {
        let filter = &self.state;
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
    }

    /// Jump to the first line in a filtered view (Vim-style gg)
    pub fn jump_to_top_in_filter(&mut self, data_state: &mut LogViewState) {
        let filter = &self.state;
        if filter.filtered_indices.is_empty() {
            return;
        }

        let first_line_index = filter.filtered_indices[0];
        data_state.selected_line_index = Some(first_line_index);
    }

    /// Jump to the last line in a filtered view (Vim-style G)
    pub fn jump_to_bottom_in_filter(&mut self, data_state: &mut LogViewState) {
        let filter = &self.state;
        if filter.filtered_indices.is_empty() {
            return;
        }

        let last_pos = filter.filtered_indices.len() - 1;
        let last_line_index = filter.filtered_indices[last_pos];
        data_state.selected_line_index = Some(last_line_index);
    }

    /// Move selection up by one page in a filtered view
    pub fn page_up_in_filter(&mut self, data_state: &mut LogViewState) {
        // A page is approximately 20-30 lines in typical terminal views
        const PAGE_SIZE: i32 = 25;
        self.move_selection_in_filter(-PAGE_SIZE, data_state);
    }

    /// Move selection down by one page in a filtered view
    pub fn page_down_in_filter(&mut self, data_state: &mut LogViewState) {
        const PAGE_SIZE: i32 = 25;
        self.move_selection_in_filter(PAGE_SIZE, data_state);
    }
}

impl LogCrabTab for FilterView {
    fn title(&mut self) -> egui::WidgetText {
        self.state.name.clone().into()
    }

    fn render(
        &mut self,
        ui: &mut egui::Ui,
        data_state: &mut LogViewState,
        global_config: &mut GlobalConfig,
    ) -> bool {
        self.render_filter(ui, data_state, global_config)
    }

    fn process_events(
        &mut self,
        actions: &[ShortcutAction],
        data_state: &mut LogViewState,
    ) -> bool {
        let mut should_save = false;
        for action in actions {
            match action {
                ShortcutAction::MoveDown => {
                    self.move_selection_in_filter(1, data_state);
                }
                ShortcutAction::MoveUp => {
                    self.move_selection_in_filter(-1, data_state);
                }
                ShortcutAction::ToggleBookmark => {
                    data_state.toggle_bookmark_for_selected();
                    should_save = true;
                }
                ShortcutAction::JumpToTop => {
                    self.jump_to_top_in_filter(data_state);
                }
                ShortcutAction::JumpToBottom => {
                    self.jump_to_bottom_in_filter(data_state);
                }
                ShortcutAction::PageUp => {
                    self.page_up_in_filter(data_state);
                }
                ShortcutAction::PageDown => {
                    self.page_down_in_filter(data_state);
                }
                ShortcutAction::FocusSearch => {
                    self.focus_search_next_frame();
                }
                ShortcutAction::NewFilterTab => {}
                ShortcutAction::NewBookmarksTab => {}
                ShortcutAction::CloseTab => {}
                ShortcutAction::CycleTab => {}
                ShortcutAction::ReverseCycleTab => {}
                ShortcutAction::OpenFile => {}
                ShortcutAction::RenameFilter => {
                    self.change_filtername_window =
                        Some(ChangeFilternameWindow::new(self.state.name.clone()));
                }
                ShortcutAction::FocusPaneLeft => {}
                ShortcutAction::FocusPaneDown => {}
                ShortcutAction::FocusPaneUp => {}
                ShortcutAction::FocusPaneRight => {}
            }
        }
        should_save
    }

    fn request_filter_update(&mut self, lines: Arc<Vec<LogLine>>) {
        self.state.request_filter_update(lines);
    }

    fn try_into_stored_filter(&self) -> Option<SavedFilter> {
        Some((&self.state).into())
    }

    fn check_filter_results(&mut self) -> bool {
        self.state.check_filter_results();
        self.state.is_filtering
    }
}
