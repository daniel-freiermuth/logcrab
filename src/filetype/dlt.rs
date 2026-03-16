// LogCrab - GPL-3.0-or-later
// Copyright (C) 2026 Daniel Freiermuth

use chrono::{DateTime, Local};
use dashmap::DashMap;
use dlt_core::read::{read_message, DltMessageReader};
use egui::Ui;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::{
    atomic::{AtomicI64, AtomicU64, Ordering},
    Arc, Mutex,
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
    pub const fn new(
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
    /// Returns the metadata + payload body: `{ecu} {session} {app} {ctx} {type} {payload}`.
    /// Used directly by `message()` and as the trailing part of `display_message()`.
    fn format_body(&self) -> String {
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

        format!("{ecu_header} {session_id} {app_id} {ctx_id} {message_type} {payload}")
    }

    /// Returns the `[<storage_time> (<diff>) <storage_ecu>]` prefix for inferred-monotonic mode.
    fn format_time_prefix(&self, inferred_time: DateTime<Local>) -> String {
        let storage_ecu = self
            .dlt_message
            .storage_header
            .as_ref()
            .map_or("", |sh| sh.ecu_id.as_str());
        let diff_str = format_time_diff(self.storage_time.signed_duration_since(inferred_time));
        format!("[{} ({diff_str}) {storage_ecu}]", self.storage_time)
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
    /// Whether this calibration was opened in inferred-monotonic mode.
    /// When `false` the result updates `storage_offset_ms` instead of `boot_times`.
    pub is_inferred: bool,
    /// Raw storage timestamp of the right-clicked line (before any offset).
    /// Used to compute the new `storage_offset_ms` in storage-time mode.
    pub storage_time: chrono::DateTime<chrono::Local>,
    /// The calibration UI window.
    pub window: crate::filetype::CalibrationWindow,
}

/// Per-source persistent state for DLT log sources.
///
/// Owns its interior synchronization so it can live in a bare `Arc` with no
/// outer `RwLock`:
/// - `storage_offset_ms`: `AtomicI64` — lock-free reads from rayon worker threads
/// - `boot_times`: `Arc<DashMap>` — inline writes from `DltFileType::read()` without locking
/// - `calibration`: `Mutex<Option<...>>` — UI-thread-only, always uncontended
pub struct DltFileState {
    /// Storage-time mode: offset added to every `storage_time` timestamp.
    pub storage_offset_ms: AtomicI64,
    /// Inferred-time mode: corrected boot times per `(ecu_id, app_id)`.
    ///
    /// Seeded inline during file loading (first-seen storage heuristic). User
    /// calibration writes into this map and the values are persisted to
    /// `.crab`. On re-open, persisted values take precedence over the
    /// freshly computed defaults, preserving calibration across sessions.
    pub boot_times: Arc<DashMap<(String, String), DateTime<Local>>>,
    /// Open calibration window, if any. Not persisted.
    pub calibration: Mutex<Option<DltCalibrationState>>,
}

impl DltFileState {
    #[inline]
    pub fn storage_offset_ms(&self) -> i64 {
        self.storage_offset_ms.load(Ordering::Relaxed)
    }
}

impl Default for DltFileState {
    fn default() -> Self {
        Self {
            storage_offset_ms: AtomicI64::new(0),
            boot_times: Arc::new(DashMap::new()),
            calibration: Mutex::new(None),
        }
    }
}

impl std::fmt::Debug for DltFileState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DltFileState")
            .field("storage_offset_ms", &self.storage_offset_ms())
            .field("boot_times_count", &self.boot_times.len())
            .finish_non_exhaustive()
    }
}

impl Clone for DltFileState {
    /// Deep-clones `boot_times` into a fresh `Arc<DashMap>`.
    /// Calibration is transient UI state and is not cloned.
    fn clone(&self) -> Self {
        let bt: DashMap<(String, String), DateTime<Local>> = self
            .boot_times
            .iter()
            .map(|e| (e.key().clone(), *e.value()))
            .collect();
        Self {
            storage_offset_ms: AtomicI64::new(self.storage_offset_ms()),
            boot_times: Arc::new(bt),
            calibration: Mutex::new(None),
        }
    }
}

/// Separator used to encode `(ecu_id, app_id)` tuple keys as JSON-safe strings.
/// ASCII unit-separator (0x1F) is safe: DLT IDs are printable ASCII.
const BOOT_TIME_KEY_SEP: char = '\x1F';

fn boot_times_to_string_map(
    bt: &DashMap<(String, String), DateTime<Local>>,
) -> std::collections::BTreeMap<String, DateTime<Local>> {
    bt.iter()
        .map(|e| {
            let key = format!("{}{BOOT_TIME_KEY_SEP}{}", e.key().0, e.key().1);
            (key, *e.value())
        })
        .collect()
}

fn string_map_to_boot_times(
    map: std::collections::BTreeMap<String, DateTime<Local>>,
) -> DashMap<(String, String), DateTime<Local>> {
    map.into_iter()
        .filter_map(|(k, v)| {
            let (ecu, app) = k.split_once(BOOT_TIME_KEY_SEP)?;
            Some(((ecu.to_string(), app.to_string()), v))
        })
        .collect()
}

impl serde::Serialize for DltFileState {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = s.serialize_struct("DltFileState", 2)?;
        state.serialize_field("storage_offset_ms", &self.storage_offset_ms())?;
        state.serialize_field("boot_times", &boot_times_to_string_map(&self.boot_times))?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for DltFileState {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Helper {
            #[serde(default)]
            storage_offset_ms: i64,
            #[serde(default)]
            boot_times: std::collections::BTreeMap<String, DateTime<Local>>,
        }
        let h = Helper::deserialize(d)?;
        Ok(Self {
            storage_offset_ms: AtomicI64::new(h.storage_offset_ms),
            boot_times: Arc::new(string_map_to_boot_times(h.boot_times)),
            calibration: Mutex::new(None),
        })
    }
}

// ============================================================================
// EguiConfig for DltTimestampSource
// ============================================================================

impl EguiConfig for crate::config::DltTimestampSource {
    fn egui_render(&mut self, ui: &mut Ui) -> bool {
        ui.separator();
        ui.label("DLT Timestamp Source:");
        let mut changed = false;
        ui.horizontal(|ui| {
            changed |= ui
                .selectable_value(self, Self::StorageTime, "Storage Timestamp")
                .changed();
            changed |= ui
                .selectable_value(self, Self::InferredMonotonic, "Infer From Monotonic")
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
            storage_offset_ms: std::sync::atomic::AtomicI64::new(time_offset_ms),
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
                    if let Some(boot_time) = file_state.boot_times.get(&key) {
                        return *boot_time + chrono::TimeDelta::microseconds(header_us);
                    }
                }
                // Fallback: no boot_time for this app yet
                self.storage_time + chrono::Duration::milliseconds(file_state.storage_offset_ms())
            }
            DltTimestampSource::StorageTime => {
                self.storage_time + chrono::Duration::milliseconds(file_state.storage_offset_ms())
            }
        }
    }

    fn message(&self) -> String {
        self.format_body()
    }

    fn display_message(
        &self,
        config: &crate::config::DltTimestampSource,
        file_state: &DltFileState,
    ) -> String {
        use crate::config::DltTimestampSource;
        let body = self.format_body();
        match config {
            DltTimestampSource::InferredMonotonic => {
                // In inferred-monotonic mode prepend [<storage_time> (<diff>) <storage_ecu>]
                // so the user always sees the relationship between storage and monotonic time.
                let inferred_time = self.timestamp(config, file_state);
                format!("{} {body}", self.format_time_prefix(inferred_time))
            }
            DltTimestampSource::StorageTime => {
                // In storage-time mode prepend [<offset>] when a calibration offset
                // has been applied, consistent with how other file types behave.
                let offset_ms = file_state.storage_offset_ms();
                if offset_ms != 0 {
                    format!(
                        "[{}] {body}",
                        format_time_diff(chrono::Duration::milliseconds(offset_ms))
                    )
                } else {
                    body
                }
            }
        }
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
        file_state: &DltFileState,
    ) {
        if ui.button("\u{23F1} Calibrate Time Here").clicked() {
            use crate::config::DltTimestampSource;

            let is_inferred = matches!(config, DltTimestampSource::InferredMonotonic)
                && self.header_timestamp_us.is_some();

            // Current display time: inferred if available, otherwise storage.
            let current_time = if is_inferred {
                let header_us = self
                    .header_timestamp_us
                    .expect("header_timestamp_us is Some when is_inferred");
                let key = (self.ecu_id.clone(), self.app_id.clone());
                file_state
                    .boot_times
                    .get(&key)
                    .map_or(self.storage_time, |bt| {
                        *bt + chrono::TimeDelta::microseconds(header_us)
                    })
            } else {
                self.storage_time + chrono::Duration::milliseconds(file_state.storage_offset_ms())
            };

            *file_state
                .calibration
                .lock()
                .expect("calibration lock poisoned") = Some(DltCalibrationState {
                ecu_id: self.ecu_id.clone(),
                app_id: self.app_id.clone(),
                header_timestamp_us: self.header_timestamp_us.unwrap_or(0),
                is_inferred,
                storage_time: self.storage_time,
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
    fn egui_render_file_state(&self, ui: &egui::Ui) -> bool {
        let mut cal_guard = self.calibration.lock().expect("calibration lock poisoned");
        let Some(cal) = cal_guard.as_mut() else {
            return false;
        };
        match cal.window.render(ui) {
            Ok(Some((target_time, apply_to_all_apps))) => {
                if cal.is_inferred {
                    let new_boot_time =
                        target_time - chrono::TimeDelta::microseconds(cal.header_timestamp_us);
                    let key = (cal.ecu_id.clone(), cal.app_id.clone());
                    let ecu_id = cal.ecu_id.clone();

                    if apply_to_all_apps {
                        for mut entry in self.boot_times.iter_mut() {
                            if entry.key().0 == ecu_id {
                                *entry.value_mut() = new_boot_time;
                            }
                        }
                        // Insert if no entry for this ECU existed yet.
                        self.boot_times.entry(key).or_insert(new_boot_time);
                    } else {
                        self.boot_times.insert(key, new_boot_time);
                    }
                } else {
                    // Storage-time mode: derive the offset from the raw storage timestamp.
                    let offset_ms = (target_time - cal.storage_time).num_milliseconds();
                    self.storage_offset_ms
                        .store(offset_ms, std::sync::atomic::Ordering::Relaxed);
                }

                *cal_guard = None;
                true
            }
            Ok(None) => false,
            Err(()) => {
                *cal_guard = None;
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
/// Holds a clone of the `Arc<DashMap>` from [`DltFileState::boot_times`] so that
/// each `read(n)` call can write newly discovered `(ECU, App)` boot-times directly
/// into the shared map — no lock acquisition, no end-of-chunk batch flush.
pub struct DltFileType {
    reader: DltMessageReader<ByteCountReader<BufReader<File>>>,
    /// Shared boot-time map — same `Arc` as `DltFileState::boot_times`.
    boot_times: Arc<DashMap<(String, String), DateTime<Local>>>,
    bytes_read_rc: Arc<AtomicU64>,
    line_number: usize,
}

impl InputFileType for DltFileType {
    type LineType = DltLogLine;

    const FILE_EXTENSIONS: &'static [&'static str] = &["dlt"];

    /// Open a DLT file for pull-based reading.
    fn open(
        path: &Path,
        _config: crate::config::DltTimestampSource,
        file_state: Arc<DltFileState>,
    ) -> anyhow::Result<Self> {
        use anyhow::Context as _;
        // Clone the boot_times Arc so read() can write into it without
        // ever touching the outer Arc<DltFileState>.
        let boot_times = Arc::clone(&file_state.boot_times);
        let file =
            File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
        let inner = ByteCountReader::new(BufReader::new(file));
        let bytes_read_rc = inner.bytes_read_arc();
        let reader = DltMessageReader::new(inner, true);
        Ok(Self {
            reader,
            boot_times,
            bytes_read_rc,
            line_number: 1,
        })
    }

    fn read(&mut self, lines_to_read: usize) -> anyhow::Result<Vec<Self::LineType>> {
        let mut result = Vec::with_capacity(lines_to_read);

        // Safety cap: avoid spinning on files with many un-parseable messages.
        let attempt_limit = lines_to_read * 10 + 64;
        let mut attempts = 0;

        while result.len() < lines_to_read && attempts < attempt_limit {
            attempts += 1;
            match read_message(&mut self.reader, None) {
                Ok(Some(dlt_core::parse::ParsedMessage::Item(msg))) => {
                    if let Some(line) = convert_dlt_message(&msg, self.line_number) {
                        if let Some(header_us) = line.header_timestamp_us {
                            let key = (line.ecu_id.clone(), line.app_id.clone());
                            // Write directly into the shared DashMap — no lock, no buffering.
                            // First-seen wins; persisted calibration loaded at open time is
                            // already present and or_insert_with leaves it untouched.
                            self.boot_times.entry(key).or_insert_with(|| {
                                line.storage_time - chrono::TimeDelta::microseconds(header_us)
                            });
                        }
                        result.push(line);
                        self.line_number += 1;
                    }
                }
                Ok(Some(_)) => {}  // skip non-Item messages (e.g. skipped bytes)
                Ok(None) => break, // EOF
                Err(e) => {
                    tracing::warn!("Failed to parse DLT message: {e:?}");
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

/// Convert a `dlt_core::dlt::Message` to `DltLogLine`.
pub fn convert_dlt_message(msg: &dlt_core::dlt::Message, line_number: usize) -> Option<DltLogLine> {
    let storage_time = storage_time_to_datetime(&msg.storage_header.as_ref()?.timestamp)?;

    if msg.header.ecu_id.is_none() {
        tracing::warn!("DLT message missing ECU ID for line {line_number}");
    }
    if msg.extended_header.is_none() {
        tracing::error!("DLT message missing Extended Header for line {line_number}");
        return None;
    }
    if msg.storage_header.is_none() {
        tracing::error!("DLT message missing Storage Header for line {line_number}");
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
