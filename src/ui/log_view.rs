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
use std::collections::HashMap;
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
    /// Highlight matches from all filters in the text with alpha blending for overlaps
    pub fn highlight_text_with_filters(
        text: &str,
        base_color: Color32,
        all_filter_highlights: &[FilterHighlight],
    ) -> egui::text::LayoutJob {
        let mut job = LayoutJob::default();

        if text.is_empty() {
            return job;
        }

        // Collect all matches from all filters
        let mut matches: Vec<(usize, usize, Color32)> = Vec::new();

        for highlight in all_filter_highlights.iter().rev() {
            for mat in highlight.regex.find_iter(text).flatten() {
                matches.push((mat.start(), mat.end(), highlight.color));
            }
        }

        if matches.is_empty() {
            // No matches, return plain text
            job.append(
                text,
                0.0,
                TextFormat {
                    color: base_color,
                    ..Default::default()
                },
            );
            return job;
        }

        // Create a character-level color map for blending overlapping highlights
        let mut char_colors: Vec<Option<Color32>> = vec![None; text.len()];

        for (start, end, color) in matches {
            let range_end = end.min(text.len());
            for char_color in &mut char_colors[start..range_end] {
                *char_color = Some(match *char_color {
                    None => color,
                    Some(existing) => Self::blend_colors(existing, color),
                });
            }
        }

        // Build the job by merging adjacent characters with the same color
        let mut current_start = 0;
        let mut current_color = char_colors[0];

        for i in 1..text.len() {
            let next_color = char_colors[i];

            if next_color != current_color {
                // Color changed, append the current segment
                if let Some(bg_color) = current_color {
                    let text_color = Self::choose_text_color(bg_color);
                    job.append(
                        &text[current_start..i],
                        0.0,
                        TextFormat {
                            color: text_color,
                            background: bg_color,
                            ..Default::default()
                        },
                    );
                } else {
                    job.append(
                        &text[current_start..i],
                        0.0,
                        TextFormat {
                            color: base_color,
                            ..Default::default()
                        },
                    );
                }

                current_start = i;
                current_color = next_color;
            }
        }

        // Append the final segment
        if current_start < text.len() {
            if let Some(bg_color) = current_color {
                let text_color = Self::choose_text_color(bg_color);
                job.append(
                    &text[current_start..],
                    0.0,
                    TextFormat {
                        color: text_color,
                        background: bg_color,
                        ..Default::default()
                    },
                );
            } else {
                job.append(
                    &text[current_start..],
                    0.0,
                    TextFormat {
                        color: base_color,
                        ..Default::default()
                    },
                );
            }
        }

        job
    }

    /// Choose black or white text color based on background brightness
    /// Uses relative luminance calculation from WCAG guidelines
    fn choose_text_color(background: Color32) -> Color32 {
        // For semi-transparent backgrounds, blend with dark background to get effective color
        // This assumes the application has a dark theme
        let alpha = f32::from(background.a()) / 255.0;

        // Blend with dark background (black) to get effective RGB
        let effective_r = (f32::from(background.r()) / 255.0) * alpha;
        let effective_g = (f32::from(background.g()) / 255.0) * alpha;
        let effective_b = (f32::from(background.b()) / 255.0) * alpha;

        // Linearize (gamma correction) for proper luminance calculation
        let linearize = |c_norm: f32| -> f32 {
            if c_norm <= 0.03928 {
                c_norm / 12.92
            } else {
                ((c_norm + 0.055) / 1.055).powf(2.4)
            }
        };

        let r_linear = linearize(effective_r);
        let g_linear = linearize(effective_g);
        let b_linear = linearize(effective_b);

        // Calculate relative luminance: L = 0.2126 * R + 0.7152 * G + 0.0722 * B
        let luminance = 0.2126 * r_linear + 0.7152 * g_linear + 0.0722 * b_linear;

        // Use black text on bright backgrounds, white text on dark backgrounds
        // Threshold of 0.5 works well in practice
        if luminance > 0.5 {
            Color32::BLACK
        } else {
            Color32::WHITE
        }
    }

    /// Blend two colors with alpha compositing (Porter-Duff "over" operator)
    fn blend_colors(bottom: Color32, top: Color32) -> Color32 {
        // Convert to float for blending
        let bottom_a = f32::from(bottom.a()) / 255.0;
        let top_a = f32::from(top.a()) / 255.0;

        // Alpha compositing: out_a = top_a + bottom_a * (1 - top_a)
        let out_a = top_a + bottom_a * (1.0 - top_a);

        if out_a == 0.0 {
            return Color32::TRANSPARENT;
        }

        // For each color channel: out_c = (top_c * top_a + bottom_c * bottom_a * (1 - top_a)) / out_a
        let blend_channel = |top_c: u8, bottom_c: u8| -> u8 {
            let top_cf = f32::from(top_c) / 255.0;
            let bottom_cf = f32::from(bottom_c) / 255.0;

            let out_cf = (top_cf * top_a + bottom_cf * bottom_a * (1.0 - top_a)) / out_a;
            (out_cf * 255.0).round() as u8
        };

        Color32::from_rgba_premultiplied(
            blend_channel(top.r(), bottom.r()),
            blend_channel(top.g(), bottom.g()),
            blend_channel(top.b(), bottom.b()),
            (out_a * 255.0).round() as u8,
        )
    }
}

/// Named bookmark with optional description
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub line_index: usize,
    pub name: String,
    pub timestamp: DateTime<Local>,
}

/// Helper to serialize/deserialize Color32
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct SerializableColor {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl From<Color32> for SerializableColor {
    fn from(c: Color32) -> Self {
        let [r, g, b, a] = c.to_array();
        SerializableColor { r, g, b, a }
    }
}

impl From<SerializableColor> for Color32 {
    fn from(c: SerializableColor) -> Self {
        Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
    }
}

fn serialize_color<S>(color: &Color32, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    SerializableColor::from(*color).serialize(serializer)
}

fn deserialize_color<'de, D>(deserializer: D) -> Result<Color32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    SerializableColor::deserialize(deserializer).map(Color32::from)
}

fn default_filter_color() -> Color32 {
    Color32::YELLOW // Default to yellow if not specified
}

/// Saved filter configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedFilter {
    search_text: String,
    case_sensitive: bool,
    name: String,
    #[serde(
        default = "default_filter_color",
        serialize_with = "serialize_color",
        deserialize_with = "deserialize_color"
    )]
    color: Color32,
}

impl From<&SavedFilter> for FilterState {
    fn from(saved_filter: &SavedFilter) -> FilterState {
        let mut filter = FilterState::new(saved_filter.name.clone(), saved_filter.color);
        filter.search_text.clone_from(&saved_filter.search_text);
        filter.case_sensitive = saved_filter.case_sensitive;
        filter.globally_visible = true;
        filter.update_search_regex();
        filter
    }
}

impl From<&FilterState> for SavedFilter {
    fn from(filter: &FilterState) -> SavedFilter {
        SavedFilter {
            search_text: filter.search_text.clone(),
            case_sensitive: filter.case_sensitive,
            name: filter.name.clone(),
            color: filter.color,
        }
    }
}

/// .crab file format - stores all session data
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrabFile {
    bookmarks: Vec<Bookmark>,
    filters: Vec<SavedFilter>,
}

/// .crab-filters file format - stores only filters for import/export
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrabFilters {
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

    /// Global filter history (shared across all filter tabs)
    pub filter_history: Vec<String>,
}

impl LogViewState {
    /// Add a filter pattern to the global history (called when filter is committed)
    pub fn add_to_filter_history(&mut self, pattern: String) {
        if pattern.is_empty() {
            return;
        }
        // Remove if already exists to avoid duplicates
        self.filter_history.retain(|p| p != &pattern);
        // Add to front (most recent first)
        self.filter_history.insert(0, pattern);
        // Keep only last 50 entries
        if self.filter_history.len() > 50 {
            self.filter_history.truncate(50);
        }
    }
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
                filter_history: Vec::new(),
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
            FilterState::new(
                format!("Filter {}", self.monotonic_filter_counter + 1),
                color,
            )
        });
        let mut filter = Box::new(FilterView::new(self.monotonic_filter_counter, state));
        if focus_search {
            filter.focus_search_next_frame();
        }
        filter.request_filter_update(self.state.lines.clone());
        self.dock_state.push_to_focused_leaf(filter);
        self.monotonic_filter_counter += 1;
    }

    fn load_crab_file(&mut self) {
        log::debug!("Loading .crab file: {}", self.crab_file.display());
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

                if !crab_data.filters.is_empty() {
                    for (i, saved_filter) in crab_data.filters.iter().enumerate() {
                        self.add_filter_view(false, Some(saved_filter.into()));
                        log::debug!("Restored filter {}: '{}'", i, saved_filter.search_text);
                    }
                } else {
                    // No saved filters - just create one default filter
                    self.add_filter_view(false, None);
                }
            } else {
                log::warn!("Failed to parse .crab file: {}", self.crab_file.display());
                self.add_filter_view(false, None);
            }
        } else {
            log::info!(
                ".crab file does not exist yet: {}",
                self.crab_file.display()
            );
            self.add_filter_view(false, None);
        }

        // Split horizontally: 70% top for filters, 30% bottom for bookmarks
        let [top, _bottom] = self.dock_state.main_surface_mut().split_below(
            egui_dock::NodeIndex::root(),
            0.7,
            vec![Box::new(BookmarksView::default())],
        );

        // Focus top pane for adding remaining filters
        self.dock_state.main_surface_mut().set_focused_node(top);
    }

    pub fn save_crab_file(&self) {
        log::debug!("Saving .crab file: {}", self.crab_file.display());
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
                Ok(()) => log::debug!(
                    "Successfully saved .crab file with {} bookmarks, {} filters",
                    self.state.bookmarks.len(),
                    n_filters,
                ),
                Err(e) => log::error!("Failed to save .crab file: {e}"),
            }
        }
    }

    pub fn export_filters(&self, path: &PathBuf) -> Result<(), String> {
        log::debug!("Exporting filters to: {}", path.display());
        let filters = self
            .dock_state
            .iter_all_tabs()
            .filter_map(|((_surface, _node), tab)| tab.try_into_stored_filter())
            .collect::<Vec<SavedFilter>>();

        let filters_data = CrabFilters { filters };

        let json = serde_json::to_string_pretty(&filters_data)
            .map_err(|e| format!("Failed to serialize filters: {e}"))?;

        fs::write(path, json).map_err(|e| format!("Failed to write file: {e}"))?;

        log::info!(
            "Successfully exported {} filters to {}",
            filters_data.filters.len(),
            path.display()
        );
        Ok(())
    }

    pub fn import_filters(&mut self, path: &PathBuf) -> Result<usize, String> {
        log::debug!("Importing filters from: {}", path.display());
        let file_content =
            fs::read_to_string(path).map_err(|e| format!("Failed to read file: {e}"))?;

        let filters_data: CrabFilters = serde_json::from_str(&file_content)
            .map_err(|e| format!("Failed to parse filters file: {e}"))?;

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

            log::debug!("Adding bookmark: {bookmark_name}");
            e.insert(Bookmark {
                line_index,
                name: bookmark_name,
                timestamp,
            });
        } else {
            log::debug!("Removing bookmark at line {line_index}");
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
        log::debug!("Dropping LogView for file: {}", self.crab_file.display());
        self.save_crab_file();
    }
}
