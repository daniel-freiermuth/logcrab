// LogCrab - GPL-3.0-or-later
// Copyright (C) 2026 Daniel Freiermuth

use chrono::{DateTime, Datelike, Local};
use egui::Ui;
use fancy_regex::Regex;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::LazyLock;

use crate::filetype::{InputFileType, LineType, TextFileType};

// ============================================================================
// LogcatLogLine
// ============================================================================

/// Android Logcat format: MM-DD HH:MM:SS.mmm PID TID LEVEL TAG: message
#[derive(Debug, Clone)]
pub struct LogcatLogLine {
    /// Original raw line from file
    raw_line: String,
    /// Parsed timestamp
    pub timestamp: DateTime<Local>,
    /// Message portion (everything after timestamp)
    message_text: String,
    /// Original line number in source file
    pub line_number: usize,
}

impl LogcatLogLine {
    pub const fn new(
        raw_line: String,
        timestamp: DateTime<Local>,
        message_text: String,
        line_number: usize,
    ) -> Self {
        Self {
            raw_line,
            timestamp,
            message_text,
            line_number,
        }
    }
}

// ============================================================================
// LogcatFileState
// ============================================================================

/// Type alias kept for compatibility; the shared [`crate::filetype::SimpleFileState`]
/// provides all interior-mutable time-offset and calibration state.
pub type LogcatFileState = crate::filetype::SimpleFileState;

// ============================================================================
// LineType implementation
// ============================================================================

impl LineType for LogcatLogLine {
    type Config = ();
    type FileState = LogcatFileState;

    fn file_state_from_v2(time_offset_ms: i64) -> LogcatFileState {
        let s = LogcatFileState::default();
        s.set_time_offset_ms(time_offset_ms);
        s
    }

    fn timestamp(&self, _config: &(), file_state: &LogcatFileState) -> DateTime<Local> {
        self.timestamp + chrono::Duration::milliseconds(file_state.time_offset_ms())
    }

    fn message(&self) -> String {
        self.message_text.clone()
    }

    fn display_message(&self, _config: &(), file_state: &LogcatFileState) -> String {
        let offset_ms = file_state.time_offset_ms();
        if offset_ms != 0 {
            format!(
                "[{}] {}",
                crate::parser::format_time_diff(chrono::Duration::milliseconds(offset_ms)),
                self.message_text
            )
        } else {
            self.message_text.clone()
        }
    }

    fn raw(&self) -> String {
        self.raw_line.clone()
    }

    fn line_number(&self) -> usize {
        self.line_number
    }

    fn egui_render_context_menu(&self, ui: &mut Ui, _config: &(), file_state: &LogcatFileState) {
        if ui.button("⏱ Calibrate Time Here").clicked() {
            let raw_time = self.timestamp;
            let display_time =
                raw_time + chrono::Duration::milliseconds(file_state.time_offset_ms());
            *file_state
                .calibration
                .lock()
                .expect("calibration lock poisoned") = Some((
                raw_time,
                crate::filetype::CalibrationWindow::new(
                    display_time,
                    false,
                    Some(display_time),
                    raw_time,
                ),
            ));
            ui.close();
        }
    }
}

// ============================================================================
// LogcatFileType
// ============================================================================

/// Stateful reader for pure Android logcat output (no bugreport dumpstate header).
///
/// Must be registered **after** [`super::bugreport::BugreportFileType`] — bugreports also match
/// logcat lines, so bugreport wins when checked first.
pub struct LogcatFileType {
    reader: BufReader<File>,
    year: i32,
    line_number: usize,
    bytes_read: u64,
}

impl InputFileType for LogcatFileType {
    type LineType = LogcatLogLine;

    const FILE_EXTENSIONS: &'static [&'static str] = &["txt", "log"];

    /// Open a logcat file for pull-based reading.
    ///
    /// Logcat lines carry no year; the current calendar year is used.
    fn open(
        path: &Path,
        _config: (),
        _file_state: std::sync::Arc<LogcatFileState>,
    ) -> anyhow::Result<Self> {
        use anyhow::Context as _;
        let year = chrono::Local::now().year();
        let file =
            File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
        Ok(Self {
            reader: BufReader::new(file),
            year,
            line_number: 0,
            bytes_read: 0,
        })
    }

    fn read(&mut self, lines_to_read: usize) -> anyhow::Result<Vec<Self::LineType>> {
        let mut result = Vec::with_capacity(lines_to_read);
        let mut buf = String::new();
        while result.len() < lines_to_read {
            buf.clear();
            match self.reader.read_line(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    self.bytes_read += n as u64;
                    self.line_number += 1;
                    let raw = buf.trim_end_matches(['\n', '\r']).to_string();
                    if let Some(line) = parse_logcat_line(raw, self.line_number, self.year) {
                        result.push(line);
                    } else {
                        tracing::warn!(
                            "Failed to parse line {}: '{}'",
                            self.line_number,
                            buf.trim_end()
                        );
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Read error: {e}")),
            }
        }
        Ok(result)
    }

    fn bytes_consumed(&self) -> u64 {
        self.bytes_read
    }
}

impl TextFileType for LogcatFileType {
    /// Returns `true` if at least 10 of the first 100 non-empty lines match
    /// the logcat `MM-DD HH:MM:SS.mmm` timestamp pattern.
    fn looks_like(file: &mut dyn std::io::Read) -> bool {
        let mut buf = [0u8; 4096];
        let n = file.read(&mut buf).unwrap_or(0);
        let sample = String::from_utf8_lossy(&buf[..n]);
        let mut logcat_count = 0u32;
        for line in sample.lines().take(100) {
            if is_logcat_line(line) {
                logcat_count += 1;
                if logcat_count >= 10 {
                    return true;
                }
            }
        }
        false
    }
}

// ============================================================================
// Logcat parsing utilities (moved from parser/logcat.rs)
// ============================================================================

static LOGCAT_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}\.\d{3})\s+(.*)$").expect("valid regex literal")
});

/// Check if a line looks like a logcat line (starts with MM-DD HH:MM:SS.mmm)
pub fn is_logcat_line(line: &str) -> bool {
    LOGCAT_TIMESTAMP.is_match(line).unwrap_or(false)
}

/// Parse a single logcat line and return the concrete `LogcatLogLine`.
pub fn parse_logcat_line(raw: String, line_number: usize, year: i32) -> Option<LogcatLogLine> {
    if let Ok(Some(caps)) = LOGCAT_TIMESTAMP.captures(&raw) {
        let message = caps[2].to_string();
        return parse_logcat_timestamp(&caps[1], year)
            .map(|ts| LogcatLogLine::new(raw, ts, message, line_number));
    }
    None
}

fn parse_logcat_timestamp(s: &str, year: i32) -> Option<DateTime<Local>> {
    let timestamp_str = format!("{year}-{s}");
    if let Ok(naive) =
        chrono::NaiveDateTime::parse_from_str(&timestamp_str, "%Y-%m-%d %H:%M:%S%.3f")
    {
        return naive.and_local_timezone(Local).single();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    #[test]
    fn test_threadtime_format() {
        let raw = "11-20 14:23:45.123  1234  5678 I ActivityManager: Start proc com.example.app"
            .to_string();
        let line = parse_logcat_line(raw, 1, 2024).expect("should parse logcat line");
        assert_eq!(
            line.message_text,
            "1234  5678 I ActivityManager: Start proc com.example.app"
        );
    }

    #[test]
    fn test_threadtime_with_process_name() {
        let raw =
            "01-01 00:00:07.329  root     8     8 I CAM_INFO: CAM-ICP: cam_icp_mgr_process_dbg_buf"
                .to_string();
        let line = parse_logcat_line(raw, 1, 2024).expect("should parse logcat line");
        assert_eq!(
            line.message_text,
            "root     8     8 I CAM_INFO: CAM-ICP: cam_icp_mgr_process_dbg_buf"
        );
    }

    #[test]
    fn test_fallback_format() {
        let raw = "11-20 14:23:45.123 Some message without tag".to_string();
        let line = parse_logcat_line(raw, 1, 2024).expect("should parse logcat line");
        assert_eq!(line.message_text, "Some message without tag");
    }

    #[test]
    fn test_parse_with_detected_year() {
        let raw = "11-20 14:23:45.123 Test message".to_string();
        let line = parse_logcat_line(raw, 1, 2023).expect("should parse logcat line");
        assert!(line.timestamp.year() == 2023);
    }

    #[test]
    fn test_is_logcat_line() {
        assert!(is_logcat_line("11-20 14:23:45.123 some message"));
        assert!(is_logcat_line("01-01 00:00:00.000 test"));
        assert!(!is_logcat_line("2024-11-20 14:23:45 generic format"));
        assert!(!is_logcat_line("just some text"));
    }
}
