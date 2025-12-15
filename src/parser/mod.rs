pub mod dlt;
pub mod generic;
pub mod line;
pub mod logcat;

use chrono::Datelike;
use fancy_regex::Regex;
use std::sync::LazyLock;

// Normalization patterns
static NUMBER_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d+\b").unwrap());
static HEX_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b0x[0-9a-fA-F]+\b|[0-9a-fA-F]{8,}").unwrap());
static UUID_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b")
        .unwrap()
});
static URL_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://[^\s]+").unwrap());
static WHITESPACE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

/// Detected log format for a file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// Android bugreport file (contains dumpstate header with year)
    Bugreport { year: i32 },
    /// Pure logcat output (MM-DD HH:MM:SS.mmm, no year - uses current year)
    Logcat { year: i32 },
    /// Generic format (various timestamp formats with year)
    Generic,
}

/// Detect the log format by sampling the first lines of content
/// Returns the detected format, or Generic as fallback
pub fn detect_format(content: &str) -> LogFormat {
    // First check for bugreport dumpstate header - this indicates a bugreport file
    if let Some(year) = logcat::detect_year_from_header(content) {
        log::info!("Detected bugreport dumpstate header with year {year}");
        return LogFormat::Bugreport { year };
    }

    // Otherwise, sample lines to detect pure logcat format
    let mut logcat_matches = 0;
    let mut total_checked = 0;

    for line in content.lines().take(500) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Check if line matches logcat timestamp pattern (MM-DD HH:MM:SS.mmm)
        if logcat::is_logcat_line(trimmed) {
            logcat_matches += 1;
        }
        total_checked += 1;

        // Stop after finding enough logcat lines (at least 10 matches)
        if logcat_matches >= 10 {
            break;
        }

        // Also stop if we've checked too many without finding logcat
        if total_checked >= 100 && logcat_matches == 0 {
            break;
        }
    }

    // If we found logcat lines, treat as pure logcat (use current year)
    if logcat_matches > 0 {
        let year = chrono::Local::now().year();
        log::info!(
            "Detected {logcat_matches} logcat lines, using logcat format with current year {year}"
        );
        return LogFormat::Logcat { year };
    }

    log::info!("Using generic log format");
    LogFormat::Generic
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
