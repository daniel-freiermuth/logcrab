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

use crate::anomaly::{create_default_scorer, normalize_scores};
use crate::config::DltTimestampSource;
use crate::core::log_store::SourceData;
use crate::parser::{detect_format, dlt, generic, logcat, LogFormat};
use crate::ui::ProgressToastHandle;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

/// Progress callback type for DLT parsing
pub type ProgressCallback = Box<dyn Fn(f32, &str) + Send>;

/// Handles asynchronous loading and processing of log files
pub struct LogFileLoader;

impl LogFileLoader {
    /// Start loading a file in the background.
    ///
    /// The toast handle will be updated with progress and dismissed when complete.
    /// Returns the `SourceData` that will be populated with log lines.
    pub fn load_async(
        path: PathBuf,
        toast: ProgressToastHandle,
        dlt_timestamp_source: DltTimestampSource,
    ) -> Arc<SourceData> {
        let data_source = Arc::new(SourceData::new(Some(path.clone())));
        let source_clone = data_source.clone();

        thread::spawn(move || {
            Self::process_file_background(path, source_clone, toast, dlt_timestamp_source);
        });

        data_source
    }

    fn read_dlt_file(
        path: &Path,
        data_source: &Arc<SourceData>,
        toast: &ProgressToastHandle,
        dlt_timestamp_source: DltTimestampSource,
    ) -> bool {
        log::info!("Detected DLT binary file, using dlt-core parser");
        toast.update(
            0.0,
            format!("Parsing DLT binary file {}...", path.display()),
        );

        // Create progress callback that updates the toast
        let toast_clone = toast.clone();
        let progress_callback: ProgressCallback = Box::new(move |progress, message| {
            toast_clone.update(progress, message);
        });

        match dlt::parse_dlt_file_with_progress(
            path,
            data_source,
            &progress_callback,
            dlt_timestamp_source,
        ) {
            Ok(total_lines) => {
                log::info!("Successfully parsed {total_lines} DLT messages");
                true
            }
            Err(e) => {
                log::error!("Failed to parse DLT file: {e}");
                toast.set_error(format!("Failed to parse DLT file: {e}"));
                false
            }
        }
    }

    #[allow(clippy::needless_pass_by_value)] // Values are moved into thread::spawn closure
    fn process_file_background(
        path: PathBuf,
        data_source: Arc<SourceData>,
        toast: ProgressToastHandle,
        dlt_timestamp_source: DltTimestampSource,
    ) {
        let start_time = std::time::Instant::now();
        log::debug!(
            "Starting background file processing for: {}",
            path.display()
        );

        // Get file size for progress tracking
        let metadata = std::fs::metadata(&path);
        if let Err(e) = metadata {
            log::error!("Cannot read file metadata: {e}");
            toast.set_error(format!("Cannot read file: {e}"));
            return;
        }
        let file_size = metadata.unwrap().len();
        log::info!("File size: {file_size} bytes");

        // Check if this is a DLT binary file by extension
        let is_dlt_file = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dlt"));

        // Load file based on detected format
        let source_added = if is_dlt_file {
            Self::read_dlt_file(&path, &data_source, &toast, dlt_timestamp_source)
        } else {
            // Read file content first to detect format
            let Some(content) = Self::read_file_content(&path, &toast) else {
                return;
            };

            // Detect format and dispatch to appropriate parser
            let format = detect_format(&content);
            log::info!("Detected format: {format:?}");

            match format {
                LogFormat::Bugreport { year } => Self::read_bugreport_file(
                    &path,
                    &content,
                    year,
                    &data_source,
                    &toast,
                    start_time,
                    file_size,
                ),
                LogFormat::Logcat { year } => Self::read_logcat_file(
                    &path,
                    &content,
                    year,
                    &data_source,
                    &toast,
                    start_time,
                    file_size,
                ),
                LogFormat::Generic => Self::read_generic_file(
                    &path,
                    &content,
                    &data_source,
                    &toast,
                    start_time,
                    file_size,
                ),
            }
        };

        if source_added && !data_source.is_empty() {
            Self::score_lines(&data_source, &path, &toast, start_time);
        } else if data_source.is_empty() {
            toast.set_error("No log lines found in file");
        }

        // Toast auto-dismisses when dropped (handle goes out of scope)
        toast.dismiss();
    }

    /// Read file content and handle errors
    fn read_file_content(path: &Path, toast: &ProgressToastHandle) -> Option<String> {
        let file = File::open(path);
        if let Err(e) = file {
            log::error!("Cannot open file: {e}");
            toast.set_error(format!("Cannot open file: {e}"));
            return None;
        }

        let read_start = std::time::Instant::now();
        let mut file = file.unwrap();
        let mut buffer = Vec::new();
        if let Err(e) = file.read_to_end(&mut buffer) {
            log::error!("Cannot read file content: {e}");
            toast.set_error(format!("Cannot read file: {e}"));
            return None;
        }

        let read_duration = read_start.elapsed();
        log::info!(
            "File I/O took {:?} to read {} bytes",
            read_duration,
            buffer.len()
        );

        // Convert to UTF-8 with lossy conversion
        let utf8_start = std::time::Instant::now();
        let mut content = String::from_utf8_lossy(&buffer).to_string();
        let content_len = content.len();

        // Check for and remove null bytes
        let null_count = content.bytes().filter(|&b| b == 0).count();
        if null_count > 0 {
            log::warn!("File contains {null_count} null bytes which will be removed");
            content = content.replace('\0', "");
        }

        log::info!(
            "UTF-8 conversion took {:?}, original bytes: {}, UTF-8 bytes: {}, null bytes: {}",
            utf8_start.elapsed(),
            buffer.len(),
            content_len,
            null_count
        );

        Some(content)
    }

    fn read_bugreport_file(
        path: &Path,
        content: &str,
        year: i32,
        source: &Arc<SourceData>,
        toast: &ProgressToastHandle,
        start_time: std::time::Instant,
        file_size: u64,
    ) -> bool {
        log::info!("Parsing as bugreport format with year {year}");
        Self::parse_text_file(
            path,
            content,
            source,
            toast,
            start_time,
            file_size,
            |raw, line_number| logcat::parse_logcat_with_year(raw, line_number, year),
        )
    }

    fn read_logcat_file(
        path: &Path,
        content: &str,
        year: i32,
        source: &Arc<SourceData>,
        toast: &ProgressToastHandle,
        start_time: std::time::Instant,
        file_size: u64,
    ) -> bool {
        log::info!("Parsing as logcat format with year {year}");
        Self::parse_text_file(
            path,
            content,
            source,
            toast,
            start_time,
            file_size,
            |raw, line_number| logcat::parse_logcat_with_year(raw, line_number, year),
        )
    }

    fn read_generic_file(
        path: &Path,
        content: &str,
        source: &Arc<SourceData>,
        toast: &ProgressToastHandle,
        start_time: std::time::Instant,
        file_size: u64,
    ) -> bool {
        log::info!("Parsing as generic format");
        Self::parse_text_file(
            path,
            content,
            source,
            toast,
            start_time,
            file_size,
            generic::parse_generic,
        )
    }

    /// Common text file parsing logic used by both logcat and generic parsers
    fn parse_text_file<F>(
        path: &Path,
        content: &str,
        source: &Arc<SourceData>,
        toast: &ProgressToastHandle,
        start_time: std::time::Instant,
        file_size: u64,
        parse_fn: F,
    ) -> bool
    where
        F: Fn(String, usize) -> Option<crate::parser::line::LogLine>,
    {
        const CHUNK_SIZE: usize = 10_000;

        let mut chunk_lines = Vec::new();
        let mut bytes_read: usize = 0;

        profiling::scope!("parse_lines");

        let parse_start = std::time::Instant::now();
        let mut file_line_number = 0;
        let total_lines_in_content = content.lines().count();
        log::info!("File contains {total_lines_in_content} lines");

        let file_name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();

        for line_buffer in content.lines() {
            file_line_number += 1;
            bytes_read += line_buffer.len() + 1;

            if file_line_number % 500 == 0 {
                let progress = (bytes_read as f32 / file_size as f32).min(1.0);
                toast.update(
                    progress,
                    format!(
                        "Loading {}... ({} lines)",
                        file_name,
                        source.len() + chunk_lines.len()
                    ),
                );
            }

            if line_buffer.trim().is_empty() {
                continue;
            }

            let Some(log_line) = parse_fn(line_buffer.to_string(), file_line_number) else {
                continue;
            };

            // Template key is now computed lazily in LogLineCore trait
            chunk_lines.push(log_line);

            if chunk_lines.len() >= CHUNK_SIZE {
                source.append_lines(std::mem::take(&mut chunk_lines));
                let progress = (bytes_read as f32 / file_size as f32).min(1.0);
                toast.update(
                    progress,
                    format!("Loading {}... ({} lines)", file_name, source.len()),
                );
                log::debug!("Sent partial load: {} lines", source.len());
            }
        }

        if !chunk_lines.is_empty() {
            source.append_lines(chunk_lines);
        }

        let parse_duration = parse_start.elapsed();
        log::info!(
            "Parsing took {:?} to process {} lines from {}",
            parse_duration,
            source.len(),
            path.display()
        );
        log::info!(
            "Line processing stats: file_line_number={}, lines_in_content={}, parsed_lines={}, skipped={}",
            file_line_number,
            total_lines_in_content,
            source.len(),
            file_line_number - source.len()
        );

        log::info!("Total time to display file: {:?}", start_time.elapsed());
        true
    }

    /// Score lines and update toast with progress
    fn score_lines(
        data_source: &Arc<SourceData>,
        path: &Path,
        toast: &ProgressToastHandle,
        start_time: std::time::Instant,
    ) {
        static N_SKIP_INITIAL: usize = 10;

        // Switch toast to scoring phase
        toast.set_title("Calculating Anomaly Scores");
        toast.update(0.0, "Starting...");

        let score_start = std::time::Instant::now();
        log::debug!(
            "Starting background anomaly scoring for {} lines",
            data_source.len()
        );

        let mut scorer = create_default_scorer();
        let mut raw_scores = Vec::new();

        profiling::scope!("score_lines");

        let total_lines = data_source.len();

        for (idx, log_line) in data_source.clone_lines().into_iter().enumerate() {
            if idx % 1000 == 0 {
                let progress = idx as f32 / total_lines as f32;
                toast.update(progress, format!("Scoring... ({idx}/{total_lines})"));
            }

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

        // Log score statistics
        if !raw_scores.is_empty() {
            let min_raw = raw_scores.iter().copied().fold(f64::INFINITY, f64::min);
            let max_raw = raw_scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let avg_raw: f64 = raw_scores.iter().sum::<f64>() / raw_scores.len() as f64;
            log::info!(
                "Score statistics - Raw: min={:.3}, max={:.3}, avg={:.3}, total_lines={}",
                min_raw,
                max_raw,
                avg_raw,
                raw_scores.len()
            );
        }

        // Store scores directly on the source
        data_source.set_scores(&normalized_scores);

        let score_duration = score_start.elapsed();
        log::info!(
            "Anomaly scoring took {score_duration:?} for {}",
            path.display()
        );
        log::info!("Total processing time: {:?}", start_time.elapsed());
    }
}
