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
use crate::parser::line::LogLine;
use egui::{Color32, RichText, Ui, text::LayoutJob, TextFormat};
use egui_extras::{TableBuilder, Column};
use regex::{Regex, RegexBuilder};
use chrono::{DateTime, Local};
use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;
use serde::{Serialize, Deserialize};

/// Named bookmark with optional description
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Bookmark {
    line_index: usize,
    name: String,
    timestamp: Option<DateTime<Local>>,
}

/// Saved filter configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedFilter {
    search_text: String,
    case_insensitive: bool,
    is_favorite: bool,
    #[serde(default)]
    name: Option<String>,
}

/// .crab file format - stores all session data
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrabFile {
    bookmarks: Vec<Bookmark>,
    filters: Vec<SavedFilter>,
}

impl CrabFile {
    fn new() -> Self {
        CrabFile {
            bookmarks: Vec::new(),
            filters: Vec::new(),
        }
    }
}

/// Represents a single filter view with its own search criteria and cached results
struct FilterView {
    search_text: String,
    search_regex: Option<Regex>,
    regex_error: Option<String>,
    case_insensitive: bool,
    filtered_indices: Vec<usize>,
    filter_dirty: bool,
    last_rendered_selection: Option<usize>,
    highlight_color: Color32,
    is_favorite: bool,
    name: Option<String>,
    should_focus_search: bool,  // Flag to focus search input on next render
}

impl FilterView {
    fn new(highlight_color: Color32) -> Self {
        FilterView {
            search_text: String::new(),
            search_regex: None,
            regex_error: None,
            case_insensitive: false,
            filtered_indices: Vec::new(),
            filter_dirty: true,
            last_rendered_selection: None,
            highlight_color,
            is_favorite: false,
            name: None,
            should_focus_search: false,
        }
    }
    
    fn update_search_regex(&mut self) {
        if self.search_text.is_empty() {
            self.search_regex = None;
            self.regex_error = None;
        } else {
            match RegexBuilder::new(&self.search_text)
                .case_insensitive(self.case_insensitive)
                .build()
            {
                Ok(regex) => {
                    self.search_regex = Some(regex);
                    self.regex_error = None;
                }
                Err(e) => {
                    self.search_regex = None;
                    self.regex_error = Some(e.to_string());
                }
            }
        }
        self.filter_dirty = true;
    }
    
    fn matches_search(&self, line: &LogLine) -> bool {
        if let Some(ref regex) = self.search_regex {
            regex.is_match(&line.message) || regex.is_match(&line.raw)
        } else {
            true
        }
    }
    
    fn rebuild_filtered_indices(
        &mut self,
        lines: &[LogLine],
        min_score_filter: f64,
        selected_line_index: Option<usize>,
        selected_timestamp: Option<DateTime<Local>>,
    ) -> Option<usize> {
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_function!();
        
        self.filtered_indices.clear();
        self.filtered_indices.reserve(lines.len() / 10);
        
        for (idx, line) in lines.iter().enumerate() {
            if line.anomaly_score >= min_score_filter && self.matches_search(line) {
                self.filtered_indices.push(idx);
            }
        }
        self.filter_dirty = false;
        
        if let Some(selected_line_idx) = selected_line_index {
            if let Some(position) = self.filtered_indices.iter().position(|&idx| idx == selected_line_idx) {
                return Some(position);
            }
            
            if let Some(selected_ts) = selected_timestamp {
                return self.find_closest_timestamp_index(lines, selected_ts);
            }
        }
        
        None
    }
    
    fn find_closest_timestamp_index(&self, lines: &[LogLine], target_ts: DateTime<Local>) -> Option<usize> {
        if self.filtered_indices.is_empty() {
            return None;
        }
        
        let mut closest_idx = 0;
        let mut min_diff = i64::MAX;
        
        for (filtered_idx, &line_idx) in self.filtered_indices.iter().enumerate() {
            if let Some(line_ts) = lines[line_idx].timestamp {
                let diff = (line_ts.timestamp() - target_ts.timestamp()).abs();
                if diff < min_diff {
                    min_diff = diff;
                    closest_idx = filtered_idx;
                }
            }
        }
        
        Some(closest_idx)
    }
    
    fn highlight_matches(&self, text: &str, base_color: Color32) -> LayoutJob {
        let mut job = LayoutJob::default();
        
        if let Some(ref regex) = self.search_regex {
            let mut last_end = 0;
            
            for mat in regex.find_iter(text) {
                if mat.start() > last_end {
                    job.append(
                        &text[last_end..mat.start()],
                        0.0,
                        TextFormat {
                            color: base_color,
                            ..Default::default()
                        },
                    );
                }
                
                job.append(
                    mat.as_str(),
                    0.0,
                    TextFormat {
                        color: Color32::BLACK,
                        background: self.highlight_color,
                        ..Default::default()
                    },
                );
                
                last_end = mat.end();
            }
            
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
        } else {
            job.append(
                text,
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

pub struct LogView {
    pub lines: Vec<LogLine>,
    pub min_score_filter: f64,
    // Multiple filter views
    filters: Vec<FilterView>,
    // Selected line tracking
    selected_line_index: Option<usize>,
    selected_timestamp: Option<DateTime<Local>>,
    // Track last rendered selection for context view
    last_rendered_selection_context: Option<usize>,
    // Bookmarks with names
    bookmarks: HashMap<usize, Bookmark>,
    // .crab file path
    crab_file: Option<PathBuf>,
    // UI state
    show_bookmarks_panel: bool,
    bookmark_name_input: String,
    editing_bookmark: Option<usize>,
}

impl LogView {
    pub fn new() -> Self {
        // Start with 2 filters by default (yellow and light blue highlights)
        let filters = vec![
            FilterView::new(Color32::YELLOW),
            FilterView::new(Color32::LIGHT_BLUE),
        ];
        
        LogView {
            lines: Vec::new(),
            min_score_filter: 0.0,
            filters,
            selected_line_index: None,
            selected_timestamp: None,
            last_rendered_selection_context: None,
            bookmarks: HashMap::new(),
            crab_file: None,
            show_bookmarks_panel: false,
            bookmark_name_input: String::new(),
            editing_bookmark: None,
        }
    }
    
    pub fn add_filter(&mut self) {
        // Cycle through different highlight colors
        let colors = [
            Color32::YELLOW,
            Color32::LIGHT_BLUE,
            Color32::LIGHT_GREEN,
            Color32::from_rgb(255, 200, 150), // Light orange
            Color32::from_rgb(255, 150, 255), // Light magenta
            Color32::from_rgb(150, 255, 255), // Light cyan
        ];
        let color = colors[self.filters.len() % colors.len()];
        self.filters.push(FilterView::new(color));
    }
    
    /// Focus the search input for a specific filter (called by Ctrl+L)
    pub fn focus_search_input(&mut self, filter_index: usize) {
        if filter_index < self.filters.len() {
            self.filters[filter_index].should_focus_search = true;
        }
    }

    /// Move selection among all lines (context view navigation)
    pub fn move_selection_global(&mut self, delta: i32) {
        if self.lines.is_empty() { return; }
        let current = self.selected_line_index.unwrap_or_else(|| {
            if delta >= 0 { 0 } else { self.lines.len() - 1 }
        });
        let new_index = if delta < 0 {
            current.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (current + delta as usize).min(self.lines.len().saturating_sub(1))
        };
        self.selected_line_index = Some(new_index);
        self.selected_timestamp = self.lines[new_index].timestamp;
    }

    /// Move selection within a filtered view (only through matched indices)
    pub fn move_selection_in_filter(&mut self, filter_index: usize, delta: i32) {
        if filter_index >= self.filters.len() { return; }
        let filter = &self.filters[filter_index];
        if filter.filtered_indices.is_empty() { return; }

        // Determine current position within filtered list
        let current_pos = if let Some(sel) = self.selected_line_index {
            filter.filtered_indices.iter().position(|&idx| idx == sel).unwrap_or_else(|| {
                // Fallback: choose nearest by timestamp if available later improvements; for now start at beginning
                0
            })
        } else {
            if delta >= 0 { 0 } else { filter.filtered_indices.len() - 1 }
        };

        let new_pos = if delta < 0 {
            current_pos.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            (current_pos + delta as usize).min(filter.filtered_indices.len() - 1)
        };

        let new_line_index = filter.filtered_indices[new_pos];
        self.selected_line_index = Some(new_line_index);
        self.selected_timestamp = self.lines[new_line_index].timestamp;
    }
    
    /// Jump to the first line in the global view (Vim-style gg)
    pub fn jump_to_top_global(&mut self) {
        if self.lines.is_empty() { return; }
        self.selected_line_index = Some(0);
        self.selected_timestamp = self.lines[0].timestamp;
    }
    
    /// Jump to the last line in the global view (Vim-style G)
    pub fn jump_to_bottom_global(&mut self) {
        if self.lines.is_empty() { return; }
        let last_index = self.lines.len() - 1;
        self.selected_line_index = Some(last_index);
        self.selected_timestamp = self.lines[last_index].timestamp;
    }
    
    /// Jump to the first line in a filtered view (Vim-style gg)
    pub fn jump_to_top_in_filter(&mut self, filter_index: usize) {
        if filter_index >= self.filters.len() { return; }
        let filter = &self.filters[filter_index];
        if filter.filtered_indices.is_empty() { return; }
        
        let first_line_index = filter.filtered_indices[0];
        self.selected_line_index = Some(first_line_index);
        self.selected_timestamp = self.lines[first_line_index].timestamp;
    }
    
    /// Jump to the last line in a filtered view (Vim-style G)
    pub fn jump_to_bottom_in_filter(&mut self, filter_index: usize) {
        if filter_index >= self.filters.len() { return; }
        let filter = &self.filters[filter_index];
        if filter.filtered_indices.is_empty() { return; }
        
        let last_pos = filter.filtered_indices.len() - 1;
        let last_line_index = filter.filtered_indices[last_pos];
        self.selected_line_index = Some(last_line_index);
        self.selected_timestamp = self.lines[last_line_index].timestamp;
    }
    
    pub fn remove_filter(&mut self, index: usize) {
        if self.filters.len() > 1 && index < self.filters.len() {
            self.filters.remove(index);
        }
    }
    
    pub fn filter_count(&self) -> usize {
        self.filters.len()
    }
    
    pub fn get_filter_name(&self, index: usize) -> Option<String> {
        self.filters.get(index).and_then(|f| f.name.clone())
    }
    
    pub fn set_filter_name(&mut self, index: usize, name: Option<String>) {
        if let Some(filter) = self.filters.get_mut(index) {
            filter.name = name;
            self.save_crab_file();
        }
    }
    
    pub fn is_bookmarks_panel_visible(&self) -> bool {
        self.show_bookmarks_panel
    }
    
    pub fn set_lines(&mut self, lines: Vec<LogLine>) {
        self.lines = lines;
        for filter in &mut self.filters {
            filter.filter_dirty = true;
        }
    }
    
    pub fn set_bookmarks_file(&mut self, log_file_path: PathBuf) -> usize {
        let crab_path = log_file_path.with_extension("crab");
        self.crab_file = Some(crab_path.clone());
        let initial_filter_count = self.filters.len();
        self.load_crab_file();
        // Return how many filters we have after loading
        self.filters.len().saturating_sub(initial_filter_count)
    }
    
    fn load_crab_file(&mut self) {
        self.bookmarks.clear();
        
        if let Some(ref path) = self.crab_file {
            if let Ok(file_content) = fs::read_to_string(path) {
                if let Ok(crab_data) = serde_json::from_str::<CrabFile>(&file_content) {
                    // Load bookmarks
                    for bookmark in crab_data.bookmarks {
                        self.bookmarks.insert(bookmark.line_index, bookmark);
                    }
                    
                    // Load saved filters - create additional filters if needed
                    for (i, saved_filter) in crab_data.filters.iter().enumerate() {
                        // Add new filter if we don't have enough
                        while i >= self.filters.len() {
                            self.add_filter();
                        }
                        
                        // Restore filter settings
                        self.filters[i].search_text = saved_filter.search_text.clone();
                        self.filters[i].case_insensitive = saved_filter.case_insensitive;
                        self.filters[i].is_favorite = saved_filter.is_favorite;
                        self.filters[i].name = saved_filter.name.clone();
                        self.filters[i].update_search_regex();
                    }
                }
            }
        }
    }
    
    fn save_crab_file(&self) {
        if let Some(ref path) = self.crab_file {
            let crab_data = CrabFile {
                bookmarks: self.bookmarks.values().cloned().collect(),
                filters: self.filters.iter().map(|f| SavedFilter {
                    search_text: f.search_text.clone(),
                    case_insensitive: f.case_insensitive,
                    is_favorite: f.is_favorite,
                    name: f.name.clone(),
                }).collect(),
            };
            
            if let Ok(json) = serde_json::to_string_pretty(&crab_data) {
                let _ = fs::write(path, json);
            }
        }
    }
    
    fn toggle_bookmark(&mut self, line_index: usize) {
        if self.bookmarks.contains_key(&line_index) {
            self.bookmarks.remove(&line_index);
        } else {
            let timestamp = if line_index < self.lines.len() {
                self.lines[line_index].timestamp
            } else {
                None
            };
            
            self.bookmarks.insert(line_index, Bookmark {
                line_index,
                name: format!("Line {}", if line_index < self.lines.len() {
                    self.lines[line_index].line_number.to_string()
                } else {
                    line_index.to_string()
                }),
                timestamp,
            });
        }
        self.save_crab_file();
    }
    
    fn rename_bookmark(&mut self, line_index: usize, new_name: String) {
        if let Some(bookmark) = self.bookmarks.get_mut(&line_index) {
            bookmark.name = new_name;
            self.save_crab_file();
        }
    }
    
    pub fn toggle_bookmarks_panel(&mut self) {
        self.show_bookmarks_panel = !self.show_bookmarks_panel;
    }
    
    /// Toggle bookmark for the currently selected line
    pub fn toggle_bookmark_for_selected(&mut self) {
        if let Some(selected_idx) = self.selected_line_index {
            self.toggle_bookmark(selected_idx);
        }
    }
    
    /// Render a specific filter view
    pub fn render_filter(&mut self, ui: &mut Ui, filter_index: usize) {
        if filter_index >= self.filters.len() {
            ui.label("Invalid filter index");
            return;
        }
        
        // Display custom name if set, otherwise default
        let display_name = self.filters[filter_index].name.clone()
            .unwrap_or_else(|| format!("Filter View {}", filter_index + 1));
        ui.heading(&display_name);
        
        // Name and Star buttons
        ui.horizontal(|ui| {
            // Edit name button
            if ui.small_button("✏").on_hover_text("Edit filter name").clicked() {
                // Prompt for new name
                if let Some(current_name) = &self.filters[filter_index].name {
                    self.bookmark_name_input = current_name.clone();
                } else {
                    self.bookmark_name_input = format!("Filter {}", filter_index + 1);
                }
                self.editing_bookmark = Some(filter_index + 10000); // Use high number to distinguish from bookmarks
            }
            
            let star_text = if self.filters[filter_index].is_favorite { "⭐" } else { "☆" };
            if ui.button(star_text).on_hover_text("Toggle favorite filter").clicked() {
                self.filters[filter_index].is_favorite = !self.filters[filter_index].is_favorite;
                self.save_crab_file();
            }
        });
        
        ui.separator();
        
        // Search bar with favorite filters dropdown
        ui.horizontal(|ui| {
            ui.label("🔍 Search (regex):");
            
            // Collect favorite filters
            let favorites: Vec<(String, bool)> = self.filters.iter()
                .filter(|f| f.is_favorite && !f.search_text.is_empty())
                .map(|f| (f.search_text.clone(), f.case_insensitive))
                .collect();
            
            // Dropdown menu for favorites
            if !favorites.is_empty() {
                egui::ComboBox::from_id_source(format!("favorites_{}", filter_index))
                    .selected_text("⭐ Favorites")
                    .width(100.0)
                    .show_ui(ui, |ui| {
                        for (fav_text, fav_case) in favorites {
                            if ui.selectable_label(false, &fav_text).clicked() {
                                self.filters[filter_index].search_text = fav_text;
                                self.filters[filter_index].case_insensitive = fav_case;
                                self.filters[filter_index].update_search_regex();
                            }
                        }
                    });
            }
            
            // Search input with ID for Ctrl+L focusing
            let search_id = ui.id().with("search_input");
            let mut search_response = ui.add(
                egui::TextEdit::singleline(&mut self.filters[filter_index].search_text)
                    .hint_text("Enter regex pattern (e.g., ERROR|FATAL, \\d+\\.\\d+\\.\\d+\\.\\d+)")
                    .desired_width(300.0)
                    .id(search_id)
            );
            
            // Focus search input if requested by Ctrl+L
            if self.filters[filter_index].should_focus_search {
                search_response.request_focus();
                self.filters[filter_index].should_focus_search = false;
            }
            
            // If Enter is pressed in the search input, surrender focus
            if search_response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ui.memory_mut(|mem| mem.surrender_focus(search_id));
            }
            
            if search_response.changed() {
                self.filters[filter_index].update_search_regex();
            }
            
            // Checkbox
            let checkbox_response = ui.checkbox(&mut self.filters[filter_index].case_insensitive, "Case insensitive");
            
            if checkbox_response.changed() {
                self.filters[filter_index].update_search_regex();
            }
            
            if ui.button("Clear").clicked() {
                self.filters[filter_index].search_text.clear();
                self.filters[filter_index].update_search_regex();
            }
            
            if let Some(ref error) = self.filters[filter_index].regex_error {
                ui.colored_label(Color32::RED, format!("❌ {}", error));
            } else if self.filters[filter_index].search_regex.is_some() {
                ui.colored_label(Color32::GREEN, "✓ Valid regex");
            }
        });
        
        ui.separator();
        
        // Rebuild filtered indices if needed
        let mut scroll_to_row = if self.filters[filter_index].filter_dirty {
            self.filters[filter_index].rebuild_filtered_indices(
                &self.lines,
                self.min_score_filter,
                self.selected_line_index,
                self.selected_timestamp,
            )
        } else {
            None
        };
        
        // Check if selection changed
        if scroll_to_row.is_none() && self.selected_line_index.is_some() {
            if self.filters[filter_index].last_rendered_selection != self.selected_line_index {
                eprintln!("Filter {}: Selection changed from {:?} to {:?}", 
                    filter_index, 
                    self.filters[filter_index].last_rendered_selection, 
                    self.selected_line_index);
                if let Some(selected_idx) = self.selected_line_index {
                    if let Some(position) = self.filters[filter_index].filtered_indices.iter().position(|&idx| idx == selected_idx) {
                        eprintln!("Filter {}: Found exact line at position {}, will scroll", filter_index, position);
                        scroll_to_row = Some(position);
                    } else {
                        // Line not in filtered results - try to find closest by timestamp
                        if let Some(selected_ts) = self.selected_timestamp {
                            if let Some(closest_pos) = self.filters[filter_index].find_closest_timestamp_index(&self.lines, selected_ts) {
                                eprintln!("Filter {}: Line not in results, scrolling to closest timestamp at position {}", 
                                    filter_index, closest_pos);
                                scroll_to_row = Some(closest_pos);
                            } else {
                                eprintln!("Filter {}: No timestamp match found (total filtered: {})", 
                                    filter_index, 
                                    self.filters[filter_index].filtered_indices.len());
                            }
                        } else {
                            eprintln!("Filter {}: Line not in filtered results and no timestamp available", 
                                filter_index);
                        }
                        // Mark as processed so we don't keep checking on every render
                        self.filters[filter_index].last_rendered_selection = self.selected_line_index;
                    }
                }
            }
        }
        
        // Stats
        let total_lines = self.lines.len();
        let visible_lines = self.filters[filter_index].filtered_indices.len();
        
        ui.horizontal(|ui| {
            ui.label(format!("Total lines: {}", total_lines));
            ui.separator();
            ui.label(format!("Visible: {}", visible_lines));
            if self.filters[filter_index].search_regex.is_some() {
                ui.separator();
                ui.colored_label(Color32::LIGHT_BLUE, format!("🔍 {} matches", visible_lines));
            }
        });
        
        ui.separator();
        
        // Render histogram
        self.render_histogram(ui, filter_index);
        
        ui.separator();
        
        self.render_filter_table(ui, filter_index, scroll_to_row);
        
        // Handle filter name editing dialog
        if let Some(editing_id) = self.editing_bookmark {
            if editing_id >= 10000 {
                let actual_filter_index = editing_id - 10000;
                if actual_filter_index == filter_index {
                    egui::Window::new("Rename Filter")
                        .collapsible(false)
                        .resizable(false)
                        .show(ui.ctx(), |ui| {
                            ui.label("Enter filter name:");
                            let response = ui.text_edit_singleline(&mut self.bookmark_name_input);
                            response.request_focus();
                            
                            ui.horizontal(|ui| {
                                if ui.button("Save").clicked() || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                                    let new_name = if self.bookmark_name_input.trim().is_empty() {
                                        None
                                    } else {
                                        Some(self.bookmark_name_input.clone())
                                    };
                                    self.set_filter_name(filter_index, new_name);
                                    self.editing_bookmark = None;
                                    self.bookmark_name_input.clear();
                                }
                                if ui.button("Cancel").clicked() || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape))) {
                                    self.editing_bookmark = None;
                                    self.bookmark_name_input.clear();
                                }
                            });
                        });
                }
            }
        }
    }
    
    fn render_histogram(&mut self, ui: &mut Ui, filter_index: usize) {
        if self.lines.is_empty() {
            return;
        }
        
        // Get time range from filtered lines only
        let filtered_indices = &self.filters[filter_index].filtered_indices;
        if filtered_indices.is_empty() {
            ui.label("No logs match the current filter");
            return;
        }
        
        let first_ts = filtered_indices.iter()
            .find_map(|&idx| self.lines[idx].timestamp);
        let last_ts = filtered_indices.iter().rev()
            .find_map(|&idx| self.lines[idx].timestamp);
        
        if first_ts.is_none() || last_ts.is_none() {
            ui.label("No timestamps available for histogram");
            return;
        }
        
        let start_time = first_ts.unwrap();
        let end_time = last_ts.unwrap();
        let time_range = (end_time.timestamp() - start_time.timestamp()).max(1);
        
        const NUM_BUCKETS: usize = 100;
        let bucket_size = time_range as f64 / NUM_BUCKETS as f64;
        
        // Count lines per bucket (only filtered lines)
        let mut buckets = vec![0usize; NUM_BUCKETS];
        for &line_idx in &self.filters[filter_index].filtered_indices {
            if let Some(ts) = self.lines[line_idx].timestamp {
                let elapsed = (ts.timestamp() - start_time.timestamp()) as f64;
                let bucket_idx = ((elapsed / bucket_size) as usize).min(NUM_BUCKETS - 1);
                buckets[bucket_idx] += 1;
            }
        }
        
        let max_count = *buckets.iter().max().unwrap_or(&1);
        
        // Calculate selected line position if present
        let selected_bucket = if let Some(sel_idx) = self.selected_line_index {
            if sel_idx < self.lines.len() {
                if let Some(sel_ts) = self.lines[sel_idx].timestamp {
                    let elapsed = (sel_ts.timestamp() - start_time.timestamp()) as f64;
                    // Only show indicator if the selected time is within this filter's time range
                    if elapsed >= 0.0 && sel_ts.timestamp() <= end_time.timestamp() {
                        Some(((elapsed / bucket_size) as usize).min(NUM_BUCKETS - 1))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        
        // Render histogram
        let desired_size = egui::vec2(ui.available_width(), 60.0);
        let (response, painter) = ui.allocate_painter(desired_size, egui::Sense::click());
        let rect = response.rect;
        
        // Draw background
        painter.rect_filled(rect, 0.0, Color32::from_gray(20));
        
        let bar_width = rect.width() / NUM_BUCKETS as f32;
        
        // Draw bars
        for (i, &count) in buckets.iter().enumerate() {
            if count > 0 {
                let x = rect.min.x + i as f32 * bar_width;
                let height = (count as f32 / max_count as f32) * rect.height();
                let y = rect.max.y - height;
                
                let bar_rect = egui::Rect::from_min_size(
                    egui::pos2(x, y),
                    egui::vec2(bar_width.max(1.0), height),
                );
                
                // Color based on whether this bucket is selected
                let color = if Some(i) == selected_bucket {
                    Color32::from_rgb(255, 200, 100) // Orange for selected
                } else {
                    Color32::from_rgb(100, 150, 255) // Blue for normal
                };
                
                painter.rect_filled(bar_rect, 0.0, color);
            }
        }
        
        // Draw selected line indicator
        if let Some(bucket_idx) = selected_bucket {
            let x = rect.min.x + bucket_idx as f32 * bar_width + bar_width / 2.0;
            painter.vline(x, rect.y_range(), (2.0, Color32::RED));
        }
        
        // Handle clicks to jump to time
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let rel_x = (pos.x - rect.min.x) / rect.width();
                let bucket_idx = (rel_x * NUM_BUCKETS as f32) as usize;
                
                if bucket_idx < NUM_BUCKETS {
                    // Find first line in this bucket
                    let target_time = start_time.timestamp() + (bucket_idx as f64 * bucket_size) as i64;
                    
                    // Find closest filtered line to this time
                    let mut closest_idx = None;
                    let mut min_diff = i64::MAX;
                    
                    for &line_idx in &self.filters[filter_index].filtered_indices {
                        if let Some(ts) = self.lines[line_idx].timestamp {
                            let diff = (ts.timestamp() - target_time).abs();
                            if diff < min_diff {
                                min_diff = diff;
                                closest_idx = Some(line_idx);
                            }
                        }
                    }
                    
                    if let Some(idx) = closest_idx {
                        self.selected_line_index = Some(idx);
                        self.selected_timestamp = self.lines[idx].timestamp;
                    }
                }
            }
        }
        
        // Show time range below
        ui.horizontal(|ui| {
            ui.label(format!("Timeline: {} → {}", 
                start_time.format("%H:%M:%S"),
                end_time.format("%H:%M:%S")
            ));
            if let Some(sel_idx) = self.selected_line_index {
                if sel_idx < self.lines.len() {
                    if let Some(sel_ts) = self.lines[sel_idx].timestamp {
                        ui.separator();
                        ui.colored_label(Color32::YELLOW, format!("Selected: {}", sel_ts.format("%H:%M:%S%.3f")));
                    }
                }
            }
        });
    }
    
    fn render_filter_table(&mut self, ui: &mut Ui, filter_index: usize, scroll_to_row: Option<usize>) {
        let visible_lines = self.filters[filter_index].filtered_indices.len();
        
        egui::ScrollArea::horizontal()
            .id_source(format!("filtered_scroll_{}", filter_index))
            .show(ui, |ui| {
                #[cfg(feature = "cpu-profiling")]
                puffin::profile_scope!("filtered_table");
                
                let mut table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(false)
                    .sense(egui::Sense::click())
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .vscroll(true)
                    .max_scroll_height(f32::INFINITY)
                    .column(Column::initial(60.0).resizable(true).clip(true))
                    .column(Column::initial(110.0).resizable(true).clip(true))
                    .column(Column::remainder().resizable(true).clip(true))
                    .column(Column::initial(70.0).resizable(true).clip(true));
                
                if let Some(row_idx) = scroll_to_row {
                    table = table.scroll_to_row(row_idx, Some(egui::Align::Center));
                    self.filters[filter_index].last_rendered_selection = self.selected_line_index;
                }
                
                table.header(20.0, |mut header| {
                    header.col(|ui| { ui.strong("Line"); });
                    header.col(|ui| { ui.strong("Timestamp"); });
                    header.col(|ui| { ui.strong("Message"); });
                    header.col(|ui| { ui.strong("Score"); });
                })
                .body(|body| {
                    body.rows(18.0, visible_lines, |mut row| {
                        let row_index = row.index();
                        let line_idx = self.filters[filter_index].filtered_indices[row_index];
                        let line = &self.lines[line_idx];
                        
                        let is_selected = self.selected_line_index == Some(line_idx);
                        let is_bookmarked = self.bookmarks.contains_key(&line_idx);
                        let color = score_to_color(line.anomaly_score);
                        let line_number = line.line_number;
                        let timestamp_str = if let Some(ts) = line.timestamp {
                            ts.format("%H:%M:%S%.3f").to_string()
                        } else {
                            "-".to_string()
                        };
                        let message = line.message.clone();
                        let anomaly_score = line.anomaly_score;
                        let timestamp = line.timestamp;
                        
                        let mut row_clicked = false;
                        let mut row_right_clicked = false;
                        
                        let bookmark_name = if is_bookmarked {
                            self.bookmarks.get(&line_idx).map(|b| b.name.as_str())
                        } else {
                            None
                        };
                        
                        let bookmark_icon = if is_bookmarked { "★ " } else { "" };
                        let line_text = if is_selected {
                            format!("▶ {}{}", bookmark_icon, line_number)
                        } else {
                            format!("{}{}", bookmark_icon, line_number)
                        };
                        
                        self.render_table_cell(&mut row, filter_index, is_bookmarked, is_selected, &line_text, color, line_idx, "line", &mut row_clicked, &mut row_right_clicked, bookmark_name, &line.raw);
                        
                        row.col(|ui| {
                            if is_bookmarked {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            } else if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            
                            let job = self.filters[filter_index].highlight_matches(&timestamp_str, color);
                            ui.label(job);
                            
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with(filter_index).with("ts"), egui::Sense::click());
                            let response = response.on_hover_text(&line.raw);
                            if response.clicked() { row_clicked = true; }
                            if response.secondary_clicked() { row_right_clicked = true; }
                        });
                        
                        row.col(|ui| {
                            if is_bookmarked {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            } else if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            
                            let job = self.filters[filter_index].highlight_matches(&message, color);
                            ui.label(job);
                            
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with(filter_index).with("msg"), egui::Sense::click());
                            let response = response.on_hover_text(&line.raw);
                            if response.clicked() { row_clicked = true; }
                            if response.secondary_clicked() { row_right_clicked = true; }
                        });
                        
                        row.col(|ui| {
                            if is_bookmarked {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            } else if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            
                            let text = RichText::new(format!("{:.1}", anomaly_score)).strong().color(color);
                            ui.label(text);
                            
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with(filter_index).with("score"), egui::Sense::click());
                            let response = response.on_hover_text(&line.raw);
                            if response.clicked() { row_clicked = true; }
                            if response.secondary_clicked() { row_right_clicked = true; }
                        });
                        
                        if row_right_clicked {
                            self.toggle_bookmark(line_idx);
                        } else if row_clicked {
                            self.selected_line_index = Some(line_idx);
                            self.selected_timestamp = timestamp;
                        }
                    });
                });
            });
    }
    
    fn render_table_cell(&self, row: &mut egui_extras::TableRow, _filter_index: usize, is_bookmarked: bool, is_selected: bool, text: &str, color: Color32, line_idx: usize, id_suffix: &str, row_clicked: &mut bool, row_right_clicked: &mut bool, bookmark_name: Option<&str>, raw_line: &str) {
        row.col(|ui| {
            if is_bookmarked {
                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
            } else if is_selected {
                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(60, 60, 80));
            }
            
            let text = if is_selected {
                RichText::new(text).color(color).strong()
            } else {
                RichText::new(text).color(color)
            };
            let label_response = ui.label(text);
            
            // Show tooltip with bookmark name if this is the line number column and it's bookmarked
            if id_suffix == "line" && is_bookmarked {
                if let Some(name) = bookmark_name {
                    label_response.on_hover_text(format!("📑 Bookmark: {}", name));
                }
            }
            
            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with(id_suffix), egui::Sense::click());
            // Show full raw line on hover for all cells
            let response = response.on_hover_text(raw_line);
            
            if response.clicked() { *row_clicked = true; }
            if response.secondary_clicked() { *row_right_clicked = true; }
        });
    }
    
    pub fn render_context(&mut self, ui: &mut Ui) {
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_function!();
        
        ui.heading("Context View");
        ui.label("Unfiltered view centered on selected line");
        ui.separator();
        
        let Some(selected_idx) = self.selected_line_index else {
            ui.vertical_centered(|ui| {
                ui.add_space(100.0);
                ui.label("No line selected");
                ui.label("Click on a line in a filter view to see context");
            });
            return;
        };
        
        const CONTEXT_LINES: usize = 50;
        let start_idx = selected_idx.saturating_sub(CONTEXT_LINES);
        let end_idx = (selected_idx + CONTEXT_LINES + 1).min(self.lines.len());
        
        let selected_position = selected_idx - start_idx;
        let context_line_count = end_idx - start_idx;
        
        ui.label(format!("Showing lines {} - {} (selected: {})", 
            self.lines[start_idx].line_number,
            self.lines[end_idx - 1].line_number,
            self.lines[selected_idx].line_number
        ));
        ui.separator();
        
        // Render context histogram
        self.render_context_histogram(ui, start_idx, end_idx, selected_idx);
        
        ui.separator();
        
        let scroll_to_row = if self.last_rendered_selection_context != self.selected_line_index {
            self.last_rendered_selection_context = self.selected_line_index;
            Some(selected_position)
        } else {
            None
        };
        
        egui::ScrollArea::horizontal()
            .id_source("context_scroll")
            .show(ui, |ui| {
                #[cfg(feature = "cpu-profiling")]
                puffin::profile_scope!("context_table");
                
                let mut table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(false)
                    .sense(egui::Sense::click())
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .vscroll(true)
                    .max_scroll_height(f32::INFINITY)
                    .column(Column::initial(60.0).resizable(true).clip(true))
                    .column(Column::initial(110.0).resizable(true).clip(true))
                    .column(Column::remainder().resizable(true).clip(true))
                    .column(Column::initial(70.0).resizable(true).clip(true));
                
                if let Some(row_idx) = scroll_to_row {
                    table = table.scroll_to_row(row_idx, Some(egui::Align::Center));
                }
                
                table.header(20.0, |mut header| {
                    header.col(|ui| { ui.strong("Line"); });
                    header.col(|ui| { ui.strong("Timestamp"); });
                    header.col(|ui| { ui.strong("Message"); });
                    header.col(|ui| { ui.strong("Score"); });
                })
                .body(|body| {
                    body.rows(18.0, context_line_count, |mut row| {
                        let row_index = row.index();
                        let line_idx = start_idx + row_index;
                        let line = &self.lines[line_idx];
                        
                        let is_selected = line_idx == selected_idx;
                        let is_bookmarked = self.bookmarks.contains_key(&line_idx);
                        let color = score_to_color(line.anomaly_score);
                        let timestamp = line.timestamp;
                        
                        let timestamp_str = if let Some(ts) = line.timestamp {
                            ts.format("%H:%M:%S%.3f").to_string()
                        } else {
                            "-".to_string()
                        };
                        
                        let mut row_clicked = false;
                        let mut row_right_clicked = false;
                        
                        row.col(|ui| {
                            if is_bookmarked {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            } else if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            
                            let bookmark_icon = if is_bookmarked { "★ " } else { "" };
                            let text = if is_selected {
                                RichText::new(format!("▶ {}{}", bookmark_icon, line.line_number)).color(color).strong()
                            } else {
                                RichText::new(format!("{}{}", bookmark_icon, line.line_number)).color(color)
                            };
                            let label_response = ui.label(text);
                            
                            // Show bookmark name on hover if bookmarked
                            if is_bookmarked {
                                if let Some(bookmark) = self.bookmarks.get(&line_idx) {
                                    label_response.on_hover_text(format!("📑 Bookmark: {}", bookmark.name));
                                }
                            }
                            
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with("ctx_line"), egui::Sense::click());
                            let response = response.on_hover_text(&line.raw);
                            if response.clicked() { row_clicked = true; }
                            if response.secondary_clicked() { row_right_clicked = true; }
                        });
                        
                        row.col(|ui| {
                            if is_bookmarked {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            } else if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            ui.label(RichText::new(&timestamp_str).color(color));
                            
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with("ctx_ts"), egui::Sense::click());
                            let response = response.on_hover_text(&line.raw);
                            if response.clicked() { row_clicked = true; }
                            if response.secondary_clicked() { row_right_clicked = true; }
                        });
                        
                        row.col(|ui| {
                            if is_bookmarked {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            } else if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            ui.label(RichText::new(&line.message).color(color));
                            
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with("ctx_msg"), egui::Sense::click());
                            let response = response.on_hover_text(&line.raw);
                            if response.clicked() { row_clicked = true; }
                            if response.secondary_clicked() { row_right_clicked = true; }
                        });
                        
                        row.col(|ui| {
                            if is_bookmarked {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            } else if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            ui.label(RichText::new(format!("{:.1}", line.anomaly_score)).strong().color(color));
                            
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with("ctx_score"), egui::Sense::click());
                            let response = response.on_hover_text(&line.raw);
                            if response.clicked() { row_clicked = true; }
                            if response.secondary_clicked() { row_right_clicked = true; }
                        });
                        
                        if row_right_clicked {
                            self.toggle_bookmark(line_idx);
                        } else if row_clicked {
                            self.selected_line_index = Some(line_idx);
                            self.selected_timestamp = timestamp;
                        }
                    });
                });
            });
    }
    
    fn render_context_histogram(&mut self, ui: &mut Ui, start_idx: usize, end_idx: usize, selected_idx: usize) {
        if self.lines.is_empty() {
            return;
        }
        
        // Get overall time range
        let first_ts = self.lines.iter().find_map(|l| l.timestamp);
        let last_ts = self.lines.iter().rev().find_map(|l| l.timestamp);
        
        if first_ts.is_none() || last_ts.is_none() {
            return;
        }
        
        let overall_start = first_ts.unwrap();
        let overall_end = last_ts.unwrap();
        let time_range = (overall_end.timestamp() - overall_start.timestamp()).max(1);
        
        const NUM_BUCKETS: usize = 100;
        let bucket_size = time_range as f64 / NUM_BUCKETS as f64;
        
        // Count ALL lines per bucket
        let mut buckets = vec![0usize; NUM_BUCKETS];
        for line in &self.lines {
            if let Some(ts) = line.timestamp {
                let elapsed = (ts.timestamp() - overall_start.timestamp()) as f64;
                let bucket_idx = ((elapsed / bucket_size) as usize).min(NUM_BUCKETS - 1);
                buckets[bucket_idx] += 1;
            }
        }
        
        let max_count = *buckets.iter().max().unwrap_or(&1);
        
        // Calculate visible window range
        let window_start = self.lines[start_idx].timestamp;
        let window_end = self.lines[end_idx - 1].timestamp;
        let selected_ts = self.lines[selected_idx].timestamp;
        
        let window_start_bucket = window_start.map(|ts| {
            let elapsed = (ts.timestamp() - overall_start.timestamp()) as f64;
            ((elapsed / bucket_size) as usize).min(NUM_BUCKETS - 1)
        });
        
        let window_end_bucket = window_end.map(|ts| {
            let elapsed = (ts.timestamp() - overall_start.timestamp()) as f64;
            ((elapsed / bucket_size) as usize).min(NUM_BUCKETS - 1)
        });
        
        let selected_bucket = selected_ts.map(|ts| {
            let elapsed = (ts.timestamp() - overall_start.timestamp()) as f64;
            ((elapsed / bucket_size) as usize).min(NUM_BUCKETS - 1)
        });
        
        // Render histogram
        let desired_size = egui::vec2(ui.available_width(), 40.0);
        let (response, painter) = ui.allocate_painter(desired_size, egui::Sense::click());
        let rect = response.rect;
        
        // Draw background
        painter.rect_filled(rect, 0.0, Color32::from_gray(20));
        
        let bar_width = rect.width() / NUM_BUCKETS as f32;
        
        // Draw bars
        for (i, &count) in buckets.iter().enumerate() {
            if count > 0 {
                let x = rect.min.x + i as f32 * bar_width;
                let height = (count as f32 / max_count as f32) * rect.height();
                let y = rect.max.y - height;
                
                let bar_rect = egui::Rect::from_min_size(
                    egui::pos2(x, y),
                    egui::vec2(bar_width.max(1.0), height),
                );
                
                // Color: gray for normal, highlighted for visible window
                let in_window = if let (Some(start_b), Some(end_b)) = (window_start_bucket, window_end_bucket) {
                    i >= start_b && i <= end_b
                } else {
                    false
                };
                
                let color = if in_window {
                    Color32::from_rgb(150, 200, 255) // Light blue for visible window
                } else {
                    Color32::from_gray(80) // Gray for rest
                };
                
                painter.rect_filled(bar_rect, 0.0, color);
            }
        }
        
        // Draw visible window overlay
        if let (Some(start_b), Some(end_b)) = (window_start_bucket, window_end_bucket) {
            let start_x = rect.min.x + start_b as f32 * bar_width;
            let end_x = rect.min.x + (end_b + 1) as f32 * bar_width;
            let window_rect = egui::Rect::from_min_max(
                egui::pos2(start_x, rect.min.y),
                egui::pos2(end_x, rect.max.y),
            );
            painter.rect_stroke(window_rect, 0.0, (2.0, Color32::YELLOW));
        }
        
        // Draw selected line indicator
        if let Some(bucket_idx) = selected_bucket {
            let x = rect.min.x + bucket_idx as f32 * bar_width + bar_width / 2.0;
            painter.vline(x, rect.y_range(), (2.0, Color32::RED));
        }
        
        // Handle clicks to jump to time
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let rel_x = (pos.x - rect.min.x) / rect.width();
                let bucket_idx = (rel_x * NUM_BUCKETS as f32) as usize;
                
                if bucket_idx < NUM_BUCKETS {
                    let target_time = overall_start.timestamp() + (bucket_idx as f64 * bucket_size) as i64;
                    
                    // Find closest line to this time
                    let mut closest_idx = None;
                    let mut min_diff = i64::MAX;
                    
                    for (idx, line) in self.lines.iter().enumerate() {
                        if let Some(ts) = line.timestamp {
                            let diff = (ts.timestamp() - target_time).abs();
                            if diff < min_diff {
                                min_diff = diff;
                                closest_idx = Some(idx);
                            }
                        }
                    }
                    
                    if let Some(idx) = closest_idx {
                        self.selected_line_index = Some(idx);
                        self.selected_timestamp = self.lines[idx].timestamp;
                    }
                }
            }
        }
    }
    
    pub fn render_bookmarks(&mut self, ui: &mut Ui) {
        ui.heading("Bookmarks");
        ui.separator();
        
        if self.bookmarks.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(50.0);
                ui.label("No bookmarks yet");
                ui.label("Right-click on any line to bookmark it");
            });
            return;
        }
        
        let mut bookmarks: Vec<_> = self.bookmarks.values().cloned().collect();
        bookmarks.sort_by_key(|b| b.line_index);
        
        let mut to_delete = None;
        let mut to_jump = None;
        let mut to_rename = None;
        let mut should_save = false;
        
        ui.label(format!("Total bookmarks: {}", bookmarks.len()));
        ui.separator();
        
        egui::ScrollArea::horizontal()
            .id_source("bookmarks_scroll")
            .show(ui, |ui| {
                let mut table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(false)
                    .sense(egui::Sense::click())
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .vscroll(true)
                    .max_scroll_height(f32::INFINITY)
                    .column(Column::initial(60.0).resizable(true).clip(true))
                    .column(Column::initial(110.0).resizable(true).clip(true))
                    .column(Column::initial(200.0).resizable(true).clip(true))
                    .column(Column::remainder().resizable(true).clip(true))
                    .column(Column::initial(80.0).resizable(true).clip(true));
                
                table.header(20.0, |mut header| {
                    header.col(|ui| { ui.strong("Line"); });
                    header.col(|ui| { ui.strong("Timestamp"); });
                    header.col(|ui| { ui.strong("Name"); });
                    header.col(|ui| { ui.strong("Message"); });
                    header.col(|ui| { ui.strong("Actions"); });
                })
                .body(|body| {
                    body.rows(18.0, bookmarks.len(), |mut row| {
                        let row_index = row.index();
                        let bookmark = &bookmarks[row_index];
                        let line_idx = bookmark.line_index;
                        
                        let is_selected = self.selected_line_index == Some(line_idx);
                        let color = if line_idx < self.lines.len() {
                            score_to_color(self.lines[line_idx].anomaly_score)
                        } else {
                            Color32::WHITE
                        };
                        
                        let line_number = if line_idx < self.lines.len() {
                            self.lines[line_idx].line_number
                        } else {
                            line_idx
                        };
                        
                        let timestamp_str = if let Some(ts) = bookmark.timestamp {
                            ts.format("%H:%M:%S%.3f").to_string()
                        } else {
                            "-".to_string()
                        };
                        
                        let message = if line_idx < self.lines.len() {
                            self.lines[line_idx].message.clone()
                        } else {
                            String::new()
                        };
                        
                        let mut row_clicked = false;
                        
                        // Line number
                        row.col(|ui| {
                            if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            }
                            let text = if is_selected {
                                RichText::new(format!("★ ▶ {}", line_number)).color(color).strong()
                            } else {
                                RichText::new(format!("★ {}", line_number)).color(color)
                            };
                            ui.label(text);
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with("bm_line"), egui::Sense::click());
                            if response.clicked() { row_clicked = true; }
                        });
                        
                        // Timestamp
                        row.col(|ui| {
                            if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            }
                            ui.label(RichText::new(&timestamp_str).color(color));
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with("bm_ts"), egui::Sense::click());
                            if response.clicked() { row_clicked = true; }
                        });
                        
                        // Name
                        row.col(|ui| {
                            if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            }
                            
                            // Editable name field
                            if self.editing_bookmark == Some(line_idx) {
                                let response = ui.add(
                                    egui::TextEdit::singleline(&mut self.bookmark_name_input)
                                        .desired_width(ui.available_width() - 50.0)
                                );
                                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    if !self.bookmark_name_input.is_empty() {
                                        if let Some(b) = self.bookmarks.get_mut(&line_idx) {
                                            b.name = self.bookmark_name_input.clone();
                                            should_save = true;
                                        }
                                    }
                                    self.editing_bookmark = None;
                                    self.bookmark_name_input.clear();
                                }
                                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    self.editing_bookmark = None;
                                    self.bookmark_name_input.clear();
                                }
                            } else {
                                ui.label(RichText::new(&bookmark.name).color(color).strong());
                                let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with("bm_name"), egui::Sense::click());
                                if response.double_clicked() {
                                    to_rename = Some(line_idx);
                                } else if response.clicked() {
                                    row_clicked = true;
                                }
                            }
                        });
                        
                        // Message
                        row.col(|ui| {
                            if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            }
                            ui.label(RichText::new(&message).color(color));
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with("bm_msg"), egui::Sense::click());
                            if response.clicked() { row_clicked = true; }
                        });
                        
                        // Actions
                        row.col(|ui| {
                            if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            }
                            ui.horizontal(|ui| {
                                if ui.small_button("✏").on_hover_text("Rename").clicked() {
                                    to_rename = Some(line_idx);
                                }
                                if ui.small_button("🗑").on_hover_text("Delete").clicked() {
                                    to_delete = Some(line_idx);
                                }
                            });
                        });
                        
                        if row_clicked {
                            to_jump = Some((line_idx, bookmark.timestamp));
                        }
                    });
                });
            });
        
        // Handle rename action
        if let Some(line_idx) = to_rename {
            self.editing_bookmark = Some(line_idx);
            if let Some(bookmark) = self.bookmarks.get(&line_idx) {
                self.bookmark_name_input = bookmark.name.clone();
            }
        }
        
        // Apply actions
        if let Some(line_idx) = to_delete {
            self.bookmarks.remove(&line_idx);
            should_save = true;
        }
        
        if let Some((line_idx, timestamp)) = to_jump {
            self.selected_line_index = Some(line_idx);
            self.selected_timestamp = timestamp;
        }
        
        if should_save {
            self.save_crab_file();
        }
    }
}

fn score_to_color(score: f64) -> Color32 {
    if score >= 80.0 {
        Color32::from_rgb(255, 100, 100)
    } else if score >= 60.0 {
        Color32::from_rgb(255, 180, 100)
    } else if score >= 30.0 {
        Color32::from_rgb(255, 200, 200)
    } else {
        Color32::LIGHT_GRAY
    }
}
