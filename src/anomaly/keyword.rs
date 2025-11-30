use crate::anomaly::scorer::AnomalyScorer;
use crate::parser::line::LogLine;
use fancy_regex::Regex;
use lazy_static::lazy_static;

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
        if ERROR_KEYWORDS.is_match(message).unwrap_or(false) {
            score = score.max(1.0);
        }

        // FAILURE keywords = high priority
        if FAILURE_KEYWORDS.is_match(message).unwrap_or(false) {
            score = score.max(0.8);
        }

        // WARNING keywords = medium priority
        if WARNING_KEYWORDS.is_match(message).unwrap_or(false) {
            score = score.max(0.6);
        }

        // ISSUE keywords = lower priority
        if ISSUE_KEYWORDS.is_match(message).unwrap_or(false) {
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
