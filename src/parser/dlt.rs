use super::line::LogLine;
use chrono::{Local, TimeZone};
use dlt_core::dlt::{Message, MessageType};
use dlt_core::read::{read_message, DltMessageReader};
use std::fs::File;
use std::path::Path;

/// Parse a DLT binary file and return log lines
pub fn parse_dlt_file<P: AsRef<Path>>(path: P) -> Result<Vec<LogLine>, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open DLT file: {}", e))?;
    let mut reader = DltMessageReader::new(file, true);
    let mut lines = Vec::new();
    let mut line_number = 1;
    loop {
        match read_message(&mut reader, None) {
            Ok(Some(dlt_core::parse::ParsedMessage::Item(msg))) => {
                let log_line = convert_dlt_message(&msg, line_number);
                lines.push(log_line);
                line_number += 1;
            }
            Ok(Some(_)) => {
                // Ignore other variants
            }
            Ok(None) => break,
            Err(e) => {
                log::warn!("Failed to parse DLT message: {:?}", e);
                // Continue parsing despite errors
            }
        }
    }
    if lines.is_empty() {
        Err("No valid DLT messages found in file".to_string())
    } else {
        log::info!("Parsed {} DLT messages", lines.len());
        Ok(lines)
    }
}

// No parse_dlt_buffer needed; handled by parse_dlt_file

/// Convert a dlt_core::dlt::Message to LogLine
fn convert_dlt_message(msg: &Message, line_number: usize) -> LogLine {
    // Extract timestamp (if available) from header
    let timestamp = msg.header.timestamp.and_then(|ts| {
        // ts is in 0.1 ms since epoch
        let seconds = ts as u64 / 10000;
        let nanos = ((ts as u64 % 10000) * 100_000) as u32;
        Local.timestamp_opt(seconds as i64, nanos).single()
    });

    // Extract the payload as message (PayloadContent)
    let message = match &msg.payload {
        dlt_core::dlt::PayloadContent::Verbose(args) => format!("{:?}", args),
        dlt_core::dlt::PayloadContent::NonVerbose(_id, data) => format!("{:?}", data),
        dlt_core::dlt::PayloadContent::ControlMsg(_id, data) => format!("ControlMsg: {:?}", data),
        dlt_core::dlt::PayloadContent::NetworkTrace(data) => format!("NetworkTrace: {:?}", data),
    };

    // Build the raw line representation
    let raw = if let Some(ext_header) = &msg.extended_header {
        format!(
            "{} {} {} {}",
            ext_header.application_id,
            ext_header.context_id,
            format_message_info(&ext_header.message_type),
            message
        )
    } else {
        message.clone()
    };

    let mut line = LogLine::new(raw, line_number);
    line.message = message;
    line.timestamp = timestamp;
    line
}

/// Format message info (log level, type, etc.)
fn format_message_info(msg_type: &MessageType) -> String {
    match msg_type {
        MessageType::Log(level) => format!("[{:?}]", level),
        other => format!("[{:?}]", other),
    }
}
