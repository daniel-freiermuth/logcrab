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

use crate::parser::line::LogLine;
use rayon::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// A single log source with its lines, wrapped in RwLock for thread-safe access
#[derive(Debug)]
pub struct SourceData {
    lines: RwLock<Vec<LogLine>>,
    version: AtomicU64,
}

impl SourceData {
    /// Create a SourceData for a file source (id will be assigned by LogStore)
    pub fn new() -> Self {
        Self {
            lines: RwLock::new(Vec::new()),
            version: AtomicU64::new(1),
        }
    }

    /// Bump the version number (call after appending lines)
    fn bump_version(&self) {
        self.version.fetch_add(1, Ordering::SeqCst);
    }

    /// Get current version number (bumped whenever data changes)
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }

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

    /// Check if empty
    #[allow(dead_code)] // Future-proofing for multi-source support
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

    /// Get line indices matching a predicate (parallel filtering, then merge)
    ///
    /// The predicate runs in parallel across all sources and within each source.
    /// Results are collected per-source and then merged.
    /// Returns line_number values (usize) for matching lines.
    pub fn get_matching_ids<F>(&self, predicate: F) -> Vec<usize>
    where
        F: Fn(&LogLine) -> bool + Sync,
    {
        let sources = self.sources.read().unwrap();

        // Parallel filter each source, collect results
        let per_source: Vec<Vec<usize>> = sources
            .par_iter()
            .map(|source| {
                let lines = source.lines.read().unwrap();
                lines
                    .par_iter()
                    .enumerate()
                    .filter_map(|(idx, line)| if predicate(line) { Some(idx) } else { None })
                    .collect()
            })
            .collect();

        // For multiple sources: flatten and sort by line_number (timestamp-based ordering)
        let result: Vec<usize> = per_source.into_iter().flatten().collect();
        // result.par_sort_unstable();
        result
    }

    pub fn get_by_id(&self, id: usize) -> Option<LogLine> {
        let sources = self.sources.read().unwrap();
        if let Some(source) = sources.iter().next() {
            return source.get_by_id(id);
        }
        None
    }
}
