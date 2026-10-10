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

//! Session persistence for `.crab` and `.crab-filters` files.
//!
//! This module handles serialization and deserialization of session data,
//! including filters, highlights, and bookmarks.

use egui::Color32;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::core::log_store::Bookmark;

/// Current version of the .crab file format.
///
/// Version history:
/// - v2: flat `time_offset_ms` per source
/// - v3: per-filetype slug keys (e.g. `"bugreport": { ... }`)
/// - v4: bugreport state gains `state_version` + `dmesg_offset_ms`; per-filetype
///   state versioning introduced so future filetype changes don't require
///   bumping this global version. Pre-v4 `LogCrab` cannot safely write v4+
///   files (it ignores unknown fields and would silently drop calibration data
///   on save), so a global bump is required.
pub const CRAB_FILE_VERSION: u32 = 4;

/// Last legacy format version; files with version ≤ this are parsed as [`CrabFileV2`]
const CRAB_FILE_V2: u32 = 2;

/// Current version of the .crab-filters file format
pub const CRAB_FILTERS_VERSION: u32 = 1;

// ============================================================================
// Color Serialization
// ============================================================================

/// Helper to serialize/deserialize Color32
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct SerializableColor {
    red: u8,
    green: u8,
    blue: u8,
    alpha: u8,
}

impl From<Color32> for SerializableColor {
    fn from(c: Color32) -> Self {
        let [red, green, blue, alpha] = c.to_array();
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }
}

impl From<SerializableColor> for Color32 {
    fn from(c: SerializableColor) -> Self {
        Self::from_rgba_unmultiplied(c.red, c.green, c.blue, c.alpha)
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde requires passing the color by ref
fn serialize_color<S>(color: &Color32, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    SerializableColor::from(*color).serialize(serializer)
}

fn deserialize_color<'de, D>(deserializer: D) -> Result<Color32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    SerializableColor::deserialize(deserializer).map(Color32::from)
}

// ============================================================================
// Saved Data Structures
// ============================================================================

const fn default_filter_color() -> Color32 {
    Color32::YELLOW
}

const fn default_enabled() -> bool {
    true
}

const fn default_version() -> u32 {
    1 // Treat missing version as v1 for backwards compatibility
}

/// Unified saved search configuration for both filters and highlights.
///
/// This struct represents the common search configuration that can be
/// serialized/deserialized for session persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSearch {
    pub search_text: String,
    #[serde(default)]
    pub exclude_text: String,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub name: String,
    #[serde(
        default = "default_filter_color",
        serialize_with = "serialize_color",
        deserialize_with = "deserialize_color"
    )]
    pub color: Color32,
    /// Whether this search is active/visible (called "`globally_visible`" for filters, "enabled" for highlights)
    #[serde(default = "default_enabled", alias = "globally_visible")]
    pub enabled: bool,
    #[serde(default)]
    pub show_in_histogram: bool,
}

/// Type alias for backwards compatibility - filters use `SavedSearch`
pub type SavedFilter = SavedSearch;

/// Type alias for backwards compatibility - highlights use `SavedSearch`
pub type SavedHighlight = SavedSearch;

// ============================================================================
// File Formats
// ============================================================================

/// `.crab` v2 file format — legacy format used only for migration.
///
/// v2 stored the time offset as a flat `time_offset_ms` field instead of a
/// typed `file_state`. This struct is never written; it exists solely so that
/// old files can be deserialized and then converted via [`CrabFile::migrate_from_v2`].
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrabFileV2 {
    #[serde(default = "default_version")]
    version: u32,
    bookmarks: Vec<Bookmark>,
    filters: Vec<SavedFilter>,
    #[serde(default)]
    highlights: Vec<SavedHighlight>,
    #[serde(default)]
    time_offset_ms: i64,
}

/// `.crab` file format (v3+) — stores per-source session data.
///
/// Generic over `FT: InputFileType`. The `file_state` is stored in JSON under
/// `FT::SLUG` (e.g. `"bugreport"`) rather than a shared `"file_state"` key, so
/// two types sharing a `LineType` (e.g. `logcat`/`bugreport`) each have their own
/// distinct state entry and re-detection as a different format is graceful.
/// `FT::SLUG` is provided by the `HasSlug` impl generated by `register_filetypes!`.
///
/// Version history:
/// - v2: flat `time_offset_ms: i64` for source time calibration (see [`CrabFileV2`])
/// - v3: `file_state` stored under `FT::SLUG` (typed per-source state)
///
/// Individual filetype states may carry their own `version` field inside the
/// slug object to version their schema independently (see e.g. `BugreportFileState`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "<FT::LineType as crate::filetype::LineType>::FileState: serde::Serialize",
    deserialize = "<FT::LineType as crate::filetype::LineType>::FileState: for<'__de> serde::Deserialize<'__de>"
))]
pub struct CrabFile<FT: crate::filetype::InputFileType> {
    /// File format version for future compatibility
    #[serde(default = "default_version")]
    pub version: u32,
    pub bookmarks: Vec<Bookmark>,
    pub filters: Vec<SavedFilter>,
    #[serde(default)]
    pub highlights: Vec<SavedHighlight>,
    /// Per-source persistent state. Stored in JSON under `FT::SLUG`.
    #[serde(default)]
    pub file_state: <FT::LineType as crate::filetype::LineType>::FileState,
}

impl<FT: crate::filetype::InputFileType> CrabFile<FT> {
    /// Migrate a v2 session into the current v3 format.
    fn migrate_from_v2(v2: CrabFileV2) -> Self {
        use crate::filetype::LineType as _;
        tracing::info!(
            "Migrating .crab file from v{} to v{}",
            v2.version,
            CRAB_FILE_VERSION
        );
        Self {
            version: CRAB_FILE_VERSION,
            bookmarks: v2.bookmarks,
            filters: v2.filters,
            highlights: v2.highlights,
            file_state: FT::LineType::file_state_from_v2(v2.time_offset_ms),
        }
    }

    /// Load a session from an already-open file handle.
    ///
    /// Files with version ≤ v2 are deserialized as [`CrabFileV2`] and migrated;
    /// v3+ files have their `FT::SLUG` key remapped to `file_state` before
    /// deserialization.
    /// # Errors
    ///
    /// Returns an error when the requested operation cannot be completed.
    pub fn load_from_file(file: &mut std::fs::File) -> Result<Self, SessionError> {
        use std::io::{Read, Seek, SeekFrom};

        file.seek(SeekFrom::Start(0)).map_err(SessionError::Io)?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(SessionError::Io)?;

        let mut value: serde_json::Value =
            serde_json::from_str(&content).map_err(SessionError::Parse)?;

        let version = value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(1) as u32;

        // v2 and older: use the legacy parser and migrate up.
        if version <= CRAB_FILE_V2 {
            let v2: CrabFileV2 = serde_json::from_value(value).map_err(SessionError::Parse)?;
            return Ok(Self::migrate_from_v2(v2));
        }

        if version > CRAB_FILE_VERSION {
            return Err(SessionError::VersionTooNew {
                found: version,
                supported: CRAB_FILE_VERSION,
            });
        }

        // v3+: remap the per-format slug key to the canonical `file_state` key.
        if let Some(obj) = value.as_object_mut() {
            if let Some(slug_state) = obj.remove(FT::SLUG) {
                // Check the per-filetype state_version before handing to serde,
                // so "too new" reaches the caller as a typed error with a good message
                // rather than being silently swallowed as a missing field.
                let max_sv = <<FT::LineType as crate::filetype::LineType>::FileState
                as crate::filetype::LogFileState>::MAX_STATE_VERSION;
                if let Some(max_sv) = max_sv {
                    let state_version = slug_state
                        .get("state_version")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0) as u32;
                    if state_version > max_sv {
                        return Err(SessionError::StateVersionTooNew {
                            slug: FT::SLUG,
                            found: state_version,
                            supported: max_sv,
                        });
                    }
                }
                obj.insert("file_state".to_string(), slug_state);
            }
        }

        serde_json::from_value(value).map_err(SessionError::Parse)
    }

    /// Save the session to an already-open file handle.
    ///
    /// Serializes `file_state` under `FT::SLUG` rather than `"file_state"`.
    /// # Errors
    ///
    /// Returns an error when the requested operation cannot be completed.
    pub fn save_to_file(&self, file: &mut std::fs::File) -> Result<(), SessionError> {
        use std::io::{Seek, SeekFrom, Write};

        let mut value = serde_json::to_value(self).map_err(SessionError::Serialize)?;

        if let Some(obj) = value.as_object_mut() {
            if let Some(state) = obj.remove("file_state") {
                obj.insert(FT::SLUG.to_string(), state);
            }
        }

        let json = serde_json::to_string_pretty(&value).map_err(SessionError::Serialize)?;

        file.set_len(0).map_err(SessionError::Io)?;
        file.seek(SeekFrom::Start(0)).map_err(SessionError::Io)?;
        file.write_all(json.as_bytes()).map_err(SessionError::Io)?;
        file.flush().map_err(SessionError::Io)?;

        Ok(())
    }
}

/// .crab-filters file format - stores only filters for import/export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrabFilters {
    /// File format version for future compatibility
    #[serde(default = "default_version")]
    pub version: u32,
    pub filters: Vec<SavedFilter>,
}

impl CrabFilters {
    /// Load filters from a .crab-filters file
    /// # Errors
    ///
    /// Returns an error when the requested operation cannot be completed.
    pub fn load(path: &Path) -> Result<Self, SessionError> {
        let content = fs::read_to_string(path).map_err(SessionError::Io)?;
        let filters: Self = serde_json::from_str(&content).map_err(SessionError::Parse)?;

        if filters.version > CRAB_FILTERS_VERSION {
            tracing::warn!(
            ".crab-filters file version {} is newer than supported version {}. Some features may not work correctly.",
            filters.version,
            CRAB_FILTERS_VERSION
        );
        }

        Ok(filters)
    }

    /// Save filters to a .crab-filters file
    /// # Errors
    ///
    /// Returns an error when the requested operation cannot be completed.
    pub fn save(&self, path: &Path) -> Result<(), SessionError> {
        let json = serde_json::to_string_pretty(self).map_err(SessionError::Serialize)?;
        fs::write(path, json).map_err(SessionError::Io)?;
        Ok(())
    }
}

// ============================================================================
// Error Handling
// ============================================================================

/// Errors that can occur during session operations
#[derive(Debug)]
pub enum SessionError {
    Io(std::io::Error),
    Parse(serde_json::Error),
    Serialize(serde_json::Error),
    /// The .crab file was created by a newer version of `LogCrab` than this build supports.
    /// Loading is refused to prevent silent data loss when the file would be overwritten.
    VersionTooNew {
        found: u32,
        supported: u32,
    },
    /// A per-filetype `state_version` inside the slug JSON is newer than this build supports.
    /// The global .crab version is fine; only this source's typed state cannot be read.
    StateVersionTooNew {
        slug: &'static str,
        found: u32,
        supported: u32,
    },
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Parse(e) => write!(f, "Parse error: {e}"),
            Self::Serialize(e) => write!(f, "Serialization error: {e}"),
            Self::VersionTooNew { found, supported } => write!(
                f,
                ".crab file version {found} is newer than supported version {supported}"
            ),
            Self::StateVersionTooNew {
                slug,
                found,
                supported,
            } => write!(
                f,
                "{slug} state version {found} is newer than supported version {supported}"
            ),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Parse(e) => Some(e),
            Self::Serialize(e) => Some(e),
            Self::VersionTooNew { .. } => None,
            Self::StateVersionTooNew { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filetype::bugreport::{BugreportFileType, BUGREPORT_STATE_VERSION};
    use crate::filetype::generic::GenericFileType;
    use crate::filetype::HasSlug;
    use std::io::{Read, Seek, SeekFrom, Write};

    fn file_with(content: &str) -> std::fs::File {
        let mut file = tempfile::tempfile().expect("tempfile");
        file.write_all(content.as_bytes()).expect("write");
        file
    }

    fn raw_contents(file: &mut std::fs::File) -> String {
        let mut s = String::new();
        file.seek(SeekFrom::Start(0)).expect("seek");
        file.read_to_string(&mut s).expect("read");
        s
    }

    fn load<FT: crate::filetype::InputFileType>(content: &str) -> Result<CrabFile<FT>, SessionError>
    where
        <FT::LineType as crate::filetype::LineType>::FileState: for<'de> Deserialize<'de>,
    {
        CrabFile::<FT>::load_from_file(&mut file_with(content))
    }

    fn search(text: &str, color: Color32) -> SavedSearch {
        SavedSearch {
            search_text: text.to_string(),
            exclude_text: "noise".to_string(),
            case_sensitive: true,
            name: format!("{text} name"),
            color,
            enabled: false,
            show_in_histogram: true,
        }
    }

    fn bugreport_session() -> CrabFile<BugreportFileType> {
        let state = crate::filetype::bugreport::BugreportFileState::default();
        state.set_logcat_offset_ms(1234);
        state.set_dmesg_offset_ms(-5678);
        CrabFile {
            version: CRAB_FILE_VERSION,
            bookmarks: vec![
                Bookmark {
                    line_index: 7,
                    name: "first".to_string(),
                },
                Bookmark {
                    line_index: 42,
                    name: "second".to_string(),
                },
            ],
            filters: vec![search("ERROR", Color32::from_rgb(1, 2, 3))],
            highlights: vec![search("WARN", Color32::from_rgb(200, 100, 50))],
            file_state: state,
        }
    }

    #[test]
    fn round_trip_preserves_session_and_stores_state_under_slug() {
        let original = bugreport_session();
        let mut file = tempfile::tempfile().expect("tempfile");
        original.save_to_file(&mut file).expect("save");

        let raw: serde_json::Value =
            serde_json::from_str(&raw_contents(&mut file)).expect("saved file is JSON");
        assert_eq!(BugreportFileType::SLUG, "bugreport");
        assert!(raw.get("file_state").is_none(), "state must not use generic key");
        assert_eq!(raw["bugreport"]["state_version"], BUGREPORT_STATE_VERSION);
        assert_eq!(raw["bugreport"]["logcat_offset_ms"], 1234);
        assert_eq!(raw["bugreport"]["dmesg_offset_ms"], -5678);

        let loaded = CrabFile::<BugreportFileType>::load_from_file(&mut file).expect("load");
        assert_eq!(loaded.version, CRAB_FILE_VERSION);
        assert_eq!(loaded.file_state.logcat_offset_ms(), 1234);
        assert_eq!(loaded.file_state.dmesg_offset_ms(), -5678);
        // Bookmark/SavedSearch have no PartialEq; compare their serialized form.
        assert_eq!(
            serde_json::to_value(&loaded.bookmarks).unwrap(),
            serde_json::to_value(&original.bookmarks).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&loaded.filters).unwrap(),
            serde_json::to_value(&original.filters).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&loaded.highlights).unwrap(),
            serde_json::to_value(&original.highlights).unwrap()
        );
        assert_eq!(loaded.filters[0].color, Color32::from_rgb(1, 2, 3));
        assert!(!loaded.filters[0].enabled);
    }

    #[test]
    fn v1_file_without_version_migrates_time_offset() {
        let loaded = load::<GenericFileType>(
            r#"{"bookmarks":[{"line_index":3,"name":"b"}],"filters":[],"time_offset_ms":4242}"#,
        )
        .expect("v1 loads");
        assert_eq!(loaded.version, CRAB_FILE_VERSION);
        assert_eq!(loaded.file_state.time_offset_ms(), 4242);
        assert_eq!(loaded.bookmarks.len(), 1);
        assert_eq!(loaded.bookmarks[0].line_index, 3);
        assert!(loaded.highlights.is_empty());
    }

    #[test]
    fn v2_file_migrates_via_file_state_from_v2() {
        let loaded = load::<BugreportFileType>(
            r#"{"version":2,"bookmarks":[],"filters":[{"search_text":"x"}],"highlights":[],"time_offset_ms":-900}"#,
        )
        .expect("v2 loads");
        assert_eq!(loaded.version, CRAB_FILE_VERSION);
        // Bugreport maps the single legacy offset to the logcat side only.
        assert_eq!(loaded.file_state.logcat_offset_ms(), -900);
        assert_eq!(loaded.file_state.dmesg_offset_ms(), 0);
        assert_eq!(loaded.filters[0].search_text, "x");
        assert_eq!(loaded.filters[0].color, Color32::YELLOW);
        assert!(loaded.filters[0].enabled);
    }

    #[test]
    fn v2_file_with_slug_key_ignores_it() {
        // A v2 file never has slug keys; if one is present it must not be
        // treated as v3+ state (routing is by version, not by key presence).
        let loaded = load::<GenericFileType>(
            r#"{"version":2,"bookmarks":[],"filters":[],"time_offset_ms":5,"generic":{"time_offset_ms":99}}"#,
        )
        .expect("v2 loads");
        assert_eq!(loaded.file_state.time_offset_ms(), 5);
    }

    #[test]
    fn version_newer_than_supported_is_refused() {
        let content = format!(
            r#"{{"version":{},"bookmarks":[],"filters":[]}}"#,
            CRAB_FILE_VERSION + 1
        );
        match load::<GenericFileType>(&content) {
            Err(SessionError::VersionTooNew { found, supported }) => {
                assert_eq!(found, CRAB_FILE_VERSION + 1);
                assert_eq!(supported, CRAB_FILE_VERSION);
            }
            other => panic!("expected VersionTooNew, got {:?}", other.err()),
        }
    }

    #[test]
    fn version_beyond_u32_is_not_migrated_as_legacy() {
        // 2^32 + 1 truncates to 1 under `as u32`; it must never load as a
        // migrated v1 session.
        let result = load::<GenericFileType>(
            r#"{"version":4294967297,"bookmarks":[],"filters":[],"time_offset_ms":1}"#,
        );
        assert!(result.is_err(), "unexpectedly loaded as a legacy session");
    }

    #[test]
    fn non_integer_version_is_not_loaded() {
        for version in [r#""4""#, "4.5", "-1", "null"] {
            let content = format!(r#"{{"version":{version},"bookmarks":[],"filters":[]}}"#);
            let result = load::<GenericFileType>(&content);
            assert!(
                matches!(result, Err(SessionError::Parse(_))),
                "version {version}: got {:?}",
                result.as_ref().err()
            );
        }
    }

    #[test]
    fn state_version_newer_than_supported_is_refused() {
        let content = format!(
            r#"{{"version":{CRAB_FILE_VERSION},"bookmarks":[],"filters":[],
               "bugreport":{{"state_version":{},"logcat_offset_ms":1}}}}"#,
            BUGREPORT_STATE_VERSION + 1
        );
        match load::<BugreportFileType>(&content) {
            Err(SessionError::StateVersionTooNew {
                slug,
                found,
                supported,
            }) => {
                assert_eq!(slug, "bugreport");
                assert_eq!(found, BUGREPORT_STATE_VERSION + 1);
                assert_eq!(supported, BUGREPORT_STATE_VERSION);
            }
            other => panic!("expected StateVersionTooNew, got {:?}", other.err()),
        }
    }

    #[test]
    fn missing_state_version_is_treated_as_zero() {
        // logcrab 0.35 wrote bugreport state as SimpleFileState (no state_version).
        let loaded = load::<BugreportFileType>(
            r#"{"version":3,"bookmarks":[],"filters":[],"bugreport":{"time_offset_ms":77}}"#,
        )
        .expect("unversioned state loads");
        assert_eq!(loaded.file_state.logcat_offset_ms(), 77);
        assert_eq!(loaded.file_state.dmesg_offset_ms(), 0);
    }

    #[test]
    fn absent_slug_loads_default_state_and_keeps_bookmarks() {
        // Written while the file was detected as logcat, now re-detected as
        // bugreport: the foreign slug's state_version must not be checked and
        // its state must not leak into ours.
        let loaded = load::<BugreportFileType>(
            r#"{"version":4,"bookmarks":[{"line_index":1,"name":"kept"}],"filters":[],
               "logcat":{"state_version":999,"time_offset_ms":555}}"#,
        )
        .expect("loads with default state");
        assert_eq!(loaded.file_state.logcat_offset_ms(), 0);
        assert_eq!(loaded.file_state.dmesg_offset_ms(), 0);
        assert_eq!(loaded.bookmarks.len(), 1);
        assert_eq!(loaded.bookmarks[0].name, "kept");
    }

    #[test]
    fn empty_and_malformed_files_are_parse_errors() {
        for content in ["", "   \n", "{not json", r#"{"version":4,"bookmarks":"#, "[]"] {
            let result = load::<GenericFileType>(content);
            assert!(
                matches!(result, Err(SessionError::Parse(_))),
                "content {content:?}: got {:?}",
                result.as_ref().err()
            );
        }
    }

    #[test]
    fn save_truncates_longer_previous_contents() {
        let mut file = file_with(&format!("{{\"padding\":\"{}\"}}", "x".repeat(64 * 1024)));
        let session = bugreport_session();
        session.save_to_file(&mut file).expect("save");

        let raw = raw_contents(&mut file);
        assert_eq!(
            file.metadata().expect("metadata").len(),
            raw.len() as u64
        );
        assert!(!raw.contains("padding"));
        serde_json::from_str::<serde_json::Value>(&raw).expect("no trailing bytes");
        let loaded = CrabFile::<BugreportFileType>::load_from_file(&mut file).expect("load");
        assert_eq!(loaded.bookmarks.len(), 2);
    }
}
