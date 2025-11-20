use crate::parser::line::LogLine;
use crate::anomaly::scorer::AnomalyScorer;
use std::collections::VecDeque;

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

/// Severity-based scorer - ERROR/FATAL get boosted scores
pub struct SeverityScorer {
    recent_levels: VecDeque<u8>,
    window_size: usize,
}

impl SeverityScorer {
    pub fn new(window_size: usize) -> Self {
        SeverityScorer {
            recent_levels: VecDeque::with_capacity(window_size),
            window_size,
        }
    }
}

impl AnomalyScorer for SeverityScorer {
    fn score(&mut self, line: &LogLine) -> f64 {
        let current_severity = line.level.severity();
        
        if self.recent_levels.is_empty() {
            // First line - base score on severity alone
            return current_severity as f64 / 5.0;
        }
        
        // Calculate average recent severity
        let avg_severity: f64 = self.recent_levels.iter()
            .map(|&s| s as f64)
            .sum::<f64>() / self.recent_levels.len() as f64;
        
        // Score based on:
        // 1. Absolute severity (ERROR/FATAL always get points)
        // 2. Sudden severity increase (transition detection)
        let base_score = current_severity as f64 / 5.0;
        let transition_score = if current_severity as f64 > avg_severity + 1.0 {
            0.5 // Sudden jump in severity
        } else {
            0.0
        };
        
        (base_score + transition_score).min(1.0)
    }
    
    fn update(&mut self, line: &LogLine) {
        if self.recent_levels.len() >= self.window_size {
            self.recent_levels.pop_front();
        }
        self.recent_levels.push_back(line.level.severity());
    }
    
    fn reset(&mut self) {
        self.recent_levels.clear();
    }
}

impl Default for SeverityScorer {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::line::LogLevel;

    #[test]
    fn test_entropy_calculation() {
        let entropy1 = EntropyScorer::calculate_entropy("aaaa");
        let entropy2 = EntropyScorer::calculate_entropy("abcd");
        assert!(entropy2 > entropy1); // More varied text has higher entropy
    }

    #[test]
    fn test_severity_scorer() {
        let mut scorer = SeverityScorer::new(10);
        
        let mut info_line = LogLine::new("info".to_string(), 1);
        info_line.level = LogLevel::Info;
        
        let mut error_line = LogLine::new("error".to_string(), 2);
        error_line.level = LogLevel::Error;
        
        scorer.update(&info_line);
        scorer.update(&info_line);
        
        // Sudden ERROR after INFO should score high
        let score = scorer.score(&error_line);
        assert!(score > 0.5);
    }
}
