pub mod scorer;
pub mod rarity;
pub mod temporal;
pub mod entropy;
pub mod keyword;

use scorer::CompositeScorer;
use rarity::RarityScorer;
use temporal::TemporalScorer;
use entropy::EntropyScorer;
use keyword::KeywordScorer;

/// Create the default anomaly scoring pipeline
pub fn create_default_scorer() -> CompositeScorer {
    CompositeScorer::new()
        .add_scorer(Box::new(RarityScorer::new()), 3.0)          // Rarity is most important
        .add_scorer(Box::new(TemporalScorer::new(30)), 2.0)      // Temporal patterns
        .add_scorer(Box::new(EntropyScorer::new()), 1.5)         // Message entropy
        .add_scorer(Box::new(KeywordScorer::new()), 2.5)         // Keyword detection (error/warning/fail)
}

/// Normalize anomaly scores to 0-100 range
pub fn normalize_scores(scores: &[f64]) -> Vec<f64> {
    if scores.is_empty() {
        return Vec::new();
    }
    
    let min_score = scores.iter().copied().fold(f64::INFINITY, f64::min);
    let max_score = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    
    if (max_score - min_score).abs() < 1e-10 {
        // All scores are the same
        return vec![50.0; scores.len()];
    }
    
    scores.iter()
        .map(|&s| ((s - min_score) / (max_score - min_score)) * 100.0)
        .collect()
}
