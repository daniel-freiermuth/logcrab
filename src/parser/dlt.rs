use crate::config::DltTimestampSource;
use crate::core::log_file::ProgressCallback;
use crate::core::log_store::SourceData;

use super::line::{DltLogLine, LogLine};
use chrono::{DateTime, Local, TimeDelta};
use dlt_core::dlt::Message;
use dlt_core::read::{read_message, DltMessageReader};
use std::cell::Cell;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

/// Returns the offset to add to header timestamps (time since boot) to get absolute time
fn calc_boot_time_from_message(msg: &Message) -> Option<DateTime<Local>> {
    // Get storage header timestamp (absolute wall-clock time, but imprecise)
    let storage_time = msg
        .storage_header
        .as_ref()
        .and_then(|sh| storage_time_to_datetime(&sh.timestamp))?;

    // Get header timestamp (time since boot in 0.1ms units, precise)
    let boot_time_offset = msg.header.timestamp.map(dlt_header_time_to_timedelta)?;

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

pub fn storage_time_to_datetime(storage_time: &dlt_core::dlt::DltTimeStamp) -> Option<DateTime<Local>> {
    use chrono::TimeZone;
    Local
        .timestamp_opt(
            i64::from(storage_time.seconds),
            storage_time.microseconds * 1000,
        )
        .single()
}

pub const fn dlt_header_time_to_timedelta(header_time: u32) -> TimeDelta {
    TimeDelta::microseconds(header_time as i64 * 100)
}

/// A reader wrapper that tracks bytes read for progress reporting
struct ProgressReader<R> {
    inner: R,
    bytes_read: Rc<Cell<u64>>,
}

impl<R: Read> ProgressReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            bytes_read: Rc::new(Cell::new(0)),
        }
    }

    fn bytes_read_counter(&self) -> Rc<Cell<u64>> {
        Rc::clone(&self.bytes_read)
    }
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.bytes_read.set(self.bytes_read.get() + n as u64);
        Ok(n)
    }
}

/// Chunk size for incremental loading (send update every N lines)
const DLT_CHUNK_SIZE: usize = 10_000;

/// Parse a DLT binary file with incremental loading
///
/// Appends parsed lines directly to `source` in batches for progressive display.
/// Calls `progress_callback` periodically with progress updates.
/// Bumps store version after each chunk and marks source as done when complete.
///
/// Returns total number of lines parsed, or error.
pub fn parse_dlt_file_with_progress<P: AsRef<Path>>(
    path: P,
    source: &Arc<SourceData>,
    progress_callback: &ProgressCallback,
    timestamp_source: DltTimestampSource,
) -> Result<usize, String> {
    profiling::scope!("parse_dlt_file_with_progress");
    let path = path.as_ref();

    // Get file size for progress calculation
    let file_size = std::fs::metadata(path)
        .map(|m| m.len())
        .map_err(|e| format!("{e:?}"))?;

    let boot_time = match timestamp_source {
        DltTimestampSource::CalibratedMonotonic => {
            let bt = calc_boot_time_from_file(path)?;
            log::info!("Calculated DLT boot time: {bt}");
            Some(bt)
        }
        DltTimestampSource::StorageTime => {
            log::info!("Using DLT storage time for timestamps");
            None
        }
    };

    // Second pass: parse all messages with the calculated offset
    let file = File::open(path).map_err(|e| format!("Failed to open DLT file: {e}"))?;
    let progress_reader = ProgressReader::new(BufReader::new(file));
    let bytes_read_counter = progress_reader.bytes_read_counter();
    let mut reader = DltMessageReader::new(progress_reader, true);
    let mut chunk_lines = Vec::new();
    let mut line_number = 1;
    let mut last_progress_update = 0u64;

    loop {
        match read_message(&mut reader, None) {
            Ok(Some(dlt_core::parse::ParsedMessage::Item(msg))) => {
                if let Some(log_line) = convert_dlt_message(&msg, line_number, boot_time) {
                    chunk_lines.push(log_line);
                    line_number += 1;

                    // Send chunk when we have enough lines
                    if chunk_lines.len() >= DLT_CHUNK_SIZE {
                        source.append_lines(std::mem::take(&mut chunk_lines));

                        let bytes_read = bytes_read_counter.get();
                        let progress = bytes_read as f32 / file_size as f32;
                        progress_callback(
                            progress,
                            &format!("Parsing DLT... ({} messages)", source.len()),
                        );
                        last_progress_update = bytes_read;
                    } else {
                        // Report progress every ~1MB or 500 messages even without chunk
                        let bytes_read = bytes_read_counter.get();
                        if bytes_read - last_progress_update > 1_000_000 || line_number % 500 == 0 {
                            last_progress_update = bytes_read;
                            let progress = bytes_read as f32 / file_size as f32;
                            progress_callback(
                                progress,
                                &format!(
                                    "Parsing DLT... ({} messages)",
                                    source.len() + chunk_lines.len()
                                ),
                            );
                        }
                    }
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

    // Send any remaining lines
    if !chunk_lines.is_empty() {
        source.append_lines(chunk_lines);
    }

    if source.is_empty() {
        Err("No valid DLT messages found in file".to_string())
    } else {
        log::info!("Parsed {} DLT messages", source.len());
        Ok(source.len())
    }
}

// No parse_dlt_buffer needed; handled by parse_dlt_file

/// Convert a `dlt_core::dlt::Message` to `LogLine`
fn convert_dlt_message(
    msg: &Message,
    line_number: usize,
    boot_time: Option<DateTime<Local>>,
) -> Option<LogLine> {
    // Extract timestamp based on configuration
    let timestamp = if let Some(boot_time) = boot_time {
        // Use calibrated monotonic clock: boot_time + time_since_boot
        let ts = msg.header.timestamp?;
        let time_since_boot = dlt_header_time_to_timedelta(ts);
        boot_time.checked_add_signed(time_since_boot)?
    } else {
        // Use storage time directly
        let storage_header = msg.storage_header.as_ref()?;
        storage_time_to_datetime(&storage_header.timestamp)?
    };

    // Validate message has required components (lazy formatting will extract them later)
    if msg.header.ecu_id.is_none() {
        log::warn!("DLT message missing ECU ID for line {line_number}");
    }
    if msg.header.session_id.is_none() {
        log::warn!("DLT message missing Session ID for line {line_number}");
    }
    if msg.extended_header.is_none() {
        log::error!("DLT message missing Extended Header for line {line_number}");
        return None;
    }
    if msg.storage_header.is_none() {
        log::error!("DLT message missing Storage Header for line {line_number}");
        return None;
    }

    // Return DLT-specific variant - message formatting is now deferred
    Some(LogLine::Dlt(DltLogLine::new(
        msg.clone(),
        timestamp,
        boot_time,
        line_number,
    )))
}
