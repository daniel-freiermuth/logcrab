use crate::anomaly::scorer::AnomalyScorer;
use crate::parser::line::LogLine;
use ahash::AHashMap;

/// Scores based on template rarity (inverse frequency)
pub struct RarityScorer {
    template_counts: AHashMap<String, u32>,
    total_lines: u32,
}

impl RarityScorer {
    pub fn new() -> Self {
        RarityScorer {
            template_counts: AHashMap::new(),
            total_lines: 0,
        }
    }
}

impl AnomalyScorer for RarityScorer {
    fn score(&mut self, line: &LogLine) -> f64 {
        if self.total_lines == 0 {
            return 1.0; // First line is always novel
        }

        let count = self
            .template_counts
            .get(&line.template_key)
            .copied()
            .unwrap_or(0);

        if count == 0 {
            // Never seen before - highly anomalous
            return 1.0;
        }

        // Inverse frequency: rare templates get higher scores
        // Simple inverse: score = 1 - (count / total)
        // But scale it so even moderately rare items get decent scores
        let frequency = count as f64 / self.total_lines as f64;

        // Use a power function to make scoring more aggressive for rare items
        // score = (1 - frequency)^0.5 gives better distribution
        let score = (1.0 - frequency).sqrt();

        score.clamp(0.0, 1.0)
    }

    fn update(&mut self, line: &LogLine) {
        *self
            .template_counts
            .entry(line.template_key.clone())
            .or_insert(0) += 1;
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

    #[test]
    fn test_rarity_scorer() {
        let mut scorer = RarityScorer::new();

        let mut line1 = LogLine::new("test message 1".to_string(), 1);
        line1.template_key = "test message <NUM>".to_string();

        let mut line2 = LogLine::new("different message".to_string(), 2);
        line2.template_key = "different message".to_string();

        // First occurrence should be highly anomalous
        let score1 = scorer.score(&line1);
        assert!(score1 > 0.8, "First occurrence should be highly anomalous");
        scorer.update(&line1);

        // Repeat the same template
        scorer.update(&line1);
        scorer.update(&line1);
        scorer.update(&line1);

        // After multiple occurrences, score should be lower
        let score_repeated = scorer.score(&line1);
        assert!(score_repeated < 0.5, "Repeated template should score lower");

        // Novel template should still score high
        let score_novel = scorer.score(&line2);
        assert!(score_novel > 0.8, "Novel template should score high");
    }
}
