use chrono::{DateTime, Local, NaiveDateTime, Datelike};
use regex::Regex;
use lazy_static::lazy_static;
use super::line::{LogLine, LogLevel};

lazy_static! {
    // Android logcat threadtime format: MM-DD HH:MM:SS.mmm  PID  TID L TAG: message
    static ref LOGCAT_THREADTIME: Regex = Regex::new(
        r"^(\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}\.\d{3})\s+(\d+)\s+(\d+)\s+([VDIWEF])\s+([^:]+):\s*(.*)$"
    ).unwrap();
    
    // Android logcat time format: MM-DD HH:MM:SS.mmm L/TAG(PID): message
    static ref LOGCAT_TIME: Regex = Regex::new(
        r"^(\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}\.\d{3})\s+([VDIWEF])/([^(]+)\(\s*(\d+)\):\s*(.*)$"
    ).unwrap();
    
    // Android logcat brief format: L/TAG(PID): message
    static ref LOGCAT_BRIEF: Regex = Regex::new(
        r"^([VDIWEF])/([^(]+)\(\s*(\d+)\):\s*(.*)$"
    ).unwrap();
    
    // Android logcat long format with date: [ MM-DD HH:MM:SS.mmm PID: TID L/TAG ]
    static ref LOGCAT_LONG: Regex = Regex::new(
        r"^\[\s*(\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}\.\d{3})\s+(\d+):\s*(\d+)\s+([VDIWEF])/([^\]]+)\s*\]\s*(.*)$"
    ).unwrap();
}

pub fn parse_logcat(raw: String, line_number: usize) -> Option<LogLine> {
    let mut line = LogLine::new(raw.clone(), line_number);
    
    // Try threadtime format first (most detailed)
    if let Some(caps) = LOGCAT_THREADTIME.captures(&raw) {
        line.timestamp = parse_logcat_timestamp(&caps[1]);
        line.pid = caps[2].parse().ok();
        line.tid = caps[3].parse().ok();
        line.level = LogLevel::from_char(caps[4].chars().next().unwrap());
        line.tag = Some(caps[5].trim().to_string());
        line.message = caps[6].to_string();
        return Some(line);
    }
    
    // Try time format
    if let Some(caps) = LOGCAT_TIME.captures(&raw) {
        line.timestamp = parse_logcat_timestamp(&caps[1]);
        line.level = LogLevel::from_char(caps[2].chars().next().unwrap());
        line.tag = Some(caps[3].trim().to_string());
        line.pid = caps[4].parse().ok();
        line.message = caps[5].to_string();
        return Some(line);
    }
    
    // Try long format
    if let Some(caps) = LOGCAT_LONG.captures(&raw) {
        line.timestamp = parse_logcat_timestamp(&caps[1]);
        line.pid = caps[2].parse().ok();
        line.tid = caps[3].parse().ok();
        line.level = LogLevel::from_char(caps[4].chars().next().unwrap());
        line.tag = Some(caps[5].trim().to_string());
        line.message = caps[6].to_string();
        return Some(line);
    }
    
    // Try brief format
    if let Some(caps) = LOGCAT_BRIEF.captures(&raw) {
        line.level = LogLevel::from_char(caps[1].chars().next().unwrap());
        line.tag = Some(caps[2].trim().to_string());
        line.pid = caps[3].parse().ok();
        line.message = caps[4].to_string();
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
        return Some(DateTime::from_naive_utc_and_offset(naive, *Local::now().offset()));
    }
    
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_threadtime_format() {
        let raw = "11-20 14:23:45.123  1234  5678 I ActivityManager: Start proc com.example.app".to_string();
        let line = parse_logcat(raw, 1).unwrap();
        assert_eq!(line.pid, Some(1234));
        assert_eq!(line.tid, Some(5678));
        assert_eq!(line.level, LogLevel::Info);
        assert_eq!(line.tag.as_deref(), Some("ActivityManager"));
    }

    #[test]
    fn test_brief_format() {
        let raw = "I/ActivityManager(1234): Start proc".to_string();
        let line = parse_logcat(raw, 1).unwrap();
        assert_eq!(line.pid, Some(1234));
        assert_eq!(line.level, LogLevel::Info);
        assert_eq!(line.tag.as_deref(), Some("ActivityManager"));
    }
}
