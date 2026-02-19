use super::line::{LogLine, LogcatLogLine};
use chrono::{DateTime, Local, NaiveDateTime};
use fancy_regex::Regex;
use std::sync::LazyLock;

// Just extract timestamp - everything after it is the message
static LOGCAT_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}\.\d{3})\s+(.*)$").expect("valid regex literal")
});

// Pattern to detect bugreport dumpstate header with year
// Matches: == dumpstate: 2025-11-27 14:08:01 ==
static DUMPSTATE_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"==\s*dumpstate:\s*(\d{4})-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}")
        .expect("valid regex literal")
});

/// Check if a line looks like a logcat line (starts with MM-DD HH:MM:SS.mmm)
pub fn is_logcat_line(line: &str) -> bool {
    LOGCAT_TIMESTAMP.is_match(line).unwrap_or(false)
}

/// Detect the year from bugreport header lines
/// Scans the provided content (should be first few KB) for dumpstate header
pub fn detect_year_from_header(content: &str) -> Option<i32> {
    // Only scan first ~50 lines for efficiency
    for line in content.lines().take(50) {
        if let Ok(Some(caps)) = DUMPSTATE_HEADER.captures(line) {
            if let Ok(year) = caps[1].parse::<i32>() {
                log::info!("Detected year {year} from bugreport dumpstate header");
                return Some(year);
            }
        }
    }
    None
}

pub fn parse_logcat_with_year(raw: String, line_number: usize, year: i32) -> Option<LogLine> {
    // Extract timestamp and treat everything after it as the message
    if let Ok(Some(caps)) = LOGCAT_TIMESTAMP.captures(&raw) {
        let message = caps[2].to_string();
        return parse_logcat_timestamp(&caps[1], year)
            .map(|ts| LogLine::Logcat(LogcatLogLine::new(raw, ts, message, line_number)));
    }
    None
}

fn parse_logcat_timestamp(s: &str, year: i32) -> Option<DateTime<Local>> {
    // Logcat format: MM-DD HH:MM:SS.mmm (no year!)
    let timestamp_str = format!("{year}-{s}");

    // Try parsing with year
    if let Ok(naive) = NaiveDateTime::parse_from_str(&timestamp_str, "%Y-%m-%d %H:%M:%S%.3f") {
        return naive.and_local_timezone(Local).single();
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::line::LogLineCore;
    use chrono::Datelike;

    #[test]
    fn test_threadtime_format() {
        let raw = "11-20 14:23:45.123  1234  5678 I ActivityManager: Start proc com.example.app"
            .to_string();
        let line = parse_logcat_with_year(raw, 1, 2024).expect("should parse logcat line");
        // Message is everything after the timestamp
        assert_eq!(
            line.message(),
            "1234  5678 I ActivityManager: Start proc com.example.app"
        );
    }

    #[test]
    fn test_threadtime_with_process_name() {
        let raw =
            "01-01 00:00:07.329  root     8     8 I CAM_INFO: CAM-ICP: cam_icp_mgr_process_dbg_buf"
                .to_string();
        let line = parse_logcat_with_year(raw, 1, 2024).expect("should parse logcat line");
        // Message is everything after the timestamp
        assert_eq!(
            line.message(),
            "root     8     8 I CAM_INFO: CAM-ICP: cam_icp_mgr_process_dbg_buf"
        );
    }

    #[test]
    fn test_fallback_format() {
        let raw = "11-20 14:23:45.123 Some message without tag".to_string();
        let line = parse_logcat_with_year(raw, 1, 2024).expect("should parse logcat line");
        assert_eq!(line.message(), "Some message without tag");
    }

    #[test]
    fn test_detect_year_from_header() {
        let content = "
    ========================================================
    == dumpstate: 2024-11-27 14:08:01
    ========================================================
    ";
        assert_eq!(detect_year_from_header(content), Some(2024));
    }

    #[test]
    fn test_parse_with_detected_year() {
        let raw = "11-20 14:23:45.123 Test message".to_string();
        let line = parse_logcat_with_year(raw, 1, 2023).expect("should parse logcat line");
        assert!(line.timestamp().year() == 2023);
    }

    #[test]
    fn test_is_logcat_line() {
        assert!(is_logcat_line("11-20 14:23:45.123 some message"));
        assert!(is_logcat_line("01-01 00:00:00.000 test"));
        assert!(!is_logcat_line("2024-11-20 14:23:45 generic format"));
        assert!(!is_logcat_line("just some text"));
    }
}
