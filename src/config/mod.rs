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

use crate::core::SearchRule;
use crate::input::ShortcutAction;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// DLT timestamp source configuration
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DltTimestampSource {
    /// Use storage header timestamp (wall-clock time, less precise)
    StorageTime,
    /// Use calibrated monotonic clock (boot time + header timestamp, more precise)
    #[default]
    CalibratedMonotonic,
}

/// Global user configuration stored in config directory
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Keyboard shortcuts
    #[serde(default)]
    pub shortcuts: HashMap<ShortcutAction, String>,

    /// Favorite filters that appear in all sessions
    #[serde(default)]
    pub favorite_filters: Vec<FavoriteFilter>,

    /// Use bright/light theme instead of dark (default: false)
    #[serde(default)]
    pub bright_mode: bool,

    /// Last directory used for opening log files
    #[serde(default)]
    pub last_log_directory: Option<PathBuf>,

    /// Last directory used for filter files (import/export)
    #[serde(default)]
    pub last_filters_directory: Option<PathBuf>,

    /// DLT timestamp source (storage time or calibrated monotonic clock)
    #[serde(default)]
    pub dlt_timestamp_source: DltTimestampSource,

    /// Show bookmarks as markers in the timeline/histogram (default: false)
    #[serde(default)]
    pub show_bookmarks_in_timeline: bool,
}

/// A favorite filter that can be quickly added to any log
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FavoriteFilter {
    pub search_text: String,
    pub case_sensitive: bool,
    #[serde(default)]
    pub name: String,
}

impl FavoriteFilter {
    /// Create a new favorite with the given parameters, using `search_text` as the default name
    pub fn new(search_text: String, case_sensitive: bool) -> Self {
        let name = search_text.clone();
        Self {
            search_text,
            case_sensitive,
            name,
        }
    }

    /// Get the display name for this favorite (returns name if set, otherwise `search_text`)
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.search_text
        } else {
            &self.name
        }
    }

    /// Check if this favorite matches a search rule's search criteria.
    pub fn matches(&self, rule: &SearchRule) -> bool {
        rule.matches_search(&self.search_text, self.case_sensitive)
    }
}

impl GlobalConfig {
    /// Get the path to the global config file
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|config_dir| {
            let app_config = config_dir.join("logcrab");
            app_config.join("config.json")
        })
    }

    /// Load global config from disk, returning defaults if not found
    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            if path.exists() {
                log::info!("Loading global config from {}", path.display());
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    if let Ok(config) = serde_json::from_str::<Self>(&contents) {
                        log::info!(
                            "Loaded {} shortcuts and {} favorite filters",
                            config.shortcuts.len(),
                            config.favorite_filters.len()
                        );
                        return config;
                    }
                }
            } else {
                log::info!("No global config found, using defaults");
            }
        }

        Self::default()
    }

    /// Save global config to disk
    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path().ok_or("Could not determine config directory")?;

        // Create directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {e}"))?;
        }

        // Serialize to JSON
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {e}"))?;

        // Write to file
        std::fs::write(&path, json).map_err(|e| format!("Failed to write config file: {e}"))?;

        log::info!("Saved global config to {}", path.display());
        Ok(())
    }
}
