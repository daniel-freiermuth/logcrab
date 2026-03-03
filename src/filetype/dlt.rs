// LogCrab - GPL-3.0-or-later
// Copyright (C) 2026 Daniel Freiermuth

use chrono::{DateTime, Local};
use dlt_core::read::{read_message, DltMessageReader};
use egui::Ui;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use crate::filetype::{BinaryFileType, EguiConfig, InputFileType, LineType};
use crate::parser::format_time_diff;

// ============================================================================
// DltLogLine
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

    /// Format DLT message for display (expensive, construct lazily)
    fn format_message(&self) -> String {
        use dlt_core::dlt::PayloadContent;

        let ecu_header = self
            .dlt_message
            .header
            .ecu_id
            .as_deref()
            .unwrap_or("UnknownECU");
        let session_id = self.dlt_message.header.session_id.unwrap_or(0);

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

        if self.boot_time.is_some() {
            let time_diff = storage_time.signed_duration_since(self.timestamp);
            let diff_str = format_time_diff(time_diff);
            format!(
                "[{storage_time} ({diff_str}) {storage_ecu}] {ecu_header} {session_id} {app_id} {ctx_id} {message_type} {payload}"
            )
        } else {
            self.dlt_message.header.timestamp.map_or_else(
                || {
                    format!(
                        "[{storage_ecu}] {ecu_header} {session_id} {app_id} {ctx_id} {message_type} {payload}"
                    )
                },
                |header_ts| {
                    let monotonic_micros = i64::from(header_ts) * 100;
                    let monotonic_secs = monotonic_micros as f64 / 1_000_000.0;
                    format!(
                        "[{monotonic_secs:.3}s {storage_ecu}] {ecu_header} {session_id} {app_id} {ctx_id} {message_type} {payload}"
                    )
                },
            )
        }
    }
}

// ============================================================================
// DltFileState
// ============================================================================

/// Per-source persistent state for DLT log sources.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DltFileState {
    /// Per-source time calibration offset applied on top of parsed timestamps.
    #[serde(default)]
    pub time_offset_ms: i64,

    /// Open calibration window (created by `egui_render_context_menu`).
    /// Driven each frame by `egui_render_file_state`; `None` when no calibration is in progress.
    #[serde(skip)]
    pub calibration: Option<crate::filetype::CalibrationState>,
}

// ============================================================================
// EguiConfig for DltTimestampSource
// ============================================================================

impl EguiConfig for crate::config::DltTimestampSource {
    fn egui_render(&mut self, ui: &mut Ui) -> bool {
        use crate::config::DltTimestampSource;
        ui.separator();
        ui.label("DLT Timestamp Source:");
        let mut changed = false;
        ui.horizontal(|ui| {
            changed |= ui
                .selectable_value(self, DltTimestampSource::StorageTime, "Storage Timestamp")
                .changed();
            changed |= ui
                .selectable_value(
                    self,
                    DltTimestampSource::CalibratedMonotonic,
                    "Derive From Monotonic",
                )
                .on_hover_text("More precise in limited timespans")
                .changed();
        });
        changed
    }
}

// ============================================================================
// LineType implementation
// ============================================================================

impl LineType for DltLogLine {
    /// `DltTimestampSource` selects between storage-header wall-clock time and
    /// calibrated monotonic timestamps.  Shared across all DLT sources in a
    /// session via `Arc<RwLock<DltTimestampSource>>`.
    type Config = crate::config::DltTimestampSource;
    type FileState = DltFileState;

    fn file_state_from_v2(time_offset_ms: i64) -> DltFileState {
        DltFileState {
            time_offset_ms,
            ..Default::default()
        }
    }

    fn timestamp(
        &self,
        _config: &crate::config::DltTimestampSource,
        file_state: &DltFileState,
    ) -> DateTime<Local> {
        self.timestamp + chrono::Duration::milliseconds(file_state.time_offset_ms)
    }

    fn message(&self) -> String {
        self.format_message()
    }

    fn display_message(&self, _file_state: &DltFileState) -> String {
        // DLT already embeds the storage time (and calibration diff) inside
        // the formatted message brackets — no extra offset prefix needed.
        self.message()
    }

    fn raw(&self) -> String {
        format!("{:?}", self.dlt_message)
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

    fn egui_render_context_menu(
        &self,
        ui: &mut Ui,
        config: &crate::config::DltTimestampSource,
        file_state: &mut DltFileState,
    ) {
        if ui.button("\u{23F1} Calibrate Time Here").clicked() {
            use crate::config::DltTimestampSource;

            let display_time =
                self.timestamp + chrono::Duration::milliseconds(file_state.time_offset_ms);
            let storage_time = self
                .dlt_message
                .storage_header
                .as_ref()
                .and_then(|sh| storage_time_to_datetime(&sh.timestamp));
            let is_dlt = matches!(config, DltTimestampSource::CalibratedMonotonic)
                && self.boot_time.is_some();

            file_state.calibration = Some((
                self.timestamp,
                crate::filetype::CalibrationWindow::new(
                    display_time,
                    is_dlt,
                    Some(display_time),
                    storage_time,
                ),
            ));
            ui.close();
        }
    }

}

impl crate::filetype::LogFileState for DltFileState {
    fn egui_render_file_state(&mut self, ui: &egui::Ui) -> bool {
        if let Some(offset_ms) = crate::filetype::render_calibration(ui, &mut self.calibration) {
            self.time_offset_ms = offset_ms;
            true
        } else {
            false
        }
    }
}

// ============================================================================
// DltFileType (InputFileType + BinaryFileType)
// ============================================================================

/// Minimal `Read` wrapper that counts bytes consumed, used for `ChunkedLoader` progress.
struct ByteCountReader<R> {
    inner: R,
    count: Arc<AtomicU64>,
}

impl<R: Read> ByteCountReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            count: Arc::new(AtomicU64::new(0)),
        }
    }

    fn bytes_read_arc(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.count)
    }
}

impl<R: Read> Read for ByteCountReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.count.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

/// Stateful streaming reader for AUTOSAR Diagnostic Log and Trace (`.dlt`) files.
///
/// Each `read(n)` call drives `DltMessageReader` to parse up to `n` DLT messages,
/// maintaining per-`(ECU, App)` boot-time state for `CalibratedMonotonic` mode.
pub struct DltFileType {
    reader: DltMessageReader<ByteCountReader<BufReader<File>>>,
    boot_times: HashMap<(String, String), DateTime<Local>>,
    timestamp_source: crate::config::DltTimestampSource,
    bytes_read_rc: Arc<AtomicU64>,
    file_size: u64,
    line_number: usize,
}

impl DltFileType {
    pub const fn file_size(&self) -> u64 {
        self.file_size
    }
}

impl InputFileType for DltFileType {
    type LineType = DltLogLine;

    const FILE_EXTENSIONS: &'static [&'static str] = &["dlt"];

    /// Open a DLT file for pull-based reading.
    fn open(
        path: &Path,
        config: crate::config::DltTimestampSource,
    ) -> Result<Self, String> {
        let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let file =
            File::open(path).map_err(|e| format!("Failed to open {}: {e}", path.display()))?;
        let inner = ByteCountReader::new(BufReader::new(file));
        let bytes_read_rc = inner.bytes_read_arc();
        let reader = DltMessageReader::new(inner, true);
        Ok(Self {
            reader,
            boot_times: HashMap::new(),
            timestamp_source: config,
            bytes_read_rc,
            file_size,
            line_number: 1,
        })
    }

    fn read(&mut self, lines_to_read: usize) -> Result<Vec<Self::LineType>, String> {
        use crate::config::DltTimestampSource;

        let use_calibrated =
            matches!(self.timestamp_source, DltTimestampSource::CalibratedMonotonic);
        let mut result = Vec::with_capacity(lines_to_read);

        // Safety cap: avoid spinning on files with many un-parseable messages.
        let attempt_limit = lines_to_read * 10 + 64;
        let mut attempts = 0;

        while result.len() < lines_to_read && attempts < attempt_limit {
            attempts += 1;
            match read_message(&mut self.reader, None) {
                Ok(Some(dlt_core::parse::ParsedMessage::Item(msg))) => {
                    let boot_time = if use_calibrated {
                        let ecu_id = msg.header.ecu_id.as_ref().map(ToString::to_string);
                        let app_id = msg
                            .extended_header
                            .as_ref()
                            .map(|ext| ext.application_id.clone());
                        if let (Some(ecu), Some(app)) = (ecu_id, app_id) {
                            let key = (ecu.clone(), app.clone());
                            if !self.boot_times.contains_key(&key) {
                                if let Some(bt) = calc_boot_time_from_message(&msg) {
                                    self.boot_times.insert(key.clone(), bt);
                                }
                            }
                            self.boot_times.get(&key).copied()
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if let Some(line) = convert_dlt_message(&msg, self.line_number, boot_time) {
                        result.push(line);
                        self.line_number += 1;
                    }
                }
                Ok(Some(_)) => {} // skip non-Item messages (e.g. skipped bytes)
                Ok(None) => break, // EOF
                Err(e) => {
                    log::warn!("Failed to parse DLT message: {e:?}");
                    // continue — DLT files sometimes have minor corruption
                }
            }
        }

        Ok(result)
    }

    fn bytes_consumed(&self) -> u64 {
        self.bytes_read_rc.load(Ordering::Relaxed)
    }
}

impl BinaryFileType for DltFileType {
    /// DLT storage header magic: `DLT\x01`
    const MAGIC_BYTES: &'static [&'static [u8]] = &[b"DLT\x01"];
}

// ============================================================================
// DLT parsing utilities (moved from parser/dlt.rs)
// ============================================================================

/// Returns the offset to add to header timestamps (time since boot) to get absolute time
pub(crate) fn calc_boot_time_from_message(msg: &dlt_core::dlt::Message) -> Option<DateTime<Local>> {
    let storage_time = msg
        .storage_header
        .as_ref()
        .and_then(|sh| storage_time_to_datetime(&sh.timestamp))?;
    let boot_time_offset = msg.header.timestamp.map(dlt_header_time_to_timedelta)?;
    storage_time.checked_sub_signed(boot_time_offset)
}

pub fn storage_time_to_datetime(
    storage_time: &dlt_core::dlt::DltTimeStamp,
) -> Option<DateTime<Local>> {
    use chrono::TimeZone;
    Local
        .timestamp_opt(
            i64::from(storage_time.seconds),
            storage_time.microseconds * 1000,
        )
        .single()
}

pub const fn dlt_header_time_to_timedelta(header_time: u32) -> chrono::TimeDelta {
    chrono::TimeDelta::microseconds(header_time as i64 * 100)
}

/// Convert a `dlt_core::dlt::Message` to `DltLogLine`.
pub(crate) fn convert_dlt_message(
    msg: &dlt_core::dlt::Message,
    line_number: usize,
    boot_time: Option<DateTime<Local>>,
) -> Option<DltLogLine> {
    let timestamp = if let Some(boot_time) = boot_time {
        let ts = msg.header.timestamp?;
        let time_since_boot = dlt_header_time_to_timedelta(ts);
        boot_time.checked_add_signed(time_since_boot)?
    } else {
        let storage_header = msg.storage_header.as_ref()?;
        storage_time_to_datetime(&storage_header.timestamp)?
    };

    if msg.header.ecu_id.is_none() {
        log::warn!("DLT message missing ECU ID for line {line_number}");
    }
    if msg.extended_header.is_none() {
        log::error!("DLT message missing Extended Header for line {line_number}");
        return None;
    }
    if msg.storage_header.is_none() {
        log::error!("DLT message missing Storage Header for line {line_number}");
        return None;
    }

    Some(DltLogLine::new(msg.clone(), timestamp, boot_time, line_number))
}
