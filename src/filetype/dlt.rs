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
    Arc, RwLock,
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
    /// Storage header wall-clock timestamp (always available)
    pub storage_time: DateTime<Local>,
    /// Header timestamp in microseconds (time since boot).
    /// `None` when the DLT message has no header timestamp field.
    pub header_timestamp_us: Option<i64>,
    /// Cached ECU ID (empty string when absent)
    pub ecu_id: String,
    /// Cached application ID (empty string when absent)
    pub app_id: String,
    /// Original line number in source file
    pub line_number: usize,
    /// Anomaly score (mutable)
    pub anomaly_score: f64,
}

impl DltLogLine {
    pub fn new(
        dlt_message: dlt_core::dlt::Message,
        storage_time: DateTime<Local>,
        header_timestamp_us: Option<i64>,
        ecu_id: String,
        app_id: String,
        line_number: usize,
    ) -> Self {
        Self {
            dlt_message,
            storage_time,
            header_timestamp_us,
            ecu_id,
            app_id,
            line_number,
            anomaly_score: 0.0,
        }
    }

    /// Format DLT message for display.
    ///
    /// `inferred_time` is the calibrated monotonic timestamp when available
    /// (i.e. when in `InferredMonotonic` mode and a boot-time exists for this
    /// line's `(ecu_id, app_id)`). When `None`, storage-time display is used.
    fn format_message(&self, inferred_time: Option<DateTime<Local>>) -> String {
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

        let storage_ecu = self
            .dlt_message
            .storage_header
            .as_ref()
            .map_or("", |sh| sh.ecu_id.as_str());

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

        let storage_time = self.storage_time;

        if let Some(inferred_t) = inferred_time {
            let time_diff = storage_time.signed_duration_since(inferred_t);
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

/// Pending calibration for a DLT source.
///
/// Created by `egui_render_context_menu`; driven each frame by
/// `DltFileState::egui_render_file_state`. `#[serde(skip)]` — not persisted.
#[derive(Debug, Clone)]
pub struct DltCalibrationState {
    /// ECU ID of the right-clicked line.
    pub ecu_id: String,
    /// Application ID of the right-clicked line.
    pub app_id: String,
    /// Header timestamp of the right-clicked line in microseconds (time since boot).
    pub header_timestamp_us: i64,
    /// The calibration UI window.
    pub window: crate::filetype::CalibrationWindow,
}

/// Per-source persistent state for DLT log sources.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DltFileState {
    /// Storage-time mode: offset added to every storage_time timestamp.
    #[serde(default)]
    pub storage_offset_ms: i64,

    /// Inferred-time mode: corrected boot times per `(ecu_id, app_id)`.
    ///
    /// Seeded during file loading (first-seen storage heuristic). User
    /// calibration writes into this map and the values are persisted to
    /// `.crab`. On re-open, persisted values take precedence over the
    /// freshly computed defaults, preserving calibration across sessions.
    #[serde(default)]
    pub boot_times: HashMap<(String, String), DateTime<Local>>,

    /// Open calibration window, if any. Not persisted.
    #[serde(skip)]
    pub calibration: Option<DltCalibrationState>,
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
                    DltTimestampSource::InferredMonotonic,
                    "Infer From Monotonic",
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
    /// inferred monotonic timestamps.  Shared across all DLT sources in a
    /// session via `Arc<RwLock<DltTimestampSource>>`.
    type Config = crate::config::DltTimestampSource;
    type FileState = DltFileState;

    fn file_state_from_v2(time_offset_ms: i64) -> DltFileState {
        DltFileState {
            storage_offset_ms: time_offset_ms,
            ..Default::default()
        }
    }

    fn timestamp(
        &self,
        config: &crate::config::DltTimestampSource,
        file_state: &DltFileState,
    ) -> DateTime<Local> {
        use crate::config::DltTimestampSource;
        match config {
            DltTimestampSource::InferredMonotonic => {
                if let Some(header_us) = self.header_timestamp_us {
                    let key = (self.ecu_id.clone(), self.app_id.clone());
                    if let Some(&boot_time) = file_state.boot_times.get(&key) {
                        return boot_time + chrono::TimeDelta::microseconds(header_us);
                    }
                }
                // Fallback: no boot_time for this app yet
                self.storage_time + chrono::Duration::milliseconds(file_state.storage_offset_ms)
            }
            DltTimestampSource::StorageTime => {
                self.storage_time + chrono::Duration::milliseconds(file_state.storage_offset_ms)
            }
        }
    }

    fn message(&self) -> String {
        self.format_message(None)
    }

    fn display_message(&self, file_state: &DltFileState) -> String {
        // Compute calibrated inferred time from file_state.boot_times, if available.
        // Shown regardless of the current DltTimestampSource setting so the display
        // consistently reflects any calibration the user has applied.
        let inferred_time = self.header_timestamp_us.and_then(|header_us| {
            let key = (self.ecu_id.clone(), self.app_id.clone());
            file_state
                .boot_times
                .get(&key)
                .map(|&bt| bt + chrono::TimeDelta::microseconds(header_us))
        });
        self.format_message(inferred_time)
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

            let is_inferred = matches!(config, DltTimestampSource::InferredMonotonic)
                && self.header_timestamp_us.is_some();

            // Current display time: inferred if available, otherwise storage.
            let current_time = if is_inferred {
                let header_us = self.header_timestamp_us.unwrap();
                let key = (self.ecu_id.clone(), self.app_id.clone());
                file_state
                    .boot_times
                    .get(&key)
                    .map_or(self.storage_time, |&bt| {
                        bt + chrono::TimeDelta::microseconds(header_us)
                    })
            } else {
                self.storage_time
                    + chrono::Duration::milliseconds(file_state.storage_offset_ms)
            };

            file_state.calibration = Some(DltCalibrationState {
                ecu_id: self.ecu_id.clone(),
                app_id: self.app_id.clone(),
                header_timestamp_us: self.header_timestamp_us.unwrap_or(0),
                window: crate::filetype::CalibrationWindow::new(
                    current_time,
                    is_inferred,
                    Some(current_time),
                    Some(self.storage_time),
                ),
            });
            ui.close();
        }
    }

}

impl crate::filetype::LogFileState for DltFileState {
    fn egui_render_file_state(&mut self, ui: &egui::Ui) -> bool {
        let Some(cal) = self.calibration.as_mut() else {
            return false;
        };
        match cal.window.render(ui) {
            Ok(Some((target_time, apply_to_all_apps))) => {
                // new boot_time for the right-clicked (ecu, app)
                let new_boot_time =
                    target_time - chrono::TimeDelta::microseconds(cal.header_timestamp_us);
                let key = (cal.ecu_id.clone(), cal.app_id.clone());
                let ecu_id = cal.ecu_id.clone();

                if apply_to_all_apps {
                    // Shift all apps in this ECU by the same delta.
                    if let Some(&old_bt) = self.boot_times.get(&key) {
                        let delta = new_boot_time.signed_duration_since(old_bt);
                        for ((ecu, _), bt) in &mut self.boot_times {
                            if *ecu == ecu_id {
                                *bt = *bt + delta;
                            }
                        }
                    } else {
                        // No prior entry: set this app only.
                        self.boot_times.insert(key, new_boot_time);
                    }
                } else {
                    self.boot_times.insert(key, new_boot_time);
                }

                self.calibration = None;
                true
            }
            Ok(None) => false,
            Err(()) => {
                self.calibration = None;
                false
            }
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
/// writing newly discovered `(ECU, App)` boot-times into `file_state` for
/// `InferredMonotonic` mode (persisted calibration values are never overwritten).
pub struct DltFileType {
    reader: DltMessageReader<ByteCountReader<BufReader<File>>>,
    timestamp_source: crate::config::DltTimestampSource,
    bytes_read_rc: Arc<AtomicU64>,
    file_size: u64,
    line_number: usize,
    /// Shared file state — DltFileType writes boot-times here during `read()`.
    file_state: Arc<RwLock<DltFileState>>,
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
        file_state: Arc<RwLock<DltFileState>>,
    ) -> Result<Self, String> {
        let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let file =
            File::open(path).map_err(|e| format!("Failed to open {}: {e}", path.display()))?;
        let inner = ByteCountReader::new(BufReader::new(file));
        let bytes_read_rc = inner.bytes_read_arc();
        let reader = DltMessageReader::new(inner, true);
        Ok(Self {
            reader,
            timestamp_source: config,
            bytes_read_rc,
            file_size,
            line_number: 1,
            file_state,
        })
    }

    fn read(&mut self, lines_to_read: usize) -> Result<Vec<Self::LineType>, String> {
        use crate::config::DltTimestampSource;

        let use_inferred =
            matches!(self.timestamp_source, DltTimestampSource::InferredMonotonic);
        let mut result = Vec::with_capacity(lines_to_read);

        // Collect newly discovered (ecu, app) → boot_time pairs locally;
        // merged into file_state in a single lock at the end of the chunk.
        let mut new_boot_times: HashMap<(String, String), DateTime<Local>> = HashMap::new();

        // Safety cap: avoid spinning on files with many un-parseable messages.
        let attempt_limit = lines_to_read * 10 + 64;
        let mut attempts = 0;

        while result.len() < lines_to_read && attempts < attempt_limit {
            attempts += 1;
            match read_message(&mut self.reader, None) {
                Ok(Some(dlt_core::parse::ParsedMessage::Item(msg))) => {
                    if let Some(line) = convert_dlt_message(&msg, self.line_number) {
                        if use_inferred {
                            if let Some(header_us) = line.header_timestamp_us {
                                let key = (line.ecu_id.clone(), line.app_id.clone());
                                // First-seen wins (persisted calibration will also win via or_insert below)
                                new_boot_times.entry(key).or_insert_with(|| {
                                    line.storage_time
                                        - chrono::TimeDelta::microseconds(header_us)
                                });
                            }
                        }
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

        // Merge discovered boot_times into file_state.
        // Existing entries (persisted calibration) are never overwritten.
        if use_inferred && !new_boot_times.is_empty() {
            let mut state = self.file_state.write().expect("file_state lock poisoned");
            for (key, bt) in new_boot_times {
                state.boot_times.entry(key).or_insert(bt);
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
) -> Option<DltLogLine> {
    let storage_time = storage_time_to_datetime(&msg.storage_header.as_ref()?.timestamp)?;

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

    let header_timestamp_us = msg.header.timestamp.map(|ts| i64::from(ts) * 100);
    let ecu_id = msg.header.ecu_id.as_deref().unwrap_or("").to_string();
    let app_id = msg
        .extended_header
        .as_ref()
        .map_or(String::new(), |ext| ext.application_id.clone());

    Some(DltLogLine::new(
        msg.clone(),
        storage_time,
        header_timestamp_us,
        ecu_id,
        app_id,
        line_number,
    ))
}
