// LogCrab - GPL-3.0-or-later
// Copyright (C) 2026 Daniel Freiermuth

use chrono::{DateTime, Datelike, Local, NaiveDateTime, TimeZone};
use egui::Ui;
use fancy_regex::Regex;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{LazyLock, Mutex};

use super::dmesg::{parse_dmesg_line, DmesgLogLine};
use super::logcat::{parse_logcat_line, LogcatLogLine};
use crate::filetype::{CalibrationState, InputFileType, LineType, LogFileState, TextFileType};

// ============================================================================
// Bugreport parsing utilities
// ============================================================================

static DUMPSTATE_FULL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"==\s*dumpstate:\s*(\d{4}-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2})")
        .expect("valid regex literal")
});

/// `Uptime: up 0 weeks, 0 days, 0 hours, 6 minutes,  load average: …`
static UPTIME_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^Uptime: up (\d+) weeks?, (\d+) days?, (\d+) hours?, (\d+) minutes?")
        .expect("valid regex literal")
});

/// Detect the year from bugreport header lines.
/// Delegates to [`detect_header_info`] to avoid a duplicate scan.
pub fn detect_year_from_header(content: &str) -> Option<i32> {
    detect_header_info(content).map(|(year, _)| year)
}

/// Extract year and approximate boot time from the bugreport header.
///
/// Reads up to 100 lines looking for both the `dumpstate:` timestamp and the
/// `Uptime: up …` summary line. Returns `(year, boot_time_ms_from_utc_epoch)`.
///
/// The uptime is minute-granular, so the boot time is accurate to ~±60 seconds.
/// Returns `None` if the dumpstate header is missing (not a bugreport).
pub fn detect_header_info(content: &str) -> Option<(i32, i64)> {
    let mut dumpstate_dt: Option<DateTime<Local>> = None;
    let mut uptime_minutes: Option<i64> = None;

    for line in content.lines().take(100) {
        if dumpstate_dt.is_none() {
            if let Ok(Some(caps)) = DUMPSTATE_FULL.captures(line) {
                let dt_str = caps[1].to_string();
                if let Ok(ndt) = NaiveDateTime::parse_from_str(&dt_str, "%Y-%m-%d %H:%M:%S") {
                    if let Some(dt) = Local.from_local_datetime(&ndt).single() {
                        dumpstate_dt = Some(dt);
                    }
                }
            }
        }

        if uptime_minutes.is_none() {
            if let Ok(Some(caps)) = UPTIME_LINE.captures(line) {
                let weeks: i64 = caps[1].parse().unwrap_or(0);
                let days: i64 = caps[2].parse().unwrap_or(0);
                let hours: i64 = caps[3].parse().unwrap_or(0);
                let minutes: i64 = caps[4].parse().unwrap_or(0);
                uptime_minutes =
                    Some(weeks * 7 * 24 * 60 + days * 24 * 60 + hours * 60 + minutes);
            }
        }

        if dumpstate_dt.is_some() && uptime_minutes.is_some() {
            break;
        }
    }

    let dumpstate = dumpstate_dt?;
    let year = dumpstate.year();

    let boot_time_ms = match uptime_minutes {
        Some(minutes) => {
            let boot_time = dumpstate - chrono::Duration::milliseconds(minutes * 60 * 1000);
            let ms = boot_time.timestamp_millis();
            tracing::info!("Bugreport: dumpstate={dumpstate}, uptime={minutes}min → boot_time_ms={ms}");
            ms
        }
        None => {
            tracing::warn!(
                "Bugreport: dumpstate={dumpstate} found but no 'Uptime:' line in the \
                 preview buffer — dmesg boot-time offset cannot be auto-detected. \
                 Use 'Calibrate Dmesg Time Here' to align dmesg timestamps manually."
            );
            0
        }
    };

    Some((year, boot_time_ms))
}

/// Schema version for `BugreportFileState` inside the `.crab` slug object.
///
/// - v0 (0.35, unversioned): `LogcatFileState` aka `SimpleFileState` — `{"time_offset_ms": N}`
/// - v1 (0.36+): `BugreportFileState` — `{"state_version": 1, "logcat_offset_ms": N, "dmesg_offset_ms": N}`
pub const BUGREPORT_STATE_VERSION: u32 = 1;

// ============================================================================
// BugreportFileState — v1 (legacy, migration only)
// ============================================================================

/// Deserialization shape for bugreport state written by logcrab 0.35.
///
/// At that time `BugreportFileType` used `LogcatFileState` (= `SimpleFileState`)
/// as its `FileState`, which serialised as `{"time_offset_ms": N}` with no
/// `state_version` field — hence V0 (unversioned/pre-versioning era).
/// This struct exists solely so the migration path in
/// [`BugreportFileState::migrate_from_v0`] is explicit and named.
#[derive(serde::Deserialize)]
struct BugreportFileStateV0 {
    #[serde(default)]
    time_offset_ms: i64,
}

// ============================================================================
// BugreportFileState
// ============================================================================

/// Per-source state for bugreport files.
///
/// The logcat and dmesg halves are calibrated independently so they can be
/// aligned against each other. `logcat_offset_ms` defaults to 0 (logcat lines
/// already carry absolute wall-clock timestamps). `dmesg_offset_ms` is
/// initialised to the auto-detected boot time (see `detect_header_info`) on
/// first open; thereafter it is persisted in the `.crab` file so that a
/// user-applied fine-tuning survives session reload.
pub struct BugreportFileState {
    pub logcat_offset_ms: AtomicI64,
    pub dmesg_offset_ms: AtomicI64,
    #[allow(clippy::type_complexity)]
    pub logcat_calibration: Mutex<Option<CalibrationState>>,
    #[allow(clippy::type_complexity)]
    pub dmesg_calibration: Mutex<Option<CalibrationState>>,
}

impl BugreportFileState {
    /// Migrate a v0 (unversioned) bugreport state into the current format.
    ///
    /// v0 only had one time offset (the logcat side). `dmesg_offset_ms` is left
    /// at 0 so that [`BugreportFileType::open_inner`] will auto-detect the boot
    /// time from the dumpstate header on first open.
    fn migrate_from_v0(v0: BugreportFileStateV0) -> Self {
        let s = Self::default();
        s.set_logcat_offset_ms(v0.time_offset_ms);
        s
    }
    #[inline]
    pub fn logcat_offset_ms(&self) -> i64 {
        self.logcat_offset_ms.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_logcat_offset_ms(&self, v: i64) {
        self.logcat_offset_ms.store(v, Ordering::Relaxed);
    }

    #[inline]
    pub fn dmesg_offset_ms(&self) -> i64 {
        self.dmesg_offset_ms.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_dmesg_offset_ms(&self, v: i64) {
        self.dmesg_offset_ms.store(v, Ordering::Relaxed);
    }

    /// Set `dmesg_offset_ms` to `boot_time_ms` only when it is still 0.
    ///
    /// A non-zero value means the session was restored from a `.crab` file that
    /// already carries a persisted (possibly user-adjusted) offset; we must not
    /// overwrite it. A real Android device can never have a boot time of exactly
    /// the Unix epoch, so 0 is a safe sentinel for "not yet initialised."
    pub fn init_dmesg_offset_if_zero(&self, boot_time_ms: i64) {
        let _ = self.dmesg_offset_ms.compare_exchange(
            0,
            boot_time_ms,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }
}

impl Default for BugreportFileState {
    fn default() -> Self {
        Self {
            logcat_offset_ms: AtomicI64::new(0),
            dmesg_offset_ms: AtomicI64::new(0),
            logcat_calibration: Mutex::new(None),
            dmesg_calibration: Mutex::new(None),
        }
    }
}

impl std::fmt::Debug for BugreportFileState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BugreportFileState")
            .field("logcat_offset_ms", &self.logcat_offset_ms())
            .field("dmesg_offset_ms", &self.dmesg_offset_ms())
            .finish_non_exhaustive()
    }
}

impl Clone for BugreportFileState {
    fn clone(&self) -> Self {
        Self {
            logcat_offset_ms: AtomicI64::new(self.logcat_offset_ms()),
            dmesg_offset_ms: AtomicI64::new(self.dmesg_offset_ms()),
            logcat_calibration: Mutex::new(None), // calibration is transient
            dmesg_calibration: Mutex::new(None),
        }
    }
}

impl serde::Serialize for BugreportFileState {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = s.serialize_struct("BugreportFileState", 3)?;
        state.serialize_field("state_version", &BUGREPORT_STATE_VERSION)?;
        state.serialize_field("logcat_offset_ms", &self.logcat_offset_ms())?;
        state.serialize_field("dmesg_offset_ms", &self.dmesg_offset_ms())?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for BugreportFileState {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Peek at state_version to pick the right deserialization path.
        // Values > BUGREPORT_STATE_VERSION are caught upstream in
        // `CrabFile::load_from_file` before serde is called, so only v1 and v2
        // reach this branch.
        #[derive(serde::Deserialize)]
        struct VersionPeek {
            #[serde(default)] // absent in v1 → 0, treated as v1
            state_version: u32,
            #[serde(flatten)]
            rest: serde_json::Value,
        }
        let peeked = VersionPeek::deserialize(d)?;
        match peeked.state_version {
            0 => {
                let v0: BugreportFileStateV0 =
                    serde_json::from_value(peeked.rest).map_err(serde::de::Error::custom)?;
                Ok(Self::migrate_from_v0(v0))
            }
            _ => {
                // v1 (current). Any version that somehow slips past the upstream
                // check is also handled here best-effort.
                #[derive(serde::Deserialize)]
                struct V1 {
                    #[serde(default)]
                    logcat_offset_ms: i64,
                    #[serde(default)]
                    dmesg_offset_ms: i64,
                }
                let v1: V1 = serde_json::from_value(peeked.rest)
                    .map_err(serde::de::Error::custom)?;
                Ok(Self {
                    logcat_offset_ms: AtomicI64::new(v1.logcat_offset_ms),
                    dmesg_offset_ms: AtomicI64::new(v1.dmesg_offset_ms),
                    logcat_calibration: Mutex::new(None),
                    dmesg_calibration: Mutex::new(None),
                })
            }
        }
    }
}

impl LogFileState for BugreportFileState {
    const MAX_STATE_VERSION: Option<u32> = Some(BUGREPORT_STATE_VERSION);

    fn egui_render_file_state(&self, ui: &egui::Ui) -> bool {
        use crate::filetype::render_calibration;

        let logcat_changed = {
            let mut cal = self
                .logcat_calibration
                .lock()
                .expect("logcat calibration lock poisoned");
            render_calibration(ui, &mut cal).is_some_and(|offset_ms| {
                self.set_logcat_offset_ms(offset_ms);
                true
            })
        };

        let dmesg_changed = {
            let mut cal = self
                .dmesg_calibration
                .lock()
                .expect("dmesg calibration lock poisoned");
            render_calibration(ui, &mut cal).is_some_and(|offset_ms| {
                self.set_dmesg_offset_ms(offset_ms);
                true
            })
        };

        logcat_changed || dmesg_changed
    }
}

// ============================================================================
// BugreportLogLine
// ============================================================================

/// A single parsed line from an Android bugreport — either a logcat record or a
/// dmesg kernel log entry.
#[derive(Debug, Clone)]
pub enum BugreportLogLine {
    Logcat(LogcatLogLine),
    Dmesg(DmesgLogLine),
}

impl LineType for BugreportLogLine {
    type Config = ();
    type FileState = BugreportFileState;

    fn file_state_from_v2(time_offset_ms: i64) -> BugreportFileState {
        // Legacy .crab files only had one offset; map it to the logcat side.
        let s = BugreportFileState::default();
        s.set_logcat_offset_ms(time_offset_ms);
        s
    }

    fn timestamp(&self, _config: &(), file_state: &BugreportFileState) -> DateTime<Local> {
        match self {
            BugreportLogLine::Logcat(l) => {
                l.timestamp
                    + chrono::Duration::milliseconds(file_state.logcat_offset_ms())
            }
            BugreportLogLine::Dmesg(l) => {
                l.timestamp
                    + chrono::Duration::milliseconds(file_state.dmesg_offset_ms())
            }
        }
    }

    fn message(&self) -> String {
        match self {
            BugreportLogLine::Logcat(l) => l.message(),
            BugreportLogLine::Dmesg(l) => l.message(),
        }
    }

    fn display_message(&self, _config: &(), file_state: &BugreportFileState) -> String {
        match self {
            BugreportLogLine::Logcat(l) => {
                let offset_ms = file_state.logcat_offset_ms();
                if offset_ms != 0 {
                    format!(
                        "[{}] {}",
                        crate::parser::format_time_diff(chrono::Duration::milliseconds(
                            offset_ms
                        )),
                        l.message()
                    )
                } else {
                    l.message()
                }
            }
            // Dmesg timestamps are boot-relative and always shifted by the
            // boot-time offset — showing that offset as a prefix would be
            // meaningless noise ("+20,000d"). Display the raw message only.
            BugreportLogLine::Dmesg(l) => l.message(),
        }
    }

    fn raw(&self) -> String {
        match self {
            BugreportLogLine::Logcat(l) => l.raw(),
            BugreportLogLine::Dmesg(l) => l.raw(),
        }
    }

    fn line_number(&self) -> usize {
        match self {
            BugreportLogLine::Logcat(l) => l.line_number,
            BugreportLogLine::Dmesg(l) => l.line_number,
        }
    }

    fn anomaly_score(&self) -> f64 {
        match self {
            BugreportLogLine::Logcat(l) => l.anomaly_score,
            BugreportLogLine::Dmesg(l) => l.anomaly_score,
        }
    }

    fn set_anomaly_score(&mut self, score: f64) {
        match self {
            BugreportLogLine::Logcat(l) => l.anomaly_score = score,
            BugreportLogLine::Dmesg(l) => l.anomaly_score = score,
        }
    }

    fn egui_render_context_menu(
        &self,
        ui: &mut Ui,
        _config: &(),
        file_state: &BugreportFileState,
    ) {
        match self {
            BugreportLogLine::Logcat(line) => {
                if ui.button("⏱ Calibrate Logcat Time Here").clicked() {
                    let raw_time = line.timestamp;
                    let display_time = raw_time
                        + chrono::Duration::milliseconds(file_state.logcat_offset_ms());
                    *file_state
                        .logcat_calibration
                        .lock()
                        .expect("logcat calibration lock poisoned") = Some((
                        raw_time,
                        crate::filetype::CalibrationWindow::new(
                            display_time,
                            false,
                            Some(display_time),
                            display_time,
                        ),
                    ));
                    ui.close();
                }
            }
            BugreportLogLine::Dmesg(line) => {
                if ui.button("⏱ Calibrate Dmesg Time Here").clicked() {
                    let raw_time = line.timestamp;
                    let display_time = raw_time
                        + chrono::Duration::milliseconds(file_state.dmesg_offset_ms());
                    *file_state
                        .dmesg_calibration
                        .lock()
                        .expect("dmesg calibration lock poisoned") = Some((
                        raw_time,
                        crate::filetype::CalibrationWindow::new(
                            display_time,
                            false,
                            Some(display_time),
                            display_time,
                        ),
                    ));
                    ui.close();
                }
            }
        }
    }
}

// ============================================================================
// BugreportFileType
// ============================================================================

/// Stateful reader for Android bugreport files.
///
/// Parses both logcat lines and kernel dmesg lines (`[SSSSS.UUUUUU] message`)
/// embedded in the bugreport. The dmesg timestamps are boot-relative; the
/// boot-time offset is auto-derived from the `dumpstate:` wall-clock header and
/// the `Uptime: up … N minutes` summary (minute-granular precision). Users can
/// fine-tune both the logcat and dmesg offsets independently through the context
/// menu calibration widget.
///
/// Must be registered **before** [`super::logcat::LogcatFileType`] so that
/// `looks_like` is checked first (bugreport ⊂ logcat pattern-space).
pub struct BugreportFileType {
    reader: BufReader<File>,
    year: i32,
    line_number: usize,
    bytes_read: u64,
    /// Pending dmesg entry that may still receive continuation lines.
    dmesg_pending: Option<DmesgLogLine>,
    logcat_count: usize,
    dmesg_count: usize,
}

impl Drop for BugreportFileType {
    fn drop(&mut self) {
        tracing::info!(
            "Bugreport: {} logcat lines, {} dmesg lines ({} total)",
            self.logcat_count,
            self.dmesg_count,
            self.logcat_count + self.dmesg_count,
        );
    }
}

impl BugreportFileType {
    fn open_inner(
        path: &Path,
        file_state: &BugreportFileState,
    ) -> anyhow::Result<Self> {
        use anyhow::Context as _;
        let mut file =
            File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;

        // Read enough to capture the dumpstate header AND the Uptime line which
        // may come after several long bootconfig lines (empirically ~8 KB is safe).
        let mut preview_buf = [0u8; 8192];
        let preview_n = file.read(&mut preview_buf).unwrap_or(0);
        let preview = String::from_utf8_lossy(&preview_buf[..preview_n]);

        let (year, boot_time_ms) = detect_header_info(&preview).unwrap_or_else(|| {
            tracing::warn!(
                "No dumpstate header found in {}, using current year / zero dmesg offset",
                path.display()
            );
            (chrono::Local::now().year(), 0)
        });

        // Apply the auto-detected boot time only when the session has no saved
        // calibration (compare-exchange against 0).
        if boot_time_ms != 0 {
            file_state.init_dmesg_offset_if_zero(boot_time_ms);
        }

        file.seek(SeekFrom::Start(0))
            .with_context(|| format!("Failed to seek {}", path.display()))?;

        Ok(Self {
            reader: BufReader::new(file),
            year,
            line_number: 0,
            bytes_read: 0,
            dmesg_pending: None,
            logcat_count: 0,
            dmesg_count: 0,
        })
    }
}

impl InputFileType for BugreportFileType {
    type LineType = BugreportLogLine;

    const FILE_EXTENSIONS: &'static [&'static str] = &["txt", "zip"];

    fn open(
        path: &::std::path::Path,
        _config: (),
        file_state: std::sync::Arc<BugreportFileState>,
    ) -> anyhow::Result<Self> {
        Self::open_inner(path, &file_state)
    }

    fn read(&mut self, lines_to_read: usize) -> anyhow::Result<Vec<Self::LineType>> {
        let mut result = Vec::with_capacity(lines_to_read);
        let mut buf = Vec::new();
        loop {
            if result.len() >= lines_to_read {
                break;
            }
            buf.clear();
            match self.reader.read_until(b'\n', &mut buf) {
                Ok(0) => {
                    // EOF: flush any pending dmesg entry.
                    if let Some(pending) = self.dmesg_pending.take() {
                        self.dmesg_count += 1;
                        result.push(BugreportLogLine::Dmesg(pending));
                    }
                    break;
                }
                Ok(n) => {
                    self.bytes_read += n as u64;
                    self.line_number += 1;
                    if std::str::from_utf8(&buf).is_err() {
                        tracing::warn!(
                            "Invalid UTF-8 at line {}; replacing broken bytes with U+FFFD",
                            self.line_number
                        );
                    }
                    let raw = String::from_utf8_lossy(&buf)
                        .trim_end_matches(['\n', '\r'])
                        .to_string();

                    // Section separators flush the dmesg pending buffer. They
                    // mark transitions between log sections and can never be
                    // dmesg continuation lines.
                    if raw.starts_with("------") {
                        if let Some(pending) = self.dmesg_pending.take() {
                            self.dmesg_count += 1;
                            result.push(BugreportLogLine::Dmesg(pending));
                        }
                        continue;
                    }

                    // Try dmesg format first — it's syntactically unambiguous.
                    if let Some(entry) =
                        parse_dmesg_line(raw.clone(), self.line_number)
                    {
                        if let Some(pending) = self.dmesg_pending.take() {
                            self.dmesg_count += 1;
                            result.push(BugreportLogLine::Dmesg(pending));
                        }
                        self.dmesg_pending = Some(entry);
                        continue;
                    }

                    // Try logcat format.
                    if let Some(line) =
                        parse_logcat_line(raw.clone(), self.line_number, self.year)
                    {
                        if let Some(pending) = self.dmesg_pending.take() {
                            self.dmesg_count += 1;
                            result.push(BugreportLogLine::Dmesg(pending));
                        }
                        self.logcat_count += 1;
                        result.push(BugreportLogLine::Logcat(line));
                        continue;
                    }

                    // Unrecognised line: treat as dmesg continuation if there is
                    // an active pending entry, otherwise silently skip.
                    if let Some(ref mut pending) = self.dmesg_pending {
                        pending.append_continuation(&raw);
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Read error: {e}")),
            }
        }
        Ok(result)
    }

    fn bytes_consumed(&self) -> u64 {
        self.bytes_read
    }
}

impl TextFileType for BugreportFileType {
    /// Returns `true` when the file starts with an Android `dumpstate` header that
    /// includes an explicit year.
    fn looks_like(file: &mut dyn std::io::Read) -> bool {
        let mut buf = [0u8; 4096];
        let n = file.read(&mut buf).unwrap_or(0);
        let sample = String::from_utf8_lossy(&buf[..n]);
        detect_year_from_header(&sample).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_year_from_header() {
        let content = "\n    ========================================================\n    == dumpstate: 2024-11-27 14:08:01\n    ========================================================\n    ";
        assert_eq!(detect_year_from_header(content), Some(2024));
    }

    #[test]
    fn test_detect_year_from_header_missing() {
        assert_eq!(
            detect_year_from_header("plain logcat output\n11-20 14:23:45.123 msg"),
            None
        );
    }

    #[test]
    fn test_detect_header_info_year() {
        let content = "========================================================\n== dumpstate: 2026-03-11 14:25:49\n========================================================\nUptime: up 0 weeks, 0 days, 0 hours, 6 minutes,  load average: 1.0, 1.0, 1.0\n";
        let (year, _boot_ms) = detect_header_info(content).expect("should parse");
        assert_eq!(year, 2026);
    }

    #[test]
    fn test_detect_header_info_boot_time() {
        let content = "========================================================\n== dumpstate: 2026-03-11 14:25:49\n========================================================\nUptime: up 0 weeks, 0 days, 0 hours, 6 minutes,  load average: 1.0, 1.0, 1.0\n";
        let (_year, boot_ms) = detect_header_info(content).expect("should parse");
        // Boot time ≈ dumpstate − 6 minutes
        let dumpstate_ms = Local
            .from_local_datetime(
                &NaiveDateTime::parse_from_str("2026-03-11 14:25:49", "%Y-%m-%d %H:%M:%S")
                    .unwrap(),
            )
            .single()
            .unwrap()
            .timestamp_millis();
        let expected = dumpstate_ms - 6 * 60 * 1000;
        assert_eq!(boot_ms, expected);
    }
}
