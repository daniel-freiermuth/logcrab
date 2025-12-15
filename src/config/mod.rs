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

use crate::input::ShortcutAction;
use crate::ui::tabs::filter_tab::filter_state::FilterState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Global user configuration stored in config directory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Keyboard shortcuts
    #[serde(default)]
    pub shortcuts: HashMap<ShortcutAction, String>,

    /// Favorite filters that appear in all sessions
    #[serde(default)]
    pub favorite_filters: Vec<FavoriteFilter>,

    /// Hide January 1st timestamps from histogram (default: true)
    #[serde(default = "default_hide_epoch")]
    pub hide_epoch_in_histogram: bool,

    /// Use bright/light theme instead of dark (default: false)
    #[serde(default)]
    pub bright_mode: bool,

    /// Last directory used for opening log files
    #[serde(default)]
    pub last_log_directory: Option<PathBuf>,

    /// Last directory used for filter files (import/export)
    #[serde(default)]
    pub last_filters_directory: Option<PathBuf>,
}

const fn default_hide_epoch() -> bool {
    true
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            shortcuts: HashMap::new(),
            favorite_filters: Vec::new(),
            hide_epoch_in_histogram: true,
            bright_mode: false,
            last_log_directory: None,
            last_filters_directory: None,
        }
    }
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

    pub fn matches(&self, filter: &FilterState) -> bool {
        self.search_text == filter.search.search_text && self.case_sensitive == filter.search.case_sensitive
    }
}

impl GlobalConfig {
    /// Get the path to the global config file
    pub fn config_path() -> Option<PathBuf> {
        if let Some(config_dir) = dirs::config_dir() {
            let app_config = config_dir.join("logcrab");
            Some(app_config.join("config.json"))
        } else {
            None
        }
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
