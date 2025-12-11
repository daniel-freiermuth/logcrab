use std::collections::HashMap;

use crate::anomaly::scorer::AnomalyScorer;
use crate::parser::line::LogLine;

/// Scores based on template rarity (inverse frequency)
pub struct RarityScorer {
    template_counts: HashMap<String, u32>,
    total_lines: u32,
}

impl RarityScorer {
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
        let frequency = f64::from(count) / f64::from(self.total_lines);

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
