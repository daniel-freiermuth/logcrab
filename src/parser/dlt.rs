use super::line::LogLine;
use chrono::{DateTime, Local, TimeDelta, TimeZone};
use dlt_core::dlt::{DltTimeStamp, Message};
use dlt_core::read::{read_message, DltMessageReader};
use std::fs::File;
use std::path::Path;

/// Format a time difference with 4 significant digits and appropriate unit
fn format_time_diff(diff: TimeDelta) -> String {
    let sign = if diff < TimeDelta::zero() { "-" } else { "+" };
    let nanos = diff.num_nanoseconds().unwrap_or(0).unsigned_abs();

    let (value, unit) = if nanos >= 60_000_000_000 {
        // Minutes
        (nanos as f64 / 60_000_000_000.0, "m")
    } else if nanos >= 1_000_000_000 {
        // Seconds
        (nanos as f64 / 1_000_000_000.0, "s")
    } else if nanos >= 1_000_000 {
        // Milliseconds
        (nanos as f64 / 1_000_000.0, "ms")
    } else if nanos >= 1_000 {
        // Microseconds
        (nanos as f64 / 1_000.0, "µs")
    } else {
        // Nanoseconds
        (nanos as f64, "ns")
    };

    // Format with 4 significant digits
    let formatted = if value >= 1000.0 {
        format!("{sign}{value:>4.0}{unit}")
    } else if value >= 100.0 {
        format!("{sign}{value:>4.1}{unit}")
    } else if value >= 10.0 {
        format!("{sign}{value:>4.2}{unit}")
    } else {
        format!("{sign}{value:>4.3}{unit}")
    };
    formatted
}

/// Returns the offset to add to header timestamps (time since boot) to get absolute time
fn calc_boot_time_from_message(msg: &Message) -> Option<DateTime<Local>> {
    // Get storage header timestamp (absolute wall-clock time, but imprecise)
    let storage_time = msg
        .storage_header
        .as_ref()
        .map(|sh| storage_time_to_datetime(&sh.timestamp))?;

    // Get header timestamp (time since boot in 0.1ms units, precise)
    let boot_time_offset = msg
        .header
        .timestamp
        .map(dlt_header_time_to_timedelta)?;

    // Boot time = storage_time - time_since_boot
    storage_time.checked_sub_signed(boot_time_offset)
}

fn calc_boot_time_from_file(path: &Path) -> Result<DateTime<Local>, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open DLT file: {e}"))?;
    let mut reader = DltMessageReader::new(file, true);
    loop {
        match read_message(&mut reader, None) {
            Ok(Some(dlt_core::parse::ParsedMessage::Item(msg))) => {
                if let Some(offset) = calc_boot_time_from_message(&msg) {
                    return Ok(offset);
                }
                log::warn!("First DLT message missing timestamp info, trying next");
            }
            Ok(Some(_)) => continue,
            Ok(None) => {
                return Err("No valid DLT messages found to calculate boot offset".to_owned());
            }
            Err(e) => {
                log::warn!("Failed to parse DLT message while finding offset: {e:?}");
            }
        }
    }
}

fn storage_time_to_datetime(storage_time: &DltTimeStamp) -> DateTime<Local> {
    Local
        .timestamp_opt(
            i64::from(storage_time.seconds),
            storage_time.microseconds * 1000,
        )
        .single()
        .expect("Invalid storage timestamp")
}

fn dlt_header_time_to_timedelta(header_time: u32) -> TimeDelta {
    TimeDelta::microseconds(header_time as i64 * 100)
}

/// Parse a DLT binary file and return log lines
pub fn parse_dlt_file<P: AsRef<Path>>(path: P) -> Result<Vec<LogLine>, String> {
    let path = path.as_ref();

    let boot_time = calc_boot_time_from_file(path)?;

    log::info!("Calculated DLT boot time: {boot_time}");

    // Second pass: parse all messages with the calculated offset
    let file = File::open(path).map_err(|e| format!("Failed to open DLT file: {e}"))?;
    let mut reader = DltMessageReader::new(file, true);
    let mut lines = Vec::new();
    let mut line_number = 1;
    loop {
        match read_message(&mut reader, None) {
            Ok(Some(dlt_core::parse::ParsedMessage::Item(msg))) => {
                if let Some(log_line) = convert_dlt_message(&msg, line_number, boot_time) {
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
                log::warn!("Failed to parse DLT message: {e:?}");
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

/// Convert a `dlt_core::dlt::Message` to `LogLine`
fn convert_dlt_message(
    msg: &Message,
    line_number: usize,
    boot_time: DateTime<Local>,
) -> Option<LogLine> {
    // Extract timestamp: boot_time + time_since_boot
    let timestamp = if let Some(ts) = msg.header.timestamp {
        let time_since_boot = dlt_header_time_to_timedelta(ts);
        if let Some(ts) = boot_time.checked_add_signed(time_since_boot) {
            ts
        } else {
            log::error!("Invalid timestamp in DLT message for line {line_number}");
            return None;
        }
    } else {
        log::error!("DLT message missing timestamp for line {line_number}");
        return None;
    };
    let ecu_header = if let Some(ecu_id) = &msg.header.ecu_id {
        ecu_id.clone()
    } else {
        log::warn!("DLT message missing ECU ID for line {line_number}");
        "UnknownECU".to_string()
    };
    let session_id = msg.header.session_id.unwrap_or_else(|| {
        log::warn!("DLT message missing Session ID for line {line_number}");
        0
    });
    let (message_type, app_id, ctx_id) = if let Some(ext_header) = &msg.extended_header {
        (
            ext_header.message_type.clone(),
            ext_header.application_id.clone(),
            ext_header.context_id.clone(),
        )
    } else {
        log::error!("DLT message missing Extended Header for line {line_number}");
        return None;
    };
    let (storage_ecu, storage_time) = if let Some(storage_header) = &msg.storage_header {
        (
            storage_header.ecu_id.clone(),
            storage_time_to_datetime(&storage_header.timestamp),
        )
    } else {
        log::error!("DLT message missing Storage Header for line {line_number}");
        return None;
    };

    let time_diff = storage_time.signed_duration_since(timestamp);

    // Extract the payload as message (PayloadContent)
    let payload = if let dlt_core::dlt::PayloadContent::Verbose(args) = &msg.payload {
        args.iter()
            .filter_map(|arg| match &arg.value {
                dlt_core::dlt::Value::StringVal(s) => Some(s.clone()),
                dlt_core::dlt::Value::U32(v) => Some(format!("{v}")),
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
            .join(" || ")
    } else {
        log::error!(
            "Unsupported DLT payload {:?} for line {}",
            msg.payload,
            line_number
        );
        return None;
    };

    let diff_str = format_time_diff(time_diff);

    let message = format!(
        "[{storage_time} ({diff_str}) {storage_ecu}] {ecu_header} {session_id} {app_id} {ctx_id} {message_type:?} {payload}"
    );

    let raw = format!("{msg:?}");

    Some(LogLine {
        raw,
        line_number,
        timestamp,
        message,
        template_key: String::new(),
    })
}
