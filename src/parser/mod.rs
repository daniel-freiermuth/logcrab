pub mod dlt;
pub mod generic;
pub mod line;
pub mod logcat;

use lazy_static::lazy_static;
use line::LogLine;
use fancy_regex::Regex;

lazy_static! {
    // Normalization patterns
    static ref NUMBER_PATTERN: Regex = Regex::new(r"\b\d+\b").unwrap();
    static ref HEX_PATTERN: Regex = Regex::new(r"\b0x[0-9a-fA-F]+\b|[0-9a-fA-F]{8,}").unwrap();
    static ref UUID_PATTERN: Regex = Regex::new(
        r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b"
    ).unwrap();
    static ref URL_PATTERN: Regex = Regex::new(r"https?://[^\s]+").unwrap();
    static ref WHITESPACE_PATTERN: Regex = Regex::new(r"\s+").unwrap();
}

pub fn parse_line(raw: String, line_number: usize) -> Option<LogLine> {
    // Try DLT format first
    if let Some(mut line) = dlt::parse_dlt(raw.clone(), line_number) {
        // Skip lines without timestamp
        line.timestamp?;
        line.template_key = normalize_message(&line.message);
        return Some(line);
    }

    // Try logcat format
    if let Some(mut line) = logcat::parse_logcat(raw.clone(), line_number) {
        // Skip lines without timestamp
        line.timestamp?;
        line.template_key = normalize_message(&line.message);
        return Some(line);
    }

    // Fall back to generic parser
    let mut line = generic::parse_generic(raw, line_number);

    // Skip lines without timestamp
    line.timestamp?;

    line.template_key = normalize_message(&line.message);
    Some(line)
}

/// Normalize a log message to create a template key
/// This helps identify structurally similar messages
pub fn normalize_message(message: &str) -> String {
    let mut normalized = message.to_lowercase();

    // Replace UUIDs first (before hex, since UUIDs contain hex)
    normalized = UUID_PATTERN.replace_all(&normalized, "<UUID>").to_string();

    // Replace URLs
    normalized = URL_PATTERN.replace_all(&normalized, "<URL>").to_string();

    // Replace hex values
    normalized = HEX_PATTERN.replace_all(&normalized, "<HEX>").to_string();

    // Replace numbers
    normalized = NUMBER_PATTERN.replace_all(&normalized, "<NUM>").to_string();

    // Normalize whitespace
    normalized = WHITESPACE_PATTERN.replace_all(&normalized, " ").to_string();

    normalized.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_message() {
        let msg = "User 12345 logged in from 192.168.1.100";
        let normalized = normalize_message(msg);
        assert_eq!(
            normalized,
            "user <NUM> logged in from <NUM>.<NUM>.<NUM>.<NUM>"
        );
    }

    #[test]
    fn test_normalize_uuid() {
        let msg = "Request ID: 550e8400-e29b-41d4-a716-446655440000";
        let normalized = normalize_message(msg);
        assert!(normalized.contains("<UUID>"));
    }

    #[test]
    fn test_normalize_url() {
        let msg = "Fetching https://api.example.com/data";
        let normalized = normalize_message(msg);
        assert!(normalized.contains("<URL>"));
    }
}
