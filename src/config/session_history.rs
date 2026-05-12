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
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use chrono::{DateTime, Local};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Maximum number of sessions to remember
const MAX_SESSIONS: usize = 20;

/// Current schema version for the session history file.
///
/// Bump when the format changes in a backwards-incompatible way.
/// Old binaries that don't know this version will fall back to defaults
/// rather than silently corrupting the file.
///
/// History:
///   v1 — initial versioned schema
pub const SESSION_HISTORY_VERSION: u32 = 1;

/// A recorded session: a set of files that were open together
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedSession {
    /// The file paths that were part of this session
    pub files: Vec<PathBuf>,
    /// When this session was last used
    pub last_used: DateTime<Local>,
}

impl RecordedSession {
    /// Check whether this session contains the given file path.
    /// Compares canonicalized paths when possible, falls back to direct comparison.
    pub fn contains_file(&self, path: &Path) -> bool {
        let canonical = path.canonicalize().ok();
        self.files.iter().any(|f| {
            if let Some(ref c) = canonical {
                if let Ok(fc) = f.canonicalize() {
                    return &fc == c;
                }
            }
            f == path
        })
    }

    /// Display-friendly label: comma-separated file names
    pub fn display_label(&self) -> String {
        self.files
            .iter()
            .filter_map(|p| p.file_name())
            .map(|n| n.to_string_lossy())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Check if all files in this session still exist on disk
    pub fn all_files_exist(&self) -> bool {
        self.files.iter().all(|f| f.exists())
    }

    /// Check if this session has the exact same set of files (order-independent)
    pub fn same_files(&self, other_files: &[PathBuf]) -> bool {
        if self.files.len() != other_files.len() {
            return false;
        }
        // Canonicalize both sides for comparison
        let mut a: Vec<PathBuf> = self
            .files
            .iter()
            .map(|f| f.canonicalize().unwrap_or_else(|_| f.clone()))
            .collect();
        let mut b: Vec<PathBuf> = other_files
            .iter()
            .map(|f| f.canonicalize().unwrap_or_else(|_| f.clone()))
            .collect();
        a.sort();
        b.sort();
        a == b
    }
}

/// Persistent session history stored alongside the global config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHistory {
    /// Schema version — used for forward-compatibility detection.
    pub schema_version: u32,

    #[serde(default)]
    pub sessions: Vec<RecordedSession>,

    /// If `true`, the on-disk file was written by a newer binary.
    /// `update()` will skip writing to avoid downgrading the format.
    #[serde(skip)]
    pub read_only: bool,
}

impl Default for SessionHistory {
    fn default() -> Self {
        Self {
            schema_version: SESSION_HISTORY_VERSION,
            sessions: Vec::new(),
            read_only: false,
        }
    }
}

impl SessionHistory {
    /// Path to the session history file
    fn history_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("logcrab").join("session_history.json"))
    }

    /// Parse JSON contents into a `SessionHistory`, handling version probing.
    fn parse_contents(contents: &str) -> Self {
        #[derive(Deserialize)]
        struct VersionProbe {
            schema_version: Option<u32>,
        }

        let file_version = serde_json::from_str::<VersionProbe>(contents)
            .map(|p| p.schema_version.unwrap_or(0))
            .unwrap_or(0);

        if file_version > SESSION_HISTORY_VERSION {
            tracing::warn!(
                "Session history schema version {} is newer than this binary's {} — \
                 using defaults (read-only: will not overwrite)",
                file_version,
                SESSION_HISTORY_VERSION
            );
            return Self {
                read_only: true,
                ..Self::default()
            };
        }

        // v0 = old file that never wrote schema_version: inject it so serde
        // can deserialize without losing existing data.
        let parse_result: Option<Self> = if file_version == 0 {
            tracing::info!("Session history has no schema_version, treating as v0 and migrating");
            serde_json::from_str::<serde_json::Value>(contents)
                .ok()
                .and_then(|mut v| {
                    v.as_object_mut()?.insert(
                        "schema_version".to_string(),
                        serde_json::json!(SESSION_HISTORY_VERSION),
                    );
                    serde_json::from_value::<Self>(v).ok()
                })
        } else {
            serde_json::from_str::<Self>(contents).ok()
        };

        match parse_result {
            None => {
                tracing::warn!("Failed to parse session history, using defaults");
                Self::default()
            }
            Some(mut history) => {
                if history.schema_version < SESSION_HISTORY_VERSION {
                    tracing::info!(
                        "Migrated session history from schema v{} to v{}",
                        history.schema_version,
                        SESSION_HISTORY_VERSION
                    );
                    history.schema_version = SESSION_HISTORY_VERSION;
                }
                history
            }
        }
    }

    /// Load session history from disk (shared lock for concurrent safety)
    pub fn load() -> Self {
        let Some(path) = Self::history_path() else {
            return Self::default();
        };
        if !path.exists() {
            return Self::default();
        }
        match std::fs::OpenOptions::new().read(true).open(&path) {
            Ok(mut file) => {
                if file.lock_shared().is_err() {
                    tracing::warn!("Failed to lock session history for reading");
                    return Self::default();
                }
                let mut contents = String::new();
                if file.read_to_string(&mut contents).is_err() {
                    return Self::default();
                }
                // Lock releases when file is dropped
                Self::parse_contents(&contents)
            }
            Err(e) => {
                tracing::warn!("Failed to open session history: {e}");
                Self::default()
            }
        }
    }

    /// Atomically update the on-disk session history.
    ///
    /// Acquires an exclusive lock, re-reads the current on-disk state, applies
    /// `f`, writes back, and releases the lock. This prevents concurrent
    /// instances from clobbering each other's changes.
    ///
    /// When the on-disk file was written by a newer binary (version >
    /// `SESSION_HISTORY_VERSION`), `f` is applied only to the in-memory state
    /// and no write occurs, preserving the file.
    ///
    /// Returns the updated history so the caller can replace its cached copy.
    pub fn update(f: impl FnOnce(&mut SessionHistory)) -> Result<SessionHistory, String> {
        let path = Self::history_path().ok_or("Could not determine config directory")?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {e}"))?;
        }

        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| format!("Failed to open session history file: {e}"))?;

        file.lock_exclusive()
            .map_err(|e| format!("Failed to lock session history file: {e}"))?;

        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .map_err(|e| format!("Failed to read session history: {e}"))?;

        let mut history = if contents.is_empty() {
            Self::default()
        } else {
            Self::parse_contents(&contents)
        };

        f(&mut history);

        if history.read_only {
            tracing::warn!(
                "Session history is read-only (on-disk version is newer) — changes not persisted"
            );
            return Ok(history);
        }

        let json = serde_json::to_string_pretty(&history)
            .map_err(|e| format!("Failed to serialize session history: {e}"))?;

        file.seek(SeekFrom::Start(0))
            .map_err(|e| format!("Seek failed: {e}"))?;
        file.set_len(0)
            .map_err(|e| format!("Truncate failed: {e}"))?;
        file.write_all(json.as_bytes())
            .map_err(|e| format!("Write failed: {e}"))?;

        // Lock releases when file is dropped
        Ok(history)
    }

    /// Record a session (set of currently open files).
    ///
    /// If an identical session already exists (same files), update its timestamp.
    /// Otherwise, push a new entry (evicting the oldest if at capacity).
    pub fn record(&mut self, files: Vec<PathBuf>) {
        if files.is_empty() {
            return;
        }

        let now = Local::now();

        // Deduplicate: if the exact same set of files already exists, just bump its timestamp
        if let Some(existing) = self.sessions.iter_mut().find(|s| s.same_files(&files)) {
            existing.last_used = now;
        } else {
            self.sessions.push(RecordedSession {
                files,
                last_used: now,
            });
        }

        // Sort by most recently used first
        self.sessions
            .sort_by(|a, b| b.last_used.cmp(&a.last_used));

        // Trim to max
        self.sessions.truncate(MAX_SESSIONS);
    }

    /// Remove sessions whose files no longer exist
    pub fn prune_missing(&mut self) {
        self.sessions.retain(|s| s.all_files_exist());
    }

    /// Find all sessions containing the given file
    pub fn sessions_containing(&self, path: &Path) -> Vec<&RecordedSession> {
        self.sessions.iter().filter(|s| s.contains_file(path)).collect()
    }
}
