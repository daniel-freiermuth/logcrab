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

use crate::core::session::{CrabFile, SessionError, CRAB_FILE_VERSION};
use crate::core::{SavedFilter, SavedHighlight};
use crate::filetype::{
    btsnoop::BtsnoopFileType, bugreport::BugreportFileType, dlt::DltFileType, dmesg::DmesgFileType,
    generic::GenericFileType, logcat::LogcatFileType, otel::OtelFileType, pcap::PcapFileType,
};
use crate::filetype::{
    btsnoop::BtsnoopLogLine, bugreport::BugreportLogLine, dlt::DltLogLine, dmesg::DmesgLogLine,
    generic::GenericLogLine, logcat::LogcatLogLine, otel::OtelLogLine, pcap::PcapLogLine,
};
use crate::filetype::{InputFileType, LineType, LogFileState};
use crate::ui::tabs::bookmarks_tab::BookmarkData;
use chrono::Local;
use egui;
use indexmap::IndexMap;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, RwLock};

use arc_swap::ArcSwap;
use dashmap::DashMap;

/// Global counter for generating unique source IDs.
/// Source IDs are stable across the lifetime of a source, even when other sources are removed.
static SOURCE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Lock-free storage for anomaly scores.
///
/// Uses `ArcSwap` for atomic pointer swaps — readers never block, and writers
/// simply build a new `Vec<f64>` and swap it in atomically. This avoids the
/// need for a write lock on `lines` when updating scores.
pub struct ScoreStore {
    /// Anomaly scores indexed by line position (same length as `lines`).
    /// Default score is 0.0; scores range from 0 to 100.
    scores: ArcSwap<Vec<f64>>,
}

impl ScoreStore {
    /// Create a new empty score store.
    pub fn new() -> Self {
        Self {
            scores: ArcSwap::new(Arc::new(Vec::new())),
        }
    }

    /// Set all scores atomically. The provided slice is copied into a new `Arc`.
    pub fn set_all(&self, scores: &[f64]) {
        self.scores.store(Arc::new(scores.to_vec()));
    }

    /// Get the score for a specific line index. Returns 0.0 if out of bounds.
    pub fn get(&self, index: usize) -> f64 {
        let guard = self.scores.load();
        guard.get(index).copied().unwrap_or(0.0)
    }

    /// Resize the internal vec to accommodate new lines (fills with 0.0).
    /// Called when lines are appended to keep scores in sync.
    pub fn resize(&self, new_len: usize) {
        let current = self.scores.load();
        if current.len() < new_len {
            let mut new_scores: Vec<f64> = (**current).clone();
            new_scores.resize(new_len, 0.0);
            self.scores.store(Arc::new(new_scores));
        }
    }
}

impl Default for ScoreStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ScoreStore {
    fn clone(&self) -> Self {
        Self {
            scores: ArcSwap::new(Arc::clone(&self.scores.load())),
        }
    }
}

impl std::fmt::Debug for ScoreStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self.scores.load();
        write!(f, "ScoreStore({} scores)", guard.len())
    }
}

/// A single log source with its lines, wrapped in `RwLock` for thread-safe access
pub struct SourceData<FT>
where
    FT: InputFileType,
{
    /// Unique, stable identifier for this source (does not change when other sources are removed)
    source_id: u64,
    /// Path to the source file
    file_path: PathBuf,
    /// Log lines in file order (index = `line_number` - 1, eternal)
    lines: RwLock<Vec<FT::LineType>>,
    /// Indices into `lines`, sorted by timestamp for time-ordered iteration
    by_timestamp: RwLock<Vec<usize>>,
    /// File type config — shared across all sources of this type (e.g. DLT timestamp source setting).
    /// Wrapped in `Arc<RwLock>` so a single instance is shared and can be mutated from the UI.
    pub config: Arc<RwLock<<FT::LineType as LineType>::Config>>,
    /// Per-source file state — user-visible state specific to this file (e.g. time offsets, calibration).
    /// Each `FileState` type owns its interior synchronization; no outer `RwLock` is needed.
    /// Wrapped in `Arc` so that types like `DltFileState` can clone the Arc into the background
    /// loader (e.g. to share the `boot_times` `DashMap`). For all other types the background
    /// loader ignores this Arc entirely.
    pub file_state: Arc<<FT::LineType as LineType>::FileState>,
    /// Bookmarks for this source, keyed by line index within this source
    bookmarks: RwLock<HashMap<usize, Bookmark>>,
    /// Path to the `.crab` session file (immutable after construction).
    crab_path: PathBuf,
    /// OS exclusive lock on the `.crab` session file.
    ///
    /// `None` — lock released because the file was written by a newer `LogCrab`;
    ///           all reads and writes are refused.
    /// `Some(mutex)` — lock held; mutex provides `&mut File` for writes.
    crab: Option<Mutex<File>>,
    version: AtomicU64,
    /// Flag to request cancellation of background loading/scoring operations
    cancel_requested: AtomicBool,
}

impl<FT: InputFileType> std::fmt::Debug for SourceData<FT> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceData")
            .field("source_id", &self.source_id)
            .field("file_path", &self.file_path)
            .field("version", &self.version.load(AtomicOrdering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl<FT: InputFileType> SourceData<FT>
where
    FT::LineType: Clone,
{
    /// Create a `SourceData` for a file source.
    ///
    /// Acquires an exclusive lock on the `.crab` session file to prevent multiple
    /// instances from clobbering each other's session state. Parsed session data
    /// (bookmarks, file state) is applied immediately; the saved filters and
    /// highlights are returned to the caller so they never need to be stored.
    ///
    /// If the lock is already held by another instance the file is opened in
    /// **read-only mode**: bookmarks and saved state are not loaded, and writes
    /// are silently skipped for the lifetime of this source.
    ///
    /// Returns `(Self, saved_filters, saved_highlights)`.
    pub fn new(
        file_path: PathBuf,
        config: Arc<RwLock<<FT::LineType as LineType>::Config>>,
        warnings: &crate::ui::ToastSender,
    ) -> (Self, Vec<SavedFilter>, Vec<SavedHighlight>) {
        assert!(
            file_path.file_name().is_some(),
            "file_path must have a filename component: {}",
            file_path.display()
        );

        let crab_path = Self::compute_crab_path(&file_path);
        let (lock_file, maybe_crab) = Self::acquire_crab_lock(&crab_path).map_or_else(
            || {
                tracing::warn!(
                    "Cannot lock {} — opening read-only (file already open in another instance)",
                    crab_path.display()
                );
                warnings.send(format!(
                    "'{}' is already open in another LogCrab instance — \
                    opened read-only (bookmarks and filters not loaded)",
                    file_path
                        .file_name()
                        .unwrap_or(file_path.as_os_str())
                        .to_string_lossy()
                ));
                (None, None)
            },
            |lock_file| Self::open_crab_file(lock_file, &crab_path, warnings),
        );

        // Consume the parsed CrabFile immediately — apply bookmarks/file_state
        // here and return filters/highlights to the caller so nothing lingers.
        let (filters, highlights, bookmarks_vec, file_state_arc) = match maybe_crab {
            Some(crab) => {
                tracing::info!(
                    "Loaded {} bookmarks from {}",
                    crab.bookmarks.len(),
                    crab_path.display()
                );
                (
                    crab.filters,
                    crab.highlights,
                    crab.bookmarks,
                    Arc::new(crab.file_state),
                )
            }
            None => (vec![], vec![], vec![], Arc::new(Default::default())),
        };

        let sd = Self {
            source_id: SOURCE_ID_COUNTER.fetch_add(1, AtomicOrdering::Relaxed),
            file_path,
            lines: RwLock::new(Vec::new()),
            by_timestamp: RwLock::new(Vec::new()),
            config,
            file_state: file_state_arc,
            bookmarks: RwLock::new(
                bookmarks_vec
                    .into_iter()
                    .map(|b| (b.line_index, b))
                    .collect(),
            ),
            crab_path,
            crab: lock_file.map(Mutex::new),
            version: AtomicU64::new(1),
            cancel_requested: AtomicBool::new(false),
        };
        (sd, filters, highlights)
    }

    /// Parse the `.crab` file immediately after locking it.
    ///
    /// Returns `(Some(file), Some(data))` on success.
    /// Returns `(Some(file), None)` when the file is empty or unparseable.
    /// Returns `(None, None)` on `VersionTooNew`, releasing the OS lock.
    fn open_crab_file(
        file: File,
        crab_path: &Path,
        warnings: &crate::ui::ToastSender,
    ) -> (Option<File>, Option<CrabFile<FT>>) {
        let mut file = file;
        match CrabFile::<FT>::load_from_file(&mut file) {
            Ok(data) => (Some(file), Some(data)),
            Err(SessionError::Io(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                (Some(file), None) // Empty .crab file (just created)
            }
            Err(SessionError::Parse(_)) => {
                (Some(file), None) // Unparseable, fine for a new session
            }
            Err(SessionError::VersionTooNew { found, supported }) => {
                let msg = format!(
                    ".crab file {} was written by a newer LogCrab (v{found}, app supports up to v{supported}); \
                     bookmarks and file state not loaded",
                    crab_path.display()
                );
                tracing::warn!("{msg}");
                warnings.send(msg);
                // Drop `file` here to release the OS lock.
                (None, None)
            }
            Err(SessionError::StateVersionTooNew { slug, found, supported }) => {
                let msg = format!(
                    ".crab file {}: {slug} state was written by a newer LogCrab \
                     (state v{found}, app supports up to v{supported}); \
                     calibration and file state not loaded",
                    crab_path.display()
                );
                tracing::warn!("{msg}");
                warnings.send(msg);
                // Drop `file` here to release the OS lock — keeping it would
                // allow a future save to overwrite the newer state with our
                // older format, silently destroying the user's calibration.
                (None, None)
            }
            Err(e) => {
                tracing::warn!("Failed to load .crab file {}: {e}", crab_path.display());
                (Some(file), None)
            }
        }
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
                tracing::error!("Cannot open .crab file {}: {e}", crab_path.display());
                return None;
            }
        };

        // Try to acquire exclusive lock
        match file.try_lock_exclusive() {
            Ok(()) => {
                tracing::info!(
                    "Successfully acquired exclusive lock on {}",
                    crab_path.display()
                );
                Some(file)
            }
            Err(e) => {
                tracing::error!(
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

    /// Check if cancellation has been requested
    pub fn is_cancelled(&self) -> bool {
        self.cancel_requested.load(AtomicOrdering::SeqCst)
    }

    // ========================================================================
    // Bookmark Management
    // ========================================================================

    /// Add or update a bookmark for a line in this source
    pub(crate) fn set_bookmark(&self, line_index: usize, name: String) {
        profiling::scope!("SourceData::bookmarks::write");
        let bookmark = Bookmark { line_index, name };
        self.bookmarks
            .write()
            .expect("bookmarks lock poisoned")
            .insert(line_index, bookmark);
    }

    /// Remove a bookmark from this source
    pub(crate) fn remove_bookmark(&self, line_index: usize) -> Option<Bookmark> {
        profiling::scope!("SourceData::bookmarks::write");
        self.bookmarks
            .write()
            .expect("bookmarks lock poisoned")
            .remove(&line_index)
    }

    /// Check if a line has a bookmark
    pub(crate) fn has_bookmark(&self, line_index: usize) -> bool {
        profiling::scope!("SourceData::bookmarks::read");
        self.bookmarks
            .read()
            .expect("bookmarks lock poisoned")
            .contains_key(&line_index)
    }

    /// Get a bookmark by line index
    pub(crate) fn get_bookmark(&self, line_index: usize) -> Option<Bookmark> {
        profiling::scope!("SourceData::bookmarks::read");
        self.bookmarks
            .read()
            .expect("bookmarks lock poisoned")
            .get(&line_index)
            .cloned()
    }

    /// Get all bookmarks for this source
    pub(crate) fn get_bookmarks(&self) -> Vec<Bookmark> {
        profiling::scope!("SourceData::bookmarks::read");
        self.bookmarks
            .read()
            .expect("bookmarks lock poisoned")
            .values()
            .cloned()
            .collect()
    }

    /// Save bookmarks to this source's .crab file
    /// Note: filters and highlights are passed in since they're shared across sources
    pub fn save_crab_file(&self, filters: &[SavedFilter], highlights: &[SavedHighlight]) {
        let Some(mutex) = &self.crab else {
            tracing::warn!(
                "Skipping save to {} — .crab file is from a newer version of LogCrab",
                self.crab_path.display()
            );
            return;
        };
        let mut file = mutex.lock().expect("crab mutex poisoned");
        let crab_data = CrabFile::<FT> {
            version: CRAB_FILE_VERSION,
            bookmarks: self.get_bookmarks(),
            filters: filters.to_vec(),
            highlights: highlights.to_vec(),
            file_state: (*self.file_state).clone(),
        };
        match crab_data.save_to_file(&mut file) {
            Ok(()) => tracing::debug!(
                "Saved .crab file {} with {} bookmarks",
                self.crab_path.display(),
                crab_data.bookmarks.len()
            ),
            Err(e) => tracing::error!(
                "Failed to save .crab file {}: {e}",
                self.crab_path.display()
            ),
        }
    }

    // ========================================================================
    // Time Synchronization
    // ========================================================================

    /// Re-sort `by_timestamp` using the current config and file-state, then bump the version.
    ///
    /// Call this after the shared `config` arc has been mutated externally (e.g.
    /// `DltTimestampSource` was changed), so that timestamp ordering and dependent
    /// filter caches are invalidated.
    pub fn rebuild_time_index(&self) {
        let lines = self.lines.read().expect("lines lock poisoned");
        let config = self.config.read().expect("config lock poisoned");
        let file_state = &*self.file_state;
        let mut indices: Vec<usize> = (0..lines.len()).collect();
        indices.par_sort_by_key(|&idx| lines[idx].timestamp(&config, file_state));
        drop(lines);
        drop(config);
        *self
            .by_timestamp
            .write()
            .expect("by_timestamp lock poisoned") = indices;
        self.bump_version();
    }

    /// Drive any open calibration window for this source (one per frame).
    ///
    /// The `FileState` impl writes the new offset into itself on confirm;
    /// this method bumps the source version so dependent views invalidate.
    /// Returns `true` when an offset was applied.
    pub fn render_file_state(&self, ui: &egui::Ui) -> bool {
        let changed = self.file_state.egui_render_file_state(ui);
        if changed {
            self.rebuild_time_index();
        }
        changed
    }

    // ========================================================================
    // Line Management
    // ========================================================================

    /// Append lines to this source
    ///
    /// Lines are stored in file order (append-only). Only the timestamp index
    /// needs to be rebuilt. Heavy work is done outside the write lock.
    pub fn append_lines(&self, lines: Vec<FT::LineType>) {
        if lines.is_empty() {
            return;
        }

        profiling::scope!("SourceData::append_lines");

        let config = self.config.read().expect("config lock poisoned");
        let file_state = &*self.file_state;

        // Append lines and capture the range of new indices atomically
        let new_start_idx = {
            profiling::scope!("SourceData::lines::write");
            let mut lines_guard = self.lines.write().expect("lines lock poisoned");
            let start_idx = lines_guard.len();
            tracing::debug!(
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
            indices.par_sort_by_key(|&idx| lines_read[idx].timestamp(&*config, file_state));
            (lines_read, indices)
        };

        // Merge into by_timestamp atomically - hold write lock during merge
        {
            profiling::scope!("SourceData::by_timestamp::write");
            let mut by_ts_guard = self
                .by_timestamp
                .write()
                .expect("by_timestamp lock poisoned");

            profiling::scope!("merge_timestamp_indices");
            let existing_len = by_ts_guard.len();
            let mut merged = Vec::with_capacity(existing_len + new_by_ts.len());
            let mut i_exist = 0;
            let mut j_new = 0;

            while i_exist < existing_len && j_new < new_by_ts.len() {
                let ts_exist = lines_guard[by_ts_guard[i_exist]].timestamp(&*config, file_state);
                let ts_new = lines_guard[new_by_ts[j_new]].timestamp(&*config, file_state);
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
        drop(config);
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

    /// Look up a single line and return it as the display [`LogLine`] DTO.
    ///
    /// Acquires `lines`, `config`, and `file_state` locks exactly once so the
    /// timestamp, message, and all other fields are computed under the same
    /// read epoch.  Returns `None` when `line_index` is out of range.
    #[allow(clippy::significant_drop_tightening)]
    pub fn get_as_log_line(&self, line_index: usize) -> Option<LogLine> {
        profiling::scope!("SourceData::get_as_log_line");
        let lines = self.lines.read().expect("lines lock poisoned");
        let config = self.config.read().expect("config lock poisoned");
        let file_state = &*self.file_state;
        let line = lines.get(line_index)?;
        Some(LogLine {
            timestamp: line.timestamp(&*config, file_state),
            message: line.display_message(&*config, file_state),
            raw: line.raw(),
            line_number: line.line_number(),
            anomaly_score: 0.0, // Scores are stored at LogStore level, populated by get_by_id
        })
    }

    /// Filter lines by their *display message* and *raw* string, in timestamp order.
    ///
    /// Unlike `filter_sorted_mapped`, the predicate receives the display message produced
    /// by `display_message(config, file_state)` — which includes any active overlays such
    /// as SOME/IP SD decoded entries — and the raw string.  All config and file-state locks
    /// are acquired once for the whole scan.
    pub fn filter_sorted_by_search<F>(&self, predicate: &F) -> Vec<usize>
    where
        F: Fn(&str, &str) -> bool + Sync,
    {
        profiling::scope!("SourceData::filter_sorted_by_search");
        let lines = self.lines.read().expect("lines lock poisoned");
        let config = self.config.read().expect("config lock poisoned");
        let file_state = &*self.file_state;
        self.by_timestamp
            .read()
            .expect("by_timestamp lock poisoned")
            .par_iter()
            .filter_map(|&idx| {
                let line = &lines[idx];
                let display_msg = line.display_message(&*config, file_state);
                let raw = line.raw();
                predicate(&display_msg, &raw).then_some(idx)
            })
            .collect()
    }

    /// Render format-specific context menu items for the line at `line_index`.
    ///
    /// Must be called inside an egui `context_menu` closure.
    pub fn render_line_context_menu(&self, line_index: usize, ui: &mut egui::Ui) {
        let lines = self.lines.read().expect("lines lock poisoned");
        let config = self.config.read().expect("config lock poisoned");
        let file_state = &*self.file_state;
        if let Some(line) = lines.get(line_index) {
            line.egui_render_context_menu(ui, &*config, file_state);
        }
    }
}

crate::register_filetypes! {
    binary {
        dlt:     Dlt:     DltFileType:     DltLogLine,
        btsnoop: Btsnoop: BtsnoopFileType: BtsnoopLogLine,
        pcap:    Pcap:    PcapFileType:    PcapLogLine,
    }
    text {
        bugreport: Bugreport: BugreportFileType: BugreportLogLine,
        logcat:    Logcat:   LogcatFileType:    LogcatLogLine,
        dmesg:     Dmesg:    DmesgFileType:     DmesgLogLine,
        otel:      Otel:     OtelFileType:      OtelLogLine,
        generic:   Generic:  GenericFileType:   GenericLogLine,
    }
}

/// Display-ready log line, produced by [`LogStore::get_by_id`].
///
/// All fields are pre-computed under the source locks so callers never need a
/// second lookup or lock acquisition.  Format-specific dispatch (context menus,
/// calibration UI) goes through [`StoreID`] + [`LogStore`] methods.
#[derive(Debug, Clone)]
pub struct LogLine {
    /// Fully-adjusted timestamp: config-selected clock + calibration offset.
    pub timestamp: chrono::DateTime<chrono::Local>,
    /// Rendered message text.
    pub message: String,
    /// Original raw source text.
    pub raw: String,
    /// 1-based line number within the source file.
    pub line_number: usize,
    /// Anomaly score in [0, 100].
    pub anomaly_score: f64,
}

impl LogLine {
    /// Compute the normalised template key for anomaly detection.
    /// This is computed on-demand rather than stored to avoid expensive
    /// regex normalization when not needed (e.g., histogram rendering).
    pub fn template_key(&self) -> String {
        crate::parser::normalize_message(&self.message)
    }
}

/// Central storage for log lines from one or more sources
///
/// Thread-safe: can be shared across threads with Arc<LogStore>
/// Uses `IndexMap` for O(1) source lookup by ID while maintaining insertion order.
pub struct LogStore {
    /// Sources indexed by their stable `source_id` for O(1) lookup.
    /// `IndexMap` maintains insertion order for consistent UI display.
    sources: RwLock<IndexMap<u64, DataSourceVariant>>,
    /// Version counter that increments when sources are added or removed.
    /// This ensures cache invalidation even when line counts happen to sum to the same value.
    sources_version: AtomicU64,
    /// Anomaly scores keyed by source_id. Each source has its own ScoreStore.
    /// Stored at LogStore level (not SourceData) because scores are analysis metadata.
    scores: DashMap<u64, ScoreStore>,
}

impl std::fmt::Debug for LogStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogStore")
            .field(
                "sources_count",
                &self.sources.read().map(|s| s.len()).unwrap_or(0),
            )
            .field("sources_version", &self.sources_version)
            .field("scores_count", &self.scores.len())
            .finish()
    }
}

impl Clone for LogStore {
    fn clone(&self) -> Self {
        profiling::scope!("LogStore::sources::read");
        Self {
            sources: RwLock::new(self.sources.read().expect("sources lock poisoned").clone()),
            sources_version: AtomicU64::new(self.sources_version.load(AtomicOrdering::SeqCst)),
            scores: self.scores.clone(),
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
        match (
            store.adjusted_timestamp(self),
            store.adjusted_timestamp(other),
        ) {
            (Some(self_time), Some(other_time)) => {
                // Both lines exist: compare by calibrated timestamp, then structurally for stability
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
            scores: DashMap::new(),
        })
    }

    /// Rebuild the timestamp-sorted index on every source in the store.
    ///
    /// Writes the relevant `file_config` field into each source's config arc before
    /// rebuilding, so that line ordering reflects the latest settings. Call this
    /// after mutating any field of `GlobalFileConfig` (e.g. via `render`).
    pub fn rebuild_all_time_indices(&self, file_config: &GlobalFileConfig) {
        profiling::scope!("LogStore::rebuild_all_time_indices");
        let sources = self.sources.read().expect("sources lock poisoned");
        for source in sources.values() {
            source.apply_file_config_and_rebuild(file_config);
        }
    }

    /// Insert a pre-constructed [`DataSourceVariant`] directly into the store.
    ///
    /// Used when the caller already has a `DataSourceVariant` (e.g. from
    /// [`crate::core::LogFileLoader::load_file`]) and does not need the concrete
    /// typed `Arc` afterwards.
    pub fn add_source(self: &Arc<Self>, variant: DataSourceVariant) {
        profiling::scope!("LogStore::sources::write");
        let id = variant.source_id();
        self.sources
            .write()
            .expect("sources lock poisoned")
            .insert(id, variant);
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
                if let Ok(source_canonical) = source.file_path().canonicalize() {
                    return &source_canonical == canonical;
                }
            }
            source.file_path() == path
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
            .map(DataSourceVariant::version)
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
            .map(DataSourceVariant::len)
            .sum()
    }

    /// Get the source name (filename) for a given `StoreID`
    pub fn get_source_name(&self, id: &StoreID) -> Option<String> {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");
        sources.get(&id.source_id).map(|source| {
            source
                .file_path()
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
                    .file_path()
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
        // Also remove scores for this source
        self.scores.remove(&source_id);
        self.sources_version.fetch_add(1, AtomicOrdering::SeqCst);
        tracing::info!("Removed source: {}", path.display());
        Some(path)
    }

    // ========================================================================
    // Anomaly Score Management
    // ========================================================================

    /// Set anomaly scores for a source. Scores are indexed by line position.
    ///
    /// This is lock-free for readers — scores are swapped atomically.
    pub fn set_scores(&self, source_id: u64, scores: &[f64]) {
        profiling::scope!("LogStore::set_scores");
        self.scores
            .entry(source_id)
            .or_insert_with(ScoreStore::new)
            .set_all(scores);
        // Bump version so UI knows to refresh
        self.sources_version.fetch_add(1, AtomicOrdering::SeqCst);
    }

    /// Get the anomaly score for a specific line. Returns 0.0 if not found.
    pub fn get_score(&self, source_id: u64, line_index: usize) -> f64 {
        self.scores
            .get(&source_id)
            .map_or(0.0, |store| store.get(line_index))
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

    /// Drive all open calibration windows across every source (one per frame).
    ///
    /// Returns `true` if any source applied a new offset (caller should set `modified = true`).
    pub fn render_file_states(&self, ui: &egui::Ui) -> bool {
        profiling::scope!("LogStore::render_file_states");
        let sources = self.sources.read().expect("sources lock poisoned");
        sources
            .values()
            .fold(false, |acc, s| s.render_file_state(ui) || acc)
    }

    /// Render type-specific context menu items for the line at `id`.
    ///
    /// Returns `true` if the source was found. Must be called inside an egui
    /// `context_menu` closure.
    #[allow(clippy::significant_drop_tightening)]
    pub fn render_typed_context_menu_items(&self, id: &StoreID, ui: &mut egui::Ui) -> bool {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");
        let Some(variant) = sources.get(&id.source_id) else {
            return false;
        };
        variant.render_context_menu(id.line_index, ui);
        true
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
        F: Fn(&str, &str) -> bool + Sync,
    {
        profiling::scope!("LogStore::get_matching_ids");
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");

        // Parallel filter each source, collect results
        let per_source: Vec<Vec<StoreID>> = {
            profiling::scope!("parallel_filter_sources");
            sources
                .par_values()
                .map(|source| {
                    let source_id = source.source_id();
                    source
                        .filter_sorted_by_search(&predicate)
                        .into_iter()
                        .map(|line_index| StoreID {
                            source_id,
                            line_index,
                        })
                        .collect()
                })
                .collect()
        };

        // Release sources lock before merge
        drop(sources);

        // K-way merge of sorted sources by timestamp
        self.merge_sorted_sources(per_source)
    }

    /// K-way merge of pre-sorted `StoreID` vectors by timestamp
    fn merge_sorted_sources(&self, sources: Vec<Vec<StoreID>>) -> Vec<StoreID> {
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

        // Initialize heap with first element from each non-empty source
        for (src_idx, iter) in iters.iter_mut().enumerate() {
            if let Some((id, adjusted_time)) =
                iter.find_map(|id| self.adjusted_timestamp(&id).map(|time| (id, time)))
            {
                heap.push(Reverse((adjusted_time, src_idx, id)));
            }
        }

        // Merge
        while let Some(Reverse((_, src_idx, id))) = heap.pop() {
            result.push(id);

            // Push the next element from this source onto the heap
            if let Some((next_id, adjusted_time)) =
                iters[src_idx].find_map(|id| self.adjusted_timestamp(&id).map(|time| (id, time)))
            {
                heap.push(Reverse((adjusted_time, src_idx, next_id)));
            }
        }

        result
    }

    /// Get the fully-calibrated timestamp for the line identified by `id`.
    ///
    /// Delegates to [`DataSourceVariant::adjusted_timestamp`] which locks `config`
    /// and `file_state` and calls `LineType::timestamp()`.  Both config-driven
    /// source selection (e.g. DLT ECU/session/storage clock) and the per-source
    /// calibration offset are applied.  Returns `None` if the source or line is
    /// not found.
    pub fn adjusted_timestamp(&self, id: &StoreID) -> Option<chrono::DateTime<Local>> {
        profiling::scope!("LogStore::adjusted_timestamp");
        let sources = self.sources.read().expect("sources lock poisoned");
        sources
            .get(&id.source_id)
            .and_then(|s| s.adjusted_timestamp(id.line_index))
    }

    /// Find the position of the line closest to a target timestamp in a sorted list.
    /// Returns the index position within `filtered_indices`.
    ///
    /// Assumes `filtered_indices` are sorted by timestamp.
    pub fn find_closest_line_position_by_time(
        &self,
        filtered_indices: &[StoreID],
        target_time: chrono::DateTime<Local>,
    ) -> Option<usize> {
        profiling::scope!("LogStore::find_closest_line_position_by_time");
        if filtered_indices.is_empty() {
            return None;
        }

        // Binary search to find insertion point
        let idx = filtered_indices.partition_point(|line_idx| {
            self.adjusted_timestamp(line_idx)
                .is_some_and(|ts| ts < target_time)
        });

        // Compare neighbors around the insertion point to find the closest
        match idx {
            0 => Some(0),
            i if i >= filtered_indices.len() => Some(filtered_indices.len() - 1),
            i => {
                let before_ts = self.adjusted_timestamp(&filtered_indices[i - 1])?;
                let after_ts = self.adjusted_timestamp(&filtered_indices[i])?;

                let dist_before = (target_time - before_ts).abs();
                let dist_after = (after_ts - target_time).abs();

                if dist_before <= dist_after {
                    Some(i - 1)
                } else {
                    Some(i)
                }
            }
        }
    }

    pub fn get_by_id(&self, id: &StoreID) -> Option<LogLine> {
        profiling::scope!("LogStore::sources::read");
        let sources = self.sources.read().expect("sources lock poisoned");
        let mut line = sources.get(&id.source_id)?.get_log_line(id.line_index)?;
        // Populate anomaly score from store-level score storage
        line.anomaly_score = self.get_score(id.source_id, id.line_index);
        Some(line)
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
