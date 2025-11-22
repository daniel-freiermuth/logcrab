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
use crate::parser::{line::LogLine, parse_line};
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

/// Messages sent during background file loading
pub enum LoadMessage {
    Progress(f32, String),
    Complete(Vec<LogLine>, PathBuf),
    Error(String),
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

    fn process_file_background(path: PathBuf, tx: Sender<LoadMessage>, ctx: egui::Context) {
        log::debug!("Starting background file processing for: {:?}", path);
        
        // Get file size for progress tracking
        let metadata = std::fs::metadata(&path);
        if let Err(e) = metadata {
            log::error!("Cannot read file metadata: {}", e);
            let _ = tx.send(LoadMessage::Error(format!("Cannot read file: {}", e)));
            return;
        }
        let file_size = metadata.unwrap().len() as f32;
        log::debug!("File size: {} bytes", file_size);

        let file = File::open(&path);
        if let Err(e) = file {
            log::error!("Cannot open file: {}", e);
            let _ = tx.send(LoadMessage::Error(format!("Cannot open file: {}", e)));
            return;
        }

        // Read file with lossy UTF-8 conversion to handle non-UTF8 characters
        let mut file = file.unwrap();
        let mut buffer = Vec::new();
        if let Err(e) = file.read_to_end(&mut buffer) {
            log::error!("Cannot read file content: {}", e);
            let _ = tx.send(LoadMessage::Error(format!("Cannot read file: {}", e)));
            return;
        }

        log::debug!("Read {} bytes from file", buffer.len());
        
        // Convert to UTF-8 with lossy conversion (replaces invalid UTF-8 with � character)
        let content = String::from_utf8_lossy(&buffer);

        let mut scorer = create_default_scorer();
        let mut lines = Vec::new();
        let mut raw_scores = Vec::new();

        let mut bytes_read: usize = 0;

        // First pass: parse and score
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_scope!("parse_and_score");

        let mut file_line_number = 0;
        for line_buffer in content.lines() {
            file_line_number += 1;
            bytes_read += line_buffer.len() + 1; // +1 for newline

            // Update progress based on bytes read (first 80% of total progress)
            if file_line_number % 500 == 0 {
                let progress = 0.8 * (bytes_read as f32 / file_size).min(1.0);
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

            let mut log_line = log_line;

            // Score before updating (key requirement!)
            let score = scorer.score(&log_line);
            log_line.anomaly_score = score;
            raw_scores.push(score);

            // Update scorer state
            scorer.update(&log_line);

            lines.push(log_line);
        }

        let _ = tx.send(LoadMessage::Progress(
            0.8,
            format!("Normalizing scores for {}...", path.display()),
        ));
        ctx.request_repaint();

        // Second pass: normalize scores to 0-100
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_scope!("normalize_scores");

        let normalized_scores = normalize_scores(&raw_scores);

        let _ = tx.send(LoadMessage::Progress(
            0.9,
            format!("Finalizing {}...", path.display()),
        ));
        ctx.request_repaint();

        // Log score statistics
        if !raw_scores.is_empty() {
            let min_raw = raw_scores.iter().copied().fold(f64::INFINITY, f64::min);
            let max_raw = raw_scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let avg_raw: f64 = raw_scores.iter().sum::<f64>() / raw_scores.len() as f64;
            log::info!(
                "Score statistics - Raw: min={:.3}, max={:.3}, avg={:.3}, total_lines={}",
                min_raw, max_raw, avg_raw, raw_scores.len()
            );

            let min_norm = normalized_scores
                .iter()
                .copied()
                .fold(f64::INFINITY, f64::min);
            let max_norm = normalized_scores
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            let avg_norm: f64 =
                normalized_scores.iter().sum::<f64>() / normalized_scores.len() as f64;
            log::info!(
                "Score statistics - Normalized: min={:.3}, max={:.3}, avg={:.3}",
                min_norm, max_norm, avg_norm
            );
        }

        log::debug!("Finalizing {} log lines", lines.len());

        for (line, &norm_score) in lines.iter_mut().zip(normalized_scores.iter()) {
            line.anomaly_score = norm_score;
        }

        log::info!("File processing complete for: {:?}", path);
        let _ = tx.send(LoadMessage::Complete(lines, path));
        ctx.request_repaint();
    }
}
