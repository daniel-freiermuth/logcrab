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
use crate::ui::windows::ChangeFilternameWindow;
use egui::Color32;

use chrono::{DateTime, Local};
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
struct SavedFilter {
    search_text: String,
    case_insensitive: bool,
    #[serde(default)]
    name: Option<String>,
}

/// .crab file format - stores all session data
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrabFile {
    bookmarks: Vec<Bookmark>,
    filters: Vec<SavedFilter>,
}

pub struct LogView {
    pub lines: Arc<Vec<LogLine>>,
    // Multiple filter views
    pub filters: Vec<FilterState>,
    // Selected line tracking
    pub selected_line_index: Option<usize>,
    // Bookmarks with names
    pub bookmarks: HashMap<usize, Bookmark>,
    // .crab file path
    crab_file: Option<PathBuf>,
    pub change_filtername_window: Option<ChangeFilternameWindow>,
}

impl LogView {
    pub fn new() -> Self {
        // Start with 2 filters by default (yellow and light blue highlights)
        let filters = vec![
            FilterState::new(Color32::YELLOW),
            FilterState::new(Color32::LIGHT_BLUE),
        ];

        LogView {
            lines: Arc::new(Vec::new()),
            filters,
            selected_line_index: None,
            bookmarks: HashMap::new(),
            crab_file: None,
            change_filtername_window: None,
        }
    }

    pub fn add_filter(&mut self) {
        log::debug!("Adding new filter (index: {})", self.filters.len());
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
        self.save_crab_file();
    }

    pub fn remove_filter(&mut self, index: usize) {
        log::debug!("Removing filter at index: {}", index);
        self.filters.remove(index);
        self.save_crab_file();
    }

    /// Check if any filter is currently processing in the background
    /// Also checks for completed filter results to update status
    pub fn is_any_filter_active(&mut self) -> bool {
        // Check all filters for completed results, even if they're not being rendered
        for filter in &mut self.filters {
            filter.check_filter_results();
        }
        // Return true if any filter is still processing
        self.filters.iter().any(|f| f.is_filtering)
    }

    /// Start renaming a filter (opens the rename dialog)
    pub fn start_rename_filter(&mut self, filter_index: usize) {
        let starting_name = if let Some(current_name) = &self.filters[filter_index].name {
            current_name.clone()
        } else {
            format!("Filter {}", filter_index + 1)
        };
        self.change_filtername_window = Some(ChangeFilternameWindow::new(starting_name));
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

    pub fn set_lines(&mut self, lines: Arc<Vec<LogLine>>) {
        log::info!(
            "Setting {} log lines, requesting background filtering for {} filters",
            lines.len(),
            self.filters.len()
        );
        self.lines = lines;
        // Request background filtering for all filters
        for filter in &mut self.filters {
            filter.request_filter_update(Arc::clone(&self.lines));
        }
    }

    pub fn set_bookmarks_file(&mut self, log_file_path: PathBuf) -> usize {
        let crab_path = log_file_path.with_extension("crab");
        self.crab_file = Some(crab_path.clone());
        let initial_filter_count = self.filters.len();
        self.load_crab_file();

        // Request filter updates for any newly loaded or restored filters
        // This ensures filters loaded from .crab file start background filtering immediately
        for filter in &mut self.filters {
            if filter.filter_dirty {
                filter.request_filter_update(Arc::clone(&self.lines));
            }
        }

        // Return how many filters we have after loading
        self.filters.len().saturating_sub(initial_filter_count)
    }

    fn load_crab_file(&mut self) {
        self.bookmarks.clear();

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
                        self.filters[i].name = saved_filter.name.clone();
                        self.filters[i].update_search_regex();
                        log::debug!("Restored filter {}: '{}'", i, saved_filter.search_text);
                    }
                } else {
                    log::warn!("Failed to parse .crab file: {:?}", path);
                }
            } else {
                log::debug!(".crab file does not exist yet: {:?}", path);
            }
        }
    }

    pub fn save_crab_file(&self) {
        if let Some(ref path) = self.crab_file {
            log::debug!("Saving .crab file: {:?}", path);
            let crab_data = CrabFile {
                bookmarks: self.bookmarks.values().cloned().collect(),
                filters: self
                    .filters
                    .iter()
                    .map(|f| SavedFilter {
                        search_text: f.search_text.clone(),
                        case_insensitive: f.case_insensitive,
                        name: f.name.clone(),
                    })
                    .collect(),
            };

            if let Ok(json) = serde_json::to_string_pretty(&crab_data) {
                match fs::write(path, json) {
                    Ok(_) => log::debug!(
                        "Successfully saved .crab file with {} bookmarks, {} filters",
                        self.bookmarks.len(),
                        self.filters.len()
                    ),
                    Err(e) => log::error!("Failed to save .crab file: {}", e),
                }
            }
        }
    }

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
        self.save_crab_file();
    }

    /// Toggle bookmark for the currently selected line
    pub fn toggle_bookmark_for_selected(&mut self) {
        if let Some(selected_idx) = self.selected_line_index {
            self.toggle_bookmark(selected_idx);
        }
    }
}
