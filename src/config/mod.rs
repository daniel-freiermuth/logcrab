// LogCrab - GPL-3.0-or-later
// This file is part of LogCrab.
//
// Copyright (C) 2026 Daniel Freiermuth
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
    /// Use storage header timestamp (wall-clock time)
    #[default]
    StorageTime,
    /// Use inferred monotonic clock (boot time + header timestamp, more precise in limited timespans)
    InferredMonotonic,
}

/// Global user configuration stored in config directory
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    /// Per-format file type configuration (e.g. DLT timestamp source).
    /// Serialized to the global config file so settings persist across sessions.
    #[serde(default)]
    pub file_config: crate::core::log_store::GlobalFileConfig,

    /// Show bookmarks as markers in the timeline/histogram (default: false)
    #[serde(default)]
    pub show_bookmarks_in_timeline: bool,

    /// Use LogBERT sidecar for anomaly scoring (default: false)
    #[serde(default)]
    pub use_sidecar_scoring: bool,

    /// Color logs by ML score instead of legacy scorer (default: false)
    #[serde(default)]
    pub color_by_ml_score: bool,

    /// Sidecar server host
    #[serde(default = "default_sidecar_host")]
    pub sidecar_host: String,

    /// Sidecar server port
    #[serde(default = "default_sidecar_port")]
    pub sidecar_port: u16,

    /// Selected model id (slug) for anomaly detection.
    /// `None` means no model is selected; sidecar scoring will be skipped.
    #[serde(default)]
    pub selected_model: Option<String>,
}

fn default_sidecar_host() -> String {
    crate::anomaly::sidecar_client::SidecarClient::default_host().to_string()
}

const fn default_sidecar_port() -> u16 {
    crate::anomaly::sidecar_client::SidecarClient::default_port()
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            shortcuts: HashMap::new(),
            favorite_filters: Vec::new(),
            bright_mode: false,
            last_log_directory: None,
            last_filters_directory: None,
            file_config: crate::core::log_store::GlobalFileConfig::default(),
            show_bookmarks_in_timeline: false,
            use_sidecar_scoring: false,
            color_by_ml_score: false,
            sidecar_host: default_sidecar_host(),
            sidecar_port: default_sidecar_port(),
            selected_model: None,
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
                tracing::info!("Loading global config from {}", path.display());
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    if let Ok(config) = serde_json::from_str::<Self>(&contents) {
                        tracing::info!(
                            "Loaded {} shortcuts and {} favorite filters",
                            config.shortcuts.len(),
                            config.favorite_filters.len()
                        );
                        return config;
                    }
                }
            } else {
                tracing::info!("No global config found, using defaults");
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

        tracing::info!("Saved global config to {}", path.display());
        Ok(())
    }
}
