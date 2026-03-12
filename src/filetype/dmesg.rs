// LogCrab - GPL-3.0-or-later
// Copyright (C) 2026 Daniel Freiermuth

use chrono::{DateTime, Local, TimeZone, Utc};
use egui::Ui;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::LazyLock;

use crate::filetype::{InputFileType, LineType, TextFileType};

// ============================================================================
// DmesgLogLine
// ============================================================================

/// Dmesg kernel log line: `[SECONDS.MICROSECONDS] message`
///
/// The timestamp is seconds since system boot. It is stored as a
/// `DateTime<Local>` anchored at the Unix epoch so that relative ordering and
/// calibration both work correctly — the absolute wall-clock value is
/// meaningless until the user calibrates the source.
#[derive(Debug, Clone)]
pub struct DmesgLogLine {
    /// Original raw line from file
    raw_line: String,
    /// Parsed timestamp (epoch + boot-relative duration)
    pub timestamp: DateTime<Local>,
    /// Message portion (everything after `[SECONDS.MICROSECONDS] `)
    message_text: String,
    /// Original line number in source file
    pub line_number: usize,
    /// Anomaly score (mutable)
    pub anomaly_score: f64,
}

impl DmesgLogLine {
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
            anomaly_score: 0.0,
        }
    }
}

// ============================================================================
// DmesgFileState
// ============================================================================

/// Type alias kept for compatibility; the shared [`crate::filetype::SimpleFileState`]
/// provides all interior-mutable time-offset and calibration state.
pub type DmesgFileState = crate::filetype::SimpleFileState;

// ============================================================================
// LineType implementation
// ============================================================================

impl LineType for DmesgLogLine {
    type Config = ();
    type FileState = DmesgFileState;

    fn file_state_from_v2(time_offset_ms: i64) -> DmesgFileState {
        let s = DmesgFileState::default();
        s.set_time_offset_ms(time_offset_ms);
        s
    }

    fn timestamp(&self, _config: &(), file_state: &DmesgFileState) -> DateTime<Local> {
        self.timestamp + chrono::Duration::milliseconds(file_state.time_offset_ms())
    }

    fn message(&self) -> String {
        self.message_text.clone()
    }

    fn display_message(&self, _config: &(), file_state: &DmesgFileState) -> String {
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

    fn anomaly_score(&self) -> f64 {
        self.anomaly_score
    }

    fn set_anomaly_score(&mut self, score: f64) {
        self.anomaly_score = score;
    }

    fn egui_render_context_menu(&self, ui: &mut Ui, _config: &(), file_state: &DmesgFileState) {
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
                    None,
                ),
            ));
            ui.close();
        }
    }
}

// ============================================================================
// DmesgFileType
// ============================================================================

/// Stateful reader for kernel dmesg log files.
///
/// Expects lines of the form `[SECONDS.MICROSECONDS] message` as produced by
/// `dmesg` or captured from the kernel ring buffer. Lines that do not carry a
/// timestamp header are treated as continuations and appended (with `\n`) to
/// the most-recently-seen timestamped entry.
pub struct DmesgFileType {
    reader: BufReader<File>,
    line_number: usize,
    bytes_read: u64,
    /// Last parsed entry, held back until we know it has no more continuations.
    pending: Option<DmesgLogLine>,
}

impl InputFileType for DmesgFileType {
    type LineType = DmesgLogLine;

    const FILE_EXTENSIONS: &'static [&'static str] = &["log", "txt"];

    fn open(
        path: &Path,
        _config: (),
        _file_state: std::sync::Arc<DmesgFileState>,
    ) -> Result<Self, String> {
        let file =
            File::open(path).map_err(|e| format!("Failed to open {}: {e}", path.display()))?;
        Ok(Self {
            reader: BufReader::new(file),
            line_number: 0,
            bytes_read: 0,
            pending: None,
        })
    }

    fn read(&mut self, lines_to_read: usize) -> Result<Vec<Self::LineType>, String> {
        let mut result = Vec::with_capacity(lines_to_read);
        let mut buf = String::new();
        let mut eof = false;
        for _ in 0..lines_to_read {
            buf.clear();
            match self.reader.read_line(&mut buf) {
                Ok(0) => {
                    eof = true;
                    break;
                }
                Ok(n) => {
                    self.bytes_read += n as u64;
                    self.line_number += 1;
                    let raw = buf.trim_end_matches(['\n', '\r']).to_string();
                    if let Some(new_entry) = parse_dmesg_line(raw.clone(), self.line_number) {
                        // New timestamped entry: flush the previous pending one.
                        if let Some(prev) = self.pending.take() {
                            result.push(prev);
                        }
                        self.pending = Some(new_entry);
                    } else if let Some(ref mut prev) = self.pending {
                        // Continuation line: append to the pending entry.
                        prev.raw_line.push('\n');
                        prev.raw_line.push_str(&raw);
                        prev.message_text.push('\n');
                        prev.message_text.push_str(&raw);
                    }
                    // Orphan continuation (no pending entry yet) is silently dropped.
                }
                Err(e) => return Err(format!("Read error: {e}")),
            }
        }
        if eof {
            // Flush the final entry once the file is exhausted.
            if let Some(last) = self.pending.take() {
                result.push(last);
            }
        }
        Ok(result)
    }

    fn bytes_consumed(&self) -> u64 {
        self.bytes_read
    }
}

impl TextFileType for DmesgFileType {
    /// Returns `true` if at least 10 of the first 100 non-empty lines match
    /// the dmesg `[SECONDS.MICROSECONDS]` timestamp pattern.
    fn looks_like(file: &mut dyn std::io::Read) -> bool {
        let mut buf = [0u8; 4096];
        let n = file.read(&mut buf).unwrap_or(0);
        let sample = String::from_utf8_lossy(&buf[..n]);
        let mut dmesg_count = 0u32;
        for line in sample.lines().take(100) {
            if is_dmesg_line(line) {
                dmesg_count += 1;
                if dmesg_count >= 10 {
                    return true;
                }
            }
        }
        false
    }
}

// ============================================================================
// Dmesg parsing utilities
// ============================================================================

/// Matches `[SECONDS.MICROSECONDS] rest` — the canonical dmesg timestamp format.
/// Seconds may be any non-negative integer; fractional part is exactly 6 digits.
static DMESG_TIMESTAMP: LazyLock<fancy_regex::Regex> = LazyLock::new(|| {
    fancy_regex::Regex::new(r"^\[\s*(\d+)\.(\d{6})\] (.*)$").expect("valid regex literal")
});

/// Returns `true` when the line starts with a dmesg-style `[SSSSSS.UUUUUU]` header.
pub fn is_dmesg_line(line: &str) -> bool {
    DMESG_TIMESTAMP.is_match(line).unwrap_or(false)
}

/// Parse a single dmesg line into a [`DmesgLogLine`].
///
/// The boot-relative timestamp is anchored at the Unix epoch so that
/// relative ordering is preserved. Returns `None` for lines that do not
/// match the expected format.
pub fn parse_dmesg_line(raw: String, line_number: usize) -> Option<DmesgLogLine> {
    let caps = DMESG_TIMESTAMP.captures(&raw).ok()??;
    let secs: i64 = caps[1].parse().ok()?;
    let micros: i64 = caps[2].parse().ok()?;
    let message = caps[3].to_string();
    let total_micros = secs * 1_000_000 + micros;
    let timestamp = Utc
        .timestamp_opt(total_micros / 1_000_000, ((total_micros % 1_000_000) * 1_000) as u32)
        .single()?
        .with_timezone(&Local);
    Some(DmesgLogLine::new(raw, timestamp, message, line_number))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_line() {
        let raw = "[    0.000000] Initializing cgroup subsys cpuset".to_string();
        let line = parse_dmesg_line(raw, 1).expect("should parse dmesg line");
        assert_eq!(line.message_text, "Initializing cgroup subsys cpuset");
        assert_eq!(line.timestamp.timestamp_micros(), 0);
    }

    #[test]
    fn test_parse_large_timestamp() {
        let raw = "[42798.603585] init: service 'foo' requested start".to_string();
        let line = parse_dmesg_line(raw, 2).expect("should parse dmesg line");
        assert_eq!(
            line.message_text,
            "init: service 'foo' requested start"
        );
        // 42798 seconds + 603585 microseconds
        assert_eq!(
            line.timestamp.timestamp_micros(),
            42798 * 1_000_000 + 603_585
        );
    }

    #[test]
    fn test_is_dmesg_line() {
        assert!(is_dmesg_line("[    0.000000] Linux version 6.1.0"));
        assert!(is_dmesg_line("[42798.603585] init: service 'foo' started"));
        assert!(!is_dmesg_line("2024-01-01 00:00:00 some generic log"));
        assert!(!is_dmesg_line("01-20 14:23:45.123 logcat line"));
        assert!(!is_dmesg_line("just plain text"));
        assert!(!is_dmesg_line("[42798.60358] wrong fractional digits"));
    }

    #[test]
    fn test_non_matching_returns_none() {
        assert!(parse_dmesg_line("not a dmesg line".to_string(), 1).is_none());
    }

    // ---- multi-line merging via DmesgFileType::read() ----

    fn make_reader(content: &str) -> DmesgFileType {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().expect("tmpfile");
        tmp.write_all(content.as_bytes()).expect("write");
        let path = tmp.path().to_owned();
        let ft = DmesgFileType {
            reader: BufReader::new(File::open(&path).expect("open")),
            line_number: 0,
            bytes_read: 0,
            pending: None,
        };
        drop(tmp);
        ft
    }

    #[test]
    fn test_multiline_continuation_appended() {
        let content = "[   1.000000] First line\ncontinuation one\ncontinuation two\n[   2.000000] Second entry\n";
        let mut ft = make_reader(content);
        let lines = ft.read(100).expect("read");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].message_text, "First line\ncontinuation one\ncontinuation two");
        assert_eq!(lines[0].raw_line, "[   1.000000] First line\ncontinuation one\ncontinuation two");
        assert_eq!(lines[1].message_text, "Second entry");
    }

    #[test]
    fn test_orphan_continuation_dropped() {
        // A continuation before any timestamped line should be silently dropped.
        let content = "orphan line\n[   1.000000] Real entry\n";
        let mut ft = make_reader(content);
        let lines = ft.read(100).expect("read");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].message_text, "Real entry");
    }

    #[test]
    fn test_last_entry_flushed_on_eof() {
        let content = "[   1.000000] Only entry\ncontinuation\n";
        let mut ft = make_reader(content);
        let lines = ft.read(100).expect("read");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].message_text, "Only entry\ncontinuation");
    }

    #[test]
    fn test_pending_carried_across_batches() {
        // Read in batches of 1 source-line to exercise cross-batch continuation.
        let content = "[   1.000000] Entry\ncontinuation\n[   2.000000] Next\n";
        let mut ft = make_reader(content);
        // Batch 1: reads "[   1.000000] Entry" — pending, no output yet.
        let b1 = ft.read(1).expect("read");
        assert!(b1.is_empty(), "pending must not be flushed mid-batch");
        // Batch 2: reads "continuation" — appended, no new flush.
        let b2 = ft.read(1).expect("read");
        assert!(b2.is_empty());
        // Batch 3: reads "[   2.000000] Next" — flushes first entry, holds second.
        let b3 = ft.read(1).expect("read");
        assert_eq!(b3.len(), 1);
        assert_eq!(b3[0].message_text, "Entry\ncontinuation");
        // Batch 4: EOF — flushes second entry.
        let b4 = ft.read(1).expect("read");
        assert_eq!(b4.len(), 1);
        assert_eq!(b4[0].message_text, "Next");
    }
}
