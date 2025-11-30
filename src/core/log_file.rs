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
use crate::parser::{dlt, line::LogLine, parse_line};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;

/// Messages sent during background file loading
pub enum LoadMessage {
    Progress(f32, String),
    Complete(Arc<Vec<LogLine>>, PathBuf),
    Error(String),
    /// Sent periodically during scoring with progress updates
    ScoringProgress(String),
    /// Sent when scoring is complete with updated lines
    ScoringComplete(Vec<f64>),
}

/// Handles asynchronous loading and processing of log files
pub struct LogFileLoader;

impl LogFileLoader {
    /// Start loading a file in the background
    /// Returns a receiver for progress updates and completion
    pub fn load_async(path: PathBuf, ctx: egui::Context) -> Receiver<LoadMessage> {
        let (tx, rx) = channel();

        thread::spawn(move || {
            Self::process_file_background(path, tx, ctx);
        });

        rx
    }

    fn read_dlt_file(
        path: PathBuf,
        tx: Sender<LoadMessage>,
        ctx: egui::Context,
    ) -> Arc<Vec<LogLine>> {
        log::info!("Detected DLT binary file, using dlt-core parser");
        let _ = tx.send(LoadMessage::Progress(
            0.5,
            format!("Parsing DLT binary file {}...", path.display()),
        ));
        ctx.request_repaint();

        match dlt::parse_dlt_file(&path) {
            Ok(lines) => {
                log::info!("Successfully parsed {} DLT messages", lines.len());
                let lines_arc = Arc::new(lines);
                let _ = tx.send(LoadMessage::Complete(Arc::clone(&lines_arc), path.clone()));
                ctx.request_repaint();
                lines_arc
            }
            Err(e) => {
                log::error!("Failed to parse DLT file: {}", e);
                let _ = tx.send(LoadMessage::Error(format!(
                    "Failed to parse DLT file: {}",
                    e
                )));
                Arc::new(Vec::new())
            }
        }
    }

    fn process_file_background(path: PathBuf, tx: Sender<LoadMessage>, ctx: egui::Context) {
        let start_time = std::time::Instant::now();
        log::debug!("Starting background file processing for: {:?}", path);

        // Get file size for progress tracking
        let metadata = std::fs::metadata(&path);
        if let Err(e) = metadata {
            log::error!("Cannot read file metadata: {}", e);
            let _ = tx.send(LoadMessage::Error(format!("Cannot read file: {}", e)));
            return;
        }
        let file_size = metadata.unwrap().len();
        log::debug!("File size: {} bytes", file_size);

        // Check if this is a DLT binary file by extension or magic bytes
        let is_dlt_file = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("dlt"))
            .unwrap_or(false);

        // If it's a DLT file, parse it differently
        let lines_arc = if is_dlt_file {
            Self::read_dlt_file(path.clone(), tx.clone(), ctx.clone())
        } else {
            Self::read_generic_file(path.clone(), tx.clone(), ctx.clone(), start_time, file_size)
        };

        if !lines_arc.is_empty() {
            Self::score_and_send_lines(
                lines_arc.clone(),
                path.clone(),
                tx.clone(),
                ctx.clone(),
                start_time,
            );
        }
    }

    fn read_generic_file(
        path: PathBuf,
        tx: Sender<LoadMessage>,
        ctx: egui::Context,
        start_time: std::time::Instant,
        file_size: u64,
    ) -> Arc<Vec<LogLine>> {
        let file = File::open(&path);
        if let Err(e) = file {
            log::error!("Cannot open file: {}", e);
            let _ = tx.send(LoadMessage::Error(format!("Cannot open file: {}", e)));
            return Arc::new(Vec::new());
        }

        // Read file with lossy UTF-8 conversion to handle non-UTF8 characters
        let read_start = std::time::Instant::now();
        let mut file = file.unwrap();
        let mut buffer = Vec::new();
        if let Err(e) = file.read_to_end(&mut buffer) {
            log::error!("Cannot read file content: {}", e);
            let _ = tx.send(LoadMessage::Error(format!("Cannot read file: {}", e)));
            return Arc::new(Vec::new());
        }

        let read_duration = read_start.elapsed();
        log::info!(
            "File I/O took {:?} to read {} bytes",
            read_duration,
            buffer.len()
        );

        // Convert to UTF-8 with lossy conversion (replaces invalid UTF-8 with � character)
        let utf8_start = std::time::Instant::now();
        let content = String::from_utf8_lossy(&buffer);
        log::info!("UTF-8 conversion took {:?}", utf8_start.elapsed());

        let mut lines = Vec::new();

        let mut bytes_read: usize = 0;

        // First pass: parse lines WITHOUT scoring for fast display
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_scope!("parse_lines");

        let parse_start = std::time::Instant::now();
        let mut file_line_number = 0;
        for line_buffer in content.lines() {
            file_line_number += 1;
            bytes_read += line_buffer.len() + 1; // +1 for newline

            if file_line_number % 500 == 0 {
                let progress = (bytes_read as f32 / file_size as f32).min(1.0);
                let _ = tx.send(LoadMessage::Progress(
                    progress,
                    format!("Loading {}... ({} lines)", path.display(), lines.len()),
                ));
                ctx.request_repaint();
            }

            if line_buffer.trim().is_empty() {
                continue;
            }

            let log_line = match parse_line(line_buffer.to_string(), file_line_number) {
                Some(line) => line,
                None => continue, // Skip lines without timestamp
            };

            lines.push(log_line);
        }

        let parse_duration = parse_start.elapsed();
        log::info!(
            "Parsing took {:?} to process {} lines from {:?}",
            parse_duration,
            lines.len(),
            path
        );

        // Wrap in Arc for cheap cloning
        let arc_start = std::time::Instant::now();
        let lines_arc = Arc::new(lines);
        log::info!("Arc wrapping took {:?}", arc_start.elapsed());

        // Send the parsed lines immediately so user can start working
        // Arc clone is cheap (just increments reference count)
        let send_start = std::time::Instant::now();
        let _ = tx.send(LoadMessage::Complete(Arc::clone(&lines_arc), path.clone()));
        ctx.request_repaint();
        log::info!("Sending Complete message took {:?}", send_start.elapsed());
        log::info!("Total time to display file: {:?}", start_time.elapsed());
        lines_arc
    }

    /// Helper to score and send lines (used for both text and DLT files)
    fn score_and_send_lines(
        lines_arc: Arc<Vec<LogLine>>,
        path: PathBuf,
        tx: Sender<LoadMessage>,
        ctx: egui::Context,
        start_time: std::time::Instant,
    ) {
        // Now calculate anomaly scores in the background
        let score_start = std::time::Instant::now();
        log::debug!(
            "Starting background anomaly scoring for {} lines",
            lines_arc.len()
        );

        let mut scorer = create_default_scorer();
        let mut raw_scores = Vec::new();

        #[cfg(feature = "cpu-profiling")]
        puffin::profile_scope!("score_lines");

        let total_lines = lines_arc.len();

        for (idx, log_line) in lines_arc.iter().enumerate() {
            if idx % 1000 == 0 {
                let _ = tx.send(LoadMessage::ScoringProgress(format!(
                    "Scoring... ({}/{})",
                    idx, total_lines
                )));
                ctx.request_repaint();
            }

            let score = if idx < 10 {
                0.0
            } else {
                scorer.score(log_line)
            };
            raw_scores.push(score);
            scorer.update(log_line);
        }

        let _ = tx.send(LoadMessage::ScoringProgress(
            "Normalizing scores...".to_string(),
        ));
        ctx.request_repaint();

        #[cfg(feature = "cpu-profiling")]
        puffin::profile_scope!("normalize_scores");

        let normalized_scores = normalize_scores(&raw_scores);

        let _ = tx.send(LoadMessage::ScoringProgress(
            "Finalizing scores...".to_string(),
        ));
        ctx.request_repaint();

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

        let score_duration = score_start.elapsed();
        log::info!("Anomaly scoring took {:?} for {:?}", score_duration, path);
        log::info!("Total processing time: {:?}", start_time.elapsed());
        let _ = tx.send(LoadMessage::ScoringComplete(normalized_scores));
        ctx.request_repaint();
    }
}
