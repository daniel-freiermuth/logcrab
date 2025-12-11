use crate::parser::line::LogLine;

/// Trait for anomaly scoring components
pub trait AnomalyScorer: Send {
    /// Score a line before updating internal state
    /// Returns a score in [0.0, 1.0] where higher = more anomalous
    fn score(&mut self, line: &LogLine) -> f64;

    /// Update internal state after scoring
    fn update(&mut self, line: &LogLine);
}

/// Composite scorer that combines multiple scoring strategies
pub struct CompositeScorer {
    scorers: Vec<(Box<dyn AnomalyScorer>, f64)>, // (scorer, weight)
}

impl CompositeScorer {
    pub fn new() -> Self {
        Self {
            scorers: Vec::new(),
        }
    }

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
