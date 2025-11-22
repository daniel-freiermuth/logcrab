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
use crate::state::FilterState;
use crate::ui::views::{FilterView, FilterViewEvent, BookmarksView, BookmarksViewEvent};
use crate::ui::components::BookmarkData;
use egui::{Color32, Ui};

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

pub struct LogView {
    pub lines: Vec<LogLine>,
    pub min_score_filter: f64,
    // Multiple filter views
    filters: Vec<FilterState>,
    // Selected line tracking
    selected_line_index: Option<usize>,
    selected_timestamp: Option<DateTime<Local>>,
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
            FilterState::new(Color32::YELLOW),
            FilterState::new(Color32::LIGHT_BLUE),
        ];
        
        LogView {
            lines: Vec::new(),
            min_score_filter: 0.0,
            filters,
            selected_line_index: None,
            selected_timestamp: None,
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
        self.filters.push(FilterState::new(color));
    }
    
    /// Focus the search input for a specific filter (called by Ctrl+L)
    pub fn focus_search_input(&mut self, filter_index: usize) {
        if filter_index < self.filters.len() {
            self.filters[filter_index].should_focus_search = true;
        }
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
        
        // Convert bookmarks HashMap to simple HashMap<usize, String> for the component
        let bookmarked_lines: HashMap<usize, String> = self.bookmarks.iter()
            .map(|(&idx, bookmark)| (idx, bookmark.name.clone()))
            .collect();
        
        // Collect filter data needed for favorites (avoiding borrow issues)
        let all_filters_data: Vec<(String, bool, bool)> = self.filters.iter()
            .map(|f| (f.search_text.clone(), f.case_insensitive, f.is_favorite))
            .collect();
        
        // Temporarily take out the filter we're rendering
        let mut current_filter = std::mem::replace(
            &mut self.filters[filter_index], 
            FilterState::new(Color32::YELLOW)
        );
        
        // Create a temporary filters list for favorites lookup
        let temp_filters: Vec<FilterState> = all_filters_data.iter().map(|(search, case_ins, fav)| {
            let mut f = FilterState::new(Color32::YELLOW);
            f.search_text = search.clone();
            f.case_insensitive = *case_ins;
            f.is_favorite = *fav;
            f
        }).collect();
        
        // Render using FilterView
        let events = FilterView::render(
            ui,
            &self.lines,
            &mut current_filter,
            filter_index,
            &temp_filters,
            self.selected_line_index,
            self.selected_timestamp,
            &bookmarked_lines,
            self.min_score_filter,
        );
        
        // Put the filter back
        self.filters[filter_index] = current_filter;
        
        // Handle events
        for event in events {
            match event {
                FilterViewEvent::LineSelected { line_index, timestamp } => {
                    self.selected_line_index = Some(line_index);
                    self.selected_timestamp = timestamp;
                }
                FilterViewEvent::BookmarkToggled { line_index } => {
                    self.toggle_bookmark(line_index);
                }
                FilterViewEvent::FilterNameEditRequested => {
                    // Prompt for new name
                    if let Some(current_name) = &self.filters[filter_index].name {
                        self.bookmark_name_input = current_name.clone();
                    } else {
                        self.bookmark_name_input = format!("Filter {}", filter_index + 1);
                    }
                    self.editing_bookmark = Some(filter_index + 10000); // Use high number to distinguish from bookmarks
                }
                FilterViewEvent::FavoriteToggled => {
                    self.filters[filter_index].is_favorite = !self.filters[filter_index].is_favorite;
                    self.save_crab_file();
                }
            }
        }
        
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
    
    
    pub fn render_bookmarks(&mut self, ui: &mut Ui) {
        // Convert bookmarks to BookmarkData format
        let mut bookmarks: Vec<BookmarkData> = self.bookmarks.values()
            .map(|b| BookmarkData {
                line_index: b.line_index,
                name: b.name.clone(),
                timestamp: b.timestamp,
            })
            .collect();
        bookmarks.sort_by_key(|b| b.line_index);
        
        // Render using BookmarksView
        let events = BookmarksView::render(
            ui,
            &self.lines,
            bookmarks,
            self.selected_line_index,
            self.editing_bookmark,
            &mut self.bookmark_name_input,
        );
        
        // Handle events
        let mut should_save = false;
        for event in events {
            match event {
                BookmarksViewEvent::BookmarkClicked { line_index, timestamp } => {
                    self.selected_line_index = Some(line_index);
                    self.selected_timestamp = timestamp;
                }
                BookmarksViewEvent::BookmarkDeleted { line_index } => {
                    self.bookmarks.remove(&line_index);
                    should_save = true;
                }
                BookmarksViewEvent::BookmarkRenamed { line_index, new_name } => {
                    if let Some(b) = self.bookmarks.get_mut(&line_index) {
                        b.name = new_name;
                        should_save = true;
                    }
                    self.editing_bookmark = None;
                    self.bookmark_name_input.clear();
                }
                BookmarksViewEvent::StartRenaming { line_index } => {
                    self.editing_bookmark = Some(line_index);
                    if let Some(bookmark) = self.bookmarks.get(&line_index) {
                        self.bookmark_name_input = bookmark.name.clone();
                    }
                }
            }
        }
        
        if should_save {
            self.save_crab_file();
        }
    }
}
