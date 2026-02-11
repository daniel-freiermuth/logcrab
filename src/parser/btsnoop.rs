// LogCrab - GPL-3.0-or-later
// This file is part of LogCrab.
//
// Copyright (C) 2025 Daniel Freiermuth
//
// LogCrab is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// LogCrab is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with LogCrab.  If not, see <https://www.gnu.org/licenses/>.

//! BTSnoop (Bluetooth HCI log) file parser for Bluetooth traffic analysis.
//!
//! Supports Android btsnoop logs and btmon format.

use crate::core::log_file::ProgressCallback;
use crate::core::log_store::SourceData;

use super::line::{BtsnoopLogLine, LogLine};
use chrono::{DateTime, Local, TimeZone};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

/// Initial chunk size for incremental loading
const BTSNOOP_INITIAL_CHUNK_SIZE: usize = 1 << 12; // 4,096 packets
const BTSNOOP_MAX_CHUNK_SIZE: usize = 1 << 18; // 262,144 packets
const BTSNOOP_CHUNKS_BEFORE_GROWTH: usize = 3; // Double chunk size every 3 chunks

/// Represents a parsed HCI packet for display
#[derive(Debug, Clone)]
pub struct HciPacketInfo {
    /// Packet timestamp
    pub timestamp: DateTime<Local>,
    /// HCI packet type (Command, Event, ACL Data, SCO Data, etc.)
    pub packet_type: String,
    /// Direction (Sent/Received or Host→Controller/Controller→Host)
    pub direction: String,
    /// Packet length in bytes
    pub length: u32,
    /// Brief packet info (opcode, event code, handle, etc.)
    pub info: String,
}

impl HciPacketInfo {
    /// Format as a display message
    pub fn format_message(&self) -> String {
        if self.info.is_empty() {
            format!(
                "{} {} {} Len={}",
                self.packet_type, self.direction, self.info, self.length
            )
        } else {
            format!(
                "{} {} {} Len={}",
                self.packet_type, self.direction, self.info, self.length
            )
        }
    }

    /// Format as raw line (more detailed)
    pub fn format_raw(&self) -> String {
        format!(
            "[{}] {} {} {} Length={}",
            self.timestamp.format("%H:%M:%S%.6f"),
            self.packet_type,
            self.direction,
            self.info,
            self.length
        )
    }
}

/// Parse HCI packet data and extract packet info
fn parse_hci_packet(
    packet: &btsnoop::Packet,
    timestamp: DateTime<Local>,
) -> Option<HciPacketInfo> {
    profiling::scope!("parse_hci_packet");

    let data = &packet.packet_data;
    if data.is_empty() {
        return None;
    }

    // Determine direction from flags
    let direction = match packet.header.packet_flags.direction {
        btsnoop::DirectionFlag::Sent => "Sent",
        btsnoop::DirectionFlag::Received => "Rcvd",
    };

    // Parse HCI packet type (first byte for some formats, or from flags)
    let (packet_type, info) = parse_hci_type_and_info(data, &packet.header.packet_flags);

    Some(HciPacketInfo {
        timestamp,
        packet_type,
        direction: direction.to_string(),
        length: packet.header.original_length,
        info,
    })
}

/// Parse HCI packet type and extract basic info
fn parse_hci_type_and_info(data: &[u8], _flags: &btsnoop::PacketFlags) -> (String, String) {
    if data.is_empty() {
        return ("Unknown".to_string(), String::new());
    }

    // Parse based on HCI packet type indicator (first byte in most formats)
    match data.first() {
        Some(0x01) => {
            // HCI Command packet
            if data.len() >= 3 {
                let opcode = u16::from_le_bytes([data[1], data[2]]);
                let param_len = data[3];

                let cmd_name = get_hci_command_name(opcode);
                (
                    "HCI_CMD".to_string(),
                    format!("{cmd_name} (0x{opcode:04x}) ParamLen={param_len}"),
                )
            } else {
                ("HCI_CMD".to_string(), "Truncated".to_string())
            }
        }
        Some(0x02) => {
            // ACL Data
            if data.len() >= 5 {
                let handle = u16::from_le_bytes([data[1], data[2]]) & 0x0FFF;
                let pb_flag = (data[2] >> 4) & 0x03;
                let bc_flag = (data[2] >> 6) & 0x03;
                let data_len = u16::from_le_bytes([data[3], data[4]]);
                (
                    "ACL_DATA".to_string(),
                    format!("Handle=0x{handle:04x} PB={pb_flag} BC={bc_flag} Len={data_len}"),
                )
            } else {
                ("ACL_DATA".to_string(), "Truncated".to_string())
            }
        }
        Some(0x03) => {
            // SCO Data
            if data.len() >= 4 {
                let handle = u16::from_le_bytes([data[1], data[2]]) & 0x0FFF;
                let data_len = data[3];
                (
                    "SCO_DATA".to_string(),
                    format!("Handle=0x{handle:04x} Len={data_len}"),
                )
            } else {
                ("SCO_DATA".to_string(), "Truncated".to_string())
            }
        }
        Some(0x04) => {
            // HCI Event
            if data.len() >= 3 {
                let event_code = data[1];
                let param_len = data[2];
                let event_name = get_hci_event_name(event_code);
                (
                    "HCI_EVT".to_string(),
                    format!("{event_name} (0x{event_code:02x}) ParamLen={param_len}"),
                )
            } else {
                ("HCI_EVT".to_string(), "Truncated".to_string())
            }
        }
        Some(0x05) => {
            // ISO Data
            ("ISO_DATA".to_string(), String::new())
        }
        _ => {
            // Unknown packet type
            ("HCI".to_string(), format!("Type=0x{:02x}", data[0]))
        }
    }
}

/// Get human-readable HCI command name from opcode
fn get_hci_command_name(opcode: u16) -> &'static str {
    match opcode {
        0x0000 => "NOP",
        0x0401 => "Inquiry",
        0x0402 => "Inquiry_Cancel",
        0x0405 => "Create_Connection",
        0x0406 => "Disconnect",
        0x0408 => "Accept_Connection_Request",
        0x0409 => "Reject_Connection_Request",
        0x040B => "Authentication_Requested",
        0x040D => "Set_Connection_Encryption",
        0x0419 => "Remote_Name_Request",
        0x041A => "Remote_Name_Request_Cancel",
        0x041F => "Read_Remote_Extended_Features",
        0x0801 => "Hold_Mode",
        0x0803 => "Sniff_Mode",
        0x0804 => "Exit_Sniff_Mode",
        0x0C01 => "Set_Event_Mask",
        0x0C03 => "Reset",
        0x0C13 => "Change_Local_Name",
        0x0C14 => "Read_Local_Name",
        0x0C23 => "Read_Class_Of_Device",
        0x0C24 => "Write_Class_Of_Device",
        0x1001 => "Read_Local_Version_Information",
        0x1002 => "Read_Local_Supported_Commands",
        0x1003 => "Read_Local_Supported_Features",
        0x1005 => "Read_Buffer_Size",
        0x1009 => "Read_BD_ADDR",
        0x200D => "LE_Set_Scan_Enable",
        0x200E => "LE_Create_Connection",
        0x2010 => "LE_Read_Remote_Used_Features",
        0x2013 => "LE_Connection_Update",
        _ => "Unknown_Command",
    }
}

/// Get human-readable HCI event name from event code
fn get_hci_event_name(event_code: u8) -> &'static str {
    match event_code {
        0x01 => "Inquiry_Complete",
        0x02 => "Inquiry_Result",
        0x03 => "Connection_Complete",
        0x04 => "Connection_Request",
        0x05 => "Disconnection_Complete",
        0x06 => "Authentication_Complete",
        0x07 => "Remote_Name_Request_Complete",
        0x08 => "Encryption_Change",
        0x0B => "Read_Remote_Supported_Features_Complete",
        0x0C => "Read_Remote_Version_Information_Complete",
        0x0E => "Command_Complete",
        0x0F => "Command_Status",
        0x13 => "Number_Of_Completed_Packets",
        0x16 => "Encryption_Key_Refresh_Complete",
        0x17 => "IO_Capability_Request",
        0x18 => "IO_Capability_Response",
        0x1B => "Simple_Pairing_Complete",
        0x23 => "Read_Remote_Extended_Features_Complete",
        0x3E => "LE_Meta_Event",
        _ => "Unknown_Event",
    }
}

/// Parse a btsnoop file with incremental loading
///
/// Appends parsed lines directly to `source` in batches for progressive display.
/// Calls `progress_callback` periodically with progress updates.
///
/// Returns total number of packets parsed, or error.
pub fn parse_btsnoop_file_with_progress<P: AsRef<Path>>(
    path: P,
    source: &Arc<SourceData>,
    progress_callback: &ProgressCallback,
) -> Result<usize, String> {
    profiling::scope!("parse_btsnoop_file_with_progress");
    let path = path.as_ref();
    log::info!("Starting btsnoop parsing: {}", path.display());

    let start_time = std::time::Instant::now();

    // Read entire file (btsnoop crate requires byte slice)
    let mut file = File::open(path).map_err(|e| format!("Failed to open btsnoop file: {e}"))?;
    let file_size = file
        .metadata()
        .map(|m| m.len())
        .unwrap_or(0);

    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .map_err(|e| format!("Failed to read btsnoop file: {e}"))?;

    log::info!("Read {} bytes into memory", buffer.len());

    // Parse the btsnoop file
    let btsnoop_file = btsnoop::parse_btsnoop_file(&buffer)
        .map_err(|e| format!("Failed to parse btsnoop file: {e:?}"))?;

    log::info!(
        "Parsed btsnoop header: datalink={:?}, {} packets",
        btsnoop_file.header.datalink_type,
        btsnoop_file.packets.len()
    );

    // Convert packets to log lines incrementally
    let mut chunk_lines = Vec::new();
    let mut line_number = 1;
    let mut last_log_time = std::time::Instant::now();
    let mut packets_since_log = 0;
    let mut chunk_count = 0;
    let mut current_chunk_size = BTSNOOP_INITIAL_CHUNK_SIZE;

    for packet in &btsnoop_file.packets {
        // Convert timestamp from microseconds since epoch
        let timestamp_micros = packet.header.timestamp_microseconds;
        let timestamp_secs = i64::try_from(timestamp_micros / 1_000_000)
            .unwrap_or_else(|_| Local::now().timestamp());
        let timestamp_nanos = ((timestamp_micros % 1_000_000) * 1_000) as u32;

        let timestamp = Local
            .timestamp_opt(timestamp_secs, timestamp_nanos)
            .single()
            .unwrap_or_else(Local::now);

        if let Some(hci_info) = parse_hci_packet(packet, timestamp) {
            let log_line = LogLine::Btsnoop(BtsnoopLogLine::new(hci_info, line_number));
            chunk_lines.push(log_line);
            line_number += 1;
            packets_since_log += 1;

            if chunk_lines.len() >= current_chunk_size {
                source.append_lines(std::mem::take(&mut chunk_lines));
                chunk_count += 1;

                // Grow chunk size exponentially
                if chunk_count % BTSNOOP_CHUNKS_BEFORE_GROWTH == 0
                    && current_chunk_size < BTSNOOP_MAX_CHUNK_SIZE
                {
                    current_chunk_size = (current_chunk_size * 2).min(BTSNOOP_MAX_CHUNK_SIZE);
                    log::debug!("Increased chunk size to {} packets", current_chunk_size);
                }

                let progress = source.len() as f32 / btsnoop_file.packets.len() as f32;
                progress_callback(
                    progress,
                    &format!("Parsing btsnoop... ({} packets)", source.len()),
                );

                // Log performance every 5 seconds
                let now = std::time::Instant::now();
                if now.duration_since(last_log_time).as_secs() >= 5 {
                    let elapsed = now.duration_since(start_time).as_secs_f64();
                    let rate = packets_since_log as f64
                        / now.duration_since(last_log_time).as_secs_f64();
                    log::info!(
                        "Parsed {} packets in {:.2}s ({:.0} pkt/s)",
                        source.len(),
                        elapsed,
                        rate
                    );
                    last_log_time = now;
                    packets_since_log = 0;
                }
            }
        }
    }

    // Send any remaining lines
    if !chunk_lines.is_empty() {
        source.append_lines(chunk_lines);
    }

    let elapsed = start_time.elapsed();
    let total_packets = source.len();
    log::info!(
        "Completed btsnoop parsing: {} packets in {:.2}s ({:.0} pkt/s, {} MB total)",
        total_packets,
        elapsed.as_secs_f64(),
        total_packets as f64 / elapsed.as_secs_f64(),
        file_size / 1_048_576
    );

    if source.is_empty() {
        Err("No valid HCI packets found in btsnoop file".to_string())
    } else {
        Ok(source.len())
    }
}
