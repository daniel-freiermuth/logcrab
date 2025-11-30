use super::line::LogLine;
use chrono::{Local, TimeDelta, TimeZone};
use dlt_core::dlt::Message;
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
                if let Some(log_line) = convert_dlt_message(&msg, line_number) {
                    lines.push(log_line);
                    line_number += 1;
                } else {
                    log::warn!("Skipped DLT message without valid timestamp");
                }
            }
            Ok(Some(_)) => {
                log::warn!("Skipped non-item DLT message");
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
fn convert_dlt_message(msg: &Message, line_number: usize) -> Option<LogLine> {
    // Extract timestamp (if available) from header
    let timestamp = if let Some(ts) = msg.header.timestamp {
        // ts is in 0.1 ms since epoch
        let seconds = ts as u64 / 10000;
        let nanos = ((ts as u64 % 10000) * 100_000) as u32;
        if let Some(ts) = Local.timestamp_opt(seconds as i64, nanos).single() {
            ts
        } else {
            log::error!("Invalid timestamp in DLT message for line {}", line_number);
            return None;
        }
    } else {
        log::error!("DLT message missing timestamp for line {}", line_number);
        return None;
    };
    let ecu_header = if let Some(ecu_id) = &msg.header.ecu_id {
        ecu_id.clone()
    } else {
        log::warn!("DLT message missing ECU ID for line {}", line_number);
        "UnknownECU".to_string()
    };
    let session_id = if let Some(session) = msg.header.session_id {
        session
    } else {
        log::warn!("DLT message missing Session ID for line {}", line_number);
        0
    };
    let (message_type, app_id, ctx_id) = if let Some(ext_header) = &msg.extended_header {
        (
            ext_header.message_type.clone(),
            ext_header.application_id.clone(),
            ext_header.context_id.clone(),
        )
    } else {
        log::error!(
            "DLT message missing Extended Header for line {}",
            line_number
        );
        return None;
    };
    let (storage_ecu, storage_time) = if let Some(storage_header) = &msg.storage_header {
        (
            storage_header.ecu_id.clone(),
            TimeDelta::new(
                storage_header.timestamp.seconds as i64,
                storage_header.timestamp.microseconds * 1000,
            )
            .expect("Invalid storage timestamp"),
        )
    } else {
        log::error!(
            "DLT message missing Storage Header for line {}",
            line_number
        );
        return None;
    };

    // Extract the payload as message (PayloadContent)
    let payload = match &msg.payload {
        dlt_core::dlt::PayloadContent::Verbose(args) => args
            .iter()
            .filter_map(|arg| match &arg.value {
                dlt_core::dlt::Value::StringVal(s) => Some(s.clone()),
                dlt_core::dlt::Value::U32(v) => Some(format!("{}", v)),
                _ => {
                    log::error!(
                        "Unsupported DLT verbose argument {:?} for line {}",
                        arg.value,
                        line_number
                    );
                    None
                }
            })
            .collect::<Vec<String>>()
            .join(" || "),
        _ => {
            log::error!(
                "Unsupported DLT payload {:?} for line {}",
                msg.payload,
                line_number
            );
            return None;
        }
    };

    let message = format!(
        "[{} {}] {} {} {} {} {:?} {}",
        storage_time, storage_ecu, ecu_header, session_id, app_id, ctx_id, message_type, payload
    );

    let raw = format!("{:?}", msg);

    Some(LogLine {
        raw,
        line_number,
        timestamp,
        message,
        template_key: "".to_string(),
    })
}
