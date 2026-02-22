// LogCrab - GPL-3.0-or-later
// This file is part of LogCrab.
//
// Copyright (C) 2026 Daniel Freiermuth
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

use chrono::{DateTime, Local};

/// Format time difference with 3 significant digits and appropriate unit
pub fn format_time_diff(diff: chrono::Duration) -> String {
    let sign = if diff < chrono::Duration::zero() {
        "-"
    } else {
        "+"
    };

    // Use absolute value for calculations
    let abs_diff = if diff < chrono::Duration::zero() {
        -diff
    } else {
        diff
    };

    // Select appropriate unit based on magnitude
    let (value, unit) = if abs_diff.num_days().abs() >= 1 {
        // Days (as float for fractional days)
        let total_secs = abs_diff.num_seconds() as f64;
        (total_secs / 86400.0, "d")
    } else if abs_diff.num_hours().abs() >= 1 {
        // Hours
        let total_secs = abs_diff.num_seconds() as f64;
        (total_secs / 3600.0, "h")
    } else if abs_diff.num_minutes().abs() >= 1 {
        // Minutes
        let total_secs = abs_diff.num_seconds() as f64;
        (total_secs / 60.0, "m")
    } else if abs_diff.num_seconds().abs() >= 1 {
        // Seconds
        let total_millis = abs_diff.num_milliseconds() as f64;
        (total_millis / 1000.0, "s")
    } else if let Some(micros) = abs_diff.num_microseconds() {
        if micros.abs() >= 1000 {
            // Milliseconds
            (micros.abs() as f64 / 1000.0, "ms")
        } else if micros.abs() >= 1 {
            // Microseconds
            (micros.abs() as f64, "µs")
        } else if let Some(nanos) = abs_diff.num_nanoseconds() {
            // Nanoseconds
            (nanos.abs() as f64, "ns")
        } else {
            (0.0, "ns")
        }
    } else {
        // Fallback for very large durations
        let total_secs = abs_diff.num_seconds() as f64;
        (total_secs / 86400.0, "d")
    };

    // Format with 3 significant digits
    if value >= 100.0 {
        format!("{sign}{value:>3.0}{unit}")
    } else if value >= 10.0 {
        format!("{sign}{value:>3.1}{unit}")
    } else if value >= 1.0 {
        format!("{sign}{value:>3.2}{unit}")
    } else {
        format!("{sign}{value:>4.3}{unit}")
    }
}

/// Common interface for all log line types
pub trait LogLineCore {
    /// Get the raw timestamp of this log line (without time offsets)
    ///
    /// ⚠️  For timeline operations (sorting, binning, navigation) in multi-source
    /// sessions, use `LogStore::get_adjusted_timestamp()` instead to account for
    /// per-source time calibration offsets.
    fn uncalibrated_timestamp(&self) -> DateTime<Local>;

    /// Get the formatted message (may be constructed lazily)
    fn message(&self) -> String;

    /// Get the raw line as it appeared in the source (may be constructed lazily)
    fn raw(&self) -> String;

    /// Get the normalized template key for anomaly detection (computed lazily)
    fn template_key(&self) -> String;

    /// Get the original line number in the source file
    fn line_number(&self) -> usize;

    /// Get the anomaly score
    fn anomaly_score(&self) -> f64;

    /// Set the anomaly score
    fn set_anomaly_score(&mut self, score: f64);
}

/// Enum wrapping all log line variants
#[derive(Debug, Clone)]
pub enum LogLineVariant {
    Generic(GenericLogLine),
    Logcat(LogcatLogLine),
    Dlt(DltLogLine),
    Pcap(PcapLogLine),
    Btsnoop(BtsnoopLogLine),
}

impl LogLineCore for LogLineVariant {
    fn uncalibrated_timestamp(&self) -> DateTime<Local> {
        match self {
            Self::Generic(l) => l.uncalibrated_timestamp(),
            Self::Logcat(l) => l.uncalibrated_timestamp(),
            Self::Dlt(l) => l.uncalibrated_timestamp(),
            Self::Pcap(l) => l.uncalibrated_timestamp(),
            Self::Btsnoop(l) => l.uncalibrated_timestamp(),
        }
    }

    fn message(&self) -> String {
        match self {
            Self::Generic(l) => l.message(),
            Self::Logcat(l) => l.message(),
            Self::Dlt(l) => l.message(),
            Self::Pcap(l) => l.message(),
            Self::Btsnoop(l) => l.message(),
        }
    }

    fn raw(&self) -> String {
        match self {
            Self::Generic(l) => l.raw(),
            Self::Logcat(l) => l.raw(),
            Self::Dlt(l) => l.raw(),
            Self::Pcap(l) => l.raw(),
            Self::Btsnoop(l) => l.raw(),
        }
    }

    fn template_key(&self) -> String {
        match self {
            Self::Generic(l) => l.template_key(),
            Self::Logcat(l) => l.template_key(),
            Self::Dlt(l) => l.template_key(),
            Self::Pcap(l) => l.template_key(),
            Self::Btsnoop(l) => l.template_key(),
        }
    }

    fn line_number(&self) -> usize {
        match self {
            Self::Generic(l) => l.line_number(),
            Self::Logcat(l) => l.line_number(),
            Self::Dlt(l) => l.line_number(),
            Self::Pcap(l) => l.line_number(),
            Self::Btsnoop(l) => l.line_number(),
        }
    }

    fn anomaly_score(&self) -> f64 {
        match self {
            Self::Generic(l) => l.anomaly_score(),
            Self::Logcat(l) => l.anomaly_score(),
            Self::Dlt(l) => l.anomaly_score(),
            Self::Pcap(l) => l.anomaly_score(),
            Self::Btsnoop(l) => l.anomaly_score(),
        }
    }

    fn set_anomaly_score(&mut self, score: f64) {
        match self {
            Self::Generic(l) => l.set_anomaly_score(score),
            Self::Logcat(l) => l.set_anomaly_score(score),
            Self::Dlt(l) => l.set_anomaly_score(score),
            Self::Pcap(l) => l.set_anomaly_score(score),
            Self::Btsnoop(l) => l.set_anomaly_score(score),
        }
    }
}

// ============================================================================
// Generic Log Line
// ============================================================================

/// Generic text-based log line with timestamp
#[derive(Debug, Clone)]
pub struct GenericLogLine {
    /// Original raw line from file
    raw_line: String,
    /// Parsed timestamp
    pub timestamp: DateTime<Local>,
    /// Message portion (everything after timestamp, or whole line if no timestamp)
    message_text: String,
    /// Original line number in source file
    pub line_number: usize,
    /// Anomaly score (mutable)
    pub anomaly_score: f64,
}

impl GenericLogLine {
    pub const fn new(
        raw_line: String,
        timestamp: DateTime<Local>,
        message_text: String,
        line_number: usize,
    ) -> Self {
        Self {
            raw_line,
            timestamp,
            message_text,
            line_number,
            anomaly_score: 0.0,
        }
    }
}

impl LogLineCore for GenericLogLine {
    fn uncalibrated_timestamp(&self) -> DateTime<Local> {
        self.timestamp
    }

    fn message(&self) -> String {
        self.message_text.clone()
    }

    fn raw(&self) -> String {
        self.raw_line.clone()
    }

    fn template_key(&self) -> String {
        crate::parser::normalize_message(&self.message_text)
    }

    fn line_number(&self) -> usize {
        self.line_number
    }

    fn anomaly_score(&self) -> f64 {
        self.anomaly_score
    }

    fn set_anomaly_score(&mut self, score: f64) {
        self.anomaly_score = score;
    }
}

// ============================================================================
// Logcat Log Line
// ============================================================================

/// Android Logcat format: MM-DD HH:MM:SS.mmm PID TID LEVEL TAG: message
#[derive(Debug, Clone)]
pub struct LogcatLogLine {
    /// Original raw line from file
    raw_line: String,
    /// Parsed timestamp
    pub timestamp: DateTime<Local>,
    /// Message portion (everything after timestamp)
    message_text: String,
    /// Original line number in source file
    pub line_number: usize,
    /// Anomaly score (mutable)
    pub anomaly_score: f64,
}

impl LogcatLogLine {
    pub const fn new(
        raw_line: String,
        timestamp: DateTime<Local>,
        message_text: String,
        line_number: usize,
    ) -> Self {
        Self {
            raw_line,
            timestamp,
            message_text,
            line_number,
            anomaly_score: 0.0,
        }
    }
}

impl LogLineCore for LogcatLogLine {
    fn uncalibrated_timestamp(&self) -> DateTime<Local> {
        self.timestamp
    }

    fn message(&self) -> String {
        self.message_text.clone()
    }

    fn raw(&self) -> String {
        self.raw_line.clone()
    }

    fn template_key(&self) -> String {
        crate::parser::normalize_message(&self.message_text)
    }

    fn line_number(&self) -> usize {
        self.line_number
    }

    fn anomaly_score(&self) -> f64 {
        self.anomaly_score
    }

    fn set_anomaly_score(&mut self, score: f64) {
        self.anomaly_score = score;
    }
}

// ============================================================================
// DLT Log Line
// ============================================================================

/// DLT (Diagnostic Log and Trace) binary format log line
#[derive(Debug, Clone)]
pub struct DltLogLine {
    /// Parsed DLT message structure
    pub dlt_message: dlt_core::dlt::Message,
    /// Parsed timestamp
    pub timestamp: DateTime<Local>,
    /// Boot time offset (for `CalibratedMonotonic` mode)
    pub boot_time: Option<DateTime<Local>>,
    /// Original line number in source file
    pub line_number: usize,
    /// Anomaly score (mutable)
    pub anomaly_score: f64,
}

impl DltLogLine {
    pub const fn new(
        dlt_message: dlt_core::dlt::Message,
        timestamp: DateTime<Local>,
        boot_time: Option<DateTime<Local>>,
        line_number: usize,
    ) -> Self {
        Self {
            dlt_message,
            timestamp,
            boot_time,
            line_number,
            anomaly_score: 0.0,
        }
    }

    /// Format time difference with 3 significant digits and appropriate unit
    fn format_time_diff(diff: chrono::Duration) -> String {
        format_time_diff(diff)
    }

    /// Format DLT message for display (expensive, construct lazily)
    fn format_message(&self) -> String {
        use dlt_core::dlt::PayloadContent;

        // Extract ECU and session info
        let ecu_header = self
            .dlt_message
            .header
            .ecu_id
            .as_deref()
            .unwrap_or("UnknownECU");
        let session_id = self.dlt_message.header.session_id.unwrap_or(0);

        // Extract extended header info (app_id, context_id, message_type)
        let (message_type, app_id, ctx_id) = self.dlt_message.extended_header.as_ref().map_or_else(
            || ("Unknown".to_string(), "", ""),
            |ext_header| {
                (
                    format!("{:?}", ext_header.message_type),
                    ext_header.application_id.as_str(),
                    ext_header.context_id.as_str(),
                )
            },
        );

        // Extract storage time and ECU
        let (storage_ecu, storage_time) = &self.dlt_message.storage_header.as_ref().map_or(
            ("", self.timestamp),
            |storage_header| {
                use chrono::TimeZone;
                let secs = i64::from(storage_header.timestamp.seconds);
                let nsecs = storage_header.timestamp.microseconds * 1000;
                let ts = chrono::Local
                    .timestamp_opt(secs, nsecs)
                    .single()
                    .unwrap_or(self.timestamp);
                (storage_header.ecu_id.as_str(), ts)
            },
        );

        // Extract payload
        let payload = match &self.dlt_message.payload {
            PayloadContent::Verbose(args) => {
                let formatted_args: Vec<String> = args
                    .iter()
                    .map(|arg| {
                        let val_str = match &arg.value {
                            dlt_core::dlt::Value::StringVal(s) => s.clone(),
                            dlt_core::dlt::Value::U32(v) => format!("{v}"),
                            dlt_core::dlt::Value::U64(v) => format!("{v}"),
                            dlt_core::dlt::Value::U8(v) => format!("{v}"),
                            dlt_core::dlt::Value::U16(v) => format!("{v}"),
                            dlt_core::dlt::Value::I32(v) => format!("{v}"),
                            dlt_core::dlt::Value::I64(v) => format!("{v}"),
                            dlt_core::dlt::Value::I8(v) => format!("{v}"),
                            dlt_core::dlt::Value::I16(v) => format!("{v}"),
                            dlt_core::dlt::Value::F32(v) => format!("{v}"),
                            dlt_core::dlt::Value::F64(v) => format!("{v}"),
                            dlt_core::dlt::Value::Bool(v) => format!("{v}"),
                            dlt_core::dlt::Value::U128(v) => format!("{v}"),
                            dlt_core::dlt::Value::I128(v) => format!("{v}"),
                            dlt_core::dlt::Value::Raw(bytes) => format!("{bytes:02x?}"),
                        };

                        arg.name
                            .as_ref()
                            .map(|name| format!("{name}: {val_str}"))
                            .unwrap_or(val_str)
                    })
                    .collect();
                formatted_args.join(" || ")
            }
            PayloadContent::NonVerbose(_, bytes) => format!("{bytes:02x?}"),
            PayloadContent::ControlMsg(_, bytes) => format!("ControlMsg: {bytes:02x?}"),
            PayloadContent::NetworkTrace(traces) => {
                format!("NetworkTrace: {} traces", traces.len())
            }
        };

        // Format message based on whether we have boot_time
        if self.boot_time.is_some() {
            // CalibratedMonotonic mode: show storage time with diff to calibrated time
            let time_diff = storage_time.signed_duration_since(self.timestamp);
            let diff_str = Self::format_time_diff(time_diff);
            format!(
                "[{storage_time} ({diff_str}) {storage_ecu}] {ecu_header} {session_id} {app_id} {ctx_id} {message_type} {payload}"
            )
        } else {
            // StorageTime mode: show monotonic timestamp (header timestamp)
            self.dlt_message.header.timestamp.map_or_else(
                || // Fallback if no header timestamp
                format!(
                    "[{storage_ecu}] {ecu_header} {session_id} {app_id} {ctx_id} {message_type} {payload}"
                ),
                |header_ts| {
                    let monotonic_micros = i64::from(header_ts) * 100; // header timestamp in 0.1ms units
                    let monotonic_secs = monotonic_micros as f64 / 1_000_000.0;
                    format!(
                        "[{monotonic_secs:.3}s {storage_ecu}] {ecu_header} {session_id} {app_id} {ctx_id} {message_type} {payload}"
                    )
                })
        }
    }
}

impl LogLineCore for DltLogLine {
    fn uncalibrated_timestamp(&self) -> DateTime<Local> {
        self.timestamp
    }

    fn message(&self) -> String {
        self.format_message()
    }

    fn raw(&self) -> String {
        // For DLT, "raw" is the debug format of the entire message
        format!("{:?}", self.dlt_message)
    }

    fn template_key(&self) -> String {
        let msg = self.format_message();
        crate::parser::normalize_message(&msg)
    }

    fn line_number(&self) -> usize {
        self.line_number
    }

    fn anomaly_score(&self) -> f64 {
        self.anomaly_score
    }

    fn set_anomaly_score(&mut self, score: f64) {
        self.anomaly_score = score;
    }
}

// ============================================================================
// PCAP Log Line
// ============================================================================

/// PCAP (Packet Capture) format log line representing a network packet
#[derive(Debug, Clone)]
pub struct PcapLogLine {
    /// Parsed packet information
    pub packet_info: crate::parser::pcap::PacketInfo,
    /// Original packet number in source file
    pub line_number: usize,
    /// Anomaly score (mutable)
    pub anomaly_score: f64,
}

impl PcapLogLine {
    pub const fn new(packet_info: crate::parser::pcap::PacketInfo, line_number: usize) -> Self {
        Self {
            packet_info,
            line_number,
            anomaly_score: 0.0,
        }
    }
}

impl LogLineCore for PcapLogLine {
    fn uncalibrated_timestamp(&self) -> DateTime<Local> {
        self.packet_info.timestamp
    }

    fn message(&self) -> String {
        self.packet_info.format_message()
    }

    fn raw(&self) -> String {
        self.packet_info.format_raw()
    }

    fn template_key(&self) -> String {
        // For pcap, normalize the protocol + port combination
        crate::parser::normalize_message(&self.packet_info.format_message())
    }

    fn line_number(&self) -> usize {
        self.line_number
    }

    fn anomaly_score(&self) -> f64 {
        self.anomaly_score
    }

    fn set_anomaly_score(&mut self, score: f64) {
        self.anomaly_score = score;
    }
}

// ============================================================================
// BTSnoop Log Line
// ============================================================================

/// `BTSnoop` (Bluetooth HCI log) format log line representing an HCI packet
#[derive(Debug, Clone)]
pub struct BtsnoopLogLine {
    /// Parsed HCI packet information
    pub hci_info: crate::parser::btsnoop::HciPacketInfo,
    /// Original packet number in source file
    pub line_number: usize,
    /// Anomaly score (mutable)
    pub anomaly_score: f64,
}

impl BtsnoopLogLine {
    pub const fn new(hci_info: crate::parser::btsnoop::HciPacketInfo, line_number: usize) -> Self {
        Self {
            hci_info,
            line_number,
            anomaly_score: 0.0,
        }
    }
}

impl LogLineCore for BtsnoopLogLine {
    fn uncalibrated_timestamp(&self) -> DateTime<Local> {
        self.hci_info.timestamp
    }

    fn message(&self) -> String {
        self.hci_info.format_message()
    }

    fn raw(&self) -> String {
        self.hci_info.format_raw()
    }

    fn template_key(&self) -> String {
        // For btsnoop, normalize the packet type + direction combination
        crate::parser::normalize_message(&self.hci_info.format_message())
    }

    fn line_number(&self) -> usize {
        self.line_number
    }

    fn anomaly_score(&self) -> f64 {
        self.anomaly_score
    }

    fn set_anomaly_score(&mut self, score: f64) {
        self.anomaly_score = score;
    }
}
