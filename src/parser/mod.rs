use fancy_regex::Regex;
use std::sync::LazyLock;
use tracing::warn;

/// Format time difference with 3 significant digits and appropriate unit
pub fn format_time_diff(diff: chrono::Duration) -> String {
    let sign = if diff < chrono::Duration::zero() {
        "-"
    } else {
        "+"
    };

    // Use absolute value for calculations
    let abs_diff = if diff < chrono::Duration::zero() {
        -diff
    } else {
        diff
    };

    // Select appropriate unit based on magnitude
    let (value, unit) = if abs_diff.num_days().abs() >= 1 {
        // Days (as float for fractional days)
        let total_secs = abs_diff.num_seconds() as f64;
        (total_secs / 86400.0, "d")
    } else if abs_diff.num_hours().abs() >= 1 {
        // Hours
        let total_secs = abs_diff.num_seconds() as f64;
        (total_secs / 3600.0, "h")
    } else if abs_diff.num_minutes().abs() >= 1 {
        // Minutes
        let total_secs = abs_diff.num_seconds() as f64;
        (total_secs / 60.0, "m")
    } else if abs_diff.num_seconds().abs() >= 1 {
        // Seconds
        let total_millis = abs_diff.num_milliseconds() as f64;
        (total_millis / 1000.0, "s")
    } else if let Some(micros) = abs_diff.num_microseconds() {
        if micros.abs() >= 1000 {
            // Milliseconds
            (micros.abs() as f64 / 1000.0, "ms")
        } else if micros.abs() >= 1 {
            // Microseconds
            (micros.abs() as f64, "µs")
        } else if let Some(nanos) = abs_diff.num_nanoseconds() {
            // Nanoseconds
            (nanos.abs() as f64, "ns")
        } else {
            (0.0, "ns")
        }
    } else {
        // Fallback for very large durations
        let total_secs = abs_diff.num_seconds() as f64;
        (total_secs / 86400.0, "d")
    };

    // Format with 3 significant digits
    if value >= 100.0 {
        format!("{sign}{value:>3.0}{unit}")
    } else if value >= 10.0 {
        format!("{sign}{value:>3.1}{unit}")
    } else if value >= 1.0 {
        format!("{sign}{value:>3.2}{unit}")
    } else {
        format!("{sign}{value:>4.3}{unit}")
    }
}

// Normalization patterns
static NUMBER_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d+\b").expect("valid regex literal"));
static HEX_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b0x[0-9a-fA-F]+\b|[0-9a-fA-F]{8,}").expect("valid regex literal")
});
static UUID_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b")
        .expect("valid regex literal")
});
static URL_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://[^\s]+").expect("valid regex literal"));
static WHITESPACE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("valid regex literal"));

/// Apply regex replacement, keeping original on error (e.g., `BacktrackLimitExceeded`)
fn try_replace(text: &str, pattern: &Regex, replacement: &str) -> String {
    pattern.try_replacen(text, 0, replacement).map_or_else(
        |e| {
            warn!(
                "Regex replacement failed ({}), keeping original text (len={})",
                e,
                text.len()
            );
            text.to_string()
        },
        std::borrow::Cow::into_owned,
    )
}

/// Normalize a log message to create a template key
/// This helps identify structurally similar messages
pub fn normalize_message(message: &str) -> String {
    let mut normalized = message.to_lowercase();

    // Replace UUIDs first (before hex, since UUIDs contain hex)
    normalized = try_replace(&normalized, &UUID_PATTERN, "<UUID>");

    // Replace URLs
    normalized = try_replace(&normalized, &URL_PATTERN, "<URL>");

    // Replace hex values
    normalized = try_replace(&normalized, &HEX_PATTERN, "<HEX>");

    // Replace numbers
    normalized = try_replace(&normalized, &NUMBER_PATTERN, "<NUM>");

    // Normalize whitespace
    normalized = try_replace(&normalized, &WHITESPACE_PATTERN, " ");

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
