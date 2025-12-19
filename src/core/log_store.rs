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
use crate::parser::line::LogLine;
use crate::ui::tabs::bookmarks_tab::BookmarkData;
use chrono::Local;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, RwLock};

/// Information about a log source for UI display
#[derive(Debug, Clone)]
pub struct SourceInfo {
    /// Index in the store's sources list
    pub index: usize,
    /// Display name (usually the filename)
    pub name: String,
    /// Time offset in seconds applied to this source's timestamps
    pub time_offset_secs: i64,
}

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
    /// Time offset in seconds to apply to all timestamps from this source.
    /// Positive values shift timestamps forward (source is behind), negative shift back.
    /// Use this to align sources recorded in different timezones.
    time_offset_secs: AtomicI64,
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
            time_offset_secs: AtomicI64::new(0),
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
    // Time Offset Management
    // ========================================================================

    /// Get the time offset in seconds for this source.
    /// Positive values mean the source timestamps are shifted forward.
    pub fn time_offset_secs(&self) -> i64 {
        self.time_offset_secs.load(AtomicOrdering::SeqCst)
    }

    /// Set the time offset in seconds for this source.
    /// Positive values shift timestamps forward (use when source was recorded in an earlier timezone).
    /// For example, if source is in UTC and local is UTC+1, set offset to +3600.
    pub fn set_time_offset_secs(&self, offset: i64) {
        self.time_offset_secs.store(offset, AtomicOrdering::SeqCst);
        self.bump_version(); // Trigger re-sort and re-render
    }

    /// Apply the time offset to a timestamp
    pub fn adjust_timestamp(&self, ts: chrono::DateTime<Local>) -> chrono::DateTime<Local> {
        let offset = self.time_offset_secs();
        if offset == 0 {
            ts
        } else {
            ts + chrono::Duration::seconds(offset)
        }
    }

    // ========================================================================
    // Bookmark Management
    // ========================================================================

    /// Add or update a bookmark for a line in this source
    fn set_bookmark(&self, line_index: usize, name: String) {
        let bookmark = Bookmark { line_index, name };
        self.bookmarks.write().unwrap().insert(line_index, bookmark);
    }

    /// Remove a bookmark from this source
    fn remove_bookmark(&self, line_index: usize) -> Option<Bookmark> {
        self.bookmarks.write().unwrap().remove(&line_index)
    }

    /// Check if a line has a bookmark
    fn has_bookmark(&self, line_index: usize) -> bool {
        self.bookmarks.read().unwrap().contains_key(&line_index)
    }

    /// Get a bookmark by line index
    fn get_bookmark(&self, line_index: usize) -> Option<Bookmark> {
        self.bookmarks.read().unwrap().get(&line_index).cloned()
    }

    /// Get all bookmarks for this source
    fn get_bookmarks(&self) -> Vec<Bookmark> {
        self.bookmarks.read().unwrap().values().cloned().collect()
    }

    /// Load bookmarks and time offset from this source's .crab file
    fn load_bookmarks(&self) {
        let Some(crab_path) = self.crab_file_path() else {
            return;
        };

        match CrabFile::load(&crab_path) {
            Ok(crab_data) => {
                log::info!(
                    "Loaded {} bookmarks and time_offset={}s from {}",
                    crab_data.bookmarks.len(),
                    crab_data.time_offset_secs,
                    crab_path.display()
                );
                let mut bookmarks = self.bookmarks.write().unwrap();
                for bookmark in crab_data.bookmarks {
                    bookmarks.insert(bookmark.line_index, bookmark);
                }
                // Load time offset
                self.time_offset_secs
                    .store(crab_data.time_offset_secs, AtomicOrdering::SeqCst);
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

    /// Save bookmarks and time offset to this source's .crab file
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
            time_offset_secs: self.time_offset_secs(),
        };

        match crab_data.save(&crab_path) {
            Ok(()) => log::debug!(
                "Saved .crab file {} with {} bookmarks, time_offset={}s",
                crab_path.display(),
                crab_data.bookmarks.len(),
                crab_data.time_offset_secs
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
        let new_start_idx = self.lines.read().unwrap().len();
        {
            self.lines.write().unwrap().extend(lines);
        }

        let lines = self.lines.read().unwrap();
        let existing_by_ts = self.by_timestamp.read().unwrap().clone();

        // Build timestamp index for new lines, sorted by timestamp
        let new_by_ts = {
            profiling::scope!("sort_new_indices");
            let mut indices: Vec<usize> = (new_start_idx..lines.len()).collect();
            indices.par_sort_by_key(|&idx| lines[idx].timestamp);
            indices
        };

        // Merge existing and new timestamp indices (both are sorted by timestamp)
        let merged_by_ts = {
            profiling::scope!("merge_timestamp_indices");
            let mut merged = Vec::with_capacity(existing_by_ts.len() + new_by_ts.len());
            let mut i_exist = 0;
            let mut j_new = 0;
            while i_exist < existing_by_ts.len() && j_new < new_by_ts.len() {
                let ts_exist = lines[existing_by_ts[i_exist]].timestamp;
                let ts_new = lines[new_by_ts[j_new]].timestamp;
                if ts_exist <= ts_new {
                    merged.push(existing_by_ts[i_exist]);
                    i_exist += 1;
                } else {
                    merged.push(new_by_ts[j_new]);
                    j_new += 1;
                }
            }
            merged.extend_from_slice(&existing_by_ts[i_exist..]);
            merged.extend_from_slice(&new_by_ts[j_new..]);
            merged
        };

        {
            profiling::scope!("swap_lines_and_index");
            *self.by_timestamp.write().unwrap() = merged_by_ts;
        }
        self.bump_version();
    }

    /// Set anomaly scores for lines (indexed by position in the vector)
    /// Scores vec should be same length as lines
    pub fn set_scores(&self, scores: &[f64]) {
        profiling::scope!("SourceData::set_scores");
        let mut guard = self.lines.write().unwrap();
        for (idx, &score) in scores.iter().enumerate() {
            if let Some(line) = guard.get_mut(idx) {
                line.anomaly_score = score;
            }
        }
        drop(guard);
        self.bump_version();
    }

    /// Get the number of lines
    pub fn len(&self) -> usize {
        self.lines.read().unwrap().len()
    }

    /// Check if this source has no lines
    pub fn is_empty(&self) -> bool {
        self.lines.read().unwrap().is_empty()
    }

    /// Get a clone of all lines for iteration
    /// This clones the entire Vec - use sparingly (e.g., one-time scoring)
    pub fn clone_lines(&self) -> Vec<LogLine> {
        profiling::scope!("SourceData::clone_lines");
        self.lines.read().unwrap().clone()
    }

    pub fn get_by_id(&self, id: usize) -> Option<LogLine> {
        let guard = self.lines.read().unwrap();
        guard.get(id).cloned()
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
    /// Compare two StoreIDs by their adjusted timestamps
    pub fn cmp(&self, other: &StoreID, store: &LogStore) -> Ordering {
        let t1 = store
            .get_adjusted_timestamp(self)
            .expect("Tried to compare non-existent line (self).");
        let t2 = store
            .get_adjusted_timestamp(other)
            .expect("Tried to compare non-existent line (other).");

        match t1.cmp(&t2) {
            Ordering::Equal => match self.source_index.cmp(&other.source_index) {
                Ordering::Equal => self.line_index.cmp(&other.line_index),
                ord => ord,
            },
            ord => ord,
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
        self.sources.write().unwrap().push(Arc::clone(source_data));
    }

    /// Check if a file with the given path is already loaded in the store
    pub fn contains_file(&self, path: &Path) -> bool {
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
        self.sources
            .read()
            .unwrap()
            .iter()
            .map(|s| s.version())
            .sum()
    }

    /// Get total number of lines across all sources
    pub fn total_lines(&self) -> usize {
        self.sources.read().unwrap().iter().map(|s| s.len()).sum()
    }

    /// Get the source name (filename) for a given StoreID
    pub fn get_source_name(&self, id: &StoreID) -> Option<String> {
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
        let sources = self.sources.read().unwrap();
        if let Some(source) = sources.get(id.source_index) {
            source.set_bookmark(id.line_index, name);
        }
    }

    /// Remove a bookmark
    pub fn remove_bookmark(&self, id: &StoreID) -> Option<Bookmark> {
        let sources = self.sources.read().unwrap();
        sources
            .get(id.source_index)
            .and_then(|s| s.remove_bookmark(id.line_index))
    }

    /// Check if a line has a bookmark
    pub fn has_bookmark(&self, id: &StoreID) -> bool {
        let sources = self.sources.read().unwrap();
        sources
            .get(id.source_index)
            .is_some_and(|s| s.has_bookmark(id.line_index))
    }

    /// Get a bookmark by `StoreID`
    pub fn get_bookmark(&self, id: &StoreID) -> Option<BookmarkData> {
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

        // Use a min-heap: (adjusted_timestamp, source_idx, store_id) - Reverse for min-heap behavior
        let mut heap: BinaryHeap<Reverse<(chrono::DateTime<Local>, usize, StoreID)>> =
            BinaryHeap::new();

        // Initialize heap with first element from each non-empty source
        for (src_idx, iter) in iters.iter_mut().enumerate() {
            if let Some(id) = iter.next() {
                // Use adjusted timestamp for sorting
                if let Some(ts) = self.get_adjusted_timestamp(&id) {
                    heap.push(Reverse((ts, src_idx, id)));
                }
            }
        }

        // Merge
        while let Some(Reverse((_, src_idx, id))) = heap.pop() {
            result.push(id);

            // Push the next element from this source onto the heap
            if let Some(next_id) = iters[src_idx].next() {
                // Use adjusted timestamp for sorting
                if let Some(ts) = self.get_adjusted_timestamp(&next_id) {
                    heap.push(Reverse((ts, src_idx, next_id)));
                }
            }
        }

        result
    }

    pub fn get_by_id(&self, id: &StoreID) -> Option<LogLine> {
        let sources = self.sources.read().unwrap();
        sources.get(id.source_index)?.get_by_id(id.line_index)
    }

    /// Get the adjusted timestamp for a line, applying the source's time offset
    pub fn get_adjusted_timestamp(&self, id: &StoreID) -> Option<chrono::DateTime<Local>> {
        let sources = self.sources.read().unwrap();
        let source = sources.get(id.source_index)?;
        let line = source.get_by_id(id.line_index)?;
        Some(source.adjust_timestamp(line.timestamp))
    }

    /// Get the time offset in seconds for a source
    pub fn get_source_time_offset(&self, source_index: usize) -> Option<i64> {
        let sources = self.sources.read().unwrap();
        sources.get(source_index).map(|s| s.time_offset_secs())
    }

    /// Set the time offset in seconds for a source
    pub fn set_source_time_offset(&self, source_index: usize, offset_secs: i64) {
        let sources = self.sources.read().unwrap();
        if let Some(source) = sources.get(source_index) {
            source.set_time_offset_secs(offset_secs);
        }
    }

    /// Get all sources with their names and time offsets
    pub fn get_source_info(&self) -> Vec<SourceInfo> {
        let sources = self.sources.read().unwrap();
        sources
            .iter()
            .enumerate()
            .map(|(idx, source)| SourceInfo {
                index: idx,
                name: source
                    .file_path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("Source {idx}")),
                time_offset_secs: source.time_offset_secs(),
            })
            .collect()
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
