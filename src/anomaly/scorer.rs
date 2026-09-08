use crate::core::log_store::LogLine;

/// Trait for anomaly scoring components
pub trait AnomalyScorer: Send {
    /// Score a line before updating internal state.
    /// Returns a score in [0.0, 1.0] where higher = more anomalous.
    fn score(&mut self, line: &LogLine) -> f64;

    /// Update internal state after scoring.
    fn update(&mut self, line: &LogLine);
}

/// Composite scorer that combines multiple scoring strategies
pub struct CompositeScorer {
    scorers: Vec<(Box<dyn AnomalyScorer>, f64)>, // (scorer, weight)
}

impl CompositeScorer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            scorers: Vec::new(),
        }
    }

    #[must_use]
    pub fn add_scorer(mut self, scorer: Box<dyn AnomalyScorer>, weight: f64) -> Self {
        self.scorers.push((scorer, weight));
        self
    }

    pub fn score(&mut self, line: &LogLine) -> f64 {
        let total_weight: f64 = self.scorers.iter().map(|(_, w)| w).sum();

        if total_weight == 0.0 {
            return 0.0;
        }

        let weighted_sum: f64 = self
            .scorers
            .iter_mut()
            .map(|(scorer, weight)| scorer.score(line) * *weight)
            .sum();

        weighted_sum / total_weight
    }

    pub fn update(&mut self, line: &LogLine) {
        for (scorer, _) in &mut self.scorers {
            scorer.update(line);
        }
    }
}

impl Default for CompositeScorer {
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

    /// A trivial scorer that always returns a fixed value.
    struct ConstantScorer(f64);

    impl AnomalyScorer for ConstantScorer {
        fn score(&mut self, _line: &LogLine) -> f64 {
            self.0
        }
        fn update(&mut self, _line: &LogLine) {}
    }

    #[test]
    fn composite_no_scorers_returns_zero() {
        let mut scorer = CompositeScorer::new();
        let line = make_line("test");
        assert!((scorer.score(&line) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn composite_zero_total_weight_returns_zero() {
        let mut scorer = CompositeScorer::new().add_scorer(Box::new(ConstantScorer(0.5)), 0.0);
        let line = make_line("test");
        assert!((scorer.score(&line) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn composite_single_scorer_passes_through() {
        let mut scorer = CompositeScorer::new().add_scorer(Box::new(ConstantScorer(0.8)), 1.0);
        let line = make_line("test");
        assert!((scorer.score(&line) - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn composite_weighted_average() {
        let mut scorer = CompositeScorer::new()
            .add_scorer(Box::new(ConstantScorer(1.0)), 3.0)
            .add_scorer(Box::new(ConstantScorer(0.0)), 1.0);
        let line = make_line("test");
        // (1.0*3.0 + 0.0*1.0) / 4.0 = 0.75
        assert!((scorer.score(&line) - 0.75).abs() < f64::EPSILON);
    }
}
