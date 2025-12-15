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

//! Session persistence for `.crab` and `.crab-filters` files.
//!
//! This module handles serialization and deserialization of session data,
//! including filters, highlights, and bookmarks.

use chrono::{DateTime, Local};
use egui::Color32;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Current version of the .crab file format
pub const CRAB_FILE_VERSION: u32 = 2;

/// Current version of the .crab-filters file format
pub const CRAB_FILTERS_VERSION: u32 = 1;

// ============================================================================
// Color Serialization
// ============================================================================

/// Helper to serialize/deserialize Color32
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct SerializableColor {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl From<Color32> for SerializableColor {
    fn from(c: Color32) -> Self {
        let [r, g, b, a] = c.to_array();
        Self { r, g, b, a }
    }
}

impl From<SerializableColor> for Color32 {
    fn from(c: SerializableColor) -> Self {
        Self::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
    }
}

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

/// Named bookmark with optional description
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub line_index: usize,
    pub name: String,
    pub timestamp: DateTime<Local>,
}

/// Unified saved search configuration for both filters and highlights.
///
/// This struct represents the common search configuration that can be
/// serialized/deserialized for session persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSearch {
    pub search_text: String,
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
    /// Whether this search is active/visible (called "globally_visible" for filters, "enabled" for highlights)
    #[serde(default = "default_enabled", alias = "globally_visible")]
    pub enabled: bool,
    #[serde(default)]
    pub show_in_histogram: bool,
}

/// Type alias for backwards compatibility - filters use SavedSearch
pub type SavedFilter = SavedSearch;

/// Type alias for backwards compatibility - highlights use SavedSearch
pub type SavedHighlight = SavedSearch;

// ============================================================================
// File Formats
// ============================================================================

/// .crab file format - stores all session data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrabFile {
    /// File format version for future compatibility
    #[serde(default = "default_version")]
    pub version: u32,
    pub bookmarks: Vec<Bookmark>,
    pub filters: Vec<SavedFilter>,
    #[serde(default)]
    pub highlights: Vec<SavedHighlight>,
}

impl CrabFile {
    /// Load a session from a .crab file
    pub fn load(path: &Path) -> Result<Self, SessionError> {
        let content = fs::read_to_string(path).map_err(SessionError::Io)?;
        let crab_file: Self = serde_json::from_str(&content).map_err(SessionError::Parse)?;

        if crab_file.version > CRAB_FILE_VERSION {
            log::warn!(
                ".crab file version {} is newer than supported version {}. Some features may not work correctly.",
                crab_file.version,
                CRAB_FILE_VERSION
            );
        }

        Ok(crab_file)
    }

    /// Save the session to a .crab file
    pub fn save(&self, path: &Path) -> Result<(), SessionError> {
        let json = serde_json::to_string_pretty(self).map_err(SessionError::Serialize)?;
        fs::write(path, json).map_err(SessionError::Io)?;
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
    pub fn load(path: &Path) -> Result<Self, SessionError> {
        let content = fs::read_to_string(path).map_err(SessionError::Io)?;
        let filters: Self = serde_json::from_str(&content).map_err(SessionError::Parse)?;

        if filters.version > CRAB_FILTERS_VERSION {
            log::warn!(
                ".crab-filters file version {} is newer than supported version {}. Some features may not work correctly.",
                filters.version,
                CRAB_FILTERS_VERSION
            );
        }

        Ok(filters)
    }

    /// Save filters to a .crab-filters file
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
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Parse(e) => write!(f, "Parse error: {e}"),
            Self::Serialize(e) => write!(f, "Serialization error: {e}"),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Parse(e) => Some(e),
            Self::Serialize(e) => Some(e),
        }
    }
}
