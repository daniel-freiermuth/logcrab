use super::line::{GenericLogLine, LogLine};
use chrono::{DateTime, Datelike, Local, TimeZone};
use fancy_regex::Regex;
use std::sync::LazyLock;

// ISO 8601: 2025-11-20T14:23:45.123Z or 2025-11-20 14:23:45.123
// Supports timezone offsets with or without colon: +01:00 or +0100
static ISO_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4}-\d{2}-\d{2}[T\s]\d{2}:\d{2}:\d{2}(?:\.\d{3})?(?:Z|[+-]\d{2}:?\d{2})?)")
        .expect("valid regex literal")
});

// Alternative date format with hyphens: 2025-11-26-09:58:05
static HYPHENATED_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4}-\d{2}-\d{2}-\d{2}:\d{2}:\d{2}(?:\.\d{3})?)").expect("valid regex literal")
});

// Common syslog: Nov 20 14:23:45 or Nov 20 14:23:45.123
static SYSLOG_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([A-Z][a-z]{2}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2}(?:\.\d{3})?)")
        .expect("valid regex literal")
});

// Timestamp with milliseconds: [2025-11-20 14:23:45.123]
static BRACKETED_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\[(\d{4}-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}(?:\.\d{3})?)\]")
        .expect("valid regex literal")
});

// Logcat format: MM-DD HH:MM:SS.mmm
static LOGCAT_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}\.\d{3})").expect("valid regex literal")
});

pub fn parse_generic(raw: String, line_number: usize) -> Option<LogLine> {
    let mut timestamp = None;
    let mut remaining = raw.as_str();

    // Try to extract timestamp - try various formats
    if let Ok(Some(caps)) = HYPHENATED_TIMESTAMP.captures(remaining) {
        // Format: 2025-11-26-09:58:05 -> convert to "2025-11-26 09:58:05"
        let date_part = &caps[1][..10]; // "2025-11-26"
        let time_part = &caps[1][11..]; // "09:58:05" or "09:58:05.123"
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

        // Normalize timezone offset: convert +0100 to +01:00 for RFC3339 compatibility
        let normalized_ts = if let Some(tz_pos) = ts_str.rfind(|c| c == '+' || c == '-') {
            let (datetime_part, tz_part) = ts_str.split_at(tz_pos);
            // Check if timezone is in format +0100 (5 chars: sign + 4 digits, no colon)
            if tz_part.len() == 5 && !tz_part.contains(':') {
                // Insert colon: +0100 -> +01:00
                format!(
                    "{}{}:{}",
                    datetime_part.replace(' ', "T"),
                    &tz_part[..3],
                    &tz_part[3..]
                )
            } else {
                ts_str.replace(' ', "T")
            }
        } else {
            ts_str.replace(' ', "T")
        };

        if let Ok(dt) = DateTime::parse_from_rfc3339(&normalized_ts) {
            timestamp = Some(dt.with_timezone(&Local));
            remaining = remaining[caps[0].len()..].trim_start();
        } else if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(&caps[1], "%Y-%m-%d %H:%M:%S%.3f")
        {
            // Handle timestamps with milliseconds but no timezone
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        } else if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(&caps[1], "%Y-%m-%d %H:%M:%S")
        {
            // Try without milliseconds
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
            // Try without milliseconds
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        }
    } else if let Ok(Some(caps)) = LOGCAT_TIMESTAMP.captures(remaining) {
        // Logcat format: MM-DD HH:MM:SS.mmm (no year!)
        let current_year = Local::now().year();
        let timestamp_str = format!("{}-{}", current_year, &caps[1]);
        if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(&timestamp_str, "%Y-%m-%d %H:%M:%S%.3f")
        {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        }
    } else if let Ok(Some(caps)) = SYSLOG_TIMESTAMP.captures(remaining) {
        // Parse syslog format (assuming current year)
        let current_year = Local::now().year();
        let ts_str = format!("{} {}", current_year, &caps[1]);
        // Try with milliseconds first, then without
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&ts_str, "%Y %b %d %H:%M:%S%.3f") {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        } else if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(&ts_str, "%Y %b %d %H:%M:%S")
        {
            timestamp = Local.from_local_datetime(&naive).single();
            remaining = remaining[caps[0].len()..].trim_start();
        }
    }

    // Everything after timestamp is the message (or use whole raw if no timestamp found)
    let message = if remaining.is_empty() {
        raw.clone()
    } else {
        remaining.to_string()
    };

    timestamp.map(|ts| LogLine::Generic(GenericLogLine::new(raw, ts, message, line_number)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::line::LogLineCore;

    #[test]
    fn test_iso_timestamp() {
        let raw = "2025-11-20T14:23:45.123Z ERROR Connection failed".to_string();
        let line = parse_generic(raw, 1);
        assert_eq!(
            line.expect("should parse ISO timestamp").message(),
            "ERROR Connection failed"
        );
    }

    #[test]
    fn test_hyphenated_timestamp() {
        let raw = "2025-11-26-09:58:05 , [402.037] ,cnss: fatal: SMMU fault happened with IOVA 0x0"
            .to_string();
        let line = parse_generic(raw, 1).expect("should parse hyphenated timestamp");
        assert_eq!(
            line.message(),
            ", [402.037] ,cnss: fatal: SMMU fault happened with IOVA 0x0"
        );
        assert_eq!(
            line.timestamp().format("%Y-%m-%d %H:%M:%S").to_string(),
            "2025-11-26 09:58:05"
        );
    }

    #[test]
    fn test_syslog_format() {
        let raw = "Nov 20 14:23:45 INFO Application started".to_string();
        let line = parse_generic(raw, 1);
        assert_eq!(
            line.expect("should parse syslog format").message(),
            "INFO Application started"
        );
    }

    #[test]
    fn test_iso_timestamp_with_space_and_milliseconds() {
        // ISO 8601 with space separator and milliseconds (no timezone)
        let raw = "2025-11-20 14:23:45.123 ERROR Connection failed".to_string();
        let line =
            parse_generic(raw, 1).expect("Should parse ISO timestamp with space and milliseconds");
        assert_eq!(line.message(), "ERROR Connection failed");
        assert_eq!(
            line.timestamp().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            "2025-11-20 14:23:45.123"
        );
    }

    #[test]
    fn test_iso_timestamp_with_space_no_milliseconds() {
        // ISO 8601 with space separator, no milliseconds
        let raw = "2025-11-20 14:23:45 WARN Timeout occurred".to_string();
        let line =
            parse_generic(raw, 1).expect("Should parse ISO timestamp with space, no milliseconds");
        assert_eq!(line.message(), "WARN Timeout occurred");
        assert_eq!(
            line.timestamp().format("%Y-%m-%d %H:%M:%S").to_string(),
            "2025-11-20 14:23:45"
        );
    }

    #[test]
    fn test_bracketed_timestamp_with_milliseconds() {
        let raw = "[2025-11-20 14:23:45.123] DEBUG Processing request".to_string();
        let line =
            parse_generic(raw, 1).expect("Should parse bracketed timestamp with milliseconds");
        assert_eq!(line.message(), "DEBUG Processing request");
        assert_eq!(
            line.timestamp().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            "2025-11-20 14:23:45.123"
        );
    }

    #[test]
    fn test_bracketed_timestamp_without_milliseconds() {
        let raw = "[2025-11-20 14:23:45] INFO Service started".to_string();
        let line =
            parse_generic(raw, 1).expect("Should parse bracketed timestamp without milliseconds");
        assert_eq!(line.message(), "INFO Service started");
        assert_eq!(
            line.timestamp().format("%Y-%m-%d %H:%M:%S").to_string(),
            "2025-11-20 14:23:45"
        );
    }

    #[test]
    fn test_logcat_timestamp_format() {
        // Logcat format: MM-DD HH:MM:SS.mmm (no year)
        let raw = "11-20 14:23:45.123 E/ActivityManager: Process crashed".to_string();
        let line = parse_generic(raw, 1).expect("Should parse logcat timestamp format");
        assert_eq!(line.message(), "E/ActivityManager: Process crashed");
        // Year is assumed to be current year
        let current_year = Local::now().year();
        assert_eq!(
            line.timestamp().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            format!("{current_year}-11-20 14:23:45.123")
        );
    }

    #[test]
    fn test_syslog_timestamp_with_milliseconds() {
        // Syslog format with milliseconds: Feb 03 23:26:34.864
        let raw =
            "Feb 03 23:26:34.864 qcgpio[gpio_drv.c:1222]: dalcfg_query_item_name gpio_driver done"
                .to_string();
        let line = parse_generic(raw, 1).expect("Should parse syslog timestamp with milliseconds");
        assert_eq!(
            line.message(),
            "qcgpio[gpio_drv.c:1222]: dalcfg_query_item_name gpio_driver done"
        );
        // Year is assumed to be current year
        let current_year = Local::now().year();
        assert_eq!(
            line.timestamp().format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            format!("{current_year}-02-03 23:26:34.864")
        );
    }

    #[test]
    fn test_iso_timestamp_with_timezone_offset() {
        // ISO 8601 with timezone offset
        let raw = "2025-11-20T14:23:45+05:30 INFO Server running".to_string();
        let line = parse_generic(raw, 1).expect("Should parse ISO timestamp with timezone offset");
        assert_eq!(line.message(), "INFO Server running");
    }

    #[test]
    fn test_iso_timestamp_with_ms_and_timezone_offset() {
        // ISO 8601 with milliseconds and timezone offset with colon
        let raw = "2026-02-05T09:20:23.638+01:00 INFO Server started".to_string();
        let line = parse_generic(raw, 1)
            .expect("Should parse ISO timestamp with milliseconds and timezone offset");
        assert_eq!(line.message(), "INFO Server started");
        assert_eq!(
            line.timestamp().format("%Y-%m-%d").to_string(),
            "2026-02-05"
        );
    }

    #[test]
    fn test_iso_timestamp_with_timezone_offset_no_colon() {
        // ISO 8601 with timezone offset without colon (e.g., +0100 instead of +01:00)
        let raw = "2026-02-05T09:20:23+0100 INFO Application started".to_string();
        let line = parse_generic(raw, 1)
            .expect("Should parse ISO timestamp with timezone offset without colon");
        assert_eq!(line.message(), "INFO Application started");
        // Verify the timestamp is correctly parsed
        assert_eq!(
            line.timestamp().format("%Y-%m-%d").to_string(),
            "2026-02-05"
        );
    }

    #[test]
    fn test_iso_timestamp_with_negative_timezone_no_colon() {
        // ISO 8601 with negative timezone offset without colon
        let raw = "2026-02-10T15:30:00-0500 WARN Connection timeout".to_string();
        let line = parse_generic(raw, 1)
            .expect("Should parse ISO timestamp with negative timezone offset without colon");
        assert_eq!(line.message(), "WARN Connection timeout");
    }
}
