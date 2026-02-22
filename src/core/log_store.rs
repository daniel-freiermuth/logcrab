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

use crate::core::session::{CrabFile, CRAB_FILE_VERSION};
use crate::core::{SavedFilter, SavedHighlight};
use crate::parser::line::{LogLine, LogLineCore};
use crate::ui::tabs::bookmarks_tab::BookmarkData;
use chrono::{Datelike, Local};
use indexmap::IndexMap;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, RwLock};

/// Global counter for generating unique source IDs.
/// Source IDs are stable across the lifetime of a source, even when other sources are removed.
static SOURCE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// A single log source with its lines, wrapped in `RwLock` for thread-safe access
#[derive(Debug)]
pub struct SourceData {
    /// Unique, stable identifier for this source (does not change when other sources are removed)
    source_id: u64,
    /// Path to the source file
    file_path: PathBuf,
    /// Log lines in file order (index = `line_number` - 1, eternal)
    lines: RwLock<Vec<LogLine>>,
    /// Indices into `lines`, sorted by timestamp for time-ordered iteration
    by_timestamp: RwLock<Vec<usize>>,
    /// Bookmarks for this source, keyed by line index within this source
    bookmarks: RwLock<HashMap<usize, Bookmark>>,
    /// Time offset in milliseconds (for non-DLT file synchronization)
    time_offset_ms: RwLock<i64>,
    /// Locked .crab file handle and path to prevent multiple instances from opening the same session.
    /// The OS-level file lock (via fs2) is held for the lifetime of this `SourceData`,
    /// providing exclusive access. The Mutex allows interior mutability for reads/writes.
    /// Path is stored alongside to avoid recomputation and ensure single source of truth.
    crab_lock: Mutex<(File, PathBuf)>,
    version: AtomicU64,
    /// Flag to request cancellation of background loading/scoring operations
    cancel_requested: AtomicBool,
}

impl SourceData {
    /// Create a `SourceData` for a file source
    ///
    /// Acquires an exclusive lock on the .crab session file to prevent
    /// multiple instances from opening the same file simultaneously.
    ///
    /// Returns `None` if the lock cannot be acquired (file already open in another instance).
    pub fn new(file_path: PathBuf) -> Option<Self> {
        assert!(
            file_path.file_name().is_some(),
            "file_path must have a filename component: {}",
            file_path.display()
        );

        let crab_path = Self::compute_crab_path(&file_path);
        let crab_lock = Self::acquire_crab_lock(&crab_path)?;
        Some(Self::new_with_lock(file_path, crab_lock, crab_path))
    }

    /// Create a `SourceData` with an existing .crab file lock
    ///
    /// This is useful when reloading files - we can pass the lock from the old
    /// `SourceData` to the new one, avoiding the race condition where the OS hasn't
    /// released the lock yet.
    pub fn new_with_lock(file_path: PathBuf, crab_lock: File, crab_path: PathBuf) -> Self {
        assert!(
            file_path.file_name().is_some(),
            "file_path must have a filename component: {}",
            file_path.display()
        );

        let sd = Self {
            source_id: SOURCE_ID_COUNTER.fetch_add(1, AtomicOrdering::Relaxed),
            file_path,
            lines: RwLock::new(Vec::new()),
            by_timestamp: RwLock::new(Vec::new()),
            bookmarks: RwLock::new(HashMap::new()),
            time_offset_ms: RwLock::new(0),
            crab_lock: Mutex::new((crab_lock, crab_path)),
            version: AtomicU64::new(1),
            cancel_requested: AtomicBool::new(false),
        };
        sd.load_bookmarks();
        sd
    }

    /// Compute the .crab file path for a given log file path
    fn compute_crab_path(file_path: &Path) -> PathBuf {
        let mut crab_path = file_path.to_path_buf();
        crab_path.set_file_name(format!(
            "{}.crab",
            file_path
                .file_name()
                .expect("file_path must have a filename component")
                .to_string_lossy()
        ));
        crab_path
    }

    /// Acquire an exclusive lock on the .crab file
    /// Returns None if the lock cannot be acquired (file already open in another instance)
    fn acquire_crab_lock(crab_path: &Path) -> Option<File> {
        use fs2::FileExt;
        use std::fs::OpenOptions;

        // Open or create the .crab file
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(crab_path)
        {
            Ok(f) => f,
            Err(e) => {
                log::error!("Cannot open .crab file {}: {e}", crab_path.display());
                return None;
            }
        };

        // Try to acquire exclusive lock
        match file.try_lock_exclusive() {
            Ok(()) => {
                log::info!(
                    "Successfully acquired exclusive lock on {}",
                    crab_path.display()
                );
                Some(file)
            }
            Err(e) => {
                log::error!(
                    "Cannot lock .crab file {} (already open in another instance?): {e}",
                    crab_path.display()
                );
                None
            }
        }
    }

    /// Bump the version number (call after appending lines)
    fn bump_version(&self) {
        self.version.fetch_add(1, AtomicOrdering::SeqCst);
    }

    /// Get current version number (bumped whenever data changes)
    pub fn version(&self) -> u64 {
        profiling::scope!("SourceData::version");
        self.version.load(AtomicOrdering::SeqCst)
    }

    /// Get the file path for this source
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// Get the unique, stable identifier for this source
    pub const fn source_id(&self) -> u64 {
        self.source_id
    }

    /// Check if this source is a DLT file (based on extension)
    pub fn is_dlt_file(&self) -> bool {
        self.file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dlt"))
    }

    /// Extract the .crab file lock from this source
    ///
    /// This is used when reloading files to transfer the lock to the new `SourceData`,
    /// avoiding the race condition where the OS hasn't released the lock yet.
    ///
    /// Returns (`File`, `PathBuf`) representing the locked file handle and its path.
    pub fn take_crab_lock(self) -> (File, PathBuf) {
        self.crab_lock
            .into_inner()
            .expect("crab_lock mutex poisoned")
    }

    /// Request cancellation of background loading/scoring operations
    pub fn request_cancel(&self) {
        self.cancel_requested.store(true, AtomicOrdering::SeqCst);
    }

    /// Check if cancellation has been requested
    pub fn is_cancelled(&self) -> bool {
        self.cancel_requested.load(AtomicOrdering::SeqCst)
    }

    // ========================================================================
    // Bookmark Management
    // ========================================================================

    /// Add or update a bookmark for a line in this source
    fn set_bookmark(&self, line_index: usize, name: String) {
        profiling::scope!("SourceData::bookmarks::write");
        let bookmark = Bookmark { line_index, name };
        self.bookmarks
            .write()
            .expect("bookmarks lock poisoned")
            .insert(line_index, bookmark);
    }

    /// Remove a bookmark from this source
    fn remove_bookmark(&self, line_index: usize) -> Option<Bookmark> {
        profiling::scope!("SourceData::bookmarks::write");
        self.bookmarks
            .write()
            .expect("bookmarks lock poisoned")
            .remove(&line_index)
    }

    /// Check if a line has a bookmark
    fn has_bookmark(&self, line_index: usize) -> bool {
        profiling::scope!("SourceData::bookmarks::read");
        self.bookmarks
            .read()
            .expect("bookmarks lock poisoned")
            .contains_key(&line_index)
    }

    /// Get a bookmark by line index
    fn get_bookmark(&self, line_index: usize) -> Option<Bookmark> {
        profiling::scope!("SourceData::bookmarks::read");
        self.bookmarks
            .read()
            .expect("bookmarks lock poisoned")
            .get(&line_index)
            .cloned()
    }

    /// Get all bookmarks for this source
    fn get_bookmarks(&self) -> Vec<Bookmark> {
        profiling::scope!("SourceData::bookmarks::read");
        self.bookmarks
            .read()
            .expect("bookmarks lock poisoned")
            .values()
            .cloned()
            .collect()
    }

    /// Load bookmarks from this source's .crab file
    fn load_bookmarks(&self) {
        let (file, crab_path) = &mut *self.crab_lock.lock().expect("crab_lock mutex poisoned");
        match CrabFile::load_from_file(file) {
            Ok(crab_data) => {
                log::info!(
                    "Loaded {} bookmarks from {}",
                    crab_data.bookmarks.len(),
                    crab_path.display()
                );
                profiling::scope!("SourceData::bookmarks::write");
                {
                    let mut bookmarks = self.bookmarks.write().expect("bookmarks lock poisoned");
                    for bookmark in crab_data.bookmarks {
                        bookmarks.insert(bookmark.line_index, bookmark);
                    }
                }

                // Load time offset
                *self
                    .time_offset_ms
                    .write()
                    .expect("time_offset_ms lock poisoned") = crab_data.time_offset_ms;
                if crab_data.time_offset_ms != 0 {
                    log::info!("Loaded time offset: {} ms", crab_data.time_offset_ms);
                }
            }
            Err(crate::core::SessionError::Io(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                // Empty .crab file (just created), that's fine
            }
            Err(crate::core::SessionError::Parse(_)) => {
                // Empty or invalid .crab file, that's fine for a new session
            }
            Err(e) => {
                log::warn!("Failed to load .crab file {}: {e}", crab_path.display());
            }
        }
    }

    /// Save bookmarks to this source's .crab file
    /// Note: filters and highlights are passed in since they're shared across sources
    pub fn save_crab_file(&self, filters: &[SavedFilter], highlights: &[SavedHighlight]) {
        let crab_data = CrabFile {
            version: CRAB_FILE_VERSION,
            bookmarks: self.get_bookmarks(),
            filters: filters.to_vec(),
            highlights: highlights.to_vec(),
            time_offset_ms: *self
                .time_offset_ms
                .read()
                .expect("time_offset_ms lock poisoned"),
        };

        // Use the locked file handle for writing to avoid conflicts
        // The OS-level lock (via fs2) is held for the lifetime of SourceData
        let (file, crab_path) = &mut *self.crab_lock.lock().expect("crab_lock mutex poisoned");
        match crab_data.save_to_file(file) {
            Ok(()) => log::debug!(
                "Saved .crab file {} with {} bookmarks",
                crab_path.display(),
                crab_data.bookmarks.len()
            ),
            Err(e) => log::error!("Failed to save .crab file {}: {e}", crab_path.display()),
        }
    }

    // ========================================================================
    // Time Synchronization
    // ========================================================================

    /// Get the current time offset in milliseconds
    pub fn get_time_offset_ms(&self) -> i64 {
        *self
            .time_offset_ms
            .read()
            .expect("time_offset_ms lock poisoned")
    }

    /// Set time offset for this source to synchronize with target time for a specific line
    /// Used for non-DLT files
    pub fn set_time_offset_to_target(
        &self,
        reference_line_index: usize,
        target_time: chrono::DateTime<chrono::Local>,
    ) -> Result<(), String> {
        profiling::scope!("SourceData::set_time_offset_to_target");

        // Get the reference line's original timestamp
        let original_time = self
            .lines
            .read()
            .expect("lines lock poisoned")
            .get(reference_line_index)
            .ok_or_else(|| "Reference line not found".to_string())?
            .uncalibrated_timestamp();

        // Calculate offset: target_time - original_time
        let offset_ms = target_time.timestamp_millis() - original_time.timestamp_millis();

        log::info!(
            "Setting time offset for source: {offset_ms} ms (original: {original_time}, target: {target_time})"
        );

        *self
            .time_offset_ms
            .write()
            .expect("time_offset_ms lock poisoned") = offset_ms;
        self.bump_version();

        Ok(())
    }

    // ========================================================================
    // Line Management
    // ========================================================================

    /// Append lines to this source
    ///
    /// Lines are stored in file order (append-only). Only the timestamp index
    /// needs to be rebuilt. Heavy work is done outside the write lock.
    pub fn append_lines(&self, lines: Vec<LogLine>) {
        if lines.is_empty() {
            return;
        }

        profiling::scope!("SourceData::append_lines");
        
        // Append lines and capture the range of new indices atomically
        let new_start_idx = {
            profiling::scope!("SourceData::lines::write");
            let mut lines_guard = self.lines.write().expect("lines lock poisoned");
            let start_idx = lines_guard.len();
            log::debug!(
                "Appending {} lines to existing {} lines (merge overhead)",
                lines.len(),
                start_idx
            );
            lines_guard.extend(lines);
            start_idx
        };

        // Read lines and build sorted indices for the newly appended range
        let (lines_guard, new_by_ts) = {
            profiling::scope!("SourceData::lines::read");
            let lines_read = self.lines.read().expect("lines lock poisoned");
            profiling::scope!("sort_new_indices");
            let mut indices: Vec<usize> = (new_start_idx..lines_read.len()).collect();
            indices.par_sort_by_key(|&idx| lines_read[idx].uncalibrated_timestamp());
            (lines_read, indices)
        };

        // Merge into by_timestamp atomically - hold write lock during merge
        {
            profiling::scope!("SourceData::by_timestamp::write");
            let mut by_ts_guard = self.by_timestamp.write().expect("by_timestamp lock poisoned");
            
            profiling::scope!("merge_timestamp_indices");
            let existing_len = by_ts_guard.len();
            let mut merged = Vec::with_capacity(existing_len + new_by_ts.len());
            let mut i_exist = 0;
            let mut j_new = 0;
            
            while i_exist < existing_len && j_new < new_by_ts.len() {
                let ts_exist = lines_guard[by_ts_guard[i_exist]].uncalibrated_timestamp();
                let ts_new = lines_guard[new_by_ts[j_new]].uncalibrated_timestamp();
                if ts_exist <= ts_new {
                    merged.push(by_ts_guard[i_exist]);
                    i_exist += 1;
                } else {
                    merged.push(new_by_ts[j_new]);
                    j_new += 1;
                }
            }
            
            // Append remaining elements
            merged.extend_from_slice(&by_ts_guard[i_exist..]);
            merged.extend_from_slice(&new_by_ts[j_new..]);
            
            *by_ts_guard = merged;
        }
        
        drop(lines_guard);
        self.bump_version();
    }

    /// Set anomaly scores for lines (indexed by position in the vector)
    /// Scores vec should be same length as lines
    pub fn set_scores(&self, scores: &[f64]) {
        profiling::scope!("SourceData::set_scores");
        profiling::scope!("SourceData::lines::write");
        let mut guard = self.lines.write().expect("lines lock poisoned");
        for (idx, &score) in scores.iter().enumerate() {
            if let Some(line) = guard.get_mut(idx) {
                line.set_anomaly_score(score);
            }
        }
        drop(guard);
        self.bump_version();
    }

    /// Get the number of lines
    pub fn len(&self) -> usize {
        profiling::scope!("SourceData::lines::read");
        self.lines.read().expect("lines lock poisoned").len()
    }

    /// Check if this source has no lines
    pub fn is_empty(&self) -> bool {
        profiling::scope!("SourceData::lines::read");
        self.lines.read().expect("lines lock poisoned").is_empty()
    }

    /// Get a clone of all lines for iteration
    /// This clones the entire Vec - use sparingly (e.g., one-time scoring)
    pub fn clone_lines(&self) -> Vec<LogLine> {
        profiling::scope!("SourceData::clone_lines");
        self.lines.read().expect("lines lock poisoned").clone()
    }

    pub fn get_by_id(&self, id: usize) -> Option<LogLine> {
        profiling::scope!("SourceData::lines::read");
        let guard = self.lines.read().expect("lines lock poisoned");
        guard.get(id).cloned()
    }

    /// Resynchronize DLT timestamps to a custom target time (per file, per ECU, per App)
    /// Uses the reference line to calculate `boot_time` such that it results in the target timestamp
    /// Only updates entries in this source file that match both the ECU ID and App ID
    pub fn resync_dlt_time_to_target(
        &self,
        reference_line_index: usize,
        target_time: chrono::DateTime<chrono::Local>,
        ecu_id: Option<&String>,
        app_id: Option<&String>,
    ) -> Result<(), String> {
        use crate::parser::line::LogLineVariant;

        profiling::scope!("SourceData::resync_dlt_time_to_target");

        // Validate target time is within safe range to prevent timestamp overflow
        const MIN_SAFE_YEAR: i32 = 1700;
        const MAX_SAFE_YEAR: i32 = 2250;
        let year = target_time.year();
        if !(MIN_SAFE_YEAR..=MAX_SAFE_YEAR).contains(&year) {
            return Err(format!(
                "Target time year must be between {MIN_SAFE_YEAR}-{MAX_SAFE_YEAR} (got {year})"
            ));
        }

        // Get the reference line and extract timing info
        let reference_line = {
            let guard = self.lines.read().expect("lines lock poisoned");
            guard
                .get(reference_line_index)
                .cloned()
                .ok_or_else(|| "Reference line not found".to_string())?
        };

        // Extract header timestamp (time since boot) from reference line
        let header_timestamp = match &reference_line {
            LogLineVariant::Dlt(dlt_line) => {
                // Get header timestamp (time since boot)
                let header_ts = dlt_line
                    .dlt_message
                    .header
                    .timestamp
                    .ok_or_else(|| "DLT message missing header timestamp".to_string())?;
                crate::parser::dlt::dlt_header_time_to_timedelta(header_ts)
            }
            LogLineVariant::Generic(_)
            | LogLineVariant::Logcat(_)
            | LogLineVariant::Pcap(_)
            | LogLineVariant::Btsnoop(_) => {
                return Err("Reference line is not a DLT entry".to_string())
            }
        };

        // Calculate new boot_time: target_time - time_since_boot
        let new_boot_time = target_time
            .checked_sub_signed(header_timestamp)
            .ok_or_else(|| "Failed to calculate boot time".to_string())?;

        log::info!(
            "Resyncing DLT timestamps with new boot_time: {new_boot_time} (target: {target_time}) for file, ECU: {ecu_id:?}, App: {app_id:?}"
        );

        // Update all DLT entries in this file matching the ECU and App
        {
            profiling::scope!("SourceData::lines::write");
            let mut guard = self.lines.write().expect("lines lock poisoned");
            for line in guard.iter_mut() {
                if let LogLineVariant::Dlt(dlt_line) = line {
                    // Check if this line matches the target ECU and App
                    let should_update = if let Some(target_ecu) = ecu_id {
                        // ECU must match
                        let ecu_matches = dlt_line
                            .dlt_message
                            .header
                            .ecu_id
                            .as_ref()
                            .is_some_and(|ecu| ecu.as_str() == target_ecu);

                        if !ecu_matches {
                            false
                        } else if let Some(target_app) = app_id {
                            // If app is specified, it must also match
                            dlt_line
                                .dlt_message
                                .extended_header
                                .as_ref()
                                .is_some_and(|ext| {
                                    ext.application_id.as_str() == target_app.as_str()
                                })
                        } else {
                            // No app filter, apply to all apps for this ECU
                            true
                        }
                    } else {
                        false
                    };

                    if should_update {
                        // Recalculate timestamp: new_boot_time + time_since_boot
                        if let Some(header_ts) = dlt_line.dlt_message.header.timestamp {
                            let time_since_boot =
                                crate::parser::dlt::dlt_header_time_to_timedelta(header_ts);
                            if let Some(new_timestamp) =
                                new_boot_time.checked_add_signed(time_since_boot)
                            {
                                dlt_line.timestamp = new_timestamp;
                                dlt_line.boot_time = Some(new_boot_time);
                            }
                        }
                    }
                }
            }
        }

        // Rebuild timestamp-sorted index atomically with current lines state
        {
            let lines = self.lines.read().expect("lines lock poisoned");
            let mut indices: Vec<usize> = (0..lines.len()).collect();
            indices.par_sort_by_key(|&idx| lines[idx].uncalibrated_timestamp());
            drop(lines);
            
            *self
                .by_timestamp
                .write()
                .expect("by_timestamp lock poisoned") = indices;
        }

        self.bump_version();

        Ok(())
    }

    /// Load and merge filters and highlights
    pub fn load_saved_filters_and_highlights(&self) -> (Vec<SavedFilter>, Vec<SavedHighlight>) {
        let (file, crab_path) = &mut *self.crab_lock.lock().expect("crab_lock mutex poisoned");
        match CrabFile::load_from_file(file) {
            Ok(crab_data) => {
                return (crab_data.filters, crab_data.highlights);
            }
            Err(crate::core::SessionError::Io(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                // Empty .crab file (just created), that's fine
            }
            Err(crate::core::SessionError::Parse(_)) => {
                // Empty or invalid .crab file, that's fine for a new session
            }
            Err(e) => {
                log::warn!("Failed to load .crab file {}: {e}", crab_path.display());
            }
        }
        (Vec::new(), Vec::new())
    }
}

/// Central storage for log lines from one or more sources
///
/// Thread-safe: can be shared across threads with Arc<LogStore>
/// Uses `IndexMap` for O(1) source lookup by ID while maintaining insertion order.
#[derive(Debug)]
pub struct LogStore {
    /// Sources indexed by their stable `source_id` for O(1) lookup.
    /// `IndexMap` maintains insertion order for consistent UI display.
    sources: RwLock<IndexMap<u64, Arc<SourceData>>>,
    /// Version counter that increments when sources are added or removed.
    /// This ensures cache invalidation even when line counts happen to sum to the same value.
    sources_version: AtomicU64,
}

impl Clone for LogStore {
    fn clone(&self) -> Self {
        profiling::scope!("LogStore::sources::read");
        Self {
            sources: RwLock::new(self.sources.read().expect("sources lock poisoned").clone()),
            sources_version: AtomicU64::new(self.sources_version.load(AtomicOrdering::SeqCst)),
        }
    }
}

/// Version identifier for cache invalidation.
///
/// Two-part version ensures no collisions: `sources` tracks structural changes
/// (add/remove sources), `lines` tracks data changes (lines added/modified).
/// Equality requires both components to match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct StoreVersion {
    /// Incremented when sources are added or removed
    pub sources: u64,
    /// Sum of per-source versions (incremented when lines are added/modified)
    pub lines: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StoreID {
    /// Stable source identifier (survives source removals)
    source_id: u64,
    /// Line index within the source
    line_index: usize,
}

impl StoreID {
    /// Compare two `StoreIDs` by their line timestamps.
    ///
    /// When both lines exist in the store, compares by timestamp first,
    /// then by `source_id` and `line_index` for stability.
    /// When lines are missing (e.g., during file loading), falls back to
    /// structural ordering to maintain a valid total order.
    pub fn cmp(&self, other: &Self, store: &LogStore) -> Ordering {
        let self_line = store.get_by_id(self);
        let other_line = store.get_by_id(other);

        match (self_line, other_line) {
            (Some(l1), Some(l2)) => {
                // Both lines exist: compare by adjusted timestamp (with offset), then structurally for stability
                let self_time = store.get_adjusted_timestamp(self, &l1);
                let other_time = store.get_adjusted_timestamp(other, &l2);
                self_time
                    .cmp(&other_time)
                    .then_with(|| self.source_id.cmp(&other.source_id))
                    .then_with(|| self.line_index.cmp(&other.line_index))
            }
            (Some(_), None) => Ordering::Less, // existing lines come first
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ord::cmp(self, other), // both missing: use derived Ord
        }
    }
}

impl LogStore {
    /// Create a new empty `LogStore`
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            sources: RwLock::new(IndexMap::new()),
            sources_version: AtomicU64::new(1),
        })
    }

    /// Add a source to the store
    pub fn add_source(self: &Arc<Self>, source_data: &Arc<SourceData>) {
        profiling::scope!("LogStore::sources::write");
        self.sources
            .write()
            .expect("sources lock poisoned")
            .insert(source_data.source_id(), Arc::clone(source_data));
        self.sources_version.fetch_add(1, AtomicOrdering::SeqCst);
    }

    /// Check if a file with the given path is already loaded in the store
    pub fn contains_file(&self, path: &Path) -> bool {
        profiling::scope!("LogStore::sources::read");
        let canonical_path = path.canonicalize().ok();
        let sources = self.sources.read().expect("sources lock poisoned");
        sources.values().any(|source| {
            // Try canonical comparison first, fall back to direct comparison
            if let Some(ref canonical) = canonical_path {
                if let Ok(source_canonical) = source.file_path.canonicalize() {
                    return &source_canonical == canonical;
                }
            }
            source.file_path == path
        })
    }

    /// Get current version for cache invalidation.
    ///
    /// Returns a two-part version:
    /// - `sources`: incremented when sources are added or removed
    /// - `lines`: sum of per-source versions (incremented when lines are added/modified)
    ///
    /// Comparing the full struct ensures no false cache hits.
    pub fn version(&self) -> StoreVersion {
        profiling::scope!("LogStore::version");
        let sources = self.sources_version.load(AtomicOrdering::SeqCst);
        profiling::scope!("LogStore::sources::read");
        let lines: u64 = self
            .sources
            .read()
            .expect("sources lock poisoned")
            .values()
            .map(|s| s.version())
            .sum();
        StoreVersion { sources, lines }
    }

    /// Get total number of lines across all sources
    pub fn total_lines(&self) -> usize {
        profiling::scope!("LogStore::sources::read");
        self.sources
            .read()
            .expect("sources lock poisoned")
            .values()
            .map(|s| s.len())
            .sum()
    }

    /// Get the source name (filename) for a given `StoreID`
    pub fn get_source_name(&self, id: &StoreID) -> Option<String> {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");
        sources.get(&id.source_id).map(|source| {
            source
                .file_path
                .file_name()
                .expect("file_path must have a filename component")
                .to_string_lossy()
                .into_owned()
        })
    }

    /// Get all source filenames with their stable source IDs
    pub fn get_source_filenames(&self) -> Vec<(u64, String)> {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");
        sources
            .values()
            .map(|source| {
                let source_id = source.source_id();
                let filename = source
                    .file_path
                    .file_name()
                    .expect("file_path must have a filename component")
                    .to_string_lossy()
                    .into_owned();
                (source_id, filename)
            })
            .collect()
    }

    /// Remove a source by its stable source ID
    ///
    /// Note: `StoreID`s referencing the removed source will simply fail to resolve.
    /// Other `StoreID`s remain valid since they use stable source IDs.
    pub fn remove_source(&self, source_id: u64) -> Option<PathBuf> {
        profiling::scope!("LogStore::sources::write");
        let mut sources = self.sources.write().expect("sources lock poisoned");
        let removed = sources.swap_remove(&source_id)?;
        let path = removed.file_path().to_path_buf();
        drop(sources);
        self.sources_version.fetch_add(1, AtomicOrdering::SeqCst);
        log::info!("Removed source: {}", path.display());
        Some(path)
    }

    /// Check if any DLT sources are still being loaded
    ///
    /// Returns true if any DLT source has multiple Arc references, which indicates
    /// a background loading thread is still holding a reference.
    pub fn has_loading_dlt_sources(&self) -> bool {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");
        sources
            .values()
            .filter(|s| s.is_dlt_file())
            .any(|source| Arc::strong_count(source) > 1)
    }

    /// Request cancellation of all DLT source loading/scoring operations
    pub fn cancel_dlt_loading(&self) {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");
        for source in sources.values() {
            if source.is_dlt_file() {
                source.request_cancel();
            }
        }
    }

    /// Wait for all DLT sources to finish loading (background threads to complete)
    ///
    /// Polls the Arc strong counts until they all drop to 1, indicating background
    /// threads have released their references. Returns true if all completed within
    /// the timeout, false if timeout was reached.
    pub fn wait_for_dlt_loading(&self, timeout_secs: u64) -> bool {
        use std::time::{Duration, Instant};

        let start = Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        while start.elapsed() < timeout {
            if !self.has_loading_dlt_sources() {
                return true;
            }
            // Sleep briefly to avoid busy-waiting
            std::thread::sleep(Duration::from_millis(100));
        }

        false
    }

    /// Remove all DLT sources from the store
    ///
    /// Returns tuples of (path, lock) for each removed DLT source so they can be re-added
    /// with the same lock, avoiding file locking race conditions.
    ///
    /// Note: This invalidates any cached `StoreID`s - callers should refresh their caches.
    pub fn remove_dlt_sources(&self) -> Vec<(PathBuf, File, PathBuf)> {
        profiling::scope!("LogStore::sources::write");
        let mut sources = self.sources.write().expect("sources lock poisoned");

        // Extract DLT sources and their locks
        let mut removed_with_locks = Vec::new();
        let mut remaining = IndexMap::new();

        for (source_id, source) in sources.drain(..) {
            if source.is_dlt_file() {
                let path = source.file_path().to_path_buf();
                // Try to unwrap the Arc - if there are other references, we can't take the lock
                match Arc::try_unwrap(source) {
                    Ok(source_data) => {
                        let (lock_file, lock_path) = source_data.take_crab_lock();
                        removed_with_locks.push((path, lock_file, lock_path));
                    }
                    Err(arc_source) => {
                        log::warn!(
                            "Cannot extract lock from {} - Arc has multiple references",
                            arc_source.file_path().display()
                        );
                        // Put it back in remaining sources since we can't reload it
                        remaining.insert(source_id, arc_source);
                    }
                }
            } else {
                remaining.insert(source_id, source);
            }
        }

        *sources = remaining;
        drop(sources);

        log::info!(
            "Removed {} DLT sources from store (with locks)",
            removed_with_locks.len()
        );
        removed_with_locks
    }

    // ========================================================================
    // Bookmark Management (delegates to appropriate SourceData)
    // ========================================================================

    /// Add or update a bookmark
    pub fn set_bookmark(&self, id: &StoreID, name: String) {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");
        if let Some(source) = sources.get(&id.source_id) {
            source.set_bookmark(id.line_index, name);
        }
    }

    /// Remove a bookmark
    pub fn remove_bookmark(&self, id: &StoreID) -> Option<Bookmark> {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");
        sources
            .get(&id.source_id)
            .and_then(|s| s.remove_bookmark(id.line_index))
    }

    /// Resynchronize DLT timestamps to a custom target time (per file, per ECU, per App)
    #[allow(clippy::significant_drop_tightening)] // sources can't be dropped
                                                  // early since source is a reference into it
    pub fn resync_dlt_time_to_target(
        &self,
        id: &StoreID,
        target_time: chrono::DateTime<chrono::Local>,
        ecu_id: Option<&String>,
        app_id: Option<&String>,
    ) -> Result<(), String> {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");
        let source = sources
            .get(&id.source_id)
            .ok_or_else(|| "Source not found".to_string())?;
        source.resync_dlt_time_to_target(id.line_index, target_time, ecu_id, app_id)
    }

    /// Set time offset for a source file to synchronize with target time for a specific line
    /// Used for non-DLT files
    pub fn set_time_offset_to_target(
        &self,
        id: &StoreID,
        target_time: chrono::DateTime<chrono::Local>,
    ) -> Result<(), String> {
        profiling::scope!("LogStore::sources::read");
        let source = {
            let sources = self.sources.read().expect("sources lock poisoned");
            sources
                .get(&id.source_id)
                .ok_or_else(|| "Source not found".to_string())?
                .clone()
        };
        source.set_time_offset_to_target(id.line_index, target_time)
    }

    /// Get time offset for a source
    pub fn get_time_offset_ms(&self, id: &StoreID) -> Option<i64> {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");
        sources.get(&id.source_id).map(|s| s.get_time_offset_ms())
    }

    /// Check if a line has a bookmark
    pub fn has_bookmark(&self, id: &StoreID) -> bool {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");
        sources
            .get(&id.source_id)
            .is_some_and(|s| s.has_bookmark(id.line_index))
    }

    /// Get a bookmark by `StoreID`
    pub fn get_bookmark(&self, id: &StoreID) -> Option<BookmarkData> {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");
        sources
            .get(&id.source_id)
            .and_then(|s| s.get_bookmark(id.line_index))
            .map(|b| BookmarkData {
                store_id: *id,
                name: b.name,
            })
    }

    /// Get all bookmarks across all sources, with their `StoreIDs`
    pub fn get_all_bookmarks(&self) -> Vec<BookmarkData> {
        profiling::scope!("LogStore::get_all_bookmarks");
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");
        sources
            .values()
            .flat_map(|source| {
                let source_id = source.source_id();
                source
                    .get_bookmarks()
                    .into_iter()
                    .map(move |bookmark| BookmarkData {
                        store_id: StoreID {
                            source_id,
                            line_index: bookmark.line_index,
                        },
                        name: bookmark.name,
                    })
            })
            .collect()
    }

    /// Save all sources' .crab files
    pub fn save_all_crab_files(&self, filters: &[SavedFilter], highlights: &[SavedHighlight]) {
        profiling::scope!("LogStore::save_all_crab_files");
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");
        for source in sources.values() {
            source.save_crab_file(filters, highlights);
        }
    }

    // ========================================================================
    // Line Queries
    // ========================================================================

    /// Get line indices matching a predicate, sorted by timestamp
    ///
    /// Uses the pre-sorted `by_timestamp` index within each source, then merges.
    /// Returns `StoreIDs` for matching lines, sorted by timestamp.
    pub fn get_matching_ids<F>(&self, predicate: F) -> Vec<StoreID>
    where
        F: Fn(&LogLine) -> bool + Sync,
    {
        profiling::scope!("LogStore::get_matching_ids");
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");

        // Collect time offsets to avoid recursive locking in merge
        let time_offsets: HashMap<u64, i64> = sources
            .iter()
            .map(|(&source_id, source)| (source_id, source.get_time_offset_ms()))
            .collect();

        // Parallel filter each source, collect results
        let per_source: Vec<Vec<StoreID>> = {
            profiling::scope!("parallel_filter_sources");
            sources
                .par_values()
                .map(|source| {
                    let source_id = source.source_id();
                    let lines = source.lines.read().expect("lines lock poisoned");
                    // Iterate in timestamp order, filter, collect
                    source
                        .by_timestamp
                        .read()
                        .expect("by_timestamp lock poisoned")
                        .par_iter()
                        .filter_map(|&idx| {
                            let line = &lines[idx];
                            if predicate(line) {
                                Some(StoreID {
                                    source_id,
                                    line_index: idx,
                                })
                            } else {
                                None
                            }
                        })
                        .collect() // Materialize ParIter here so that it can be iterated sequentially later
                })
                .collect()
        };

        // Release sources lock before merge
        drop(sources);

        // K-way merge of sorted sources by timestamp
        self.merge_sorted_sources(per_source, &time_offsets)
    }

    /// K-way merge of pre-sorted `StoreID` vectors by timestamp
    fn merge_sorted_sources(&self, sources: Vec<Vec<StoreID>>, time_offsets: &HashMap<u64, i64>) -> Vec<StoreID> {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;

        profiling::scope!("LogStore::merge_sorted_sources");

        let total_len: usize = sources.iter().map(Vec::len).sum();
        let mut result = Vec::with_capacity(total_len);

        // Convert to iterators
        let mut iters: Vec<_> = sources.into_iter().map(IntoIterator::into_iter).collect();

        // Use a min-heap: (timestamp, source_idx, store_id) - Reverse for min-heap behavior
        let mut heap: BinaryHeap<Reverse<(chrono::DateTime<Local>, usize, StoreID)>> =
            BinaryHeap::new();

        // Helper to compute adjusted timestamp using the provided offsets
        let get_adjusted_time = |id: &StoreID, line: &LogLine| -> chrono::DateTime<Local> {
            let offset_ms = time_offsets.get(&id.source_id).copied().unwrap_or(0);
            if offset_ms != 0 {
                line.uncalibrated_timestamp() + chrono::Duration::milliseconds(offset_ms)
            } else {
                line.uncalibrated_timestamp()
            }
        };

        // Initialize heap with first element from each non-empty source
        for (src_idx, iter) in iters.iter_mut().enumerate() {
            if let Some(id) = iter.next() {
                if let Some(line) = self.get_by_id(&id) {
                    // Apply time offset for accurate merge ordering
                    let adjusted_time = get_adjusted_time(&id, &line);
                    heap.push(Reverse((adjusted_time, src_idx, id)));
                }
            }
        }

        // Merge
        while let Some(Reverse((_, src_idx, id))) = heap.pop() {
            result.push(id);

            // Push the next element from this source onto the heap
            if let Some(next_id) = iters[src_idx].next() {
                if let Some(line) = self.get_by_id(&next_id) {
                    // Apply time offset for accurate merge ordering
                    let adjusted_time = get_adjusted_time(&next_id, &line);
                    heap.push(Reverse((adjusted_time, src_idx, next_id)));
                }
            }
        }

        result
    }

    /// Get timestamp with time offset applied
    pub fn get_adjusted_timestamp(&self, id: &StoreID, line: &LogLine) -> chrono::DateTime<Local> {
        let offset_ms = self.get_time_offset_ms(id).unwrap_or(0);
        if offset_ms != 0 {
            line.uncalibrated_timestamp() + chrono::Duration::milliseconds(offset_ms)
        } else {
            line.uncalibrated_timestamp()
        }
    }

    pub fn get_by_id(&self, id: &StoreID) -> Option<LogLine> {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");
        sources.get(&id.source_id)?.get_by_id(id.line_index)
    }
}

/// Named bookmark with optional description
///
/// Each bookmark is stored within its source's .crab file.
/// The `line_index` is the line number within that source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    /// Line index within the source (not a global `StoreID`)
    pub line_index: usize,
    pub name: String,
}
