use std::collections::HashMap;

use crate::anomaly::scorer::AnomalyScorer;
use crate::core::log_store::LogLine;

/// Scores based on template rarity (inverse frequency)
pub struct RarityScorer {
    template_counts: HashMap<String, u32>,
    total_lines: u32,
}

impl RarityScorer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            template_counts: HashMap::new(),
            total_lines: 0,
        }
    }
}

impl AnomalyScorer for RarityScorer {
    fn score(&mut self, line: &LogLine) -> f64 {
        if self.total_lines == 0 {
            return 1.0; // First line is always novel
        }

        let template_key = line.template_key();
        let count = self
            .template_counts
            .get(&template_key)
            .copied()
            .unwrap_or(0);

        if count == 0 {
            // Never seen before - highly anomalous
            return 1.0;
        }

        // Inverse frequency: rare templates get higher scores
        // Simple inverse: score = 1 - (count / total)
        // But scale it so even moderately rare items get decent scores
        let frequency = f64::from(count) / f64::from(self.total_lines);

        // Use a power function to make scoring more aggressive for rare items
        // score = (1 - frequency)^0.5 gives better distribution
        let score = (1.0 - frequency).sqrt();

        score.clamp(0.0, 1.0)
    }

    fn update(&mut self, line: &LogLine) {
        *self.template_counts.entry(line.template_key()).or_insert(0) += 1;
        self.total_lines += 1;
    }
}

impl Default for RarityScorer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;

    fn make_line(message: &str) -> LogLine {
        LogLine {
            timestamp: Local::now(),
            message: message.to_string(),
            raw: message.to_string(),
            line_number: 1,
            anomaly_score: 0.0,
            sidecar_anomaly_score: 0.0,
            sidecar_score_is_unk: false,
            sidecar_score_is_rare: false,
            sidecar_scored: false,
        }
    }

    #[test]
    fn first_line_is_novel() {
        let mut scorer = RarityScorer::new();
        let line = make_line("hello world");
        assert!((scorer.score(&line) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn unseen_template_scores_one() {
        let mut scorer = RarityScorer::new();
        let line_a = make_line("message A");
        // Score and update with line A
        scorer.score(&line_a);
        scorer.update(&line_a);

        // A brand new template should score 1.0
        let line_b = make_line("completely different message B");
        assert!((scorer.score(&line_b) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn repeated_template_scores_below_one() {
        let mut scorer = RarityScorer::new();
        let line = make_line("repeated message");
        // Feed the same template multiple times
        for _ in 0..10 {
            scorer.score(&line);
            scorer.update(&line);
        }
        // After 10 identical lines, frequency = 10/10 = 1.0
        // score = (1 - 1.0).sqrt() = 0.0
        let score = scorer.score(&line);
        assert!(
            score < 1.0,
            "repeated template should score below 1.0, got {score}"
        );
    }

    #[test]
    fn scores_are_clamped_to_unit_range() {
        let mut scorer = RarityScorer::new();
        let line = make_line("clamp test");
        // Score across several updates and verify always in [0, 1]
        for _ in 0..20 {
            let s = scorer.score(&line);
            assert!((0.0..=1.0).contains(&s), "score {s} out of [0, 1]");
            scorer.update(&line);
        }
    }
}
