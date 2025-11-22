use crate::parser::line::LogLine;
use crate::anomaly::scorer::AnomalyScorer;

/// Message entropy scorer - measures information content
/// Higher entropy = more unique/random content = potentially more interesting
pub struct EntropyScorer {
    // Track average message length and entropy for comparison
    avg_length: f64,
    avg_entropy: f64,
    sample_count: u32,
}

impl EntropyScorer {
    pub fn new() -> Self {
        EntropyScorer {
            avg_length: 0.0,
            avg_entropy: 0.0,
            sample_count: 0,
        }
    }
    
    fn calculate_entropy(text: &str) -> f64 {
        if text.is_empty() {
            return 0.0;
        }
        
        let mut char_counts = [0u32; 256];
        let total = text.len() as f64;
        
        // Count character frequencies
        for byte in text.bytes() {
            char_counts[byte as usize] += 1;
        }
        
        // Calculate Shannon entropy
        let mut entropy = 0.0;
        for &count in &char_counts {
            if count > 0 {
                let p = count as f64 / total;
                entropy -= p * p.log2();
            }
        }
        
        entropy
    }
}

impl AnomalyScorer for EntropyScorer {
    fn score(&mut self, line: &LogLine) -> f64 {
        if self.sample_count == 0 {
            return 0.5; // Neutral score for first line
        }
        
        let entropy = Self::calculate_entropy(&line.message);
        let length = line.message.len() as f64;
        
        // Score based on deviation from average
        let entropy_deviation = (entropy - self.avg_entropy).abs() / self.avg_entropy.max(1.0);
        let length_deviation = (length - self.avg_length).abs() / self.avg_length.max(1.0);
        
        // Combine deviations (unusually high or low entropy/length is anomalous)
        let score = (entropy_deviation + length_deviation) / 2.0;
        
        score.min(1.0)
    }
    
    fn update(&mut self, line: &LogLine) {
        let entropy = Self::calculate_entropy(&line.message);
        let length = line.message.len() as f64;
        
        // Running average
        self.avg_entropy = (self.avg_entropy * self.sample_count as f64 + entropy) 
            / (self.sample_count + 1) as f64;
        self.avg_length = (self.avg_length * self.sample_count as f64 + length) 
            / (self.sample_count + 1) as f64;
        
        self.sample_count += 1;
    }
    
    fn reset(&mut self) {
        self.avg_length = 0.0;
        self.avg_entropy = 0.0;
        self.sample_count = 0;
    }
}

impl Default for EntropyScorer {
    fn default() -> Self {
        Self::new()
    }
}