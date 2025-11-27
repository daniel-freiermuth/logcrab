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

use crate::input::InputAction;
use crate::parser::line::LogLine;
use crate::state::FilterState;
use crate::ui::tabs::LogCrabTab;
use chrono::DateTime;
use egui::Ui;
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
}

impl FilterView {
    pub fn new(name: String, index: usize) -> Self {
        Self { name, index }
    }
    /// Render a complete filter view
    ///
    /// Returns events that occurred during rendering
    #[allow(clippy::too_many_arguments)]
    pub fn render(
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
        let filter_bar_events = FilterBar::render(ui, filter, filter_index, &favorites);

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
}

impl LogCrabTab for FilterView {
    fn title(&mut self) -> egui::WidgetText {
        self.name.clone().into()
    }

    fn render(
        &mut self,
        ui: &mut egui::Ui,
        data_state: &mut crate::ui::LogView,
        global_config: &mut crate::config::GlobalConfig,
    ) {
        if let Some(custom_name) = data_state.get_filter_name(self.index) {
            self.name = custom_name.clone();
        }

        data_state.render_filter(ui, self.index, global_config);
    }

    fn process_events(
        &mut self,
        actions: &Vec<crate::input::InputAction>,
        data_state: &mut crate::ui::LogView,
    ) {
        for action in actions {
            match action {
                InputAction::MoveSelection(delta) => {
                    data_state.move_selection_in_filter(self.index, *delta);
                }
                InputAction::ToggleBookmark => {
                    data_state.toggle_bookmark_for_selected();
                }
                InputAction::JumpToTop => {
                    data_state.jump_to_top_in_filter(self.index);
                }
                InputAction::JumpToBottom => {
                    data_state.jump_to_bottom_in_filter(self.index);
                }
                InputAction::PageUp => {
                    data_state.page_up_in_filter(self.index);
                }
                InputAction::PageDown => {
                    data_state.page_down_in_filter(self.index);
                }
                InputAction::FocusSearch(_idx) => {}
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
