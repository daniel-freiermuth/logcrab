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
use chrono::{Local, TimeDelta};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// A single log source with its lines, wrapped in RwLock for thread-safe access
#[derive(Debug)]
pub struct SourceData {
    /// Path to the source file (None for stdin or unnamed sources)
    file_path: Option<PathBuf>,
    lines: RwLock<Vec<LogLine>>,
    /// Bookmarks for this source, keyed by line index within this source
    bookmarks: RwLock<HashMap<usize, Bookmark>>,
    version: AtomicU64,
}

impl SourceData {
    /// Create a SourceData for a file source
    pub fn new(file_path: Option<PathBuf>) -> Self {
        let sd = Self {
            file_path,
            lines: RwLock::new(Vec::new()),
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
        self.version.fetch_add(1, Ordering::SeqCst);
    }

    /// Get current version number (bumped whenever data changes)
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
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
    pub fn append_lines(&self, lines: Vec<LogLine>) {
        if lines.is_empty() {
            return;
        }
        {
            let mut guard = self.lines.write().unwrap();
            guard.extend(lines);
        }
        self.bump_version();
    }

    /// Set anomaly scores for lines (indexed by position in the vector)
    /// Scores vec should be same length as lines
    pub fn set_scores(&self, scores: &[f64]) {
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StoreID {
    source_index: usize,
    line_index: usize,
}

impl StoreID {
    pub fn distance_to(&self, other: &StoreID, store: &LogStore) -> Option<StoreIdDistance> {
        let self_line = store.get_by_id(self)?;
        let other_line = store.get_by_id(other)?;
        let t1 = self_line.timestamp;
        let t2 = other_line.timestamp;

        let time_distance = t1.signed_duration_since(t2).abs();

        let line_distance = if self.source_index == other.source_index {
            Some((self.line_index as isize - other.line_index as isize).unsigned_abs())
        } else {
            None
        };

        Some(StoreIdDistance {
            time_distance,
            line_distance,
        })
    }
}

#[derive(PartialEq, Eq)]
pub struct StoreIdDistance {
    pub time_distance: TimeDelta,
    pub line_distance: Option<usize>,
}

impl StoreIdDistance {
    fn inner_cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self
            .time_distance
            .num_milliseconds()
            .cmp(&other.time_distance.num_milliseconds())
        {
            std::cmp::Ordering::Equal => match (self.line_distance, other.line_distance) {
                (Some(ld1), Some(ld2)) => ld1.cmp(&ld2),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            },
            other => other,
        }
    }
}

impl PartialOrd for StoreIdDistance {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(std::cmp::Ord::cmp(self, other))
    }
}

impl Ord for StoreIdDistance {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.inner_cmp(other)
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

    /// Get current version number (bumped whenever data changes)
    pub fn version(&self) -> u64 {
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

    /// Get a bookmark by StoreID
    pub fn get_bookmark(&self, id: &StoreID) -> Option<BookmarkData> {
        let sources = self.sources.read().unwrap();
        sources
            .get(id.source_index)
            .and_then(|s| s.get_bookmark(id.line_index))
            .map(|b| BookmarkData {
                store_id: id.clone(),
                name: b.name,
            })
    }

    /// Get all bookmarks across all sources, with their StoreIDs
    pub fn get_all_bookmarks(&self) -> Vec<BookmarkData> {
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
        let sources = self.sources.read().unwrap();

        // Parallel filter each source, collect results
        let per_source: Vec<Vec<StoreID>> = sources
            .par_iter()
            .enumerate()
            .map(|(s_idx, source)| {
                let lines = source.lines.read().unwrap();
                lines
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
            .collect();

        // K-way merge of sorted sources by timestamp
        self.merge_sorted_sources(per_source)
    }

    /// K-way merge of pre-sorted StoreID vectors by timestamp
    fn merge_sorted_sources(&self, sources: Vec<Vec<StoreID>>) -> Vec<StoreID> {
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
/// The `line_index` is the line number within that source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    /// Line index within the source (not a global StoreID)
    pub line_index: usize,
    pub name: String,
}

