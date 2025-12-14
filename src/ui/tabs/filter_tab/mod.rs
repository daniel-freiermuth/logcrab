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

pub use filter_bar::{FilterBar, FilterInternalEvent};
pub use histogram::{Histogram, HistogramMarker};
pub use log_table::{LogTable, LogTableEvent};

use crate::config::GlobalConfig;
use crate::input::ShortcutAction;
use crate::ui::log_view::{FilterHighlight, LogViewState, SavedFilter};
use crate::ui::tabs::filter_tab::filter_state::FilterState;
use crate::ui::tabs::LogCrabTab;
use crate::ui::windows::ChangeFilternameWindow;
use egui::Ui;
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
    filter_bar: FilterBar,
}

impl FilterView {
    pub const fn new(uuid: usize, state: FilterState) -> Self {
        Self {
            uuid,
            should_focus_search: false,
            state,
            change_filtername_window: None,
            filter_bar: FilterBar::new(),
        }
    }

    pub const fn focus_search_next_frame(&mut self) {
        self.should_focus_search = true;
    }
    /// Render a complete filter view
    ///
    /// Returns events that occurred during rendering
    pub fn render(
        &mut self,
        ui: &mut Ui,
        log_view_state: &mut LogViewState,
        global_config: &mut GlobalConfig,
        bookmarked_lines: &HashMap<usize, String>,
        all_filter_highlights: &[FilterHighlight],
        histogram_markers: &[HistogramMarker],
    ) -> Vec<FilterViewEvent> {
        profiling::scope!("FilterView::render");

        let selected_line_index = log_view_state.selected_line_index;
        let mut events = Vec::new();

        // Render filter bar
        let filter_bar_events = self.filter_bar.render(
            ui,
            &mut self.state,
            self.uuid,
            global_config,
            self.should_focus_search,
            log_view_state,
        );
        self.should_focus_search = false;

        let store = &log_view_state.store;
        // Handle filter bar events
        for event in filter_bar_events {
            match event {
                FilterInternalEvent::SearchChanged
                | FilterInternalEvent::CaseInsensitiveToggled => {
                    self.state.request_filter_update(Arc::clone(store));
                    events.push(FilterViewEvent::FilterModified);
                }
                FilterInternalEvent::FavoriteSelected {
                    search_text,
                    case_sensitive,
                } => {
                    self.state.search_text = search_text;
                    self.state.case_sensitive = case_sensitive;
                    self.state.request_filter_update(Arc::clone(store));
                    events.push(FilterViewEvent::FilterModified);
                }
                FilterInternalEvent::FilterNameEditRequested => {
                    events.push(FilterViewEvent::FilterNameEditRequested);
                }
                FilterInternalEvent::FavoriteToggled => {
                    events.push(FilterViewEvent::FavoriteToggled);
                }
            }
        }

        // Check if we need to recompute based on version (Step 9)
        if self.state.cached_for_version != store.version() {
            log::trace!("Filter cache invalid for version {}", store.version());
            self.state.request_filter_update(Arc::clone(store));
        }

        ui.separator();

        // Check for completed filter results from background thread
        let needs_scroll = self.state.check_filter_results()
            || self.state.last_rendered_selection != selected_line_index;
        let scroll_to_row = if needs_scroll {
            self.state.last_rendered_selection = selected_line_index;
            // Mark as processed so we don't keep checking on every render
            Some(self.state.find_closest_timestamp_index(selected_line_index))
        } else {
            None
        };

        // Render histogram
        if let Some(hist_event) = Histogram::render(
            ui,
            store,
            &self.state.filtered_indices,
            selected_line_index,
            global_config.hide_epoch_in_histogram,
            histogram_markers,
            &mut self.state.histogram_cache,
        ) {
            events.push(FilterViewEvent::LineSelected {
                line_index: hist_event.line_index,
            });
        }

        ui.separator();

        // Render log table
        let table_events = LogTable::render(
            ui,
            store,
            &self.state,
            self.uuid,
            selected_line_index,
            bookmarked_lines,
            scroll_to_row,
            all_filter_highlights,
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
        all_filter_highlights: &[FilterHighlight],
        histogram_markers: &[HistogramMarker],
    ) {
        // Convert bookmarks HashMap to simple HashMap<usize, String> for the component
        let bookmarked_lines: HashMap<usize, String> = data_state
            .bookmarks
            .iter()
            .map(|(&idx, bookmark)| (idx, bookmark.name.clone()))
            .collect();

        // Render using FilterView
        let events = self.render(
            ui,
            data_state,
            global_config,
            &bookmarked_lines,
            all_filter_highlights,
            histogram_markers,
        );

        // Handle events
        for event in events {
            match event {
                FilterViewEvent::LineSelected { line_index } => {
                    data_state.selected_line_index = line_index;
                }
                FilterViewEvent::BookmarkToggled { line_index } => {
                    data_state.toggle_bookmark(line_index);
                    data_state.modified = true;
                }
                FilterViewEvent::FilterNameEditRequested => {
                    // Prompt for new name
                    self.change_filtername_window =
                        Some(ChangeFilternameWindow::new(self.state.name.clone()));
                }
                FilterViewEvent::FavoriteToggled => {
                    let search_text = self.state.search_text.clone();
                    let case_sensitive = self.state.case_sensitive;

                    // Check if this filter is already a favorite
                    if let Some(pos) = global_config.favorite_filters.iter().position(|f| {
                        f.search_text == search_text && f.case_sensitive == case_sensitive
                    }) {
                        // Remove from favorites
                        global_config.favorite_filters.remove(pos);
                        log::info!("Removed favorite: '{search_text}'");
                    } else {
                        // Add to favorites
                        global_config
                            .favorite_filters
                            .push(crate::config::FavoriteFilter::new(
                                search_text,
                                case_sensitive,
                            ));
                        log::info!("Added favorite: '{}'", self.state.search_text);
                    }

                    // Save global config
                    let _ = global_config.save();
                }
                FilterViewEvent::FilterModified => {
                    // Filter search text or case sensitivity changed, save to .crab file
                    data_state.modified = true;
                }
            }
        }

        // Handle filter name editing dialog
        if let Some(ref mut window) = self.change_filtername_window {
            match window.render(ui) {
                Ok(Some(new_name)) => {
                    self.state.name = new_name;
                    self.change_filtername_window = None;
                    data_state.modified = true;
                }
                Ok(None) => {
                    // Still editing
                }
                Err(()) => {
                    // Cancelled
                    self.change_filtername_window = None;
                }
            }
        }
    }

    /// Move selection within a filtered view (only through matched indices)
    pub fn move_selection_in_filter(&mut self, delta: i32, data_state: &mut LogViewState) {
        let filter = &self.state;
        if filter.filtered_indices.is_empty() {
            return;
        }

        // Determine current position within filtered list
        let current_pos = self
            .state
            .find_closest_timestamp_index(data_state.selected_line_index);

        let new_pos = if delta < 0 {
            current_pos.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (current_pos + delta as usize).min(filter.filtered_indices.len().saturating_sub(1))
        };

        let new_line_index = filter.filtered_indices[new_pos];
        data_state.selected_line_index = new_line_index;
    }

    /// Jump to the first line in a filtered view (Vim-style gg)
    pub fn jump_to_top_in_filter(&mut self, data_state: &mut LogViewState) {
        let filter = &self.state;
        if let Some(&first_line_index) = filter.filtered_indices.first() {
            data_state.selected_line_index = first_line_index;
        }
    }

    /// Jump to the last line in a filtered view (Vim-style G)
    pub fn jump_to_bottom_in_filter(&mut self, data_state: &mut LogViewState) {
        let filter = &self.state;
        if let Some(&last_line_index) = filter.filtered_indices.last() {
            data_state.selected_line_index = last_line_index;
        }
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
        let mut layout_job = egui::text::LayoutJob::default();

        layout_job.append(
            "■ ",
            0.0,
            egui::TextFormat {
                color: self.state.color,
                ..Default::default()
            },
        );

        layout_job.append(&self.state.name, 0.0, egui::TextFormat::default());

        layout_job.into()
    }

    fn render(
        &mut self,
        ui: &mut egui::Ui,
        data_state: &mut LogViewState,
        global_config: &mut GlobalConfig,
        all_filter_highlights: &[FilterHighlight],
        histogram_markers: &[HistogramMarker],
    ) {
        // Create a new highlights list with this tab's filter at the front (for priority)
        // This ensures the current tab's filter is always visible and takes precedence
        let mut highlights_with_current = Vec::with_capacity(all_filter_highlights.len() + 1);

        // Add this tab's own filter first (if it has a valid regex)
        if let Ok(regex) = &self.state.search_regex {
            if !self.state.search_text.is_empty() {
                highlights_with_current.push(FilterHighlight {
                    regex: regex.clone(),
                    color: self.state.color,
                });
            }
        }

        // Add all other global filters (excluding this one to avoid duplicates)
        for highlight in all_filter_highlights {
            // Skip if this is the same filter (compare by checking if regex patterns match)
            if let Ok(our_regex) = &self.state.search_regex {
                if highlight.regex.as_str() != our_regex.as_str() {
                    highlights_with_current.push(highlight.clone());
                }
            } else {
                highlights_with_current.push(highlight.clone());
            }
        }

        self.render_filter(
            ui,
            data_state,
            global_config,
            &highlights_with_current,
            histogram_markers,
        );
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

    fn try_into_stored_filter(&self) -> Option<SavedFilter> {
        Some((&self.state).into())
    }

    fn get_filter_highlight(&self) -> Option<FilterHighlight> {
        self.state
            .search_regex
            .as_ref()
            .ok()
            .filter(|_| !self.state.search_text.is_empty() && self.state.globally_visible)
            .map(|regex| FilterHighlight {
                regex: regex.clone(),
                color: self.state.color,
            })
    }

    fn get_histogram_marker(&self) -> Option<HistogramMarker> {
        if !self.state.show_in_histogram || self.state.filtered_indices.is_empty() {
            return None;
        }
        Some(HistogramMarker {
            name: self.state.name.clone(),
            indices: self.state.filtered_indices.clone(),
            color: self.state.color,
        })
    }

    fn context_menu(&mut self, ui: &mut egui::Ui) {
        let icon = if self.state.globally_visible {
            "👁"
        } else {
            "🚫"
        };
        let text = if self.state.globally_visible {
            "Hide in other tabs"
        } else {
            "Show in other tabs"
        };

        if ui.button(format!("{icon} {text}")).clicked() {
            self.state.globally_visible = !self.state.globally_visible;
            ui.close();
        }
    }
}
