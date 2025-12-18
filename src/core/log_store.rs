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
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, RwLock};

/// Lines and their index, kept together for atomic updates
#[derive(Debug, Clone, Default)]
struct LinesData {
    /// Log lines sorted by timestamp
    lines: Vec<LogLine>,
    /// Index for O(1) lookup: by_line_number[line_number - 1] = line_index in `lines` vec
    by_line_number: Vec<usize>,
}

/// A single log source with its lines, wrapped in RwLock for thread-safe access
#[derive(Debug)]
pub struct SourceData {
    /// Path to the source file (None for stdin or unnamed sources)
    file_path: Option<PathBuf>,
    /// Lines and index under single lock for atomic access
    data: RwLock<LinesData>,
    /// Bookmarks for this source, keyed by line number
    bookmarks: RwLock<HashMap<usize, Bookmark>>,
    version: AtomicU64,
}

impl SourceData {
    /// Create a SourceData for a file source
    pub fn new(file_path: Option<PathBuf>) -> Self {
        let sd = Self {
            file_path,
            data: RwLock::new(LinesData::default()),
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
        let bookmark = Bookmark {
            line_number: line_index,
            name,
        };
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
                let mut bookmarks = self.bookmarks.write().unwrap();
                for bookmark in crab_data.bookmarks {
                    bookmarks.insert(bookmark.line_number, bookmark);
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
    /// Heavy work (merge + index build) is done outside the write lock to avoid
    /// blocking UI reads. Only the final pointer swap happens under the lock.
    pub fn append_lines(&self, lines: Vec<LogLine>) {
        if lines.is_empty() {
            return;
        }

        // Sort incoming lines (no lock needed)
        let sorted_lines = {
            profiling::scope!("sort_appended_lines");
            let mut ls = lines;
            ls.par_sort_unstable_by_key(|line| line.timestamp);
            ls
        };

        profiling::scope!("SourceData::append_lines");

        // Clone existing lines under read lock (brief)
        let existing = {
            profiling::scope!("clone_existing_lines");
            self.data.read().unwrap().lines.clone()
        };

        // Merge outside any lock (O(n), potentially slow)
        let merged = {
            profiling::scope!("merge_lines");
            let mut merged = Vec::with_capacity(existing.len() + sorted_lines.len());
            let mut i = 0;
            let mut j = 0;
            while i < existing.len() && j < sorted_lines.len() {
                if existing[i].timestamp <= sorted_lines[j].timestamp {
                    merged.push(existing[i].clone());
                    i += 1;
                } else {
                    merged.push(sorted_lines[j].clone());
                    j += 1;
                }
            }
            if i < existing.len() {
                merged.extend_from_slice(&existing[i..]);
            }
            if j < sorted_lines.len() {
                merged.extend_from_slice(&sorted_lines[j..]);
            }
            merged
        };

        // Build by_line_number index outside lock (O(n log n))
        let by_ln = {
            profiling::scope!("build_line_number_index");
            let mut pairs: Vec<(usize, usize)> = merged
                .iter()
                .enumerate()
                .map(|(idx, line)| (idx, line.line_number))
                .collect();
            pairs.sort_unstable_by_key(|(_, ln)| *ln);
            pairs.into_iter().map(|(idx, _)| idx).collect::<Vec<_>>()
        };

        // Write lock only for the atomic swap (very fast)
        {
            profiling::scope!("swap_lines_and_index");
            let mut guard = self.data.write().unwrap();
            guard.lines = merged;
            guard.by_line_number = by_ln;
        }

        self.bump_version();
    }

    /// Set anomaly scores for lines (indexed by position in the vector)
    /// Scores vec should be same length as lines
    pub fn set_scores(&self, scores: &[f64]) {
        profiling::scope!("SourceData::set_scores");
        let mut guard = self.data.write().unwrap();
        for (idx, &score) in scores.iter().enumerate() {
            if let Some(line) = guard.lines.get_mut(idx) {
                line.anomaly_score = score;
            }
        }
        drop(guard);
        self.bump_version();
    }

    /// Get the number of lines
    pub fn len(&self) -> usize {
        self.data.read().unwrap().lines.len()
    }

    /// Check if this source has no lines
    pub fn is_empty(&self) -> bool {
        self.data.read().unwrap().lines.is_empty()
    }

    /// Get a clone of all lines for iteration
    /// This clones the entire Vec - use sparingly (e.g., one-time scoring)
    pub fn clone_lines(&self) -> Vec<LogLine> {
        profiling::scope!("SourceData::clone_lines");
        self.data.read().unwrap().lines.clone()
    }

    pub fn get_by_id(&self, id: usize) -> Option<LogLine> {
        let guard = self.data.read().unwrap();
        guard.lines.get(id).cloned()
    }

    /// Find the vector index of a line by its original line_number
    /// Returns None if no line with that line_number exists
    /// O(1) lookup using the by_line_number index
    pub fn find_index_by_line_number(&self, line_number: usize) -> Option<usize> {
        self.data
            .read()
            .unwrap()
            .by_line_number
            .get(line_number.wrapping_sub(1))
            .copied()
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StoreID {
    source_index: usize,
    line_index: usize,
}

impl StoreID {
    pub fn cmp(&self, other: &StoreID, store: &LogStore) -> Ordering {
        let self_line = store
            .get_by_id(self)
            .expect("Tried to compare non-existent line (self).");
        let other_line = store
            .get_by_id(other)
            .expect("Tried to compare non-existent line (other).");
        let t1 = self_line.timestamp;
        let t2 = other_line.timestamp;

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
    /// Create a new empty LogStore
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            sources: RwLock::new(Vec::new()),
        })
    }

    /// Add a source to the store, returns the SourceData for appending lines
    pub fn add_source(self: &Arc<Self>, source_data: Arc<SourceData>) {
        self.sources.write().unwrap().push(Arc::clone(&source_data));
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

    // ========================================================================
    // Bookmark Management (delegates to appropriate SourceData)
    // ========================================================================

    /// Add or update a bookmark
    pub fn set_bookmark(&self, id: &StoreID, name: String) {
        let sources = self.sources.read().unwrap();
        if let Some(source) = sources.get(id.source_index) {
            // Get the original line_number from the LogLine
            if let Some(line) = source.get_by_id(id.line_index) {
                source.set_bookmark(line.line_number, name);
            }
        }
    }

    /// Remove a bookmark
    pub fn remove_bookmark(&self, id: &StoreID) -> Option<Bookmark> {
        let sources = self.sources.read().unwrap();
        if let Some(source) = sources.get(id.source_index) {
            if let Some(line) = source.get_by_id(id.line_index) {
                return source.remove_bookmark(line.line_number);
            }
        }
        None
    }

    /// Check if a line has a bookmark
    pub fn has_bookmark(&self, id: &StoreID) -> bool {
        let sources = self.sources.read().unwrap();
        if let Some(source) = sources.get(id.source_index) {
            if let Some(line) = source.get_by_id(id.line_index) {
                return source.has_bookmark(line.line_number);
            }
        }
        false
    }

    /// Get a bookmark by StoreID
    pub fn get_bookmark(&self, id: &StoreID) -> Option<BookmarkData> {
        let sources = self.sources.read().unwrap();
        if let Some(source) = sources.get(id.source_index) {
            if let Some(line) = source.get_by_id(id.line_index) {
                return source.get_bookmark(line.line_number).map(|b| BookmarkData {
                    store_id: id.clone(),
                    name: b.name,
                });
            }
        }
        None
    }

    /// Get all bookmarks across all sources, with their StoreIDs
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
                    .filter_map(move |bookmark| {
                        // Find the vector index for this line_number
                        source
                            .find_index_by_line_number(bookmark.line_number)
                            .map(|line_index| BookmarkData {
                                store_id: StoreID {
                                    source_index,
                                    line_index,
                                },
                                name: bookmark.name,
                            })
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

    /// Get line indices matching a predicate (parallel filtering, then merge)
    ///
    /// The predicate runs in parallel across all sources and within each source.
    /// Results are collected per-source and then merged by timestamp.
    /// Returns StoreIDs for matching lines, sorted by timestamp.
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
                    let data = source.data.read().unwrap();
                    data.lines
                        .par_iter()
                        .enumerate()
                        .filter_map(|(idx, line)| {
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

    /// K-way merge of pre-sorted StoreID vectors by timestamp
    fn merge_sorted_sources(&self, sources: Vec<Vec<StoreID>>) -> Vec<StoreID> {
        profiling::scope!("LogStore::merge_sorted_sources");
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;

        let total_len: usize = sources.iter().map(|s| s.len()).sum();
        let mut result = Vec::with_capacity(total_len);

        // Convert to iterators
        let mut iters: Vec<_> = sources.into_iter().map(|v| v.into_iter()).collect();

        // Use a min-heap: (timestamp, source_idx, store_id) - Reverse for min-heap behavior
        let mut heap: BinaryHeap<Reverse<(chrono::DateTime<Local>, usize, StoreID)>> =
            BinaryHeap::new();

        // Initialize heap with first element from each non-empty source
        for (src_idx, iter) in iters.iter_mut().enumerate() {
            if let Some(id) = iter.next() {
                if let Some(line) = self.get_by_id(&id) {
                    heap.push(Reverse((line.timestamp, src_idx, id)));
                }
            }
        }

        // Merge
        while let Some(Reverse((_, src_idx, id))) = heap.pop() {
            result.push(id);

            // Push the next element from this source onto the heap
            if let Some(next_id) = iters[src_idx].next() {
                if let Some(line) = self.get_by_id(&next_id) {
                    heap.push(Reverse((line.timestamp, src_idx, next_id)));
                }
            }
        }

        result
    }

    pub fn get_by_id(&self, id: &StoreID) -> Option<LogLine> {
        let sources = self.sources.read().unwrap();
        sources.get(id.source_index)?.get_by_id(id.line_index)
    }
}

/// Named bookmark with optional description
///
/// Each bookmark is stored within its source's .crab file.
/// The `line_number` is the original line number from the file (stable across sorts).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    /// Original line number from the file (not the vector index)
    pub line_number: usize,
    pub name: String,
}
