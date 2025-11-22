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

use chrono::{DateTime, Local};
use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;
use serde::{Serialize, Deserialize};

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
    pub search_text: String,
    pub case_insensitive: bool,
    pub is_favorite: bool,
    #[serde(default)]
    pub name: Option<String>,
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

/// Manages session data including bookmarks and saved filters
/// Data is persisted to a .crab file alongside the log file
pub struct Session {
    bookmarks: HashMap<usize, Bookmark>,
    crab_file_path: Option<PathBuf>,
}

impl Session {
    /// Create a new session without an associated file
    pub fn new() -> Self {
        Session {
            bookmarks: HashMap::new(),
            crab_file_path: None,
        }
    }
    
    /// Create a session and associate it with a log file
    /// Automatically loads data from the .crab file if it exists
    pub fn from_log_file(log_file_path: PathBuf) -> Self {
        let crab_path = log_file_path.with_extension("crab");
        let mut session = Session {
            bookmarks: HashMap::new(),
            crab_file_path: Some(crab_path),
        };
        session.load_from_file();
        session
    }
    
    /// Load session data from the .crab file
    pub fn load_from_file(&mut self) -> Vec<SavedFilter> {
        self.bookmarks.clear();
        
        let saved_filters = if let Some(ref path) = self.crab_file_path {
            if let Ok(file_content) = fs::read_to_string(path) {
                if let Ok(crab_data) = serde_json::from_str::<CrabFile>(&file_content) {
                    // Load bookmarks
                    for bookmark in crab_data.bookmarks {
                        self.bookmarks.insert(bookmark.line_index, bookmark);
                    }
                    
                    // Return saved filters for the caller to handle
                    crab_data.filters
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        
        saved_filters
    }
    
    /// Save session data to the .crab file
    pub fn save_to_file(&self, saved_filters: &[SavedFilter]) {
        if let Some(ref path) = self.crab_file_path {
            let crab_data = CrabFile {
                bookmarks: self.bookmarks.values().cloned().collect(),
                filters: saved_filters.to_vec(),
            };
            
            if let Ok(json) = serde_json::to_string_pretty(&crab_data) {
                let _ = fs::write(path, json);
            }
        }
    }
    
    /// Add or update a bookmark
    pub fn add_bookmark(&mut self, line_index: usize, name: String, timestamp: Option<DateTime<Local>>) {
        self.bookmarks.insert(line_index, Bookmark {
            line_index,
            name,
            timestamp,
        });
    }
    
    /// Remove a bookmark
    pub fn remove_bookmark(&mut self, line_index: usize) -> bool {
        self.bookmarks.remove(&line_index).is_some()
    }
    
    /// Toggle a bookmark (add if not present, remove if present)
    pub fn toggle_bookmark(&mut self, line_index: usize, name: String, timestamp: Option<DateTime<Local>>) -> bool {
        if self.bookmarks.contains_key(&line_index) {
            self.remove_bookmark(line_index);
            false // Removed
        } else {
            self.add_bookmark(line_index, name, timestamp);
            true // Added
        }
    }
    
    /// Rename a bookmark
    pub fn rename_bookmark(&mut self, line_index: usize, new_name: String) -> bool {
        if let Some(bookmark) = self.bookmarks.get_mut(&line_index) {
            bookmark.name = new_name;
            true
        } else {
            false
        }
    }
    
    /// Check if a line is bookmarked
    pub fn is_bookmarked(&self, line_index: usize) -> bool {
        self.bookmarks.contains_key(&line_index)
    }
    
    /// Get a bookmark by line index
    pub fn get_bookmark(&self, line_index: usize) -> Option<&Bookmark> {
        self.bookmarks.get(&line_index)
    }
    
    /// Get all bookmarks sorted by line index
    pub fn get_all_bookmarks(&self) -> Vec<&Bookmark> {
        let mut bookmarks: Vec<_> = self.bookmarks.values().collect();
        bookmarks.sort_by_key(|b| b.line_index);
        bookmarks
    }
    
    /// Get the number of bookmarks
    pub fn bookmark_count(&self) -> usize {
        self.bookmarks.len()
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_session_bookmarks() {
        let mut session = Session::new();
        
        session.add_bookmark(42, "Important error".to_string(), None);
        assert!(session.is_bookmarked(42));
        assert_eq!(session.bookmark_count(), 1);
        
        let bookmark = session.get_bookmark(42).unwrap();
        assert_eq!(bookmark.name, "Important error");
        
        session.rename_bookmark(42, "Critical error".to_string());
        let bookmark = session.get_bookmark(42).unwrap();
        assert_eq!(bookmark.name, "Critical error");
        
        session.remove_bookmark(42);
        assert!(!session.is_bookmarked(42));
        assert_eq!(session.bookmark_count(), 0);
    }
    
    #[test]
    fn test_toggle_bookmark() {
        let mut session = Session::new();
        
        let added = session.toggle_bookmark(10, "Test".to_string(), None);
        assert!(added);
        assert!(session.is_bookmarked(10));
        
        let removed = session.toggle_bookmark(10, "Test".to_string(), None);
        assert!(!removed);
        assert!(!session.is_bookmarked(10));
    }
}
