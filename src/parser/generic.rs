use super::line::LogLine;
use chrono::{DateTime, Datelike, Local, TimeZone};
use fancy_regex::Regex;
use std::sync::LazyLock;

// ISO 8601: 2025-11-20T14:23:45.123Z or 2025-11-20 14:23:45.123
static ISO_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4}-\d{2}-\d{2}[T\s]\d{2}:\d{2}:\d{2}(?:\.\d{3})?(?:Z|[+-]\d{2}:\d{2})?)")
        .unwrap()
});

// Alternative date format with hyphens: 2025-11-26-09:58:05
static HYPHENATED_TIMESTAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{4}-\d{2}-\d{2}-\d{2}:\d{2}:\d{2}(?:\.\d{3})?)").unwrap());

// Common syslog: Nov 20 14:23:45
static SYSLOG_TIMESTAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Z][a-z]{2}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2})").unwrap());

// Timestamp with milliseconds: [2025-11-20 14:23:45.123]
static BRACKETED_TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\[(\d{4}-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}(?:\.\d{3})?)\]").unwrap()
});

// Logcat format: MM-DD HH:MM:SS.mmm
static LOGCAT_TIMESTAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}\.\d{3})").unwrap());

pub fn parse_generic(raw: String, line_number: usize) -> Option<LogLine> {
    let mut timestamp = None;
    let mut remaining = raw.as_str();

    // Try to extract timestamp - try various formats
    if let Ok(Some(caps)) = HYPHENATED_TIMESTAMP.captures(remaining) {
        // Format: 2025-11-26-09:58:05 -> convert to "2025-11-26 09:58:05"
        let date_part = &caps[1][..10]; // "2025-11-26"
        let time_part = &caps[1][11..]; // "09:58:05" or "09:58:05.123"
        let normalized = format!("{} {}", date_part, time_part);

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
        if let Ok(dt) = DateTime::parse_from_rfc3339(&caps[1]) {
            timestamp = Some(dt.with_timezone(&Local));
            remaining = remaining[caps[0].len()..].trim_start();
        } else if let Ok(dt) = DateTime::parse_from_str(&caps[1], "%Y-%m-%d %H:%M:%S%.3f") {
            timestamp = Some(dt.with_timezone(&Local));
            remaining = remaining[caps[0].len()..].trim_start();
        } else if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(&caps[1], "%Y-%m-%d %H:%M:%S")
        {
            // Try without milliseconds
            timestamp = Some(DateTime::from_naive_utc_and_offset(
                naive,
                *Local::now().offset(),
            ));
            remaining = remaining[caps[0].len()..].trim_start();
        }
    } else if let Ok(Some(caps)) = BRACKETED_TIMESTAMP.captures(remaining) {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&caps[1], "%Y-%m-%d %H:%M:%S%.3f")
        {
            timestamp = Some(DateTime::from_naive_utc_and_offset(
                naive,
                *Local::now().offset(),
            ));
            remaining = remaining[caps[0].len()..].trim_start();
        } else if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(&caps[1], "%Y-%m-%d %H:%M:%S")
        {
            // Try without milliseconds
            timestamp = Some(DateTime::from_naive_utc_and_offset(
                naive,
                *Local::now().offset(),
            ));
            remaining = remaining[caps[0].len()..].trim_start();
        }
    } else if let Ok(Some(caps)) = LOGCAT_TIMESTAMP.captures(remaining) {
        // Logcat format: MM-DD HH:MM:SS.mmm (no year!)
        let current_year = Local::now().year();
        let timestamp_str = format!("{}-{}", current_year, &caps[1]);
        if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(&timestamp_str, "%Y-%m-%d %H:%M:%S%.3f")
        {
            timestamp = Some(DateTime::from_naive_utc_and_offset(
                naive,
                *Local::now().offset(),
            ));
            remaining = remaining[caps[0].len()..].trim_start();
        }
    } else if let Ok(Some(caps)) = SYSLOG_TIMESTAMP.captures(remaining) {
        // Parse syslog format (assuming current year)
        let current_year = Local::now().year();
        let ts_str = format!("{} {}", current_year, &caps[1]);
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&ts_str, "%Y %b %d %H:%M:%S") {
            timestamp = Some(DateTime::from_naive_utc_and_offset(
                naive,
                *Local::now().offset(),
            ));
            remaining = remaining[caps[0].len()..].trim_start();
        }
    }

    // Everything after timestamp is the message (or use whole raw if no timestamp found)
    let message = if remaining.is_empty() {
        raw.clone()
    } else {
        remaining.to_string()
    };

    timestamp.map(|ts| LogLine::new(raw, line_number, message, ts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iso_timestamp() {
        let raw = "2025-11-20T14:23:45.123Z ERROR Connection failed".to_string();
        let line = parse_generic(raw, 1);
        assert_eq!(line.unwrap().message, "ERROR Connection failed");
    }

    #[test]
    fn test_hyphenated_timestamp() {
        let raw = "2025-11-26-09:58:05 , [402.037] ,cnss: fatal: SMMU fault happened with IOVA 0x0"
            .to_string();
        let line = parse_generic(raw, 1);
        assert!(line.is_some());
        let line = line.unwrap();
        assert_eq!(
            line.message,
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
        let line = parse_generic(raw, 1);
        assert_eq!(line.unwrap().message, "INFO Application started");
    }
}
