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

/// Current schema version. Bump this whenever the config format changes in a
/// backwards-incompatible way. Old binaries that don't know this version will
/// fall back to defaults on load rather than silently corrupting the file.
pub const SCHEMA_VERSION: u32 = 1;

/// Global user configuration stored in config directory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Schema version — no `#[serde(default)]` so that configs written by old
    /// binaries (which lack this field) fail to deserialize and fall back to
    /// defaults rather than being silently misread.
    pub schema_version: u32,

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

    /// If `true`, `save()` is a no-op. Set when the on-disk config was written
    /// by a newer binary (version > SCHEMA_VERSION) so we never silently
    /// downgrade it.
    #[serde(skip)]
    pub read_only: bool,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            read_only: false,
            shortcuts: HashMap::new(),
            favorite_filters: Vec::new(),
            bright_mode: false,
            last_log_directory: None,
            last_filters_directory: None,
            file_config: crate::core::log_store::GlobalFileConfig::default(),
            show_bookmarks_in_timeline: false,
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

    /// Load global config from disk.
    ///
    /// - **Missing `schema_version`**: treated as v0, migrated to current.
    /// - **version < current**: deserialized, then migration logic runs.
    /// - **version == current**: deserialized as-is.
    /// - **version > current**: falls back to defaults with `read_only = true`
    ///   so `save()` will not overwrite the newer-version file.
    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            if path.exists() {
                tracing::info!("Loading global config from {}", path.display());
                match std::fs::read_to_string(&path) {
                    Err(e) => {
                        tracing::warn!("Failed to read config file: {e}");
                    }
                    Ok(contents) => {
                        // Peek at just the version field first — works even if
                        // the rest of the schema changed completely.
                        #[derive(Deserialize)]
                        struct VersionProbe {
                            schema_version: Option<u32>,
                        }
                        let file_version = serde_json::from_str::<VersionProbe>(&contents)
                            .map(|p| p.schema_version.unwrap_or(0))
                            .unwrap_or(0);

                        if file_version > SCHEMA_VERSION {
                            tracing::warn!(
                                "Config schema version {} is newer than this binary's {} — \
                                 using defaults (read-only: will not overwrite)",
                                file_version,
                                SCHEMA_VERSION
                            );
                            return Self {
                                read_only: true,
                                ..Self::default()
                            };
                        }

                        // v0 = old binary that never wrote schema_version: inject
                        // the field so the struct deserializes without error.
                        let parse_result = if file_version == 0 {
                            tracing::info!(
                                "Config has no schema_version, treating as v0 and migrating"
                            );
                            serde_json::from_str::<serde_json::Value>(&contents)
                                .ok()
                                .and_then(|mut v| {
                                    v.as_object_mut()?.insert(
                                        "schema_version".to_string(),
                                        serde_json::json!(0u32),
                                    );
                                    serde_json::from_value::<Self>(v).ok()
                                })
                        } else {
                            serde_json::from_str::<Self>(&contents).ok()
                        };

                        match parse_result {
                            None => {
                                tracing::warn!("Failed to parse config, using defaults");
                            }
                            Some(mut config) => {
                                if config.schema_version < SCHEMA_VERSION {
                                    // Placeholder for future field migrations.
                                    tracing::info!(
                                        "Migrated config from schema v{} to v{}",
                                        config.schema_version,
                                        SCHEMA_VERSION
                                    );
                                    config.schema_version = SCHEMA_VERSION;
                                }
                                tracing::info!(
                                    "Loaded {} shortcuts and {} favorite filters",
                                    config.shortcuts.len(),
                                    config.favorite_filters.len()
                                );
                                return config;
                            }
                        }
                    }
                }
            } else {
                tracing::info!("No global config found, using defaults");
            }
        }

        Self::default()
    }

    /// Save global config to disk.
    ///
    /// Returns `Ok(())` without writing when `read_only` is set (config was
    /// loaded from a newer-version file that must not be overwritten).
    pub fn save(&self) -> Result<(), String> {
        if self.read_only {
            tracing::warn!("Config is read-only (on-disk version is newer) — skipping save");
            return Ok(());
        }
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
