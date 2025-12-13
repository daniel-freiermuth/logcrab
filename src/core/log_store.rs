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
use egui::Color32;
use im::Vector;
use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Status of a log source
#[derive(Debug, Clone)]
#[allow(dead_code)] // Future-proofing for multi-source support
pub enum SourceStatus {
    Loading { progress: f32 },
    Done,
    Streaming, // For future stdin support
    Tailing,   // For future file tailing support (-f/--follow)
    Error(String),
}

/// Metadata about a log source
#[derive(Debug, Clone)]
#[allow(dead_code)] // Future-proofing for multi-source support
pub struct SourceInfo {
    pub id: u16,
    pub name: String,
    pub path: Option<PathBuf>,
    pub color: Color32,
    pub status: SourceStatus,
}

/// A single log source with its lines (inner data protected by RwLock)
#[derive(Debug, Clone, Default)]
pub struct SourceDataInner {
    pub info: Option<SourceInfo>,
    pub lines: Vector<LogLine>,
}

/// A single log source with its lines, wrapped in RwLock for thread-safe access
#[derive(Debug)]
pub struct SourceData {
    inner: RwLock<SourceDataInner>,
    version: AtomicU64,
}

impl SourceData {
    /// Create a SourceData for a file source (id will be assigned by LogStore)
    pub fn from_file(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .to_string();

        Self {
            inner: RwLock::new(SourceDataInner {
                info: Some(SourceInfo {
                    id: 0,
                    name,
                    path: Some(path),
                    color: Color32::WHITE,
                    status: SourceStatus::Loading { progress: 0.0 },
                }),
                lines: Vector::new(),
            }),
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

    /*
    /// Create a SourceData for stdin (id will be assigned by LogStore)
    #[allow(dead_code)] // Future-proofing for stdin support
    pub fn from_stdin() -> Self {
        Self::new(0, "stdin".to_string(), None)
    }
    */

    /// Get the source ID
    #[allow(dead_code)] // Future-proofing for multi-source support
    pub fn id(&self) -> u16 {
        self.inner
            .read()
            .unwrap()
            .info
            .as_ref()
            .map(|i| i.id)
            .unwrap_or(0)
    }

    /// Set the source ID (called by LogStore when adding)
    fn set_id(&self, id: u16) {
        if let Some(ref mut info) = self.inner.write().unwrap().info {
            info.id = id;
        }
    }

    /// Append lines to this source
    pub fn append_lines(&self, lines: Vec<LogLine>) {
        if lines.is_empty() {
            return;
        }
        {
            let mut guard = self.inner.write().unwrap();
            for line in lines {
                guard.lines.push_back(line);
            }
        }
        self.bump_version();
    }

    /// Set loading progress
    pub fn set_loading_progress(&self, progress: f32) {
        if let Some(ref mut info) = self.inner.write().unwrap().info {
            info.status = SourceStatus::Loading { progress };
        }
    }

    /*
    /// Get loading progress (0.0-1.0, or 1.0 if done)
    pub fn loading_progress(&self) -> f32 {
        self.inner
            .read()
            .unwrap()
            .info
            .as_ref()
            .map(|info| match info.status {
                SourceStatus::Loading { progress } => progress,
                SourceStatus::Done => 1.0,
                SourceStatus::Streaming | SourceStatus::Tailing => 0.0, // Progress unknown for streaming
                SourceStatus::Error(_) => 0.0,
            })
            .unwrap_or(0.0)
    } */

    /// Mark as done loading
    pub fn set_done(&self) {
        if let Some(ref mut info) = self.inner.write().unwrap().info {
            info.status = SourceStatus::Done;
        }
    }

    /// Set anomaly scores for lines (indexed by position in the vector)
    /// Scores vec should be same length as lines
    pub fn set_scores(&self, scores: &[f64]) {
        let mut guard = self.inner.write().unwrap();
        for (idx, &score) in scores.iter().enumerate() {
            if let Some(line) = guard.lines.get_mut(idx) {
                line.anomaly_score = score;
            }
        }
        // Note: don't bump version - scores don't affect filtering/display logic
    }

    /// Get the number of lines
    pub fn len(&self) -> usize {
        self.inner.read().unwrap().lines.len()
    }

    /// Check if empty
    #[allow(dead_code)] // Future-proofing for multi-source support
    pub fn is_empty(&self) -> bool {
        self.inner.read().unwrap().lines.is_empty()
    }

    /// O(1) snapshot of lines for lock-free iteration
    /// This is cheap - im::Vector clone is O(1)
    pub fn snapshot(&self) -> Vector<LogLine> {
        self.inner.read().unwrap().lines.clone()
    }

    /// Iterate over all lines (via snapshot)
    pub fn iter_all(&self) -> impl Iterator<Item = LogLine> {
        self.snapshot().into_iter()
    }

    pub fn get_by_id(&self, id: usize) -> Option<LogLine> {
        let guard = self.inner.read().unwrap();
        guard.lines.get(id).cloned()
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
        let id = self.sources.read().unwrap().len() as u16;
        source_data.set_id(id);

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
                source
                    .snapshot()
                    .par_iter()
                    .enumerate()
                    .filter(|(_idx, line)| predicate(line))
                    .map(|(idx, _line)| idx)
                    .collect()
            })
            .collect();

        // For multiple sources: flatten and sort by line_number (timestamp-based ordering)
        let result: Vec<usize> = per_source.into_iter().flatten().collect();
        // result.par_sort_unstable();
        result
    }

    /*
    /// Get all line indices (no filtering)
    pub fn get_all_ids(&self) -> Vec<usize> {
        self.get_matching_ids(|_| true)
    } */

    pub fn get_by_id(&self, id: usize) -> Option<LogLine> {
        let sources = self.sources.read().unwrap();
        for source in sources.iter() {
            return source.get_by_id(id);
        }
        None
    }
}
