pub mod entropy;
pub mod keyword;
pub mod rarity;
pub mod scorer;
pub mod sidecar_client;
pub mod temporal;

use entropy::EntropyScorer;
use keyword::KeywordScorer;
use rarity::RarityScorer;
use scorer::CompositeScorer;
use temporal::TemporalScorer;

/// Create the default anomaly scoring pipeline
#[must_use]
pub fn create_default_scorer() -> CompositeScorer {
    CompositeScorer::new()
        .add_scorer(Box::new(RarityScorer::new()), 5.0) // Rarity is most important
        .add_scorer(Box::new(TemporalScorer::new(30)), 2.0) // Temporal patterns
        .add_scorer(Box::new(EntropyScorer::new()), 1.5) // Message entropy
        .add_scorer(Box::new(KeywordScorer::new()), 2.0) // Keyword detection (error/warning/fail)
}

/// Normalize anomaly scores to 0-100 range
pub fn normalize_scores(scores: &[f64]) -> Vec<f64> {
    profiling::scope!("normalize_scores");
    if scores.is_empty() {
        return Vec::new();
    }

    let min_score = scores.iter().copied().fold(f64::INFINITY, f64::min);
    let max_score = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    if (max_score - min_score).abs() < 1e-10 {
        // All scores are the same
        return vec![50.0; scores.len()];
    }

    scores
        .iter()
        .map(|&s| ((s - min_score) / (max_score - min_score)) * 100.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_empty_scores() {
        assert!(normalize_scores(&[]).is_empty());
    }

    #[test]
    fn normalize_single_score() {
        // Single score falls into the "all same" branch → 50.0
        let result = normalize_scores(&[7.5]);
        assert_eq!(result, vec![50.0]);
    }

    #[test]
    fn normalize_all_identical_scores() {
        let result = normalize_scores(&[3.0, 3.0, 3.0, 3.0]);
        assert_eq!(result, vec![50.0; 4]);
    }

    #[test]
    fn normalize_two_extremes() {
        // 0.0 and 1.0 should map to 0.0 and 100.0
        let result = normalize_scores(&[0.0, 1.0]);
        assert!((result[0] - 0.0).abs() < f64::EPSILON);
        assert!((result[1] - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn normalize_min_max_scaling_range() {
        let result = normalize_scores(&[10.0, 20.0, 30.0, 40.0, 50.0]);
        // Verify min maps to 0.0, max to 100.0, and middle values scale linearly
        assert!((result[0] - 0.0).abs() < f64::EPSILON);
        assert!((result[1] - 25.0).abs() < f64::EPSILON);
        assert!((result[2] - 50.0).abs() < f64::EPSILON);
        assert!((result[3] - 75.0).abs() < f64::EPSILON);
        assert!((result[4] - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn normalize_negative_scores() {
        let result = normalize_scores(&[-10.0, 0.0, 10.0]);
        assert!((result[0] - 0.0).abs() < f64::EPSILON);
        assert!((result[1] - 50.0).abs() < f64::EPSILON);
        assert!((result[2] - 100.0).abs() < f64::EPSILON);
    }
}
