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
use crate::config::DltTimestampSource;
use crate::core::log_store::SourceData;
use crate::parser::{btsnoop, detect_format, dlt, generic, logcat, pcap, LogFormat};
use crate::ui::ProgressToastHandle;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

/// Progress callback type for DLT parsing
pub type ProgressCallback = Box<dyn Fn(f32, &str) + Send>;

/// Initial chunk size for incremental text file loading
/// Start small for fast initial feedback, then grow to handle merge overhead
const TEXT_INITIAL_CHUNK_SIZE: usize = 1 << 13; // 8,192 lines
const TEXT_MAX_CHUNK_SIZE: usize = 1 << 18; // 262,144 lines
const TEXT_CHUNKS_BEFORE_GROWTH: usize = 3; // Double chunk size every 3 chunks

/// Packet capture format types
enum PacketFormat {
    Pcap,
    Btsnoop,
}

/// Handles asynchronous loading and processing of log files
pub struct LogFileLoader;

impl LogFileLoader {
    /// Start loading a file in the background.
    ///
    /// The toast handle will be updated with progress and dismissed when complete.
    /// Returns the `SourceData` that will be populated with log lines.
    ///
    /// If `crab_lock` is provided, uses it instead of acquiring a new lock.
    /// This is useful when reloading files to avoid lock release race conditions.
    ///
    /// Returns None if the .crab session file is already locked by another instance.
    pub fn load_async(
        path: PathBuf,
        toast: ProgressToastHandle,
        dlt_timestamp_source: DltTimestampSource,
        crab_lock: Option<(File, PathBuf)>,
    ) -> Option<Arc<SourceData>> {
        let data_source = if let Some((lock_file, lock_path)) = crab_lock {
            Arc::new(SourceData::new_with_lock(
                path.clone(),
                lock_file,
                lock_path,
            ))
        } else {
            Arc::new(SourceData::new(path.clone())?)
        };
        let source_clone = data_source.clone();

        thread::spawn(move || {
            Self::process_file_background(path, source_clone, toast, dlt_timestamp_source);
        });

        Some(data_source)
    }

    /// Detect packet capture format by reading magic bytes
    fn detect_packet_format(path: &Path) -> Option<PacketFormat> {
        use std::io::Read;

        let mut file = File::open(path).ok()?;
        let mut magic = [0u8; 8];
        file.read_exact(&mut magic).ok()?;

        // Check btsnoop magic: "btsnoop\0"
        if &magic == b"btsnoop\0" {
            return Some(PacketFormat::Btsnoop);
        }

        // Check PCAP magic bytes (various formats)
        match &magic[0..4] {
            // Legacy pcap (little-endian)
            [0xd4, 0xc3, 0xb2, 0xa1]
            // Legacy pcap (big-endian)
            | [0xa1, 0xb2, 0xc3, 0xd4]
            // Legacy pcap with nanosecond timestamps
            | [0x4d, 0x3c, 0xb2, 0xa1]
            | [0xa1, 0xb2, 0x3c, 0x4d]
            // pcapng (Section Header Block)
            | [0x0a, 0x0d, 0x0d, 0x0a] => Some(PacketFormat::Pcap),
            _ => None,
        }
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

    fn read_pcap_file(
        path: &Path,
        data_source: &Arc<SourceData>,
        toast: &ProgressToastHandle,
    ) -> bool {
        log::info!("Detected PCAP file, using pcap parser");
        toast.update(0.0, format!("Parsing PCAP file {}...", path.display()));

        // Create progress callback that updates the toast
        let toast_clone = toast.clone();
        let progress_callback: ProgressCallback = Box::new(move |progress, message| {
            toast_clone.update(progress, message);
        });

        match pcap::parse_pcap_file_with_progress(path, data_source, &progress_callback) {
            Ok(total_packets) => {
                log::info!("Successfully parsed {total_packets} packets");
                true
            }
            Err(e) => {
                log::error!("Failed to parse PCAP file: {e}");
                toast.set_error(format!("Failed to parse PCAP file: {e}"));
                false
            }
        }
    }

    fn read_btsnoop_file(
        path: &Path,
        data_source: &Arc<SourceData>,
        toast: &ProgressToastHandle,
    ) -> bool {
        log::info!("Detected BTSnoop file, using btsnoop parser");
        toast.update(0.0, format!("Parsing BTSnoop file {}...", path.display()));

        // Create progress callback that updates the toast
        let toast_clone = toast.clone();
        let progress_callback: ProgressCallback = Box::new(move |progress, message| {
            toast_clone.update(progress, message);
        });

        match btsnoop::parse_btsnoop_file_with_progress(path, data_source, &progress_callback) {
            Ok(total_packets) => {
                log::info!("Successfully parsed {total_packets} HCI packets");
                true
            }
            Err(e) => {
                log::error!("Failed to parse BTSnoop file: {e}");
                toast.set_error(format!("Failed to parse BTSnoop file: {e}"));
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
        let file_size = match std::fs::metadata(&path) {
            Ok(metadata) => metadata.len(),
            Err(e) => {
                log::error!("Cannot read file metadata: {e}");
                toast.set_error(format!("Cannot read file: {e}"));
                return;
            }
        };
        log::info!("File size: {file_size} bytes");

        // Check if this is a DLT binary file by extension
        let is_dlt_file = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dlt"));

        // Check if this is a PCAP or BTSnoop file by extension
        // Note: BTSnoop files sometimes have .pcap extension, so we need magic byte detection
        let has_pcap_ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                ext.eq_ignore_ascii_case("pcap") || ext.eq_ignore_ascii_case("pcapng")
            });

        // Check if this is a BTSnoop file by extension
        let has_btsnoop_ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("btsnoop"));

        // For files with .pcap extension, check magic bytes to distinguish btsnoop from pcap
        let (is_pcap_file, is_btsnoop_file) = if has_pcap_ext {
            // Check magic bytes to determine if it's btsnoop or pcap
            match Self::detect_packet_format(&path) {
                Some(PacketFormat::Btsnoop) => (false, true),
                Some(PacketFormat::Pcap) => (true, false),
                None => (true, false), // Default to PCAP if detection fails
            }
        } else {
            (false, has_btsnoop_ext)
        };

        // Load file based on detected format
        let source_added = if is_dlt_file {
            Self::read_dlt_file(&path, &data_source, &toast, dlt_timestamp_source)
        } else if is_btsnoop_file {
            Self::read_btsnoop_file(&path, &data_source, &toast)
        } else if is_pcap_file {
            Self::read_pcap_file(&path, &data_source, &toast)
        } else {
            // Read sample of file to detect format (avoids loading entire file)
            let Some(sample) = Self::read_file_sample(&path, &toast) else {
                return;
            };

            // Detect format and dispatch to appropriate parser
            let format = detect_format(&sample);
            log::info!("Detected format: {format:?}");

            match format {
                LogFormat::Bugreport { year } => Self::read_bugreport_file(
                    &path,
                    year,
                    &data_source,
                    &toast,
                    file_size,
                ),
                LogFormat::Logcat { year } => Self::read_logcat_file(
                    &path,
                    year,
                    &data_source,
                    &toast,
                    file_size,
                ),
                LogFormat::Generic => Self::read_generic_file(
                    &path,
                    &data_source,
                    &toast,
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

    /// Read first portion of file for format detection
    /// 
    /// This reads only the first ~100KB or 1000 lines (whichever comes first)
    /// to detect the file format without loading the entire file into memory.
    fn read_file_sample(path: &Path, toast: &ProgressToastHandle) -> Option<String> {
        const MAX_SAMPLE_BYTES: usize = 100 * 1024; // 100 KB
        const MAX_SAMPLE_LINES: usize = 1000;

        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                log::error!("Cannot open file: {e}");
                toast.set_error(format!("Cannot open file: {e}"));
                return None;
            }
        };

        let reader = BufReader::new(file);
        let mut sample = String::new();
        let mut lines_read = 0;

        for line_result in reader.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(e) => {
                    log::warn!("Error reading sample line: {e}");
                    break;
                }
            };

            sample.push_str(&line);
            sample.push('\n');
            lines_read += 1;

            if sample.len() >= MAX_SAMPLE_BYTES || lines_read >= MAX_SAMPLE_LINES {
                break;
            }
        }

        log::debug!("Read {lines_read} lines ({} bytes) for format detection", sample.len());
        Some(sample)
    }

    fn read_bugreport_file(
        path: &Path,
        year: i32,
        source: &Arc<SourceData>,
        toast: &ProgressToastHandle,
        file_size: u64,
    ) -> bool {
        log::info!("Parsing as bugreport format with year {year}");
        Self::parse_text_file_streaming(
            path,
            source,
            toast,
            file_size,
            |raw, line_number| logcat::parse_logcat_with_year(raw, line_number, year),
        )
    }

    fn read_logcat_file(
        path: &Path,
        year: i32,
        source: &Arc<SourceData>,
        toast: &ProgressToastHandle,
        file_size: u64,
    ) -> bool {
        log::info!("Parsing as logcat format with year {year}");
        Self::parse_text_file_streaming(
            path,
            source,
            toast,
            file_size,
            |raw, line_number| logcat::parse_logcat_with_year(raw, line_number, year),
        )
    }

    fn read_generic_file(
        path: &Path,
        source: &Arc<SourceData>,
        toast: &ProgressToastHandle,
        file_size: u64,
    ) -> bool {
        log::info!("Parsing as generic format");
        Self::parse_text_file_streaming(
            path,
            source,
            toast,
            file_size,
            generic::parse_generic,
        )
    }

    /// Streaming text file parser with incremental chunk loading
    /// 
    /// This function reads the file line-by-line from disk rather than loading
    /// the entire content into memory first. It uses exponentially growing chunk
    /// sizes to balance UI responsiveness with append performance.
    fn parse_text_file_streaming<F>(
        path: &Path,
        source: &Arc<SourceData>,
        toast: &ProgressToastHandle,
        file_size: u64,
        parse_fn: F,
    ) -> bool
    where
        F: Fn(String, usize) -> Option<crate::parser::line::LogLine>,
    {
        profiling::scope!("parse_text_file_streaming");

        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) => {
                log::error!("Cannot open file: {e}");
                toast.set_error(format!("Cannot open file: {e}"));
                return false;
            }
        };

        let reader = BufReader::new(file);
        let mut chunk_lines = Vec::new();
        let mut file_line_number = 0;
        let mut bytes_read: usize = 0;
        let mut chunk_count = 0;
        let mut current_chunk_size = TEXT_INITIAL_CHUNK_SIZE;
        let parse_start = std::time::Instant::now();

        let file_name = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();

        for line_result in reader.lines() {
            let line_buffer = match line_result {
                Ok(line) => line,
                Err(e) => {
                    log::warn!("Error reading line {file_line_number}: {e}");
                    continue;
                }
            };

            file_line_number += 1;
            bytes_read += line_buffer.len() + 1; // +1 for newline

            // Update progress every 500 lines
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

            // Skip empty lines
            if line_buffer.trim().is_empty() {
                continue;
            }

            // Parse the line
            let Some(log_line) = parse_fn(line_buffer, file_line_number) else {
                continue;
            };

            chunk_lines.push(log_line);

            // Append chunk when it reaches current size
            if chunk_lines.len() >= current_chunk_size {
                source.append_lines(std::mem::take(&mut chunk_lines));
                chunk_count += 1;

                // Grow chunk size exponentially (double every N chunks)
                if chunk_count % TEXT_CHUNKS_BEFORE_GROWTH == 0
                    && current_chunk_size < TEXT_MAX_CHUNK_SIZE
                {
                    current_chunk_size = (current_chunk_size * 2).min(TEXT_MAX_CHUNK_SIZE);
                    log::debug!("Increased chunk size to {current_chunk_size} lines");
                }

                let progress = (bytes_read as f32 / file_size as f32).min(1.0);
                toast.update(
                    progress,
                    format!("Loading {}... ({} lines)", file_name, source.len()),
                );
            }
        }

        // Append any remaining lines
        if !chunk_lines.is_empty() {
            source.append_lines(chunk_lines);
        }

        let parse_duration = parse_start.elapsed();
        log::info!(
            "Streaming parse took {:?} to process {} lines from {} ({} chunks)",
            parse_duration,
            source.len(),
            path.display(),
            chunk_count
        );
        log::info!(
            "Line processing stats: total_file_lines={}, parsed_lines={}, skipped={}",
            file_line_number,
            source.len(),
            file_line_number - source.len()
        );

        !source.is_empty()
    }

    /// Legacy text file parser (loads entire file into memory)
    /// 
    /// This is kept for compatibility with formats that need the full content
    /// upfront (e.g., certain special processing cases). New code should prefer
    /// `parse_text_file_streaming` for better memory efficiency.
    #[allow(dead_code)]
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
            // Check for cancellation every 1000 lines
            if idx % 1000 == 0 {
                if data_source.is_cancelled() {
                    log::info!("Anomaly scoring cancelled for {}", path.display());
                    toast.set_error("Scoring cancelled".to_string());
                    toast.dismiss();
                    return;
                }

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
