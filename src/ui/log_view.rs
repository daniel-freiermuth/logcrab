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
use crate::core::session::CRAB_FILTERS_VERSION;
use crate::core::{CrabFilters, LogFileLoader, LogStore, SavedFilter, SavedHighlight, SearchRule};
use crate::input::ShortcutAction;
use crate::ui::filter_highlight::FilterHighlight;
use crate::ui::session_state::SessionState;
use crate::ui::tabs::filter_tab::filter_state::FilterState;
use crate::ui::tabs::{
    navigation, BookmarksView, FilterView, HighlightsView, LogCrabTab, LogCrabTabViewer,
    PendingTabAdd,
};
use crate::ui::{PaneDirection, ProgressToastHandle, DEFAULT_PALETTE};

use chrono::Local;
use egui_dock::{DockArea, DockState, Node};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Main log viewing session for an opened file.
///
/// Responsibilities:
/// - Managing dock layout and tabs
/// - Loading/saving .crab session file
/// - Coordinating keyboard input across tabs
pub struct CrabSession {
    /// Dock state for VS Code-like tiling layout
    pub dock_state: DockState<Box<dyn LogCrabTab>>,

    /// Counter for assigning unique filter names/colors
    monotonic_filter_counter: usize,

    /// Shared state passed to all tabs
    pub state: SessionState,

    /// Pending tab add request (set by add button callback)
    pending_tab_add: Option<PendingTabAdd>,
}

impl CrabSession {
    pub fn new(store: Arc<LogStore>) -> Self {
        let mut cs = Self {
            dock_state: DockState::new(Vec::new()),
            monotonic_filter_counter: 0,
            pending_tab_add: None,
            state: SessionState::new(store),
        };
        cs.add_filter_view(false, None);

        // Split horizontally: 70% top for filters, 30% bottom for bookmarks and highlights
        let [top, _bottom] = cs.dock_state.main_surface_mut().split_below(
            egui_dock::NodeIndex::root(),
            0.7,
            vec![
                Box::new(HighlightsView::new()),
                Box::new(BookmarksView::default()),
            ],
        );

        // Focus top pane for adding remaining filters
        cs.dock_state.main_surface_mut().set_focused_node(top);
        cs
    }

    pub fn add_filter_view(&mut self, focus_search: bool, state: Option<FilterState>) {
        let color = DEFAULT_PALETTE[self.monotonic_filter_counter % DEFAULT_PALETTE.len()];

        let state = state.unwrap_or_else(|| {
            FilterState::new(
                format!("Filter {}", self.monotonic_filter_counter + 1),
                color,
            )
        });
        let mut filter = Box::new(FilterView::new(state));
        if focus_search {
            filter.focus_search_next_frame();
        }
        self.dock_state.push_to_focused_leaf(filter);
        self.monotonic_filter_counter += 1;
    }

    /// Add a file to the current session
    ///
    /// Loads the file asynchronously and adds it as an additional source to the store.
    /// Skips files that are already loaded.
    pub fn add_file(&mut self, path: PathBuf, ctx: egui::Context, toast: ProgressToastHandle) {
        // Check if the file is already loaded
        if self.state.store.contains_file(&path) {
            log::info!("Skipping already loaded file: {}", path.display());
            toast.dismiss();
            return;
        }

        log::info!("Adding file to session: {}", path.display());

        let source = LogFileLoader::load_async(path, ctx, toast);
        let (filters, highlights) = source.load_saved_filters_and_highlights();
        self.state.store.add_source(source);
        for saved_filter in &filters {
            self.add_filter_if_not_exists(saved_filter);
        }
        for saved_highlight in &highlights {
            self.add_highlight_if_not_exists(saved_highlight);
        }
    }

    fn add_filter_if_not_exists(&mut self, saved_filter: &SavedFilter) {
        // Check if a filter with the same search text already exists
        let exists = self
            .dock_state
            .iter_all_tabs()
            .filter_map(|(_, tab)| tab.try_into_stored_filter())
            .any(|f| f.search_text == saved_filter.search_text);

        if !exists {
            self.add_filter_view(false, Some(saved_filter.into()));
            log::debug!("Merged filter: '{}'", saved_filter.search_text);
        }
    }

    fn add_highlight_if_not_exists(&mut self, saved_highlight: &SavedHighlight) {
        // Check if a highlight with the same search text already exists
        let exists = self
            .state
            .highlights
            .iter()
            .any(|h| h.search.search_text == saved_highlight.search_text);

        if !exists {
            self.state.highlights.push(saved_highlight.into());
            log::debug!("Merged highlight: '{}'", saved_highlight.search_text);
        }
    }

    pub fn save_crab_file(&self) {
        log::debug!("Saving .crab files for all sources");
        let filters = self
            .dock_state
            .iter_all_tabs()
            .filter_map(|((_surface, _node), tab)| tab.try_into_stored_filter())
            .collect::<Vec<SavedFilter>>();
        let highlights: Vec<SavedHighlight> =
            self.state.highlights.iter().map(|h| h.into()).collect();

        // Save to all sources' .crab files
        // Each source saves its own bookmarks + shared filters/highlights
        self.state.store.save_all_crab_files(&filters, &highlights);

        log::debug!(
            "Saved .crab files with {} filters, {} highlights",
            filters.len(),
            highlights.len(),
        );
    }

    pub fn export_filters(&self, path: &Path) -> Result<(), String> {
        log::debug!("Exporting filters to: {}", path.display());
        let filters = self
            .dock_state
            .iter_all_tabs()
            .filter_map(|((_surface, _node), tab)| tab.try_into_stored_filter())
            .collect::<Vec<SavedFilter>>();

        let filters_data = CrabFilters {
            version: CRAB_FILTERS_VERSION,
            filters,
        };

        filters_data
            .save(path)
            .map_err(|e| format!("Failed to save filters: {e}"))?;

        log::info!(
            "Successfully exported {} filters to {}",
            filters_data.filters.len(),
            path.display()
        );
        Ok(())
    }

    pub fn import_filters(&mut self, path: &Path) -> Result<usize, String> {
        log::debug!("Importing filters from: {}", path.display());

        let filters_data =
            CrabFilters::load(path).map_err(|e| format!("Failed to load filters: {e}"))?;

        log::info!(
            "Importing .crab-filters v{} with {} filters",
            filters_data.version,
            filters_data.filters.len()
        );

        let count = filters_data.filters.len();
        for saved_filter in filters_data.filters {
            let state: FilterState = (&saved_filter).into();
            self.add_filter_view(false, Some(state));
        }

        log::info!(
            "Successfully imported {count} filters from {}",
            path.display()
        );
        Ok(count)
    }

    pub fn render(&mut self, ui: &mut egui::Ui, global_config: &mut GlobalConfig) {
        profiling::scope!("LogView::render");

        // Collect all filter highlights from all tabs
        let mut all_filter_highlights: Vec<FilterHighlight> = {
            profiling::scope!("collect_filter_highlights");
            self
                .dock_state
                .iter_all_tabs()
                .filter_map(|((_surface, _node), tab)| tab.get_filter_highlight())
                .collect()
        };

        // Add highlights from LogViewState
        for highlight in &self.state.highlights {
            if highlight.enabled && !highlight.search.search_text.is_empty() {
                if let Ok(regex) = &highlight.search.get_regex() {
                    all_filter_highlights.push(FilterHighlight {
                        regex: regex.clone(),
                        color: highlight.color,
                    });
                }
            }
        }

        // Collect histogram markers from all tabs
        let mut histogram_markers: Vec<_> = {
            profiling::scope!("collect_histogram_markers");
            self
                .dock_state
                .iter_all_tabs_mut()
                .filter_map(|((_surface, _node), tab)| tab.get_histogram_marker(&self.state.store))
                .collect()
        };

        // Add histogram markers from highlights (using cached indices)
        for highlight in &mut self.state.highlights {
            if highlight.show_in_histogram && !highlight.search.search_text.is_empty() {
                // Use name if set, otherwise fall back to search text
                let name = if highlight.name.is_empty() {
                    highlight.search.search_text.clone()
                } else {
                    highlight.name.clone()
                };
                histogram_markers.push(crate::ui::tabs::filter_tab::HistogramMarker {
                    name,
                    color: highlight.color,
                    indices: highlight
                        .search
                        .get_filtered_indices(&self.state.store)
                        .clone(),
                });
            }
        }

        // Use dock area for VS Code-like draggable/tiling layout
        {
            profiling::scope!("DockArea::show");
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
                        histogram_markers: &histogram_markers,
                    },
                );
        }
        if self.state.modified
            && self
                .state
                .last_saved
                .is_none_or(|t| (Local::now() - t).num_seconds() >= 5)
        {
            profiling::scope!("save_crab_file");
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
                PendingTabAdd::Highlights => {
                    self.dock_state
                        .push_to_focused_leaf(Box::new(HighlightsView::new()));
                }
                PendingTabAdd::Bookmarks => {
                    self.dock_state
                        .push_to_focused_leaf(Box::new(BookmarksView::default()));
                }
            }
        }

        // Handle highlight-to-filter conversion
        if let Some(highlight_index) = self.state.pending_highlight_to_filter.take() {
            if let Some(highlight) = self.state.highlights.get(highlight_index) {
                // Create a new filter with the highlight's settings
                let mut filter_state = FilterState::new(highlight.name.clone(), highlight.color);
                filter_state.search.search_text = highlight.search.search_text.clone();
                filter_state.search.case_sensitive = highlight.search.case_sensitive;
                filter_state.enabled = highlight.enabled;
                filter_state.show_in_histogram = highlight.show_in_histogram;

                self.add_filter_view(false, Some(filter_state));

                // Remove the highlight
                self.state.highlights.remove(highlight_index);
                self.state.modified = true;
            }
        }

        // Handle filter-to-highlight conversion
        if let Some(data) = self.state.pending_filter_to_highlight.take() {
            let mut highlight = SearchRule::new(data.name, data.color);
            highlight.search.search_text = data.search_text;
            highlight.search.case_sensitive = data.case_sensitive;
            highlight.enabled = data.enabled;
            highlight.show_in_histogram = data.show_in_histogram;

            self.state.highlights.push(highlight);
            self.state.modified = true;

            // Close the filter tab that was converted
            // Find the tab by uuid and remove it
            self.dock_state
                .retain_tabs(|t| t.get_uuid() != Some(data.filter_uuid));
        }
    }

    pub fn process_keyboard_input(&mut self, actions: &[ShortcutAction]) {
        profiling::function_scope!();
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

impl Drop for CrabSession {
    fn drop(&mut self) {
        log::debug!("Dropping LogView");
        self.save_crab_file();
    }
}
