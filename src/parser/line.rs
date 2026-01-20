// Re-export the new log line types
pub use crate::parser::logline_types::{
    DltLogLine, GenericLogLine, LogLineCore, LogLineVariant, LogcatLogLine,
};

// Type alias for compatibility during migration
pub type LogLine = LogLineVariant;
