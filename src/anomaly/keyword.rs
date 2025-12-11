use crate::anomaly::scorer::AnomalyScorer;
use crate::parser::line::LogLine;
use fancy_regex::Regex;
use std::sync::LazyLock;

// Keywords that indicate potential issues (case-insensitive)
static ERROR_KEYWORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(error|err|exception|fatal|critical|crash|panic|abort)\b").unwrap()
});

static WARNING_KEYWORDS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(warn|warning|caution|alert)\b").unwrap());

static FAILURE_KEYWORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(fail|failed|failure|unsuccessful|denied|rejected|timeout|timed out)\b")
        .unwrap()
});

static ISSUE_KEYWORDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(issue|problem|unable|cannot|can't|couldn't|invalid|illegal|unexpected)\b")
        .unwrap()
});

/// Keyword-based scorer - detects important keywords in messages
/// Scores based on severity of detected keywords
pub struct KeywordScorer {
    // No state needed - stateless keyword detection
}

impl KeywordScorer {
    pub const fn new() -> Self {
        Self {}
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
