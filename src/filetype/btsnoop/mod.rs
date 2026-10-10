// LogCrab - GPL-3.0-or-later
// Copyright (C) 2026 Daniel Freiermuth

mod avrcp;
mod hci;
mod hfp;
mod rfcomm;

use chrono::{DateTime, Local};
use egui::Ui;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::filetype::{BinaryFileType, InputFileType, LineType};

pub use hci::HciPacketInfo;

// ============================================================================
// BtsnoopLogLine
// ============================================================================

/// `BTSnoop` (Bluetooth HCI log) format log line representing an HCI packet
#[derive(Debug, Clone)]
pub struct BtsnoopLogLine {
    /// Parsed HCI packet information
    pub hci_info: HciPacketInfo,
    /// Original packet number in source file
    pub line_number: usize,
}

impl BtsnoopLogLine {
    #[must_use]
    pub const fn new(hci_info: HciPacketInfo, line_number: usize) -> Self {
        Self {
            hci_info,
            line_number,
        }
    }
}

// ============================================================================
// BtsnoopFileState
// ============================================================================

/// Type alias kept for compatibility; the shared [`crate::filetype::SimpleFileState`]
/// provides all interior-mutable time-offset and calibration state.
pub type BtsnoopFileState = crate::filetype::SimpleFileState;

// ============================================================================
// LineType implementation
// ============================================================================

impl LineType for BtsnoopLogLine {
    type Config = ();
    type FileState = BtsnoopFileState;

    fn file_state_from_v2(time_offset_ms: i64) -> BtsnoopFileState {
        let s = BtsnoopFileState::default();
        s.set_time_offset_ms(time_offset_ms);
        s
    }

    fn timestamp(&self, _config: &(), file_state: &BtsnoopFileState) -> DateTime<Local> {
        self.hci_info.timestamp + chrono::Duration::milliseconds(file_state.time_offset_ms())
    }

    fn message(&self) -> String {
        self.hci_info.format_message()
    }

    fn display_message(&self, _config: &(), file_state: &BtsnoopFileState) -> String {
        let offset_ms = file_state.time_offset_ms();
        if offset_ms != 0 {
            format!(
                "[{}] {}",
                crate::parser::format_time_diff(chrono::Duration::milliseconds(offset_ms)),
                self.message()
            )
        } else {
            self.hci_info.format_message()
        }
    }

    fn raw(&self) -> String {
        self.hci_info.format_raw()
    }

    fn line_number(&self) -> usize {
        self.line_number
    }

    fn egui_render_context_menu(&self, ui: &mut Ui, _config: &(), file_state: &BtsnoopFileState) {
        file_state.render_calibration_context_menu(ui, self.hci_info.timestamp);
    }
}

// ============================================================================
// BtsnoopFileType (InputFileType + BinaryFileType)
// ============================================================================

/// Stateful reader for Bluetooth HCI logs in the `BTSnoop` format.
///
/// All packets are parsed eagerly at `open()` time (the `btsnoop` crate requires the
/// full file in memory), then drained in chunks via `read()`.
pub struct BtsnoopFileType {
    lines: Vec<BtsnoopLogLine>,
    cursor: usize,
    file_size: u64,
}

impl InputFileType for BtsnoopFileType {
    type LineType = BtsnoopLogLine;

    const FILE_EXTENSIONS: &'static [&'static str] = &["log", "btsnoop"];

    fn open(
        path: &Path,
        _config: (),
        _file_state: std::sync::Arc<BtsnoopFileState>,
    ) -> anyhow::Result<Self> {
        let file_size = std::fs::metadata(path).map_or(0, |m| m.len());
        let lines = parse_btsnoop_to_lines(path)?;
        Ok(Self {
            lines,
            cursor: 0,
            file_size,
        })
    }

    fn read(&mut self, lines_to_read: usize) -> anyhow::Result<Vec<Self::LineType>> {
        let end = (self.cursor + lines_to_read).min(self.lines.len());
        let batch = self.lines[self.cursor..end].to_vec();
        self.cursor = end;
        Ok(batch)
    }

    fn bytes_consumed(&self) -> u64 {
        let total = self.lines.len();
        if total == 0 {
            return self.file_size;
        }
        (self.cursor as f64 / total as f64 * self.file_size as f64) as u64
    }
}

impl BinaryFileType for BtsnoopFileType {
    /// `BTSnoop` file magic: `btsnoop\0` (8 bytes)
    const MAGIC_BYTES: &'static [&'static [u8]] = &[b"btsnoop\0"];
}

// ============================================================================
// BTSnoop file reader
// ============================================================================

/// Parse all HCI packets from a btsnoop file and return them as typed log lines.
///
/// All packets are parsed eagerly since the `btsnoop` crate requires the entire file to be
/// in memory.
fn parse_btsnoop_to_lines<P: AsRef<Path>>(path: P) -> anyhow::Result<Vec<BtsnoopLogLine>> {
    profiling::scope!("parse_btsnoop_to_lines");
    use anyhow::Context as _;
    let path = path.as_ref();
    tracing::info!("Starting btsnoop parsing: {}", path.display());

    let mut file = File::open(path)
        .with_context(|| format!("Failed to open btsnoop file: {}", path.display()))?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .with_context(|| format!("Failed to read btsnoop file: {}", path.display()))?;

    let btsnoop_file = btsnoop::parse_btsnoop_file(&buffer)
        .map_err(|e| anyhow::anyhow!("Failed to parse btsnoop file: {e:?}"))?;

    let mut lines = Vec::with_capacity(btsnoop_file.packets.len());
    let mut line_number = 1usize;

    for packet in &btsnoop_file.packets {
        let Some(timestamp) = packet_timestamp(packet.header.timestamp_microseconds) else {
            tracing::warn!("Failed to convert packet timestamp at line {line_number}, skipping");
            line_number += 1;
            continue;
        };

        if let Some(hci_info) = hci::parse_hci_packet(packet, timestamp) {
            lines.push(BtsnoopLogLine::new(hci_info, line_number));
        }
        line_number += 1;
    }

    tracing::info!("Parsed {} HCI packets from btsnoop file", lines.len());
    Ok(lines)
}

/// Microseconds between 0000-01-01 (the btsnoop epoch) and 1970-01-01 (the unix epoch).
const BTSNOOP_UNIX_EPOCH_OFFSET_US: i64 = 0x00dc_ddb3_0f2f_8000;

/// Convert a raw btsnoop packet timestamp to a local date-time.
///
/// Returns `None` for timestamps before the unix epoch or outside chrono's representable range.
/// Deliberately avoids `btsnoop::PacketHeader::timestamp()`, which panics on pre-1970 values.
fn packet_timestamp(timestamp_microseconds: i64) -> Option<DateTime<Local>> {
    let us_since_unix = timestamp_microseconds.checked_sub(BTSNOOP_UNIX_EPOCH_OFFSET_US)?;
    if us_since_unix < 0 {
        return None;
    }
    DateTime::from_timestamp_micros(us_since_unix).map(|utc| utc.with_timezone(&Local))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::sync::Arc;

    /// 2000-01-01T00:00:00Z in btsnoop microseconds (since 0000-01-01).
    const TS_2000: i64 = 0x00E0_3AB4_4A67_6000;
    /// `HCI_Reset` command (H4 type 0x01, opcode 0x0c03, no params).
    const HCI_RESET: &[u8] = &[0x01, 0x03, 0x0c, 0x00];

    /// Serialize a btsnoop v1 file (big-endian, HCI UART datalink).
    fn btsnoop_bytes(packets: &[(i64, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"btsnoop\0");
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&1002u32.to_be_bytes());
        for (ts, data) in packets {
            let len = u32::try_from(data.len()).expect("packet len");
            buf.extend_from_slice(&len.to_be_bytes()); // original_length
            buf.extend_from_slice(&len.to_be_bytes()); // included_length
            buf.extend_from_slice(&0u32.to_be_bytes()); // flags: Sent, Data
            buf.extend_from_slice(&0u32.to_be_bytes()); // cumulative drops
            buf.extend_from_slice(&ts.to_be_bytes());
            buf.extend_from_slice(data);
        }
        buf
    }

    fn open_bytes(bytes: &[u8]) -> BtsnoopFileType {
        let mut tmp = tempfile::NamedTempFile::new().expect("tmpfile");
        tmp.write_all(bytes).expect("write");
        BtsnoopFileType::open(tmp.path(), (), Arc::new(BtsnoopFileState::default()))
            .expect("open btsnoop")
    }

    #[test]
    fn unconvertible_timestamps_are_skipped_without_panicking() {
        let bytes = btsnoop_bytes(&[
            (TS_2000, HCI_RESET),
            (0, HCI_RESET),        // zeroed timestamp: before the unix epoch
            (i64::MAX, HCI_RESET), // beyond the representable date range
            (i64::MIN, HCI_RESET), // would overflow the epoch subtraction
            (TS_2000 + 1_500_000, HCI_RESET),
        ]);
        let lines = open_bytes(&bytes).read(100).expect("read");

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].hci_info.timestamp.timestamp(), 946_684_800);
        assert_eq!(
            lines[1].hci_info.timestamp.timestamp_micros(),
            946_684_801_500_000
        );
    }

    #[test]
    fn skipped_packets_keep_physical_line_numbers() {
        let bytes = btsnoop_bytes(&[
            (TS_2000, HCI_RESET),
            (0, HCI_RESET), // bad timestamp
            (TS_2000, &[]), // empty payload: not decodable as HCI
            (TS_2000, HCI_RESET),
        ]);
        let lines = open_bytes(&bytes).read(100).expect("read");

        let numbers: Vec<usize> = lines.iter().map(|l| l.line_number).collect();
        assert_eq!(numbers, vec![1, 4]);
    }

    #[test]
    fn header_only_file_is_fully_consumed() {
        let bytes = btsnoop_bytes(&[]);
        let mut reader = open_bytes(&bytes);

        assert!(reader.read(10).expect("read").is_empty());
        assert_eq!(reader.bytes_consumed(), bytes.len() as u64);
    }

    #[test]
    fn chunked_reads_return_every_packet_once() {
        let packets: Vec<(i64, &[u8])> = (0..5).map(|i| (TS_2000 + i * 1_000, HCI_RESET)).collect();
        let bytes = btsnoop_bytes(&packets);
        let mut reader = open_bytes(&bytes);

        let mut seen = Vec::new();
        let mut last_consumed = reader.bytes_consumed();
        assert_eq!(last_consumed, 0);
        loop {
            let batch = reader.read(2).expect("read");
            if batch.is_empty() {
                break;
            }
            assert!(batch.len() <= 2);
            let consumed = reader.bytes_consumed();
            assert!(consumed > last_consumed);
            last_consumed = consumed;
            seen.extend(batch.iter().map(|l| l.line_number));
        }

        assert_eq!(seen, vec![1, 2, 3, 4, 5]);
        assert_eq!(reader.bytes_consumed(), bytes.len() as u64);
    }
}
