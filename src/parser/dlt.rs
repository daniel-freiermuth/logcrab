use super::line::LogLine;
use chrono::{DateTime, Local, NaiveDateTime};
use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    // DLT format examples:
    // 2024/11/20 14:23:45.123456 12345 ECU1 APID CTID log info V 1 [Message text]
    // Timestamp can also be in format: 14:23:45.123456 or other variations
    static ref DLT_PATTERN: Regex = Regex::new(
        r"^(\d{4}/\d{2}/\d{2}\s+\d{2}:\d{2}:\d{2}\.\d+|\d{2}:\d{2}:\d{2}\.\d+)\s+.*?\[(.*?)\]\s*$"
    ).unwrap();

    // Alternative DLT format with timestamp at the beginning
    static ref DLT_SIMPLE: Regex = Regex::new(
        r"^(\d{4}/\d{2}/\d{2}\s+\d{2}:\d{2}:\d{2}\.\d+)\s+(.*)$"
    ).unwrap();

    // Time-only format (HH:MM:SS.microseconds)
    static ref DLT_TIME_ONLY: Regex = Regex::new(
        r"^(\d{2}:\d{2}:\d{2}\.\d+)\s+(.*)$"
    ).unwrap();
}

pub fn parse_dlt(raw: String, line_number: usize) -> Option<LogLine> {
    // Try pattern with square brackets for message
    if let Some(caps) = DLT_PATTERN.captures(&raw) {
        let timestamp = parse_dlt_timestamp(&caps[1]);
        let message = caps[2].to_string();
        let mut line = LogLine::new(raw, line_number);
        line.message = message;
        line.timestamp = timestamp;
        return Some(line);
    }

    // Try simple pattern with full timestamp
    if let Some(caps) = DLT_SIMPLE.captures(&raw) {
        let timestamp = parse_dlt_timestamp(&caps[1]);
        let message = caps[2].to_string();
        let mut line = LogLine::new(raw, line_number);
        line.message = message;
        line.timestamp = timestamp;
        return Some(line);
    }

    // Try time-only pattern
    if let Some(caps) = DLT_TIME_ONLY.captures(&raw) {
        let timestamp = parse_dlt_timestamp(&caps[1]);
        let message = caps[2].to_string();
        let mut line = LogLine::new(raw, line_number);
        line.message = message;
        line.timestamp = timestamp;
        return Some(line);
    }

    None
}

fn parse_dlt_timestamp(s: &str) -> Option<DateTime<Local>> {
    // Try full timestamp format: 2024/11/20 14:23:45.123456
    if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y/%m/%d %H:%M:%S%.f") {
        return Some(DateTime::from_naive_utc_and_offset(
            naive,
            *Local::now().offset(),
        ));
    }

    // Try time-only format: 14:23:45.123456 (assume today's date)
    if let Ok(time) = chrono::NaiveTime::parse_from_str(s, "%H:%M:%S%.f") {
        let today = Local::now().date_naive();
        let naive_dt = today.and_time(time);
        return Some(DateTime::from_naive_utc_and_offset(
            naive_dt,
            *Local::now().offset(),
        ));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dlt_with_brackets() {
        let raw =
            "2024/11/20 14:23:45.123456 12345 ECU1 APID CTID log info V 1 [Application started]"
                .to_string();
        let line = parse_dlt(raw, 1).unwrap();
        assert!(line.timestamp.is_some());
        assert_eq!(line.message, "Application started");
    }

    #[test]
    fn test_dlt_simple() {
        let raw = "2024/11/20 14:23:45.123456 Application started successfully".to_string();
        let line = parse_dlt(raw, 1).unwrap();
        assert!(line.timestamp.is_some());
        assert_eq!(line.message, "Application started successfully");
    }

    #[test]
    fn test_dlt_time_only() {
        let raw = "14:23:45.123456 System initialization complete".to_string();
        let line = parse_dlt(raw, 1).unwrap();
        assert!(line.timestamp.is_some());
        assert_eq!(line.message, "System initialization complete");
    }
}
