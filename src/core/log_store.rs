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

use crate::core::session::{CrabFile, CRAB_FILE_VERSION};
use crate::core::{SavedFilter, SavedHighlight};
use crate::parser::line::{LogLine, LogLineCore};
use crate::ui::tabs::bookmarks_tab::BookmarkData;
use chrono::Local;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, RwLock};

/// A single log source with its lines, wrapped in `RwLock` for thread-safe access
#[derive(Debug)]
pub struct SourceData {
    /// Path to the source file (None for stdin or unnamed sources)
    file_path: Option<PathBuf>,
    /// Log lines in file order (index = `line_number` - 1, eternal)
    lines: RwLock<Vec<LogLine>>,
    /// Indices into `lines`, sorted by timestamp for time-ordered iteration
    by_timestamp: RwLock<Vec<usize>>,
    /// Bookmarks for this source, keyed by line index within this source
    bookmarks: RwLock<HashMap<usize, Bookmark>>,
    version: AtomicU64,
}

impl SourceData {
    /// Create a `SourceData` for a file source
    pub fn new(file_path: Option<PathBuf>) -> Self {
        let sd = Self {
            file_path,
            lines: RwLock::new(Vec::new()),
            by_timestamp: RwLock::new(Vec::new()),
            bookmarks: RwLock::new(HashMap::new()),
            version: AtomicU64::new(1),
        };
        sd.load_bookmarks();
        sd
    }

    /// Get the .crab file path for this source
    fn crab_file_path(&self) -> Option<PathBuf> {
        self.file_path.as_ref().map(|p| {
            let mut crab_path = p.clone();
            crab_path.set_file_name(format!(
                "{}.crab",
                p.file_name().unwrap_or_default().to_string_lossy()
            ));
            crab_path
        })
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

    // ========================================================================
    // Bookmark Management
    // ========================================================================

    /// Add or update a bookmark for a line in this source
    fn set_bookmark(&self, line_index: usize, name: String) {
        profiling::scope!("SourceData::bookmarks::write");
        let bookmark = Bookmark { line_index, name };
        self.bookmarks.write().unwrap().insert(line_index, bookmark);
    }

    /// Remove a bookmark from this source
    fn remove_bookmark(&self, line_index: usize) -> Option<Bookmark> {
        profiling::scope!("SourceData::bookmarks::write");
        self.bookmarks.write().unwrap().remove(&line_index)
    }

    /// Check if a line has a bookmark
    fn has_bookmark(&self, line_index: usize) -> bool {
        profiling::scope!("SourceData::bookmarks::read");
        self.bookmarks.read().unwrap().contains_key(&line_index)
    }

    /// Get a bookmark by line index
    fn get_bookmark(&self, line_index: usize) -> Option<Bookmark> {
        profiling::scope!("SourceData::bookmarks::read");
        self.bookmarks.read().unwrap().get(&line_index).cloned()
    }

    /// Get all bookmarks for this source
    fn get_bookmarks(&self) -> Vec<Bookmark> {
        profiling::scope!("SourceData::bookmarks::read");
        self.bookmarks.read().unwrap().values().cloned().collect()
    }

    /// Load bookmarks from this source's .crab file
    fn load_bookmarks(&self) {
        let Some(crab_path) = self.crab_file_path() else {
            return;
        };

        match CrabFile::load(&crab_path) {
            Ok(crab_data) => {
                log::info!(
                    "Loaded {} bookmarks from {}",
                    crab_data.bookmarks.len(),
                    crab_path.display()
                );
                profiling::scope!("SourceData::bookmarks::write");
                let mut bookmarks = self.bookmarks.write().unwrap();
                for bookmark in crab_data.bookmarks {
                    bookmarks.insert(bookmark.line_index, bookmark);
                }
            }
            Err(crate::core::SessionError::Io(ref e))
                if e.kind() == std::io::ErrorKind::NotFound =>
            {
                // No .crab file yet, that's fine
            }
            Err(e) => {
                log::warn!("Failed to load .crab file {}: {e}", crab_path.display());
            }
        }
    }

    /// Save bookmarks to this source's .crab file
    /// Note: filters and highlights are passed in since they're shared across sources
    pub fn save_crab_file(&self, filters: &[SavedFilter], highlights: &[SavedHighlight]) {
        let Some(crab_path) = self.crab_file_path() else {
            log::debug!("Skipping .crab save for source without file path");
            return;
        };

        let crab_data = CrabFile {
            version: CRAB_FILE_VERSION,
            bookmarks: self.get_bookmarks(),
            filters: filters.to_vec(),
            highlights: highlights.to_vec(),
        };

        match crab_data.save(&crab_path) {
            Ok(()) => log::debug!(
                "Saved .crab file {} with {} bookmarks",
                crab_path.display(),
                crab_data.bookmarks.len()
            ),
            Err(e) => log::error!("Failed to save .crab file {}: {e}", crab_path.display()),
        }
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
        let new_start_idx = {
            profiling::scope!("SourceData::lines::read");
            self.lines.read().unwrap().len()
        };
        {
            profiling::scope!("SourceData::lines::write");
            self.lines.write().unwrap().extend(lines);
        }

        let lines = {
            profiling::scope!("SourceData::lines::read");
            self.lines.read().unwrap()
        };
        let existing_by_ts = {
            profiling::scope!("SourceData::by_timestamp::read");
            self.by_timestamp.read().unwrap().clone()
        };

        // Build timestamp index for new lines, sorted by timestamp
        let new_by_ts = {
            profiling::scope!("sort_new_indices");
            let mut indices: Vec<usize> = (new_start_idx..lines.len()).collect();
            indices.par_sort_by_key(|&idx| lines[idx].timestamp());
            indices
        };

        // Merge existing and new timestamp indices (both are sorted by timestamp)
        let merged_by_ts = {
            profiling::scope!("merge_timestamp_indices");
            let mut merged = Vec::with_capacity(existing_by_ts.len() + new_by_ts.len());
            let mut i_exist = 0;
            let mut j_new = 0;
            while i_exist < existing_by_ts.len() && j_new < new_by_ts.len() {
                let ts_exist = lines[existing_by_ts[i_exist]].timestamp();
                let ts_new = lines[new_by_ts[j_new]].timestamp();
                if ts_exist <= ts_new {
                    merged.push(existing_by_ts[i_exist]);
                    i_exist += 1;
                } else {
                    merged.push(new_by_ts[j_new]);
                    j_new += 1;
                }
            }
            drop(lines);
            merged.extend_from_slice(&existing_by_ts[i_exist..]);
            merged.extend_from_slice(&new_by_ts[j_new..]);
            merged
        };

        {
            profiling::scope!("SourceData::by_timestamp::write");
            *self.by_timestamp.write().unwrap() = merged_by_ts;
        }
        self.bump_version();
    }

    /// Set anomaly scores for lines (indexed by position in the vector)
    /// Scores vec should be same length as lines
    pub fn set_scores(&self, scores: &[f64]) {
        profiling::scope!("SourceData::set_scores");
        profiling::scope!("SourceData::lines::write");
        let mut guard = self.lines.write().unwrap();
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
        self.lines.read().unwrap().len()
    }

    /// Check if this source has no lines
    pub fn is_empty(&self) -> bool {
        profiling::scope!("SourceData::lines::read");
        self.lines.read().unwrap().is_empty()
    }

    /// Get a clone of all lines for iteration
    /// This clones the entire Vec - use sparingly (e.g., one-time scoring)
    pub fn clone_lines(&self) -> Vec<LogLine> {
        profiling::scope!("SourceData::clone_lines");
        self.lines.read().unwrap().clone()
    }

    pub fn get_by_id(&self, id: usize) -> Option<LogLine> {
        profiling::scope!("SourceData::lines::read");
        let guard = self.lines.read().unwrap();
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

        // Get the reference line and extract timing info
        let reference_line = {
            let guard = self.lines.read().unwrap();
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
            LogLineVariant::Generic(_) | LogLineVariant::Logcat(_) => {
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
            let mut guard = self.lines.write().unwrap();
            for line in guard.iter_mut() {
                if let LogLineVariant::Dlt(dlt_line) = line {
                    // Check if this line matches the target ECU and App
                    let should_update = if let (Some(target_ecu), Some(target_app)) =
                        (ecu_id, app_id)
                    {
                        let ecu_matches = dlt_line
                            .dlt_message
                            .header
                            .ecu_id
                            .as_ref()
                            .is_some_and(|ecu| ecu.as_str() == target_ecu);
                        let app_matches = dlt_line
                            .dlt_message
                            .extended_header
                            .as_ref()
                            .is_some_and(|ext| ext.application_id.as_str() == target_app.as_str());
                        ecu_matches && app_matches
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

        // Rebuild timestamp-sorted index
        let mut indices: Vec<usize> = (0..self.lines.read().unwrap().len()).collect();
        let lines = self.lines.read().unwrap();
        indices.par_sort_by_key(|&idx| lines[idx].timestamp());
        drop(lines);
        *self.by_timestamp.write().unwrap() = indices;

        self.bump_version();

        Ok(())
    }

    /// Load and merge filters and highlights
    pub fn load_saved_filters_and_highlights(&self) -> (Vec<SavedFilter>, Vec<SavedHighlight>) {
        if let Some(crab_path) = self.crab_file_path() {
            match CrabFile::load(&crab_path) {
                Ok(crab_data) => {
                    return (crab_data.filters, crab_data.highlights);
                }
                Err(crate::core::SessionError::Io(ref e))
                    if e.kind() == std::io::ErrorKind::NotFound =>
                {
                    // No .crab file yet, that's fine
                }
                Err(e) => {
                    log::warn!("Failed to load .crab file {}: {e}", crab_path.display());
                }
            }
        }
        (Vec::new(), Vec::new())
    }
}

/// Central storage for log lines from one or more sources
///
/// Thread-safe: can be shared across threads with Arc<LogStore>
#[derive(Debug)]
pub struct LogStore {
    sources: RwLock<Vec<Arc<SourceData>>>,
}

impl Clone for LogStore {
    fn clone(&self) -> Self {
        profiling::scope!("LogStore::sources::read");
        Self {
            sources: RwLock::new(self.sources.read().unwrap().clone()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StoreID {
    source_index: usize,
    line_index: usize,
}

impl StoreID {
    /// Compare two `StoreIDs` by their line timestamps.
    ///
    /// When both lines exist in the store, compares by timestamp first,
    /// then by `source_index` and `line_index` for stability.
    /// When lines are missing (e.g., during file loading), falls back to
    /// structural ordering to maintain a valid total order.
    pub fn cmp(&self, other: &Self, store: &LogStore) -> Ordering {
        let self_line = store.get_by_id(self);
        let other_line = store.get_by_id(other);

        match (self_line, other_line) {
            (Some(l1), Some(l2)) => {
                // Both lines exist: compare by timestamp, then structurally for stability
                l1.timestamp()
                    .cmp(&l2.timestamp())
                    .then_with(|| self.source_index.cmp(&other.source_index))
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
            sources: RwLock::new(Vec::new()),
        })
    }

    /// Add a source to the store, returns the `SourceData` for appending lines
    pub fn add_source(self: &Arc<Self>, source_data: &Arc<SourceData>) {
        profiling::scope!("LogStore::sources::write");
        self.sources.write().unwrap().push(Arc::clone(source_data));
    }

    /// Check if a file with the given path is already loaded in the store
    pub fn contains_file(&self, path: &Path) -> bool {
        profiling::scope!("LogStore::sources::read");
        let canonical_path = path.canonicalize().ok();
        let sources = self.sources.read().unwrap();
        sources.iter().any(|source| {
            source.file_path.as_ref().is_some_and(|source_path| {
                // Try canonical comparison first, fall back to direct comparison
                if let Some(ref canonical) = canonical_path {
                    if let Ok(source_canonical) = source_path.canonicalize() {
                        return &source_canonical == canonical;
                    }
                }
                source_path == path
            })
        })
    }

    /// Get current version number (bumped whenever data changes)
    pub fn version(&self) -> u64 {
        profiling::scope!("LogStore::version");
        profiling::scope!("LogStore::sources::read");
        self.sources
            .read()
            .unwrap()
            .iter()
            .map(|s| s.version())
            .sum()
    }

    /// Get total number of lines across all sources
    pub fn total_lines(&self) -> usize {
        profiling::scope!("LogStore::sources::read");
        self.sources.read().unwrap().iter().map(|s| s.len()).sum()
    }

    /// Get the source name (filename) for a given `StoreID`
    pub fn get_source_name(&self, id: &StoreID) -> Option<String> {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().unwrap();
        sources.get(id.source_index).and_then(|source| {
            source.file_path.as_ref().and_then(|p| {
                p.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
        })
    }

    // ========================================================================
    // Bookmark Management (delegates to appropriate SourceData)
    // ========================================================================

    /// Add or update a bookmark
    pub fn set_bookmark(&self, id: &StoreID, name: String) {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().unwrap();
        if let Some(source) = sources.get(id.source_index) {
            source.set_bookmark(id.line_index, name);
        }
    }

    /// Remove a bookmark
    pub fn remove_bookmark(&self, id: &StoreID) -> Option<Bookmark> {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().unwrap();
        sources
            .get(id.source_index)
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
        let sources = self.sources.read().unwrap();
        let source = sources
            .get(id.source_index)
            .ok_or_else(|| "Source not found".to_string())?;
        source.resync_dlt_time_to_target(id.line_index, target_time, ecu_id, app_id)
    }

    /// Check if a line has a bookmark
    pub fn has_bookmark(&self, id: &StoreID) -> bool {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().unwrap();
        sources
            .get(id.source_index)
            .is_some_and(|s| s.has_bookmark(id.line_index))
    }

    /// Get a bookmark by `StoreID`
    pub fn get_bookmark(&self, id: &StoreID) -> Option<BookmarkData> {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().unwrap();
        sources
            .get(id.source_index)
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
        let sources = self.sources.read().unwrap();
        sources
            .iter()
            .enumerate()
            .flat_map(|(source_index, source)| {
                source
                    .get_bookmarks()
                    .into_iter()
                    .map(move |bookmark| BookmarkData {
                        store_id: StoreID {
                            source_index,
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
        let sources = self.sources.read().unwrap();
        for source in sources.iter() {
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
        let sources = self.sources.read().unwrap();

        // Parallel filter each source, collect results
        let per_source: Vec<Vec<StoreID>> = {
            profiling::scope!("parallel_filter_sources");
            sources
                .par_iter()
                .enumerate()
                .map(|(s_idx, source)| {
                    let lines = source.lines.read().unwrap();
                    // Iterate in timestamp order, filter, collect
                    source
                        .by_timestamp
                        .read()
                        .unwrap()
                        .par_iter()
                        .filter_map(|&idx| {
                            let line = &lines[idx];
                            if predicate(line) {
                                Some(StoreID {
                                    source_index: s_idx,
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

        // K-way merge of sorted sources by timestamp
        self.merge_sorted_sources(per_source)
    }

    /// K-way merge of pre-sorted `StoreID` vectors by timestamp
    fn merge_sorted_sources(&self, sources: Vec<Vec<StoreID>>) -> Vec<StoreID> {
        profiling::scope!("LogStore::merge_sorted_sources");
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;

        let total_len: usize = sources.iter().map(Vec::len).sum();
        let mut result = Vec::with_capacity(total_len);

        // Convert to iterators
        let mut iters: Vec<_> = sources.into_iter().map(IntoIterator::into_iter).collect();

        // Use a min-heap: (timestamp, source_idx, store_id) - Reverse for min-heap behavior
        let mut heap: BinaryHeap<Reverse<(chrono::DateTime<Local>, usize, StoreID)>> =
            BinaryHeap::new();

        // Initialize heap with first element from each non-empty source
        for (src_idx, iter) in iters.iter_mut().enumerate() {
            if let Some(id) = iter.next() {
                if let Some(line) = self.get_by_id(&id) {
                    heap.push(Reverse((line.timestamp(), src_idx, id)));
                }
            }
        }

        // Merge
        while let Some(Reverse((_, src_idx, id))) = heap.pop() {
            result.push(id);

            // Push the next element from this source onto the heap
            if let Some(next_id) = iters[src_idx].next() {
                if let Some(line) = self.get_by_id(&next_id) {
                    heap.push(Reverse((line.timestamp(), src_idx, next_id)));
                }
            }
        }

        result
    }

    pub fn get_by_id(&self, id: &StoreID) -> Option<LogLine> {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().unwrap();
        sources.get(id.source_index)?.get_by_id(id.line_index)
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
