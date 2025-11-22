use crate::anomaly::scorer::AnomalyScorer;
use crate::parser::line::LogLine;
use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    // Keywords that indicate potential issues (case-insensitive)
    static ref ERROR_KEYWORDS: Regex = Regex::new(
        r"(?i)\b(error|err|exception|fatal|critical|crash|panic|abort)\b"
    ).unwrap();

    static ref WARNING_KEYWORDS: Regex = Regex::new(
        r"(?i)\b(warn|warning|caution|alert)\b"
    ).unwrap();

    static ref FAILURE_KEYWORDS: Regex = Regex::new(
        r"(?i)\b(fail|failed|failure|unsuccessful|denied|rejected|timeout|timed out)\b"
    ).unwrap();

    static ref ISSUE_KEYWORDS: Regex = Regex::new(
        r"(?i)\b(issue|problem|unable|cannot|can't|couldn't|invalid|illegal|unexpected)\b"
    ).unwrap();
}

/// Keyword-based scorer - detects important keywords in messages
/// Scores based on severity of detected keywords
pub struct KeywordScorer {
    // No state needed - stateless keyword detection
}

impl KeywordScorer {
    pub fn new() -> Self {
        KeywordScorer {}
    }

    fn score_message(message: &str) -> f64 {
        let mut score: f64 = 0.0;

        // ERROR keywords = highest priority
        if ERROR_KEYWORDS.is_match(message) {
            score = score.max(1.0);
        }

        // FAILURE keywords = high priority
        if FAILURE_KEYWORDS.is_match(message) {
            score = score.max(0.8);
        }

        // WARNING keywords = medium priority
        if WARNING_KEYWORDS.is_match(message) {
            score = score.max(0.6);
        }

        // ISSUE keywords = lower priority
        if ISSUE_KEYWORDS.is_match(message) {
            score = score.max(0.4);
        }

        score
    }
}

impl AnomalyScorer for KeywordScorer {
    fn score(&mut self, line: &LogLine) -> f64 {
        Self::score_message(&line.message)
    }

    fn update(&mut self, _line: &LogLine) {
        // Stateless - no updates needed
    }
}

impl Default for KeywordScorer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_keywords() {
        let mut scorer = KeywordScorer::new();

        let error_line = LogLine::new("Database connection error occurred".to_string(), 1);
        assert_eq!(scorer.score(&error_line), 1.0);

        let exception_line = LogLine::new("NullPointerException thrown".to_string(), 2);
        assert_eq!(scorer.score(&exception_line), 1.0);
    }

    #[test]
    fn test_warning_keywords() {
        let mut scorer = KeywordScorer::new();

        let warning_line = LogLine::new("Warning: disk space low".to_string(), 1);
        assert_eq!(scorer.score(&warning_line), 0.6);
    }

    #[test]
    fn test_failure_keywords() {
        let mut scorer = KeywordScorer::new();

        let fail_line = LogLine::new("Authentication failed for user".to_string(), 1);
        assert_eq!(scorer.score(&fail_line), 0.8);

        let timeout_line = LogLine::new("Request timed out after 30s".to_string(), 2);
        assert_eq!(scorer.score(&timeout_line), 0.8);
    }

    #[test]
    fn test_normal_message() {
        let mut scorer = KeywordScorer::new();

        let normal_line = LogLine::new("User logged in successfully".to_string(), 1);
        assert_eq!(scorer.score(&normal_line), 0.0);
    }

    #[test]
    fn test_case_insensitive() {
        let mut scorer = KeywordScorer::new();

        let upper_line = LogLine::new("CONNECTION ERROR".to_string(), 1);
        assert_eq!(scorer.score(&upper_line), 1.0);

        let mixed_line = LogLine::new("WaRnInG: something happened".to_string(), 2);
        assert_eq!(scorer.score(&mixed_line), 0.6);
    }
}
