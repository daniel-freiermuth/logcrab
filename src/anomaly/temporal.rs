use crate::anomaly::scorer::AnomalyScorer;
use crate::parser::line::LogLine;
use ahash::AHashMap;
use chrono::{DateTime, Duration, Local};
use std::collections::VecDeque;

/// Temporal anomaly scorer based on time windows
pub struct TemporalScorer {
    window_duration: Duration,
    // Track when each template was last seen
    last_seen: AHashMap<String, DateTime<Local>>,
    // Track recent timestamps for burst detection
    recent_timestamps: VecDeque<DateTime<Local>>,
    // Track template counts within window
    window_template_counts: AHashMap<String, u32>,
}

impl TemporalScorer {
    pub fn new(window_seconds: i64) -> Self {
        TemporalScorer {
            window_duration: Duration::seconds(window_seconds),
            last_seen: AHashMap::new(),
            recent_timestamps: VecDeque::new(),
            window_template_counts: AHashMap::new(),
        }
    }

    fn clean_old_entries(&mut self, current_time: DateTime<Local>) {
        // Remove timestamps outside the window
        while let Some(&front_time) = self.recent_timestamps.front() {
            if current_time - front_time > self.window_duration {
                self.recent_timestamps.pop_front();
            } else {
                break;
            }
        }
    }
}

impl AnomalyScorer for TemporalScorer {
    fn score(&mut self, line: &LogLine) -> f64 {
        let current_time = line.timestamp.unwrap_or_else(Local::now);

        self.clean_old_entries(current_time);

        let mut score = 0.0;

        // Component 1: Time since last occurrence (recency)
        if let Some(&last_time) = self.last_seen.get(&line.template_key) {
            let time_diff = current_time - last_time;
            let time_diff_secs = time_diff.num_seconds().abs(); // Use absolute value for out-of-order logs

            // Compare absolute time difference to window
            let window_secs = self.window_duration.num_seconds().abs();
            if time_diff_secs > window_secs {
                score += 0.5; // Long time gap (regardless of direction)
            } else {
                // Linear decay based on time gap
                let ratio = (time_diff_secs as f64 / window_secs as f64).min(1.0);
                score += ratio * 0.3;
            }
        } else {
            // Never seen before in our tracking
            score += 0.7;
        }

        // Component 2: Burst detection
        // If we're seeing unusually high activity in the time window
        let window_size = self.recent_timestamps.len();
        if window_size > 0 {
            let rate = window_size as f64 / self.window_duration.num_seconds().abs() as f64;

            // Adaptive threshold: if rate is unusually high, boost score
            // For now, use a simple heuristic
            if window_size > 100 && rate > 10.0 {
                score += 0.3; // High activity burst
            }
        }

        score.clamp(0.0, 1.0) // Ensure score is always in [0, 1]
    }

    fn update(&mut self, line: &LogLine) {
        let current_time = line.timestamp.unwrap_or_else(Local::now);

        // Update last seen time for this template
        self.last_seen
            .insert(line.template_key.clone(), current_time);

        // Add to recent timestamps
        self.recent_timestamps.push_back(current_time);

        // Update window counts
        *self
            .window_template_counts
            .entry(line.template_key.clone())
            .or_insert(0) += 1;

        // Clean up old entries periodically
        if self.recent_timestamps.len() % 1000 == 0 {
            self.clean_old_entries(current_time);
        }
    }
}

impl Default for TemporalScorer {
    fn default() -> Self {
        Self::new(30) // 30 second window
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;

    #[test]
    fn test_temporal_scorer() {
        let mut scorer = TemporalScorer::new(10);

        let mut line1 = LogLine::new("test".to_string(), 1);
        line1.template_key = "test".to_string();
        line1.timestamp = Some(Local::now());

        // First occurrence
        let score1 = scorer.score(&line1);
        assert!(score1 > 0.5); // Novel

        scorer.update(&line1);

        // Immediate repeat
        let score2 = scorer.score(&line1);
        assert!(score2 < score1); // Less anomalous
    }
}
