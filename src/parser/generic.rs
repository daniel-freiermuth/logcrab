use chrono::{DateTime, Local, Datelike};
use regex::Regex;
use lazy_static::lazy_static;
use super::line::{LogLine, LogLevel};

lazy_static! {
    // ISO 8601: 2025-11-20T14:23:45.123Z or 2025-11-20 14:23:45.123
    static ref ISO_TIMESTAMP: Regex = Regex::new(
        r"^(\d{4}-\d{2}-\d{2}[T\s]\d{2}:\d{2}:\d{2}(?:\.\d{3})?(?:Z|[+-]\d{2}:\d{2})?)"
    ).unwrap();
    
    // Common syslog: Nov 20 14:23:45
    static ref SYSLOG_TIMESTAMP: Regex = Regex::new(
        r"^([A-Z][a-z]{2}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2})"
    ).unwrap();
    
    // Timestamp with milliseconds: [2025-11-20 14:23:45.123]
    static ref BRACKETED_TIMESTAMP: Regex = Regex::new(
        r"^\[(\d{4}-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}(?:\.\d{3})?)\]"
    ).unwrap();
    
    // Log level detection: ERROR, WARN, INFO, DEBUG, etc.
    static ref LOG_LEVEL: Regex = Regex::new(
        r"\b(VERBOSE|DEBUG|INFO|WARN(?:ING)?|ERROR?|FATAL)\b"
    ).unwrap();
}

pub fn parse_generic(raw: String, line_number: usize) -> LogLine {
    let mut line = LogLine::new(raw.clone(), line_number);
    let mut remaining = raw.as_str();
    
    // Try to extract timestamp
    if let Some(caps) = ISO_TIMESTAMP.captures(remaining) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(&caps[1]) {
            line.timestamp = Some(dt.with_timezone(&Local));
            remaining = &remaining[caps[0].len()..].trim_start();
        } else if let Ok(dt) = DateTime::parse_from_str(&caps[1], "%Y-%m-%d %H:%M:%S%.3f") {
            line.timestamp = Some(dt.with_timezone(&Local));
            remaining = &remaining[caps[0].len()..].trim_start();
        }
    } else if let Some(caps) = BRACKETED_TIMESTAMP.captures(remaining) {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&caps[1], "%Y-%m-%d %H:%M:%S%.3f") {
            line.timestamp = Some(DateTime::from_naive_utc_and_offset(naive, *Local::now().offset()));
            remaining = &remaining[caps[0].len()..].trim_start();
        }
    } else if let Some(caps) = SYSLOG_TIMESTAMP.captures(remaining) {
        // Parse syslog format (assuming current year)
        let current_year = Local::now().year();
        let ts_str = format!("{} {}", current_year, &caps[1]);
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&ts_str, "%Y %b %d %H:%M:%S") {
            line.timestamp = Some(DateTime::from_naive_utc_and_offset(naive, *Local::now().offset()));
            remaining = &remaining[caps[0].len()..].trim_start();
        }
    }
    
    // Try to extract log level
    if let Some(caps) = LOG_LEVEL.captures(remaining) {
        line.level = LogLevel::from_str(&caps[1]);
        remaining = &remaining[caps[0].len()..].trim_start();
    }
    
    // Everything else is the message
    line.message = remaining.to_string();
    
    // If message is still empty, use the whole raw line
    if line.message.is_empty() {
        line.message = raw.clone();
    }
    
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iso_timestamp() {
        let raw = "2025-11-20T14:23:45.123Z ERROR Connection failed".to_string();
        let line = parse_generic(raw, 1);
        assert!(line.timestamp.is_some());
        assert_eq!(line.level, LogLevel::Error);
        assert_eq!(line.message, "Connection failed");
    }

    #[test]
    fn test_syslog_format() {
        let raw = "Nov 20 14:23:45 INFO Application started".to_string();
        let line = parse_generic(raw, 1);
        assert!(line.timestamp.is_some());
        assert_eq!(line.level, LogLevel::Info);
    }
}
