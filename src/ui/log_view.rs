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
use crate::config::GlobalConfig;
use crate::parser::line::LogLine;
use crate::ui::tabs::filter_tab::filter_state::FilterState;
use crate::ui::tabs::{FilterView, LogCrabTab, LogCrabTabViewer, PendingTabAdd};
use crate::ui::windows::ChangeFilternameWindow;
use egui::Color32;

use chrono::{DateTime, Local};
use egui_dock::{DockArea, DockState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

/// Named bookmark with optional description
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub line_index: usize,
    pub name: String,
    pub timestamp: Option<DateTime<Local>>,
}

/// Saved filter configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedFilter {
    search_text: String,
    case_insensitive: bool,
    name: String,
}

impl From<&SavedFilter> for FilterState {
    fn from(saved_filter: &SavedFilter) -> FilterState {
        let mut filter = FilterState::new(saved_filter.name.clone(), Color32::YELLOW);
        filter.search_text = saved_filter.search_text.clone();
        filter.case_insensitive = saved_filter.case_insensitive;
        filter.update_search_regex();
        filter
    }
}

impl From<&FilterState> for SavedFilter {
    fn from(filter: &FilterState) -> SavedFilter {
        SavedFilter {
            search_text: filter.search_text.clone(),
            case_insensitive: filter.case_insensitive,
            name: filter.name.clone(),
        }
    }
}

/// .crab file format - stores all session data
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrabFile {
    bookmarks: Vec<Bookmark>,
    filters: Vec<SavedFilter>,
}

pub struct LogView {
    // .crab file path
    crab_file: Option<PathBuf>,

    /// Dock state for VS Code-like tiling layout
    pub dock_state: DockState<Box<dyn LogCrabTab>>,

    monotonic_filter_counter: usize,
    pub state: LogViewState,
}

pub struct LogViewState {
    pub lines: Arc<Vec<LogLine>>,
    // Selected line tracking
    pub selected_line_index: Option<usize>,
    // Bookmarks with names
    pub bookmarks: HashMap<usize, Bookmark>,
    pub change_filtername_window: Option<ChangeFilternameWindow>,
}

impl LogView {
    pub fn new() -> Self {
        LogView {
            crab_file: None,
            dock_state: DockState::new(Vec::new()),
            monotonic_filter_counter: 0,
            state: LogViewState {
                lines: Arc::new(Vec::new()),
                selected_line_index: None,
                bookmarks: HashMap::new(),
                change_filtername_window: None,
            },
        }
    }

    pub fn add_filter_view(&mut self, focus_search: bool, state: Option<FilterState>) {
        let colors = [
            Color32::YELLOW,
            Color32::LIGHT_BLUE,
            Color32::LIGHT_GREEN,
            Color32::from_rgb(255, 200, 150), // Light orange
            Color32::from_rgb(255, 150, 255), // Light magenta
            Color32::from_rgb(150, 255, 255), // Light cyan
        ];
        let color = colors[self.monotonic_filter_counter % colors.len()];

        let state = state.unwrap_or_else(|| {
            FilterState::new(
                format!("Filter {}", self.monotonic_filter_counter + 1),
                color,
            )
        });
        let mut filter = Box::new(FilterView::new(self.monotonic_filter_counter, state));
        if focus_search {
            filter.focus_search_next_frame();
        }
        self.dock_state.push_to_focused_leaf(filter);
        self.save_crab_file();
        self.monotonic_filter_counter += 1;
    }

    /// Check if any filter is currently processing in the background
    /// Also checks for completed filter results to update status
    pub fn is_any_filter_active(&mut self) -> bool {
        let mut any_loading = false;
        for ((_, _), tab) in self.dock_state.iter_all_tabs_mut() {
            if tab.check_filter_results() {
                any_loading = true;
            }
        }
        any_loading
    }

    pub fn set_lines(&mut self, lines: Arc<Vec<LogLine>>) {
        log::info!(
            "Setting {} log lines, requesting background filtering",
            lines.len(),
        );
        self.state.lines = lines;
        // Request background filtering for all filters
        for ((_surface, _node), tab) in &mut self.dock_state.iter_all_tabs_mut() {
            // TODO This is actually just a redraw
            tab.request_filter_update(Arc::clone(&self.state.lines));
        }
    }

    // TODO
    pub fn set_bookmarks_file(&mut self, log_file_path: PathBuf) {
        let crab_path = log_file_path.with_extension("crab");
        self.crab_file = Some(crab_path.clone());
        self.load_crab_file();

        // Request filter updates for any newly loaded or restored filters
        // This ensures filters loaded from .crab file start background filtering immediately
        for ((_surface, _node), tab) in &mut self.dock_state.iter_all_tabs_mut() {
            tab.request_filter_update(Arc::clone(&self.state.lines));
        }
    }

    fn load_crab_file(&mut self) {
        self.state.bookmarks.clear();
        self.dock_state.retain_tabs(|_| false);

        if let Some(ref path) = self.crab_file {
            log::debug!("Loading .crab file: {:?}", path);
            if let Ok(file_content) = fs::read_to_string(path) {
                if let Ok(crab_data) = serde_json::from_str::<CrabFile>(&file_content) {
                    log::info!(
                        "Loaded .crab file with {} bookmarks, {} filters",
                        crab_data.bookmarks.len(),
                        crab_data.filters.len()
                    );

                    // Load bookmarks
                    for bookmark in crab_data.bookmarks {
                        self.state.bookmarks.insert(bookmark.line_index, bookmark);
                    }

                    for (i, saved_filter) in crab_data.filters.iter().enumerate() {
                        self.add_filter_view(false, Some(saved_filter.into()));
                        log::debug!("Restored filter {}: '{}'", i, saved_filter.search_text);
                    }
                } else {
                    log::warn!("Failed to parse .crab file: {:?}", path);
                    self.add_filter_view(false, None);
                }
            } else {
                log::info!(".crab file does not exist yet: {:?}", path);
                self.add_filter_view(false, None);
            }
        }
    }

    pub fn save_crab_file(&self) {
        if let Some(ref path) = self.crab_file {
            log::debug!("Saving .crab file: {:?}", path);
            let filters = self
                .dock_state
                .iter_all_tabs()
                .filter_map(|((_surface, _node), tab)| tab.try_into_stored_filter())
                .collect::<Vec<SavedFilter>>();
            let n_filters = filters.len();
            let crab_data = CrabFile {
                bookmarks: self.state.bookmarks.values().cloned().collect(),
                filters,
            };

            if let Ok(json) = serde_json::to_string_pretty(&crab_data) {
                match fs::write(path, json) {
                    Ok(_) => log::debug!(
                        "Successfully saved .crab file with {} bookmarks, {} filters",
                        self.state.bookmarks.len(),
                        n_filters,
                    ),
                    Err(e) => log::error!("Failed to save .crab file: {}", e),
                }
            }
        }
    }

    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        global_config: &mut GlobalConfig,
        pending_tab_add: &mut Option<PendingTabAdd>,
    ) {
        let mut should_save = false;
        // Use dock area for VS Code-like draggable/tiling layout
        DockArea::new(&mut self.dock_state)
            .show_add_buttons(true)
            .show_add_popup(true)
            .show_inside(
                ui,
                &mut LogCrabTabViewer {
                    log_view: &mut self.state,
                    global_config,
                    pending_tab_add,
                    should_save: &mut should_save,
                },
            );

        if should_save {
            self.save_crab_file();
        }
    }

    pub fn process_keyboard_input(&mut self, actions: &[crate::input::ShortcutAction]) {
        let focused_tab = self.dock_state.find_active_focused().map(|(_, tab)| tab);
        if let Some(focused_tab) = focused_tab {
            if focused_tab.process_events(actions, &mut self.state) {
                self.save_crab_file();
            }
        }
    }
}

impl LogViewState {
    pub fn toggle_bookmark(&mut self, line_index: usize) {
        if let std::collections::hash_map::Entry::Vacant(e) = self.bookmarks.entry(line_index) {
            let timestamp = if line_index < self.lines.len() {
                self.lines[line_index].timestamp
            } else {
                None
            };

            let bookmark_name = format!(
                "Line {}",
                if line_index < self.lines.len() {
                    self.lines[line_index].line_number.to_string()
                } else {
                    line_index.to_string()
                }
            );

            log::debug!("Adding bookmark: {}", bookmark_name);
            e.insert(Bookmark {
                line_index,
                name: bookmark_name,
                timestamp,
            });
        } else {
            log::debug!("Removing bookmark at line {}", line_index);
            self.bookmarks.remove(&line_index);
        }
    }

    /// Toggle bookmark for the currently selected line
    pub fn toggle_bookmark_for_selected(&mut self) {
        if let Some(selected_idx) = self.selected_line_index {
            self.toggle_bookmark(selected_idx);
        }
    }
}
