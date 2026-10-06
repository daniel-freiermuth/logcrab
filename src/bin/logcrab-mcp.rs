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

//! `logcrab-mcp` — MCP server that exposes `LogBERT` ML anomaly scoring.
//!
//! Tools:
//!   - `list_models`       — list available models from the sidecar
//!   - `score_log_lines`   — score an inline list of log lines
//!   - `analyze_log_file`  — parse a log file via `logcrab-export`, then score it
//!
//! Environment variables:
//!   - `LOGCRAB_SIDECAR_HOST` (default `127.0.0.1`)
//!   - `LOGCRAB_SIDECAR_PORT` (default `8765`)
//!   - `LOGCRAB_EXPORT_BIN`   path to the `logcrab-export` binary (default: searches `$PATH`)

#![allow(clippy::missing_panics_doc, clippy::missing_errors_doc)]

use std::collections::HashMap;

use logcrab::anomaly::sidecar_client::{InputLine, SidecarClient};
use rmcp::{
    handler::server::wrapper::Parameters, tool, tool_handler, tool_router, transport::stdio,
    ServerHandler, ServiceExt,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::task;

// ── Parameter types ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
struct ScoreParams {
    /// ID of the `LogBERT` model to use (from `list_models`)
    model_id: String,
    /// Raw log lines to score
    lines: Vec<String>,
    /// Optional filetype hint (e.g. `logcat`, `dlt`, `dmesg`) — improves
    /// normalisation accuracy when the model was trained on that filetype
    filetype: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AnalyzeParams {
    /// Absolute path to the log file to analyze
    path: String,
    /// ID of the `LogBERT` model to use (from `list_models`)
    model_id: String,
    /// Maximum number of results to return sorted by anomaly score (default: 50)
    top_n: Option<usize>,
}

// ── Export record (one NDJSON line from `logcrab-export`) ────────────────────

#[derive(Debug, Deserialize)]
struct ExportRecord {
    line_number: usize,
    /// Mirrors `logcrab::export::ExportRecord::timestamp_unix_ms`; negative for
    /// pre-epoch instants (e.g. time-only lines anchored to 1970 local time).
    timestamp_unix_ms: i64,
    message: String,
    source_file: String,
    filetype: String,
}

// ── Server struct ─────────────────────────────────────────────────────────────

#[derive(Clone)]
struct LogcrabMcp {
    host: String,
    port: u16,
    export_bin: String,
}

impl LogcrabMcp {
    fn new() -> Self {
        let host = std::env::var("LOGCRAB_SIDECAR_HOST")
            .unwrap_or_else(|_| SidecarClient::default_host().to_string());
        let port = std::env::var("LOGCRAB_SIDECAR_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(SidecarClient::default_port());
        let export_bin =
            std::env::var("LOGCRAB_EXPORT_BIN").unwrap_or_else(|_| "logcrab-export".to_string());
        Self {
            host,
            port,
            export_bin,
        }
    }
}

// ── Score normalisation ───────────────────────────────────────────────────────

/// Normalise raw cross-entropy scores to \[0, 100\] via min-max scaling.
fn normalize_scores(scores: &HashMap<usize, f64>) -> HashMap<usize, f64> {
    if scores.is_empty() {
        return HashMap::new();
    }
    let min = scores.values().copied().fold(f64::INFINITY, f64::min);
    let max = scores.values().copied().fold(f64::NEG_INFINITY, f64::max);
    let range = max - min;
    scores
        .iter()
        .map(|(&ln, &s)| {
            let normalized = if range < 1e-10 {
                0.0
            } else {
                (s - min) / range * 100.0
            };
            (ln, normalized)
        })
        .collect()
}

// ── Export parsing ────────────────────────────────────────────────────────────

/// Parse the NDJSON stdout of `logcrab-export`, skipping blank lines.
fn parse_export_output(stdout: &str) -> anyhow::Result<Vec<ExportRecord>> {
    stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("failed to parse export output: {e}"))
}

// ── Result ordering ───────────────────────────────────────────────────────────

/// Sort result entries by their `score` field, most anomalous first.
fn sort_by_score_desc(entries: &mut [Value]) {
    entries.sort_by(|a, b| {
        b["score"]
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&a["score"].as_f64().unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// Convert an export timestamp to the sidecar's unsigned representation,
/// clamping pre-epoch instants to 0 (same policy as the GUI scoring path).
fn sidecar_timestamp(timestamp_unix_ms: i64) -> u64 {
    u64::try_from(timestamp_unix_ms).unwrap_or(0)
}

// ── Helper: build norm_versions map from a ModelInfo ────────────────────────

fn norm_versions_for(model: &logcrab::anomaly::sidecar_client::ModelInfo) -> HashMap<String, u32> {
    model.training_corpus.normalization_versions.clone()
}

// ── Tool implementations ──────────────────────────────────────────────────────

#[tool_router]
impl LogcrabMcp {
    /// List all available `LogBERT` models from the running sidecar.
    #[tool(
        description = "List available LogBERT models from the logcrab sidecar. Returns a JSON array of model objects including id, name, architecture, status, and training corpus info."
    )]
    async fn list_models(&self) -> String {
        let host = self.host.clone();
        let port = self.port;
        let result = task::spawn_blocking(move || {
            SidecarClient::connect(&host, port).and_then(|c| c.list_models())
        })
        .await;
        match result {
            Ok(Ok(models)) => serde_json::to_string_pretty(&models)
                .unwrap_or_else(|e| json!({"error": e.to_string()}).to_string()),
            Ok(Err(e)) => json!({"error": e.to_string()}).to_string(),
            Err(e) => json!({"error": e.to_string()}).to_string(),
        }
    }

    /// Score a list of raw log lines for anomaly using the `LogBERT` sidecar.
    #[tool(
        description = "Score raw log lines for anomaly using the LogBERT sidecar. Returns a JSON object with a `lines` array sorted by anomaly score (0–100, higher = more anomalous), plus any sidecar warnings. Provide a model_id obtained from list_models. Optionally pass filetype (e.g. logcat, dlt, dmesg) for better normalisation accuracy."
    )]
    async fn score_log_lines(
        &self,
        Parameters(ScoreParams {
            model_id,
            lines,
            filetype,
        }): Parameters<ScoreParams>,
    ) -> String {
        let host = self.host.clone();
        let port = self.port;
        let result = task::spawn_blocking(move || -> anyhow::Result<Value> {
            let client = SidecarClient::connect(&host, port)?;
            let models = client.list_models()?;
            let model = models
                .iter()
                .find(|m| m.id == model_id)
                .ok_or_else(|| anyhow::anyhow!("model '{model_id}' not found"))?;

            let owned_versions = norm_versions_for(model);
            let norm_versions: HashMap<&str, u32> = owned_versions
                .iter()
                .map(|(k, &v)| (k.as_str(), v))
                .collect();

            let ft = filetype.as_deref();
            let input_lines: Vec<InputLine> = lines
                .iter()
                .enumerate()
                .map(|(i, msg)| {
                    InputLine::new(0, i + 1, 0, msg.clone(), None, None, ft.map(str::to_string))
                })
                .collect();

            let scored = client.score_stream(&model_id, &norm_versions, &input_lines)?;
            drop(client);

            let raw: HashMap<usize, f64> =
                scored.scored.iter().map(|(&ln, e)| (ln, e.score)).collect();
            let normalized = normalize_scores(&raw);

            let mut out: Vec<Value> = lines
                .iter()
                .enumerate()
                .map(|(i, msg)| {
                    let ln = i + 1;
                    let score = *normalized.get(&ln).unwrap_or(&0.0);
                    let entry = scored.scored.get(&ln);
                    json!({
                        "line_number": ln,
                        "message": msg,
                        "score": score,
                        "is_unk": entry.is_some_and(|e| e.target_is_unk),
                        "is_rare": entry.is_some_and(|e| e.target_is_rare),
                        "filtered": entry.is_none(),
                    })
                })
                .collect();
            sort_by_score_desc(&mut out);

            Ok(json!({
                "lines": out,
                "warnings": scored.warnings,
                "total_lines": lines.len(),
            }))
        })
        .await;
        match result {
            Ok(Ok(v)) => serde_json::to_string_pretty(&v)
                .unwrap_or_else(|e| json!({"error": e.to_string()}).to_string()),
            Ok(Err(e)) => json!({"error": e.to_string()}).to_string(),
            Err(e) => json!({"error": e.to_string()}).to_string(),
        }
    }

    /// Parse a log file via `logcrab-export` (auto-detecting format) then score it.
    #[tool(
        description = "Parse a log file using logcrab-export (which auto-detects the format: logcat, DLT, PCAP, BT Snoop, dmesg, generic text, etc.) and score it for anomalies using the LogBERT sidecar. Returns the top_n most anomalous lines (default 50) as a JSON object with line metadata and anomaly scores."
    )]
    async fn analyze_log_file(
        &self,
        Parameters(AnalyzeParams {
            path,
            model_id,
            top_n,
        }): Parameters<AnalyzeParams>,
    ) -> String {
        let host = self.host.clone();
        let port = self.port;
        let export_bin = self.export_bin.clone();
        let top_n = top_n.unwrap_or(50);

        let result = task::spawn_blocking(move || -> anyhow::Result<Value> {
            // ── Run logcrab-export to parse the file ─────────────────────────
            let output = std::process::Command::new(&export_bin)
                .arg(&path)
                .output()
                .map_err(|e| anyhow::anyhow!("failed to run {export_bin}: {e}"))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("logcrab-export failed: {stderr}");
            }
            let stdout = String::from_utf8_lossy(&output.stdout);

            let records = parse_export_output(&stdout)?;

            if records.is_empty() {
                return Ok(json!({"lines": [], "warnings": [], "total_lines": 0}));
            }

            // ── Fetch model info ─────────────────────────────────────────────
            let client = SidecarClient::connect(&host, port)?;
            let models = client.list_models()?;
            let model = models
                .iter()
                .find(|m| m.id == model_id)
                .ok_or_else(|| anyhow::anyhow!("model '{model_id}' not found"))?;

            let owned_versions = norm_versions_for(model);
            let norm_versions: HashMap<&str, u32> = owned_versions
                .iter()
                .map(|(k, &v)| (k.as_str(), v))
                .collect();

            // ── Build InputLine vec ──────────────────────────────────────────
            let input_lines: Vec<InputLine> = records
                .iter()
                .map(|r| {
                    InputLine::new(
                        0,
                        r.line_number,
                        sidecar_timestamp(r.timestamp_unix_ms),
                        r.message.clone(),
                        None,
                        Some(r.source_file.clone()),
                        Some(r.filetype.clone()),
                    )
                })
                .collect();

            // ── Score ────────────────────────────────────────────────────────
            let scored = client.score_stream(&model_id, &norm_versions, &input_lines)?;
            drop(client);

            let raw: HashMap<usize, f64> =
                scored.scored.iter().map(|(&ln, e)| (ln, e.score)).collect();
            let normalized = normalize_scores(&raw);

            // ── Sort and truncate ────────────────────────────────────────────
            let mut out: Vec<Value> = records
                .iter()
                .map(|r| {
                    let score = *normalized.get(&r.line_number).unwrap_or(&0.0);
                    let entry = scored.scored.get(&r.line_number);
                    json!({
                        "line_number": r.line_number,
                        "message": r.message,
                        "source_file": r.source_file,
                        "filetype": r.filetype,
                        "timestamp_unix_ms": r.timestamp_unix_ms,
                        "score": score,
                        "is_unk": entry.is_some_and(|e| e.target_is_unk),
                        "is_rare": entry.is_some_and(|e| e.target_is_rare),
                        "filtered": entry.is_none(),
                    })
                })
                .collect();
            sort_by_score_desc(&mut out);
            out.truncate(top_n);

            Ok(json!({
                "lines": out,
                "warnings": scored.warnings,
                "total_lines": records.len(),
            }))
        })
        .await;
        match result {
            Ok(Ok(v)) => serde_json::to_string_pretty(&v)
                .unwrap_or_else(|e| json!({"error": e.to_string()}).to_string()),
            Ok(Err(e)) => json!({"error": e.to_string()}).to_string(),
            Err(e) => json!({"error": e.to_string()}).to_string(),
        }
    }
}

#[allow(
    clippy::unused_async_trait_impl,
    reason = "the tool_handler macro generates the required ServerHandler async implementation"
)]
#[tool_handler]
impl ServerHandler for LogcrabMcp {}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // MCP uses stdio for JSON-RPC transport — all logging must go to stderr
    // to avoid corrupting the protocol stream.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();

    let service = LogcrabMcp::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize records exactly as `logcrab-export` does (one JSON object per line).
    fn export_ndjson(records: &[logcrab::export::ExportRecord<'_>]) -> String {
        records
            .iter()
            .map(|r| serde_json::to_string(r).expect("serialize export record") + "\n")
            .collect()
    }

    #[test]
    fn parses_producer_records_field_for_field() {
        let ndjson = export_ndjson(&[logcrab::export::ExportRecord {
            line_number: 7,
            timestamp_unix_ms: 1_700_000_000_123,
            message: "TAG: hello".to_string(),
            source_file: "a.log",
            filetype: "logcat",
        }]);

        let records = parse_export_output(&ndjson).expect("producer output must parse");

        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.line_number, 7);
        assert_eq!(r.timestamp_unix_ms, 1_700_000_000_123);
        assert_eq!(r.message, "TAG: hello");
        assert_eq!(r.source_file, "a.log");
        assert_eq!(r.filetype, "logcat");
    }

    #[test]
    fn accepts_pre_epoch_timestamps_from_producer() {
        // Time-only / syslog-style lines are anchored to 1970-01-01 local time,
        // which is before the epoch in any UTC+ timezone (e.g. -3_599_000 in CET).
        let ndjson = export_ndjson(&[
            logcrab::export::ExportRecord {
                line_number: 1,
                timestamp_unix_ms: -3_599_000,
                message: "pre-epoch".to_string(),
                source_file: "a.log",
                filetype: "generic",
            },
            logcrab::export::ExportRecord {
                line_number: 2,
                timestamp_unix_ms: 0,
                message: "epoch".to_string(),
                source_file: "a.log",
                filetype: "generic",
            },
        ]);

        let records =
            parse_export_output(&ndjson).expect("pre-epoch record must not abort parsing");

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].timestamp_unix_ms, -3_599_000);
        assert_eq!(records[1].timestamp_unix_ms, 0);
    }

    #[test]
    fn sidecar_timestamp_clamps_pre_epoch_to_zero() {
        assert_eq!(sidecar_timestamp(-3_599_000), 0);
        assert_eq!(sidecar_timestamp(0), 0);
        assert_eq!(sidecar_timestamp(1_700_000_000_123), 1_700_000_000_123);
    }

    #[test]
    fn skips_blank_lines_and_accepts_empty_output() {
        assert!(parse_export_output("").expect("empty").is_empty());
        assert!(parse_export_output("\n\n").expect("blank").is_empty());

        let line = r#"{"line_number":1,"timestamp_unix_ms":5,"message":"m","source_file":"f","filetype":"t"}"#;
        let records = parse_export_output(&format!("\n{line}\n\n{line}\n")).expect("blank lines");
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn malformed_line_reports_parse_error() {
        let err = parse_export_output("{not json}\n").expect_err("must fail");
        assert!(err.to_string().starts_with("failed to parse export output"));
    }

    #[test]
    fn normalize_scores_empty_map() {
        assert!(normalize_scores(&HashMap::new()).is_empty());
    }

    #[test]
    fn normalize_scores_all_equal_maps_to_zero() {
        let scores = HashMap::from([(1, 3.5), (2, 3.5), (3, 3.5)]);
        let normalized = normalize_scores(&scores);
        assert_eq!(normalized.len(), 3);
        assert!(normalized.values().all(|&v| v == 0.0));
    }

    #[test]
    fn normalize_scores_maps_min_max_to_0_100() {
        let scores = HashMap::from([(1, 2.0), (2, 4.0), (3, 6.0)]);
        let normalized = normalize_scores(&scores);
        assert!((normalized[&1] - 0.0).abs() < 1e-9);
        assert!((normalized[&2] - 50.0).abs() < 1e-9);
        assert!((normalized[&3] - 100.0).abs() < 1e-9);
    }

    #[test]
    fn sort_by_score_desc_orders_most_anomalous_first() {
        let mut entries = vec![
            json!({"line_number": 1, "score": 10.0}),
            json!({"line_number": 2, "score": 100.0}),
            json!({"line_number": 3, "score": 0.0}),
            json!({"line_number": 4, "score": 55.5}),
        ];
        sort_by_score_desc(&mut entries);
        let order: Vec<u64> = entries
            .iter()
            .map(|e| e["line_number"].as_u64().expect("line_number"))
            .collect();
        assert_eq!(order, vec![2, 4, 1, 3]);
    }
}
