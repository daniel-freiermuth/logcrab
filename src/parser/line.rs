use chrono::{DateTime, Local};

#[derive(Debug, Clone)]
pub struct LogLine {
    pub raw: String,
    pub timestamp: Option<DateTime<Local>>,
    pub message: String,
    pub line_number: usize,
    pub template_key: String,
    pub anomaly_score: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Verbose,
    Debug,
    Info,
    Warning,
    Error,
    Fatal,
    Unknown,
}

impl LogLevel {
    pub fn from_char(c: char) -> Self {
        match c.to_ascii_uppercase() {
            'V' => LogLevel::Verbose,
            'D' => LogLevel::Debug,
            'I' => LogLevel::Info,
            'W' => LogLevel::Warning,
            'E' => LogLevel::Error,
            'F' => LogLevel::Fatal,
            _ => LogLevel::Unknown,
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "VERBOSE" | "V" => LogLevel::Verbose,
            "DEBUG" | "D" => LogLevel::Debug,
            "INFO" | "I" => LogLevel::Info,
            "WARNING" | "WARN" | "W" => LogLevel::Warning,
            "ERROR" | "ERR" | "E" => LogLevel::Error,
            "FATAL" | "F" => LogLevel::Fatal,
            _ => LogLevel::Unknown,
        }
    }

    pub fn to_str(&self) -> &'static str {
        match self {
            LogLevel::Verbose => "V",
            LogLevel::Debug => "D",
            LogLevel::Info => "I",
            LogLevel::Warning => "W",
            LogLevel::Error => "E",
            LogLevel::Fatal => "F",
            LogLevel::Unknown => "?",
        }
    }

    pub fn severity(&self) -> u8 {
        match self {
            LogLevel::Verbose => 0,
            LogLevel::Debug => 1,
            LogLevel::Info => 2,
            LogLevel::Warning => 3,
            LogLevel::Error => 4,
            LogLevel::Fatal => 5,
            LogLevel::Unknown => 0,
        }
    }
}

impl LogLine {
    pub fn new(raw: String, line_number: usize) -> Self {
        LogLine {
            raw,
            timestamp: None,
            message: String::new(),
            line_number,
            template_key: String::new(),
            anomaly_score: 0.0,
        }
    }
}
