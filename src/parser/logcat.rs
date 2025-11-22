use super::line::LogLine;
use chrono::{DateTime, Datelike, Local, NaiveDateTime};
use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    // Just extract timestamp - everything after it is the message
    static ref LOGCAT_TIMESTAMP: Regex = Regex::new(
        r"^(\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}\.\d{3})\s+(.*)$"
    ).unwrap();
}

pub fn parse_logcat(raw: String, line_number: usize) -> Option<LogLine> {
    // Extract timestamp and treat everything after it as the message
    if let Some(caps) = LOGCAT_TIMESTAMP.captures(&raw) {
        let message = caps[2].to_string();
        let timestamp = parse_logcat_timestamp(&caps[1]);
        let mut line = LogLine::new(raw, line_number);
        line.message = message;
        line.timestamp = timestamp;
        return Some(line);
    }

    None
}

fn parse_logcat_timestamp(s: &str) -> Option<DateTime<Local>> {
    // Logcat format: MM-DD HH:MM:SS.mmm (no year!)
    // We'll assume current year
    let current_year = Local::now().year();
    let timestamp_str = format!("{}-{}", current_year, s);

    // Try parsing with year
    if let Ok(naive) = NaiveDateTime::parse_from_str(&timestamp_str, "%Y-%m-%d %H:%M:%S%.3f") {
        return Some(DateTime::from_naive_utc_and_offset(
            naive,
            *Local::now().offset(),
        ));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_threadtime_format() {
        let raw = "11-20 14:23:45.123  1234  5678 I ActivityManager: Start proc com.example.app"
            .to_string();
        let line = parse_logcat(raw, 1).unwrap();
        assert!(line.timestamp.is_some());
        assert_eq!(line.message, "Start proc com.example.app");
    }

    #[test]
    fn test_threadtime_with_process_name() {
        let raw =
            "01-01 00:00:07.329  root     8     8 I CAM_INFO: CAM-ICP: cam_icp_mgr_process_dbg_buf"
                .to_string();
        let line = parse_logcat(raw, 1).unwrap();
        assert!(line.timestamp.is_some());
        assert_eq!(line.message, "CAM-ICP: cam_icp_mgr_process_dbg_buf");
    }

    #[test]
    fn test_fallback_format() {
        let raw = "11-20 14:23:45.123 Some message without tag".to_string();
        let line = parse_logcat(raw, 1).unwrap();
        assert!(line.timestamp.is_some());
        assert_eq!(line.message, "Some message without tag");
    }
}
