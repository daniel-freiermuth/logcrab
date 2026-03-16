// LogCrab - GPL-3.0-or-later
// Copyright (C) 2026 Daniel Freiermuth

use chrono::{DateTime, Datelike, Local, TimeZone};
use egui::Ui;
use fancy_regex::Regex;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::LazyLock;

use crate::filetype::{InputFileType, LineType, TextFileType};

// ============================================================================
// GenericLogLine
// ============================================================================

/// Generic text-based log line with timestamp
#[derive(Debug, Clone)]
pub struct GenericLogLine {
    /// Original raw line from file
    raw_line: String,
    /// Parsed timestamp
    pub timestamp: DateTime<Local>,
    /// Message portion (everything after timestamp, or whole line if no timestamp)
    message_text: String,
    /// Original line number in source file
    pub line_number: usize,
    /// Anomaly score (mutable)
    pub anomaly_score: f64,
}

impl GenericLogLine {
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
// GenericFileState
// ============================================================================

/// Type alias kept for compatibility; the shared [`crate::filetype::SimpleFileState`]
/// provides all interior-mutable time-offset and calibration state.
pub type GenericFileState = crate::filetype::SimpleFileState;

// ============================================================================
// LineType implementation
// ============================================================================

impl LineType for GenericLogLine {
    type Config = ();
    type FileState = GenericFileState;

    fn file_state_from_v2(time_offset_ms: i64) -> GenericFileState {
        let s = GenericFileState::default();
        s.set_time_offset_ms(time_offset_ms);
        s
    }

    fn timestamp(&self, _config: &(), file_state: &GenericFileState) -> DateTime<Local> {
        self.timestamp + chrono::Duration::milliseconds(file_state.time_offset_ms())
    }

    fn message(&self) -> String {
        self.message_text.clone()
    }

    fn display_message(&self, _config: &(), file_state: &GenericFileState) -> String {
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

    fn egui_render_context_menu(&self, ui: &mut Ui, _config: &(), file_state: &GenericFileState) {
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
// GenericFileType (InputFileType + TextFileType)
// ============================================================================

/// Stateful reader for generic text log files with common timestamp formats.
///
/// **Must be the last text type in the registry** — its `looks_like` always
/// returns `true`, acting as the catch-all fallback.
pub struct GenericFileType {
    reader: BufReader<File>,
    line_number: usize,
    bytes_read: u64,
}

impl InputFileType for GenericFileType {
    type LineType = GenericLogLine;

    const FILE_EXTENSIONS: &'static [&'static str] = &["txt", "log"];

    /// Open a generic text log file for pull-based reading.
    fn open(
        path: &Path,
        _config: (),
        _file_state: std::sync::Arc<GenericFileState>,
    ) -> anyhow::Result<Self> {
        use anyhow::Context as _;
        let file =
            File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
        Ok(Self {
            reader: BufReader::new(file),
            line_number: 0,
            bytes_read: 0,
        })
    }

    fn read(&mut self, lines_to_read: usize) -> anyhow::Result<Vec<Self::LineType>> {
        let mut result = Vec::with_capacity(lines_to_read);
        let mut buf = Vec::new();
        for _ in 0..lines_to_read {
            buf.clear();
            match self.reader.read_until(b'\n', &mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    self.bytes_read += n as u64;
                    self.line_number += 1;
                    let line_str = String::from_utf8_lossy(&buf);
                    let raw = line_str.trim_end_matches(['\n', '\r']).to_string();
                    if matches!(line_str, std::borrow::Cow::Owned(_)) {
                        tracing::warn!(
                            "Line {}: {} contains invalid UTF-8 bytes; replacement characters inserted",
                            self.line_number,
                            raw
                        );
                    }
                    if let Some(line) = parse_generic_line(raw, self.line_number) {
                        result.push(line);
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

impl TextFileType for GenericFileType {
    /// Always returns `true`. Generic is the catch-all; must be last in the registry.
    fn looks_like(_file: &mut dyn std::io::Read) -> bool {
        true
    }
}

// ============================================================================
// Generic text parsing utilities (moved from parser/generic.rs)
// ============================================================================

static ISO_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4}-\d{2}-\d{2}[T\s]\d{2}:\d{2}:\d{2}(?:\.\d{3})?(?:Z|[+-]\d{2}:?\d{2})?)")
        .expect("valid regex literal")
});
static HYPHENATED_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4}-\d{2}-\d{2}-\d{2}:\d{2}:\d{2}(?:\.\d{3})?)").expect("valid regex literal")
});
static SYSLOG_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([A-Z][a-z]{2}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2}(?:\.\d{3})?)")
        .expect("valid regex literal")
});
static BRACKETED_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\[(\d{4}-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}(?:\.\d{3})?)\]")
        .expect("valid regex literal")
});
static LOGCAT_TIMESTAMP_GENERIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}\.\d{3})").expect("valid regex literal")
});
static SLASH_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4}/\d{2}/\d{2}\s+\d{2}:\d{2}:\d{2}(?:\.\d+)?)").expect("valid regex literal")
});
static TIME_ONLY_TIMESTAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{2}:\d{2}:\d{2}(?:\.\d+)?)").expect("valid regex literal"));
static BRACKETED_CTIME_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\[([A-Z][a-z]{2}\s+[A-Z][a-z]{2}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2}\s+\d{4})\]")
        .expect("valid regex literal")
});

/// Parse a single line and return the concrete `GenericLogLine` if it has a recognised timestamp.
pub fn parse_generic_line(raw: String, line_number: usize) -> Option<GenericLogLine> {
    let mut timestamp = None;
    let mut remaining = raw.as_str();

    if let Ok(Some(caps)) = SLASH_TIMESTAMP.captures(remaining) {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&caps[1], "%Y/%m/%d %H:%M:%S%.f") {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        } else if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(&caps[1], "%Y/%m/%d %H:%M:%S")
        {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        }
    } else if let Ok(Some(caps)) = HYPHENATED_TIMESTAMP.captures(remaining) {
        let date_part = &caps[1][..10];
        let time_part = &caps[1][11..];
        let normalized = format!("{date_part} {time_part}");
        if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%d %H:%M:%S%.f")
        {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        } else if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%d %H:%M:%S")
        {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        }
    } else if let Ok(Some(caps)) = ISO_TIMESTAMP.captures(remaining) {
        let ts_str = &caps[1];
        let normalized_ts = ts_str.rfind(['+', '-']).map_or_else(
            || ts_str.replace(' ', "T"),
            |tz_pos| {
                let (datetime_part, tz_part) = ts_str.split_at(tz_pos);
                if tz_part.len() == 5 && !tz_part.contains(':') {
                    format!(
                        "{}{}:{}",
                        datetime_part.replace(' ', "T"),
                        &tz_part[..3],
                        &tz_part[3..]
                    )
                } else {
                    ts_str.replace(' ', "T")
                }
            },
        );
        if let Ok(dt) = DateTime::parse_from_rfc3339(&normalized_ts) {
            timestamp = Some(dt.with_timezone(&Local));
            remaining = remaining[caps[0].len()..].trim_start();
        } else if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(&caps[1], "%Y-%m-%d %H:%M:%S%.3f")
        {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        } else if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(&caps[1], "%Y-%m-%d %H:%M:%S")
        {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        }
    } else if let Ok(Some(caps)) = BRACKETED_TIMESTAMP.captures(remaining) {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&caps[1], "%Y-%m-%d %H:%M:%S%.3f")
        {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        } else if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(&caps[1], "%Y-%m-%d %H:%M:%S")
        {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        }
    } else if let Ok(Some(caps)) = BRACKETED_CTIME_TIMESTAMP.captures(remaining) {
        // e.g. [Sat Mar  7 11:53:27 2026]
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&caps[1], "%a %b %e %H:%M:%S %Y") {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        }
    } else if let Ok(Some(caps)) = LOGCAT_TIMESTAMP_GENERIC.captures(remaining) {
        let timestamp_str = format!("1970-{}", &caps[1]);
        if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(&timestamp_str, "%Y-%m-%d %H:%M:%S%.3f")
        {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        }
    } else if let Ok(Some(caps)) = SYSLOG_TIMESTAMP.captures(remaining) {
        let ts_str = format!("1970 {}", &caps[1]);
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&ts_str, "%Y %b %d %H:%M:%S%.3f") {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        } else if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(&ts_str, "%Y %b %d %H:%M:%S")
        {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        }
    } else if let Ok(Some(caps)) = TIME_ONLY_TIMESTAMP.captures(remaining) {
        let ts_str = format!("1970-01-01 {}", &caps[1]);
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&ts_str, "%Y-%m-%d %H:%M:%S%.f") {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        } else if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(&ts_str, "%Y-%m-%d %H:%M:%S")
        {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        }
    }

    let message = if remaining.is_empty() {
        raw.clone()
    } else {
        remaining.to_string()
    };
    timestamp.map(|ts| GenericLogLine::new(raw, ts, message, line_number))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iso_timestamp() {
        let raw = "2025-11-20T14:23:45.123Z ERROR Connection failed".to_string();
        let line = parse_generic_line(raw, 1).expect("should parse ISO timestamp");
        assert_eq!(line.message_text, "ERROR Connection failed");
    }

    #[test]
    fn test_hyphenated_timestamp() {
        let raw = "2025-11-26-09:58:05 , [402.037] ,cnss: fatal: SMMU fault happened with IOVA 0x0"
            .to_string();
        let line = parse_generic_line(raw, 1).expect("should parse hyphenated timestamp");
        assert_eq!(
            line.message_text,
            ", [402.037] ,cnss: fatal: SMMU fault happened with IOVA 0x0"
        );
        assert_eq!(
            line.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2025-11-26 09:58:05"
        );
    }

    #[test]
    fn test_syslog_format() {
        let raw = "Nov 20 14:23:45 INFO Application started".to_string();
        let line = parse_generic_line(raw, 1).expect("should parse syslog format");
        assert_eq!(line.message_text, "INFO Application started");
        assert_eq!(
            line.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            "1970-11-20 14:23:45"
        );
    }

    #[test]
    fn test_iso_timestamp_with_space_and_milliseconds() {
        let raw = "2025-11-20 14:23:45.123 ERROR Connection failed".to_string();
        let line = parse_generic_line(raw, 1)
            .expect("Should parse ISO timestamp with space and milliseconds");
        assert_eq!(line.message_text, "ERROR Connection failed");
        assert_eq!(
            line.timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            "2025-11-20 14:23:45.123"
        );
    }

    #[test]
    fn test_iso_timestamp_with_space_no_milliseconds() {
        let raw = "2025-11-20 14:23:45 WARN Timeout occurred".to_string();
        let line = parse_generic_line(raw, 1)
            .expect("Should parse ISO timestamp with space, no milliseconds");
        assert_eq!(line.message_text, "WARN Timeout occurred");
        assert_eq!(
            line.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2025-11-20 14:23:45"
        );
    }

    #[test]
    fn test_bracketed_timestamp_with_milliseconds() {
        let raw = "[2025-11-20 14:23:45.123] DEBUG Processing request".to_string();
        let line =
            parse_generic_line(raw, 1).expect("Should parse bracketed timestamp with milliseconds");
        assert_eq!(line.message_text, "DEBUG Processing request");
        assert_eq!(
            line.timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            "2025-11-20 14:23:45.123"
        );
    }

    #[test]
    fn test_bracketed_timestamp_without_milliseconds() {
        let raw = "[2025-11-20 14:23:45] INFO Service started".to_string();
        let line = parse_generic_line(raw, 1)
            .expect("Should parse bracketed timestamp without milliseconds");
        assert_eq!(line.message_text, "INFO Service started");
        assert_eq!(
            line.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2025-11-20 14:23:45"
        );
    }

    #[test]
    fn test_bracketed_ctime_timestamp() {
        let raw = "[Sat Mar  7 11:53:27 2026] kernel: usb 1-1: new high-speed USB device".to_string();
        let line = parse_generic_line(raw, 1).expect("should parse bracketed ctime timestamp");
        assert_eq!(
            line.message_text,
            "kernel: usb 1-1: new high-speed USB device"
        );
        assert_eq!(
            line.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-03-07 11:53:27"
        );
    }

    #[test]
    fn test_logcat_timestamp_format() {
        let raw = "11-20 14:23:45.123 E/ActivityManager: Process crashed".to_string();
        let line = parse_generic_line(raw, 1).expect("Should parse logcat timestamp format");
        assert_eq!(line.message_text, "E/ActivityManager: Process crashed");
        assert_eq!(
            line.timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            "1970-11-20 14:23:45.123"
        );
    }

    #[test]
    fn test_syslog_timestamp_with_milliseconds() {
        let raw =
            "Feb 03 23:26:34.864 qcgpio[gpio_drv.c:1222]: dalcfg_query_item_name gpio_driver done"
                .to_string();
        let line =
            parse_generic_line(raw, 1).expect("Should parse syslog timestamp with milliseconds");
        assert_eq!(
            line.message_text,
            "qcgpio[gpio_drv.c:1222]: dalcfg_query_item_name gpio_driver done"
        );
        assert_eq!(
            line.timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            "1970-02-03 23:26:34.864"
        );
    }

    #[test]
    fn test_iso_timestamp_with_timezone_offset() {
        let raw = "2025-11-20T14:23:45+05:30 INFO Server running".to_string();
        let line =
            parse_generic_line(raw, 1).expect("Should parse ISO timestamp with timezone offset");
        assert_eq!(line.message_text, "INFO Server running");
    }

    #[test]
    fn test_iso_timestamp_with_ms_and_timezone_offset() {
        let raw = "2026-02-05T09:20:23.638+01:00 INFO Server started".to_string();
        let line = parse_generic_line(raw, 1)
            .expect("Should parse ISO timestamp with milliseconds and timezone offset");
        assert_eq!(line.message_text, "INFO Server started");
        assert_eq!(line.timestamp.format("%Y-%m-%d").to_string(), "2026-02-05");
    }

    #[test]
    fn test_iso_timestamp_with_timezone_offset_no_colon() {
        let raw = "2026-02-05T09:20:23+0100 INFO Application started".to_string();
        let line = parse_generic_line(raw, 1)
            .expect("Should parse ISO timestamp with timezone offset without colon");
        assert_eq!(line.message_text, "INFO Application started");
        assert_eq!(line.timestamp.format("%Y-%m-%d").to_string(), "2026-02-05");
    }

    #[test]
    fn test_iso_timestamp_with_negative_timezone_no_colon() {
        let raw = "2026-02-10T15:30:00-0500 WARN Connection timeout".to_string();
        let line = parse_generic_line(raw, 1)
            .expect("Should parse ISO timestamp with negative timezone offset without colon");
        assert_eq!(line.message_text, "WARN Connection timeout");
    }

    #[test]
    fn test_slash_timestamp_with_microseconds() {
        let raw = "2026/03/09 01:20:14.942857 INFO Something happened".to_string();
        let line = parse_generic_line(raw, 1)
            .expect("should parse slash-separated timestamp with microseconds");
        assert_eq!(line.message_text, "INFO Something happened");
        assert_eq!(
            line.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-03-09 01:20:14"
        );
    }

    #[test]
    fn test_slash_timestamp_without_fraction() {
        let raw = "2026/03/09 01:20:14 DEBUG No fractions".to_string();
        let line = parse_generic_line(raw, 1)
            .expect("should parse slash-separated timestamp without fraction");
        assert_eq!(line.message_text, "DEBUG No fractions");
        assert_eq!(
            line.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            "2026-03-09 01:20:14"
        );
    }

    #[test]
    fn test_time_only_with_milliseconds() {
        let raw = "01:34:00.178 INFO Something happened".to_string();
        let line = parse_generic_line(raw, 1).expect("should parse time-only timestamp");
        assert_eq!(line.message_text, "INFO Something happened");
        assert_eq!(
            line.timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            "1970-01-01 01:34:00.178"
        );
    }

    #[test]
    fn test_time_only_without_fraction() {
        let raw = "01:34:00 DEBUG No fractions".to_string();
        let line =
            parse_generic_line(raw, 1).expect("should parse time-only timestamp without fraction");
        assert_eq!(line.message_text, "DEBUG No fractions");
        assert_eq!(
            line.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
            "1970-01-01 01:34:00"
        );
    }
}
