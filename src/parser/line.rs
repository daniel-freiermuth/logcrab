use chrono::{DateTime, Local};

#[derive(Debug, Clone)]
pub struct LogLine {
    pub raw: String,
    pub timestamp: DateTime<Local>,
    pub message: String,
    pub line_number: usize,
    pub template_key: String,
}

impl LogLine {
    pub fn new(
        raw: String,
        line_number: usize,
        message: String,
        timestamp: DateTime<Local>,
    ) -> Self {
        LogLine {
            raw,
            timestamp,
            message,
            line_number,
            template_key: String::new(),
        }
    }
}
