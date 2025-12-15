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
use crate::core::LogStore;
use crate::input::ShortcutAction;
use crate::ui::tabs::filter_tab::filter_state::FilterState;
use crate::ui::tabs::highlights_tab::HighlightState;
use crate::ui::tabs::{
    navigation, BookmarksView, FilterView, HighlightsView, LogCrabTab, LogCrabTabViewer,
    PendingTabAdd,
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
        all_filter_highlights: &[Self],
        dark_mode: bool,
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
                    let text_color = Self::choose_text_color(bg_color, dark_mode);
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
                let text_color = Self::choose_text_color(bg_color, dark_mode);
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
    fn choose_text_color(background: Color32, dark_mode: bool) -> Color32 {
        // For semi-transparent backgrounds, blend with the base background to get effective color
        let alpha = f32::from(background.a()) / 255.0;

        // Base background: black for dark mode, white for light mode
        let (base_r, base_g, base_b) = if dark_mode {
            (0.0, 0.0, 0.0)
        } else {
            (1.0, 1.0, 1.0)
        };

        // Blend highlight color with base background
        let effective_r = (f32::from(background.r()) / 255.0) * alpha + base_r * (1.0 - alpha);
        let effective_g = (f32::from(background.g()) / 255.0) * alpha + base_g * (1.0 - alpha);
        let effective_b = (f32::from(background.b()) / 255.0) * alpha + base_b * (1.0 - alpha);

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
        let luminance = 0.0722f32.mul_add(b_linear, 0.2126f32.mul_add(r_linear, 0.7152 * g_linear));

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
        Self { r, g, b, a }
    }
}

impl From<SerializableColor> for Color32 {
    fn from(c: SerializableColor) -> Self {
        Self::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
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

const fn default_filter_color() -> Color32 {
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
    #[serde(default = "default_globally_visible")]
    globally_visible: bool,
    #[serde(default)]
    show_in_histogram: bool,
}

const fn default_globally_visible() -> bool {
    true
}

impl From<&SavedFilter> for FilterState {
    fn from(saved_filter: &SavedFilter) -> Self {
        let mut filter = Self::new(saved_filter.name.clone(), saved_filter.color);
        filter.search_text.clone_from(&saved_filter.search_text);
        filter.case_sensitive = saved_filter.case_sensitive;
        filter.globally_visible = saved_filter.globally_visible;
        filter.show_in_histogram = saved_filter.show_in_histogram;
        filter.update_search_regex();
        filter
    }
}

impl From<&FilterState> for SavedFilter {
    fn from(filter: &FilterState) -> Self {
        Self {
            search_text: filter.search_text.clone(),
            case_sensitive: filter.case_sensitive,
            name: filter.name.clone(),
            color: filter.color,
            globally_visible: filter.globally_visible,
            show_in_histogram: filter.show_in_histogram,
        }
    }
}

/// Saved highlight configuration for .crab file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedHighlight {
    #[serde(default)]
    name: String,
    search_text: String,
    case_sensitive: bool,
    #[serde(
        serialize_with = "serialize_color",
        deserialize_with = "deserialize_color"
    )]
    color: Color32,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    show_in_histogram: bool,
}

const fn default_enabled() -> bool {
    true
}

impl From<&SavedHighlight> for HighlightState {
    fn from(saved: &SavedHighlight) -> Self {
        let mut highlight = Self::new(saved.name.clone(), saved.color);
        highlight.search_text.clone_from(&saved.search_text);
        highlight.case_sensitive = saved.case_sensitive;
        highlight.enabled = saved.enabled;
        highlight.show_in_histogram = saved.show_in_histogram;
        highlight.update_search_regex();
        highlight
    }
}

impl From<&HighlightState> for SavedHighlight {
    fn from(highlight: &HighlightState) -> Self {
        Self {
            name: highlight.name.clone(),
            search_text: highlight.search_text.clone(),
            case_sensitive: highlight.case_sensitive,
            color: highlight.color,
            enabled: highlight.enabled,
            show_in_histogram: highlight.show_in_histogram,
        }
    }
}

/// Current version of the .crab file format
const CRAB_FILE_VERSION: u32 = 2;

/// Current version of the .crab-filters file format
const CRAB_FILTERS_VERSION: u32 = 1;

/// .crab file format - stores all session data
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrabFile {
    /// File format version for future compatibility
    #[serde(default = "default_version")]
    version: u32,
    bookmarks: Vec<Bookmark>,
    filters: Vec<SavedFilter>,
    #[serde(default)]
    highlights: Vec<SavedHighlight>,
}

const fn default_version() -> u32 {
    1 // Treat missing version as v1 for backwards compatibility
}

/// .crab-filters file format - stores only filters for import/export
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrabFilters {
    /// File format version for future compatibility
    #[serde(default = "default_version")]
    version: u32,
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
    pub store: Arc<LogStore>,
    // Selected line tracking
    pub selected_line_index: usize,
    // Bookmarks with names
    pub bookmarks: HashMap<usize, Bookmark>,
    pub modified: bool,
    last_saved: Option<DateTime<Local>>,

    /// Global filter history (shared across all filter tabs)
    pub filter_history: Vec<String>,

    /// Highlight rules that apply across all tabs
    pub highlights: Vec<HighlightState>,

    /// Pending conversion requests (highlight index to convert to filter)
    pub pending_highlight_to_filter: Option<usize>,
    /// Pending conversion requests (filter data to convert to highlight)
    pub pending_filter_to_highlight: Option<FilterToHighlightData>,
}

/// Data needed to convert a filter to a highlight
#[derive(Debug, Clone)]
pub struct FilterToHighlightData {
    pub filter_uuid: usize,
    pub name: String,
    pub search_text: String,
    pub case_sensitive: bool,
    pub color: Color32,
    pub globally_visible: bool,
    pub show_in_histogram: bool,
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
    pub fn new(store: Arc<LogStore>, crab_file: PathBuf) -> Self {
        let mut view = Self {
            crab_file,
            dock_state: DockState::new(Vec::new()),
            monotonic_filter_counter: 0,
            pending_tab_add: None,
            state: LogViewState {
                filter_history: Vec::new(),
                store,
                selected_line_index: 0,
                bookmarks: HashMap::new(),
                modified: false,
                last_saved: None,
                highlights: Vec::new(),
                pending_highlight_to_filter: None,
                pending_filter_to_highlight: None,
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
        self.dock_state.push_to_focused_leaf(filter);
        self.monotonic_filter_counter += 1;
    }

    fn load_crab_file(&mut self) {
        log::debug!("Loading .crab file: {}", self.crab_file.display());
        if let Ok(file_content) = fs::read_to_string(&self.crab_file) {
            if let Ok(crab_data) = serde_json::from_str::<CrabFile>(&file_content) {
                log::info!(
                    "Loaded .crab file v{} with {} bookmarks, {} filters, {} highlights",
                    crab_data.version,
                    crab_data.bookmarks.len(),
                    crab_data.filters.len(),
                    crab_data.highlights.len()
                );

                // Future: handle version migrations here
                if crab_data.version > CRAB_FILE_VERSION {
                    log::warn!(
                        ".crab file version {} is newer than supported version {}. Some features may not work correctly.",
                        crab_data.version,
                        CRAB_FILE_VERSION
                    );
                }

                // Load bookmarks
                for bookmark in crab_data.bookmarks {
                    self.state.bookmarks.insert(bookmark.line_index, bookmark);
                }

                // Load highlights
                for saved_highlight in &crab_data.highlights {
                    self.state.highlights.push(saved_highlight.into());
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

        // Split horizontally: 70% top for filters, 30% bottom for bookmarks and highlights
        let [top, _bottom] = self.dock_state.main_surface_mut().split_below(
            egui_dock::NodeIndex::root(),
            0.7,
            vec![
                Box::new(HighlightsView::new()),
                Box::new(BookmarksView::default()),
            ],
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
        let highlights: Vec<SavedHighlight> =
            self.state.highlights.iter().map(|h| h.into()).collect();
        let n_filters = filters.len();
        let n_highlights = highlights.len();
        let crab_data = CrabFile {
            version: CRAB_FILE_VERSION,
            bookmarks: self.state.bookmarks.values().cloned().collect(),
            filters,
            highlights,
        };

        if let Ok(json) = serde_json::to_string_pretty(&crab_data) {
            match fs::write(&self.crab_file, json) {
                Ok(()) => log::debug!(
                    "Successfully saved .crab file with {} bookmarks, {} filters, {} highlights",
                    self.state.bookmarks.len(),
                    n_filters,
                    n_highlights,
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

        let filters_data = CrabFilters {
            version: CRAB_FILTERS_VERSION,
            filters,
        };

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

        log::info!(
            "Importing .crab-filters v{} with {} filters",
            filters_data.version,
            filters_data.filters.len()
        );

        // Future: handle version migrations here
        if filters_data.version > CRAB_FILTERS_VERSION {
            log::warn!(
                ".crab-filters file version {} is newer than supported version {}. Some features may not work correctly.",
                filters_data.version,
                CRAB_FILTERS_VERSION
            );
        }

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

        // Update highlight caches and check for results
        for highlight in &mut self.state.highlights {
            highlight.check_filter_results();
            highlight.ensure_cache_valid(&self.state.store);
        }

        // Collect all filter highlights from all tabs
        let mut all_filter_highlights: Vec<FilterHighlight> = self
            .dock_state
            .iter_all_tabs()
            .filter_map(|((_surface, _node), tab)| tab.get_filter_highlight())
            .collect();

        // Add highlights from LogViewState
        for highlight in &self.state.highlights {
            if highlight.enabled && !highlight.search_text.is_empty() {
                if let Ok(regex) = &highlight.search_regex {
                    all_filter_highlights.push(FilterHighlight {
                        regex: regex.clone(),
                        color: highlight.color,
                    });
                }
            }
        }

        // Collect histogram markers from all tabs
        let mut histogram_markers: Vec<_> = self
            .dock_state
            .iter_all_tabs()
            .filter_map(|((_surface, _node), tab)| tab.get_histogram_marker())
            .collect();

        // Add histogram markers from highlights (using cached indices)
        for highlight in &self.state.highlights {
            if highlight.show_in_histogram && !highlight.search_text.is_empty() {
                // Use name if set, otherwise fall back to search text
                let name = if highlight.name.is_empty() {
                    highlight.search_text.clone()
                } else {
                    highlight.name.clone()
                };
                histogram_markers.push(crate::ui::tabs::filter_tab::HistogramMarker {
                    name,
                    color: highlight.color,
                    indices: highlight.filtered_indices.clone(),
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
                filter_state.search_text = highlight.search_text.clone();
                filter_state.case_sensitive = highlight.case_sensitive;
                filter_state.globally_visible = highlight.enabled;
                filter_state.show_in_histogram = highlight.show_in_histogram;
                filter_state.update_search_regex();

                self.add_filter_view(false, Some(filter_state));

                // Remove the highlight
                self.state.highlights.remove(highlight_index);
                self.state.modified = true;
            }
        }

        // Handle filter-to-highlight conversion
        if let Some(data) = self.state.pending_filter_to_highlight.take() {
            let mut highlight = HighlightState::new(data.name, data.color);
            highlight.search_text = data.search_text;
            highlight.case_sensitive = data.case_sensitive;
            highlight.enabled = data.globally_visible;
            highlight.show_in_histogram = data.show_in_histogram;
            highlight.update_search_regex();
            highlight.request_filter_update(Arc::clone(&self.state.store));

            self.state.highlights.push(highlight);
            self.state.modified = true;

            // Close the filter tab that was converted
            // Find the tab by uuid and remove it
            self.dock_state.retain_tabs(|t| {
                t.get_uuid() != Some(data.filter_uuid)
            });
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
            let line = self.store.get_by_id(line_index).unwrap();
            let timestamp = line.timestamp;
            let line_number = line.line_number;

            let bookmark_name = format!("Line {}", line_number);

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
