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
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Global user configuration stored in config directory
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalConfig {
    /// Keyboard shortcuts
    #[serde(default)]
    pub shortcuts: HashMap<ShortcutAction, String>,

    /// Favorite filters that appear in all sessions
    #[serde(default)]
    pub favorite_filters: Vec<FavoriteFilter>,
}

/// A favorite filter that can be quickly added to any log
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FavoriteFilter {
    pub name: String,
    pub search_text: String,
    pub case_insensitive: bool,
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
                log::info!("Loading global config from {:?}", path);
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    if let Ok(config) = serde_json::from_str::<GlobalConfig>(&contents) {
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
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        // Serialize to JSON
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        // Write to file
        std::fs::write(&path, json).map_err(|e| format!("Failed to write config file: {}", e))?;

        log::info!("Saved global config to {:?}", path);
        Ok(())
    }
}
