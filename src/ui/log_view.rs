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
use crate::input::ShortcutAction;
use crate::parser::line::LogLine;
use crate::ui::tabs::filter_tab::filter_state::FilterState;
use crate::ui::tabs::{
    navigation, BookmarksView, FilterView, LogCrabTab, LogCrabTabViewer, PendingTabAdd,
};
use crate::ui::PaneDirection;
use egui::text::LayoutJob;
use egui::{Color32, TextFormat};
use fancy_regex::Regex;

use chrono::{DateTime, Local};
use egui_dock::{DockArea, DockState, Node};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

/// A filter pattern with its associated color for highlighting
#[derive(Debug, Clone)]
pub struct FilterHighlight {
    pub regex: Regex,
    pub color: Color32,
}

impl FilterHighlight {
    /// Highlight matches from all filters in the text
    pub fn highlight_text_with_filters(
        text: &str,
        base_color: Color32,
        all_filter_highlights: &[FilterHighlight],
    ) -> egui::text::LayoutJob {
        let mut job = LayoutJob::default();

        // Collect all matches from all filters with their colors
        // Use BTreeMap to keep matches sorted by start position
        let mut matches: BTreeMap<usize, (usize, Color32)> = BTreeMap::new();

        for highlight in all_filter_highlights {
            for mat in highlight.regex.find_iter(text).flatten() {
                // If there's overlap, the first filter wins (first in the tab order)
                matches
                    .entry(mat.start())
                    .or_insert((mat.end(), highlight.color));
            }
        }

        // Build the job with highlighted sections
        let mut last_end = 0;
        for (&start, &(end, color)) in &matches {
            // Skip overlapping matches
            if start < last_end {
                continue;
            }

            // Add unhighlighted text before this match
            if start > last_end {
                job.append(
                    &text[last_end..start],
                    0.0,
                    TextFormat {
                        color: base_color,
                        ..Default::default()
                    },
                );
            }

            // Add highlighted match
            job.append(
                &text[start..end],
                0.0,
                TextFormat {
                    color: Color32::BLACK,
                    background: color,
                    ..Default::default()
                },
            );

            last_end = end;
        }

        // Add remaining unhighlighted text
        if last_end < text.len() {
            job.append(
                &text[last_end..],
                0.0,
                TextFormat {
                    color: base_color,
                    ..Default::default()
                },
            );
        }

        job
    }
}

/// Named bookmark with optional description
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub line_index: usize,
    pub name: String,
    pub timestamp: DateTime<Local>,
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
        let mut filter = FilterState::new(saved_filter.name.clone());
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

/// Main log analyse view
///   Exists per opened file
/// Responsibilities:
/// - Managing view and tabs
/// - Loading/saving .crab session file
/// - Keeping state about global selection, filters, bookmarks
pub struct LogView {
    // .crab file path
    pub crab_file: PathBuf,

    /// Dock state for VS Code-like tiling layout
    pub dock_state: DockState<Box<dyn LogCrabTab>>,

    monotonic_filter_counter: usize,
    pub state: LogViewState,

    /// Pending tab add request (set by add button callback)
    pending_tab_add: Option<PendingTabAdd>,
}

pub struct LogViewState {
    pub lines: Arc<Vec<LogLine>>,
    pub scores: Option<Vec<f64>>,
    // Selected line tracking
    pub selected_line_index: usize,
    // Bookmarks with names
    pub bookmarks: HashMap<usize, Bookmark>,
    pub modified: bool,
    last_saved: Option<DateTime<Local>>,
}

impl LogView {
    pub fn new(lines: Arc<Vec<LogLine>>, crab_file: PathBuf) -> Self {
        assert!(!lines.is_empty(), "LogView requires at least one log line");
        let mut view = LogView {
            crab_file,
            dock_state: DockState::new(Vec::new()),
            monotonic_filter_counter: 0,
            pending_tab_add: None,
            state: LogViewState {
                lines,
                scores: None,
                selected_line_index: 0,
                bookmarks: HashMap::new(),
                modified: false,
                last_saved: None,
            },
        };
        view.load_crab_file();
        view
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
            FilterState::new(format!("Filter {}", self.monotonic_filter_counter + 1))
        });
        let mut filter = Box::new(FilterView::new(self.monotonic_filter_counter, color, state));
        if focus_search {
            filter.focus_search_next_frame();
        }
        filter.request_filter_update(self.state.lines.clone());
        self.dock_state.push_to_focused_leaf(filter);
        self.monotonic_filter_counter += 1;
    }

    fn load_crab_file(&mut self) {
        log::debug!("Loading .crab file: {:?}", self.crab_file);
        if let Ok(file_content) = fs::read_to_string(&self.crab_file) {
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
                log::warn!("Failed to parse .crab file: {:?}", self.crab_file);
                self.add_filter_view(false, None);
            }
        } else {
            log::info!(".crab file does not exist yet: {:?}", self.crab_file);
            self.add_filter_view(false, None);
        }
    }

    pub fn save_crab_file(&self) {
        log::debug!("Saving .crab file: {:?}", self.crab_file);
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
            match fs::write(&self.crab_file, json) {
                Ok(_) => log::debug!(
                    "Successfully saved .crab file with {} bookmarks, {} filters",
                    self.state.bookmarks.len(),
                    n_filters,
                ),
                Err(e) => log::error!("Failed to save .crab file: {}", e),
            }
        }
    }

    pub fn render(&mut self, ui: &mut egui::Ui, global_config: &mut GlobalConfig) {
        // Collect all filter highlights from all tabs
        let all_filter_highlights: Vec<FilterHighlight> = self
            .dock_state
            .iter_all_tabs()
            .filter_map(|((_surface, _node), tab)| tab.get_filter_highlight())
            .collect();

        // Use dock area for VS Code-like draggable/tiling layout
        DockArea::new(&mut self.dock_state)
            .show_add_buttons(true)
            .show_add_popup(true)
            .show_inside(
                ui,
                &mut LogCrabTabViewer {
                    log_view: &mut self.state,
                    global_config,
                    pending_tab_add: &mut self.pending_tab_add,
                    all_filter_highlights: &all_filter_highlights,
                },
            );
        if self.state.modified
            && self
                .state
                .last_saved
                .is_none_or(|t| (Local::now() - t).num_seconds() >= 5)
        {
            self.save_crab_file();
            self.state.modified = false;
            self.state.last_saved = Some(Local::now());
        }

        // Handle tab addition from add button popup (must be done after DockArea)
        if let Some(tab_type) = self.pending_tab_add.take() {
            match tab_type {
                PendingTabAdd::Filter => {
                    self.add_filter_view(false, None);
                }
                PendingTabAdd::Bookmarks => {
                    self.dock_state
                        .push_to_focused_leaf(Box::new(BookmarksView::default()));
                }
            }
        }
    }

    pub fn process_keyboard_input(&mut self, actions: &[ShortcutAction]) {
        // Execute all generated actions
        for action in actions {
            match action {
                ShortcutAction::ToggleBookmark => {}
                ShortcutAction::FocusSearch => {}
                ShortcutAction::NewFilterTab => {
                    self.add_filter_view(true, None);
                }
                ShortcutAction::NewBookmarksTab => {
                    self.dock_state
                        .push_to_focused_leaf(Box::new(BookmarksView::default()));
                }
                ShortcutAction::CloseTab => {
                    // Close the currently focused/active tab (the one the user is viewing)
                    // focused_leaf() returns which pane has keyboard focus
                    if let Some((surface_idx, node_idx)) = self.dock_state.focused_leaf() {
                        let tree = &self.dock_state[surface_idx];

                        // Each pane (leaf node) can have multiple tabs, but only one is "active" (visible).
                        // Get the active tab index from the leaf node
                        if let Node::Leaf(leaf) = &tree[node_idx] {
                            let active = leaf.active;
                            self.dock_state.remove_tab((surface_idx, node_idx, active));
                        }
                    }
                }
                ShortcutAction::CycleTab => {
                    // Cycle to the next tab in the active pane
                    if let Some((surface_idx, node_idx)) = self.dock_state.focused_leaf() {
                        let surface = &mut self.dock_state[surface_idx];

                        // Get the number of tabs and current active tab
                        if let Node::Leaf(leaf) = &mut surface[node_idx] {
                            let tab_count = leaf.tabs.len();
                            if tab_count > 1 {
                                let active = leaf.active;
                                // Cycle to next tab (wrap around to 0 if at the end)
                                let next_tab = (active.0 + 1) % tab_count;
                                leaf.active = egui_dock::TabIndex(next_tab);
                            }
                        }
                    }
                }
                ShortcutAction::ReverseCycleTab => {
                    // Cycle to the previous tab in the active pane
                    if let Some((surface_idx, node_idx)) = self.dock_state.focused_leaf() {
                        let surface = &mut self.dock_state[surface_idx];

                        // Get the number of tabs and current active tab
                        if let Node::Leaf(leaf) = &mut surface[node_idx] {
                            let tab_count = leaf.tabs.len();
                            if tab_count > 1 {
                                let active = leaf.active;
                                // Cycle to previous tab (wrap around to last if at the beginning)
                                let prev_tab = if active.0 == 0 {
                                    tab_count - 1
                                } else {
                                    active.0 - 1
                                };
                                leaf.active = egui_dock::TabIndex(prev_tab);
                            }
                        }
                    }
                }
                ShortcutAction::JumpToTop => {}
                ShortcutAction::JumpToBottom => {}
                ShortcutAction::PageUp => {}
                ShortcutAction::PageDown => {}
                ShortcutAction::OpenFile => {}
                ShortcutAction::RenameFilter => {}
                ShortcutAction::MoveUp => {}
                ShortcutAction::MoveDown => {}
                ShortcutAction::FocusPaneLeft => self.navigate_pane(PaneDirection::Left),
                ShortcutAction::FocusPaneDown => self.navigate_pane(PaneDirection::Down),
                ShortcutAction::FocusPaneUp => self.navigate_pane(PaneDirection::Up),
                ShortcutAction::FocusPaneRight => self.navigate_pane(PaneDirection::Right),
            }
        }

        let focused_tab = self.dock_state.find_active_focused().map(|(_, tab)| tab);
        if let Some(focused_tab) = focused_tab {
            if focused_tab.process_events(actions, &mut self.state) {
                self.save_crab_file();
            }
        }
    }

    fn navigate_pane(&mut self, direction: PaneDirection) {
        let tree = self.dock_state.main_surface_mut();

        // Get the currently focused node
        if let Some(current_node) = tree.focused_leaf() {
            // Find the neighbor in the specified direction
            let neighbor = navigation::find_neighbor(tree, current_node, direction);

            // If we found a neighbor, focus it
            if let Some(neighbor_idx) = neighbor {
                tree.set_focused_node(neighbor_idx);
            }
        }
    }
}

impl LogViewState {
    pub fn toggle_bookmark(&mut self, line_index: usize) {
        if let std::collections::hash_map::Entry::Vacant(e) = self.bookmarks.entry(line_index) {
            let timestamp = self.lines[line_index].timestamp;

            let bookmark_name = format!("Line {}", self.lines[line_index].line_number);

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
        self.toggle_bookmark(self.selected_line_index);
    }
}

impl Drop for LogView {
    fn drop(&mut self) {
        log::debug!("Dropping LogView for file: {:?}", self.crab_file);
        self.save_crab_file();
    }
}
