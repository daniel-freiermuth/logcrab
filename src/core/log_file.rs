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

use crate::anomaly::{create_default_scorer, normalize_scores};
use crate::core::log_store::{DataSourceVariant, GlobalFileConfig, SourceData};
use crate::core::{ChunkedLoader, SavedFilter, SavedHighlight};
use crate::filetype::{InputFileType, LineType};
use crate::ui::ProgressToastHandle;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::thread;

/// Initial chunk size for incremental loading — small for fast initial feedback.
const INITIAL_CHUNK_SIZE: usize = 1 << 13; // 8,192 items
/// Maximum chunk size — prevents excessive merge overhead.
const MAX_CHUNK_SIZE: usize = 1 << 18; // 262,144 items
/// Number of chunks between each chunk-size doubling.
const CHUNKS_BEFORE_GROWTH: usize = 3;

/// Handles asynchronous loading and processing of log files
pub struct LogFileLoader;

impl LogFileLoader {
    /// Load a file of any supported format into a typed [`DataSourceVariant`].
    ///
    /// Format detection is synchronous and fast (reads ≤16 bytes for binary,
    /// ≤100 KB for text). A background thread is then spawned to drive
    /// [`ChunkedLoader`], so this call returns before the file is fully loaded.
    ///
    /// `file_config` is the session-wide [`GlobalFileConfig`]; each typed source
    /// receives `Arc::clone` of its type's config arc so config mutations propagate live.
    ///
    /// Returns `None` only if the file cannot be opened for format detection.
    pub fn load_file(
        path: &Path,
        toast: &ProgressToastHandle,
        warnings: &crate::ui::ToastSender,
        file_config: &GlobalFileConfig,
    ) -> Option<(DataSourceVariant, Vec<SavedFilter>, Vec<SavedHighlight>)> {
        crate::core::log_store::try_open_binary(path, toast, warnings, file_config).or_else(|| {
            crate::core::log_store::open_text_source(path, toast, warnings, file_config)
        })
    }

    /// Create a typed [`SourceData<T>`], spawn a background loading thread, and
    /// return the source before loading completes.
    ///
    /// `open_fn` is called from the background thread and must return a
    /// ready-to-read [`InputFileType`] for the given path.
    pub(crate) fn load_typed<FT>(
        path: PathBuf,
        toast: &ProgressToastHandle,
        warnings: &crate::ui::ToastSender,
        config: Arc<RwLock<<FT::LineType as LineType>::Config>>,
        open_fn: impl FnOnce(&Path, Arc<<FT::LineType as LineType>::FileState>) -> anyhow::Result<FT>
            + Send
            + 'static,
    ) -> (Arc<SourceData<FT>>, Vec<SavedFilter>, Vec<SavedHighlight>)
    where
        FT: InputFileType + Send + 'static,
        FT::LineType: Clone,
    {
        let (sd, filters, highlights) = SourceData::new(path.clone(), config, warnings);
        let data_source = Arc::new(sd);
        let source_clone = Arc::clone(&data_source);
        let toast_clone = toast.clone();
        thread::spawn(move || {
            Self::background_load(path.as_path(), &source_clone, &toast_clone, open_fn);
        });
        (data_source, filters, highlights)
    }

    /// Open the file via `open_fn`, drive [`ChunkedLoader`], score, and dismiss the toast.
    fn background_load<FT>(
        path: &Path,
        data_source: &Arc<SourceData<FT>>,
        toast: &ProgressToastHandle,
        open_fn: impl FnOnce(&Path, Arc<<FT::LineType as LineType>::FileState>) -> anyhow::Result<FT>,
    ) where
        FT: InputFileType,
        FT::LineType: Clone,
    {
        let start_time = std::time::Instant::now();
        let file_size = std::fs::metadata(path).map_or(0, |m| m.len());
        let file_name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();

        tracing::debug!("background_load: opening {}", path.display());
        let mut file_type = match open_fn(path, Arc::clone(&data_source.file_state)) {
            Ok(ft) => ft,
            Err(e) => {
                tracing::error!("Failed to open {}: {e}", path.display());
                toast.set_error(format!("Failed to open file: {e}"));
                toast.dismiss();
                return;
            }
        };

        let loader = ChunkedLoader {
            initial_chunk_size: INITIAL_CHUNK_SIZE,
            max_chunk_size: MAX_CHUNK_SIZE,
            chunks_before_growth: CHUNKS_BEFORE_GROWTH,
        };

        let load_complete = loader.run(&mut file_type, data_source, &file_name, file_size, toast);

        if load_complete && !data_source.is_empty() {
            Self::score_lines(data_source, path, toast, start_time);
        } else if data_source.is_empty() {
            toast.set_error("No log lines found in file");
        }
        toast.dismiss();
    }

    /// Score all lines in `data_source` and persist the results.
    ///
    /// Iterates directly over the typed source so no intermediate `Vec<LogLine>` is
    /// allocated.  Each [`LogLine`] DTO is built under a single lock acquisition
    /// via [`SourceData::get_as_log_line`].
    fn score_lines<FT>(
        data_source: &Arc<SourceData<FT>>,
        path: &Path,
        toast: &ProgressToastHandle,
        start_time: std::time::Instant,
    ) where
        FT: InputFileType,
        FT::LineType: Clone,
    {
        static N_SKIP_INITIAL: usize = 10;

        toast.set_title("Calculating Anomaly Scores");
        toast.update(0.0, "Starting...");

        let score_start = std::time::Instant::now();
        let total_lines = data_source.len();
        tracing::debug!("Starting background anomaly scoring for {total_lines} lines");

        let mut scorer = create_default_scorer();
        let mut raw_scores = Vec::new();

        profiling::scope!("score_lines");

        for idx in 0..total_lines {
            if idx % 1000 == 0 {
                if data_source.is_cancelled() {
                    tracing::info!("Anomaly scoring cancelled for {}", path.display());
                    toast.set_error("Scoring cancelled".to_string());
                    toast.dismiss();
                    return;
                }
                let progress = idx as f32 / total_lines as f32;
                toast.update(progress, format!("Scoring... ({idx}/{total_lines})"));
            }

            let Some(log_line) = data_source.get_as_log_line(idx) else {
                tracing::warn!("Skipping scoring for line {idx} due to missing entry");
                raw_scores.push(0.0);
                continue;
            };

            if idx > N_SKIP_INITIAL - 1 {
                raw_scores.push(scorer.score(&log_line));
            }
            scorer.update(&log_line);
        }

        toast.update(0.95, "Normalizing scores...");

        profiling::scope!("normalize_scores");

        let normalized_scores = vec![0.0; N_SKIP_INITIAL]
            .into_iter()
            .chain(normalize_scores(&raw_scores))
            .collect::<Vec<f64>>();

        toast.update(1.0, "Done!");

        if !raw_scores.is_empty() {
            let min_raw = raw_scores.iter().copied().fold(f64::INFINITY, f64::min);
            let max_raw = raw_scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let avg_raw: f64 = raw_scores.iter().sum::<f64>() / raw_scores.len() as f64;
            tracing::info!(
                "Score statistics - Raw: min={:.3}, max={:.3}, avg={:.3}, total_lines={}",
                min_raw,
                max_raw,
                avg_raw,
                raw_scores.len()
            );
        }

        data_source.set_scores(&normalized_scores);

        let score_duration = score_start.elapsed();
        tracing::info!(
            "Anomaly scoring took {score_duration:?} for {}",
            path.display()
        );
        tracing::info!("Total processing time: {:?}", start_time.elapsed());
    }
}
