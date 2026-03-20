// LogCrab - GPL-3.0-or-later
// Copyright (C) 2026 Daniel Freiermuth

use chrono::{DateTime, Local};
use egui::Ui;
use opentelemetry_proto::tonic::{common::v1::any_value::Value as OTelValue, logs::v1::LogsData};
use std::fs::{metadata, File};
use std::io::Read;
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::filetype::{InputFileType, LineType, TextFileType};

// ============================================================================
// OtelLogLine
// ============================================================================

/// A single OpenTelemetry log record, parsed from OTLP JSON.
#[derive(Debug, Clone)]
pub struct OtelLogLine {
    raw_line: String,
    pub timestamp: DateTime<Local>,
    message_text: String,
    pub line_number: usize,
    pub anomaly_score: f64,
}

// ============================================================================
// OtelFileState
// ============================================================================

pub type OtelFileState = crate::filetype::SimpleFileState;

// ============================================================================
// LineType implementation
// ============================================================================

impl LineType for OtelLogLine {
    type Config = ();
    type FileState = OtelFileState;

    fn file_state_from_v2(time_offset_ms: i64) -> OtelFileState {
        let s = OtelFileState::default();
        s.set_time_offset_ms(time_offset_ms);
        s
    }

    fn timestamp(&self, _config: &(), file_state: &OtelFileState) -> DateTime<Local> {
        self.timestamp + chrono::Duration::milliseconds(file_state.time_offset_ms())
    }

    fn message(&self) -> String {
        self.message_text.clone()
    }

    fn display_message(&self, _config: &(), file_state: &OtelFileState) -> String {
        let offset_ms = file_state.time_offset_ms();
        if offset_ms != 0 {
            format!(
                "[{}] {}",
                crate::parser::format_time_diff(chrono::Duration::milliseconds(offset_ms)),
                self.message_text
            )
        } else {
            self.message_text.clone()
        }
    }

    fn raw(&self) -> String {
        self.raw_line.clone()
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

    fn egui_render_context_menu(&self, ui: &mut Ui, _config: &(), file_state: &OtelFileState) {
        if ui.button("⏱ Calibrate Time Here").clicked() {
            let raw_time = self.timestamp;
            let display_time =
                raw_time + chrono::Duration::milliseconds(file_state.time_offset_ms());
            *file_state
                .calibration
                .lock()
                .expect("calibration lock poisoned") = Some((
                raw_time,
                crate::filetype::CalibrationWindow::new(
                    display_time,
                    false,
                    Some(display_time),
                    raw_time,
                ),
            ));
            ui.close();
        }
    }
}

// ============================================================================
// OtelFileType (InputFileType + TextFileType)
// ============================================================================

/// Stateful reader for OpenTelemetry OTLP JSON log files.
///
/// Parses the standard OTLP JSON structure:
/// `{ "resourceLogs": [ { "scopeLogs": [ { "logRecords": [...] } ] } ] }`
///
/// Uses the `opentelemetry-proto` crate with serde support for deserialization.
pub struct OtelFileType {
    records: std::vec::IntoIter<OtelLogLine>,
    file_size: u64,
    bytes_read: u64,
}

impl InputFileType for OtelFileType {
    type LineType = OtelLogLine;

    const FILE_EXTENSIONS: &'static [&'static str] = &["json"];

    fn open(
        path: &Path,
        _config: (),
        _file_state: std::sync::Arc<OtelFileState>,
    ) -> anyhow::Result<Self> {
        let metadata = metadata(path)
            .map_err(|e| anyhow::anyhow!("Failed to stat {}: {e}", path.display()))?;
        let file_size = metadata.len();

        let mut file = File::open(path)
            .map_err(|e| anyhow::anyhow!("Failed to open {}: {e}", path.display()))?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {e}", path.display()))?;

        let logs_data: LogsData = serde_json::from_str(&contents)
            .map_err(|e| anyhow::anyhow!("Failed to parse OTLP JSON: {e}"))?;

        let mut line_number = 0usize;
        let mut records = Vec::new();

        for resource_log in &logs_data.resource_logs {
            let service_name = resource_log.resource.as_ref().and_then(|r| {
                r.attributes.iter().find_map(|kv| {
                    if kv.key == "service.name" {
                        kv.value.as_ref().and_then(|v| match &v.value {
                            Some(OTelValue::StringValue(s)) => Some(s.clone()),
                            _ => None,
                        })
                    } else {
                        None
                    }
                })
            });

            for scope_log in &resource_log.scope_logs {
                for log_record in &scope_log.log_records {
                    line_number += 1;

                    let nanos = if log_record.time_unix_nano > 0 {
                        log_record.time_unix_nano
                    } else {
                        log_record.observed_time_unix_nano
                    };

                    let timestamp = if nanos > 0 {
                        // not completely correct, but a data 200 years in the future is a bug
                        // TODO: error variants to surface this
                        let secs = i64::try_from(nanos / 1_000_000_000).unwrap_or_else(|_| {
                            tracing::warn!("Timestamp in log record is too large: {nanos} nanoseconds since epoch");
                            i64::MAX
                        });
                        let subsec_nanos = (nanos % 1_000_000_000) as u32;
                        DateTime::from_timestamp(secs, subsec_nanos)
                            .map_or_else(|| {
                                tracing::warn!("Invalid timestamp in log record: {nanos} nanoseconds since epoch");
                                UNIX_EPOCH.into()
                            }, |dt| dt.with_timezone(&Local))
                    } else {
                        // Into local time
                        UNIX_EPOCH.into()
                    };

                    let body = log_record.body.as_ref().map_or_else(String::new, |v| {
                        use opentelemetry_proto::tonic::common::v1::any_value::Value;
                        match &v.value {
                            Some(Value::StringValue(s)) => s.clone(),
                            Some(other) => format!("{other:?}"),
                            None => String::new(),
                        }
                    });

                    let severity = &log_record.severity_text;

                    let mut message = String::new();
                    if let Some(svc) = &service_name {
                        message.push_str(svc);
                        message.push(' ');
                    }
                    if !severity.is_empty() {
                        message.push_str(severity);
                        message.push(' ');
                    }
                    message.push_str(&body);

                    let raw = serde_json::to_string(log_record).unwrap_or_else(|_| body.clone());

                    records.push(OtelLogLine {
                        raw_line: raw,
                        timestamp,
                        message_text: message,
                        line_number,
                        anomaly_score: 0.0,
                    });
                }
            }
        }

        Ok(Self {
            records: records.into_iter(),
            file_size,
            bytes_read: 0,
        })
    }

    fn read(&mut self, lines_to_read: usize) -> anyhow::Result<Vec<Self::LineType>> {
        let batch: Vec<_> = self.records.by_ref().take(lines_to_read).collect();
        if batch.is_empty() {
            self.bytes_read = self.file_size;
        } else {
            let remaining = self.records.len();
            let total = remaining + batch.len();
            if total > 0 {
                self.bytes_read =
                    self.file_size - (self.file_size * remaining as u64 / total as u64);
            }
        }
        Ok(batch)
    }

    fn bytes_consumed(&self) -> u64 {
        self.bytes_read
    }
}

impl TextFileType for OtelFileType {
    /// Returns `true` if the file looks like OTLP JSON logs (contains `"resourceLogs"`).
    fn looks_like(file: &mut dyn std::io::Read) -> bool {
        let mut buf = [0u8; 4096];
        let n = file.read(&mut buf).unwrap_or(0);
        let sample = String::from_utf8_lossy(&buf[..n]);
        sample.contains("\"resourceLogs\"") || sample.contains("\"resource_logs\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_OTLP: &str = r#"{
        "resourceLogs": [{
            "resource": {
                "attributes": [{
                    "key": "service.name",
                    "value": { "stringValue": "test-service" }
                }]
            },
            "scopeLogs": [{
                "scope": { "name": "my-logger" },
                "logRecords": [{
                    "timeUnixNano": "1700000000000000000",
                    "severityNumber": 9,
                    "severityText": "INFO",
                    "body": { "stringValue": "Hello from OTel" },
                    "attributes": [],
                    "traceId": "",
                    "spanId": ""
                }]
            }]
        }]
    }"#;

    #[test]
    fn test_looks_like_otlp() {
        let mut cursor = std::io::Cursor::new(SAMPLE_OTLP);
        assert!(OtelFileType::looks_like(&mut cursor));
    }

    #[test]
    fn test_looks_like_rejects_plain_text() {
        let mut cursor = std::io::Cursor::new("2025-01-01 INFO some log line");
        assert!(!OtelFileType::looks_like(&mut cursor));
    }

    #[test]
    fn test_parse_otlp_records() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().expect("tmpfile");
        tmp.write_all(SAMPLE_OTLP.as_bytes()).expect("write");
        let path = tmp.path().to_owned();

        let file_state = std::sync::Arc::new(OtelFileState::default());
        let mut ft = OtelFileType::open(&path, (), file_state).expect("open");
        let lines = ft.read(100).expect("read");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].message_text, "test-service INFO Hello from OTel");
        assert_eq!(lines[0].line_number, 1);
    }
}
