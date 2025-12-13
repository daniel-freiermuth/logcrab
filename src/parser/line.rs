use chrono::{DateTime, Local};

#[derive(Debug, Clone)]
pub struct LogLine {
    pub raw: String,
    pub timestamp: DateTime<Local>,
    pub message: String,
    pub line_number: usize,
    pub template_key: String,
    pub anomaly_score: f64,
}

impl LogLine {
    pub const fn new(
        raw: String,
        line_number: usize,
        message: String,
        timestamp: DateTime<Local>,
    ) -> Self {
        Self {
            raw,
            timestamp,
            message,
            line_number,
            template_key: String::new(),
            anomaly_score: 0.0,
        }
    }
}
