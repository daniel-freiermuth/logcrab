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

//! Sidecar client for LogBERT anomaly detection — V1 WebSocket protocol.
//!
//! HTTP endpoints (`GET /v1/health`, `GET /v1/models`) use `reqwest` blocking.
//! Scoring uses a WebSocket connection (`WS /v1/score-stream`).

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::TcpStream;
use std::time::Duration;
use tungstenite::Message;

const DEFAULT_PORT: u16 = 8765;
const DEFAULT_HOST: &str = "127.0.0.1";

/// Lines per `LinesFrame` sent to the sidecar. Matches the typical model
/// `recommended_lines_per_chunk` without needing to parse the model info first.
const LINES_PER_CHUNK: usize = 512;

// ── HTTP response types ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct HealthResponse {
    pub api_version: String,
    pub status: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrainingCorpus {
    pub filter_profile: String,
    pub description: String,
    /// Filetype slug → normalisation version the training data was built with.
    pub normalization_versions: HashMap<String, u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChunkPolicy {
    pub recommended_lines_per_chunk: usize,
    pub max_lines_per_chunk: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OutputInfo {
    pub score_kind: String,
    pub higher_is_more_anomalous: bool,
    pub supports_explanations: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelInfo {
    /// Stable machine-readable slug, used as `model_id` in the scoring protocol.
    pub id: String,
    /// Human-readable label describing the model and its training domain.
    #[serde(alias = "display_name")]
    pub name: String,
    /// Model architecture identifier (e.g. `temporal_logbert`).
    pub architecture: String,
    pub kind: String,
    pub version: String,
    pub status: String,
    pub input_mode: String,
    pub training_corpus: TrainingCorpus,
    pub chunk_policy: ChunkPolicy,
    pub output: OutputInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FilterProfileInfo {
    pub id: String,
    pub description: String,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    #[allow(dead_code)]
    api_version: String,
    pub models: Vec<ModelInfo>,
    #[allow(dead_code)]
    pub filter_profiles: Vec<FilterProfileInfo>,
}

// ── WebSocket frame types — outbound (client → sidecar) ─────────────────────

#[derive(Debug, Serialize)]
struct StartFrame<'a> {
    #[serde(rename = "type")]
    type_: &'static str,
    api_version: &'static str,
    model_id: &'a str,
    filtering_mode: &'static str,
    normalization_versions: &'a HashMap<&'a str, u32>,
}

impl<'a> StartFrame<'a> {
    fn new(model_id: &'a str, normalization_versions: &'a HashMap<&'a str, u32>) -> Self {
        Self {
            type_: "start",
            api_version: "1",
            model_id,
            filtering_mode: "backend_authoritative",
            normalization_versions,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LineId {
    /// 16-bit source identifier — spec constrains to 0–65535.
    source_id: u16,
    line_number: usize,
    /// Milliseconds since Unix epoch. Non-negative per spec (minimum: 0).
    timestamp_unix_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct InputLine {
    pub line_id: LineId,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filetype: Option<String>,
}

impl InputLine {
    pub fn new(
        source_id: u16,
        line_number: usize,
        timestamp_unix_ms: u64,
        message: String,
        template_key: Option<String>,
        source_file: Option<String>,
        filetype: Option<String>,
    ) -> Self {
        Self {
            line_id: LineId { source_id, line_number, timestamp_unix_ms },
            message,
            template_key,
            source_file,
            filetype,
        }
    }
}

#[derive(Debug, Serialize)]
struct LinesFrame<'a> {
    #[serde(rename = "type")]
    type_: &'static str,
    chunk_index: usize,
    lines: &'a [InputLine],
}

#[derive(Debug, Serialize)]
struct EndFrame {
    #[serde(rename = "type")]
    type_: &'static str,
}

// ── WebSocket frame types — inbound (sidecar → client) ──────────────────────

#[derive(Debug, Deserialize)]
struct InboundLineId {
    #[allow(dead_code)]
    source_id: u16,
    line_number: usize,
    #[allow(dead_code)]
    timestamp_unix_ms: u64,
}

#[derive(Debug, Deserialize)]
struct ScoredLine {
    line_id: InboundLineId,
    score: f64,
    #[allow(dead_code)]
    score_kind: String,
    pub target_is_unk: bool,
    pub target_is_rare: bool,
}

#[derive(Debug, Deserialize)]
struct FilteredLine {
    #[allow(dead_code)]
    line_id: InboundLineId,
    #[allow(dead_code)]
    reason: String,
}

#[derive(Debug, Deserialize)]
struct ScoresFrame {
    #[allow(dead_code)]
    chunk_index: usize,
    scored: Vec<ScoredLine>,
    filtered: Vec<FilteredLine>,
}

#[derive(Debug, Deserialize)]
struct WarningFrame {
    code: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct AcceptedFrame {
    #[allow(dead_code)]
    run_id: String,
    #[allow(dead_code)]
    chunk_policy: serde_json::Value, // mirrors ChunkPolicy; we don't act on it currently
}

#[derive(Debug, Deserialize)]
struct Summary {
    #[allow(dead_code)]
    lines_received: u64,
    #[allow(dead_code)]
    lines_scored: u64,
    #[allow(dead_code)]
    lines_filtered: u64,
}

#[derive(Debug, Deserialize)]
struct CompleteFrame {
    #[allow(dead_code)]
    summary: Summary,
}

#[derive(Debug, Deserialize)]
struct ErrorFrame {
    code: String,
    message: String,
}

// ── Score stream result ──────────────────────────────────────────────────────

/// Per-line score from the sidecar.
#[derive(Debug, Clone)]
pub struct ScoreEntry {
    pub score: f64,
    pub target_is_unk: bool,
    pub target_is_rare: bool,
}

/// Result of a complete `score_stream` run.
#[derive(Debug, Default)]
pub struct ScoreStreamResult {
    /// Keyed by `line_number` from `InputLine.line_id`.
    /// Lines absent were filtered by the sidecar's corpus filter; callers assign them score 0.0.
    /// Valid as long as a single `score_stream` call covers exactly one source (no `source_id` collisions).
    pub scored: HashMap<usize, ScoreEntry>,
    /// Non-fatal warnings emitted by the sidecar (e.g. `normalization_version_mismatch`).
    pub warnings: Vec<String>,
}

// ── SidecarClient ────────────────────────────────────────────────────────────

/// Client for the LogBERT sidecar — V1 protocol.
pub struct SidecarClient {
    host: String,
    port: u16,
    http: reqwest::blocking::Client,
}

impl SidecarClient {
    pub const fn default_host() -> &'static str {
        DEFAULT_HOST
    }

    pub const fn default_port() -> u16 {
        DEFAULT_PORT
    }

    pub fn connect(host: &str, port: u16) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self { host: host.to_string(), port, http })
    }

    /// `GET /v1/health` — liveness check.
    pub fn health_check(&self) -> Result<HealthResponse> {
        let url = format!("http://{}:{}/v1/health", self.host, self.port);
        let resp = self.http.get(&url).send().context("health request failed")?;
        if !resp.status().is_success() {
            bail!("health check returned {}", resp.status());
        }
        Ok(resp.json()?)
    }

    /// `GET /v1/models` — discover available models.
    pub fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let url = format!("http://{}:{}/v1/models", self.host, self.port);
        let resp = self.http.get(&url).send().context("models request failed")?;
        if !resp.status().is_success() {
            bail!("list_models returned {}", resp.status());
        }
        let body: ModelsResponse = resp.json()?;
        Ok(body.models)
    }

    /// `WS /v1/score-stream` — stream lines to the sidecar and collect scores.
    ///
    /// Sends `lines` in chunks of [`LINES_PER_CHUNK`], waits for all `scores`
    /// frames, and returns the aggregated results.
    pub fn score_stream(
        &self,
        model_id: &str,
        normalization_versions: &HashMap<&str, u32>,
        lines: &[InputLine],
    ) -> Result<ScoreStreamResult> {
        let addr = format!("{}:{}", self.host, self.port);
        let tcp = TcpStream::connect(&addr)
            .with_context(|| format!("TCP connect to {addr} failed"))?;
        // Long timeout: large files can take minutes to score.
        tcp.set_read_timeout(Some(Duration::from_secs(600)))?;
        tcp.set_write_timeout(Some(Duration::from_secs(60)))?;

        let ws_url = format!("ws://{}:{}/v1/score-stream", self.host, self.port);
        let (mut ws, _) = tungstenite::client(ws_url, tcp)
            .context("WebSocket handshake failed")?;

        // ── Send start frame ─────────────────────────────────────────────────
        let start = StartFrame::new(model_id, normalization_versions);
        ws.send(Message::Text(serde_json::to_string(&start)?.into()))?;

        // ── Send lines frames ────────────────────────────────────────────────
        for (chunk_index, chunk) in lines.chunks(LINES_PER_CHUNK).enumerate() {
            let frame = LinesFrame { type_: "lines", chunk_index, lines: chunk };
            ws.send(Message::Text(serde_json::to_string(&frame)?.into()))?;
        }

        // ── Send end frame ───────────────────────────────────────────────────
        ws.send(Message::Text(
            serde_json::to_string(&EndFrame { type_: "end" })?.into(),
        ))?;

        // ── Read response frames ─────────────────────────────────────────────
        let mut result = ScoreStreamResult::default();

        loop {
            match ws.read()? {
                Message::Text(text) => {
                    // Peek at the `type` field to dispatch without full deserialization.
                    let envelope: serde_json::Value = serde_json::from_str(&text)
                        .context("invalid JSON from sidecar")?;
                    match envelope["type"].as_str() {
                        Some("accepted") => {
                            if let Ok(f) = serde_json::from_value::<AcceptedFrame>(envelope) {
                                tracing::debug!(run_id = %f.run_id, "sidecar accepted the run");
                            } else {
                                tracing::debug!("sidecar accepted the run");
                            }
                        }
                        Some("scores") => {
                            let frame: ScoresFrame = serde_json::from_value(envelope)
                                .context("failed to parse scores frame")?;
                            for s in frame.scored {
                                result.scored.insert(
                                    s.line_id.line_number,
                                    ScoreEntry { score: s.score, target_is_unk: s.target_is_unk, target_is_rare: s.target_is_rare },
                                );
                            }
                            // filtered lines are intentionally absent from `scored`; the
                            // caller assigns them score 0.0.
                            let _ = frame.filtered; // acknowledged, not stored
                        }
                        Some("warning") => {
                            let frame: WarningFrame = serde_json::from_value(envelope)
                                .context("failed to parse warning frame")?;
                            tracing::warn!("sidecar warning [{}]: {}", frame.code, frame.message);
                            result.warnings.push(format!("[{}] {}", frame.code, frame.message));
                        }
                        Some("complete") => {
                            if let Ok(f) = serde_json::from_value::<CompleteFrame>(envelope) {
                                tracing::debug!(
                                    lines_received = f.summary.lines_received,
                                    lines_scored = f.summary.lines_scored,
                                    lines_filtered = f.summary.lines_filtered,
                                    "sidecar signalled complete",
                                );
                            } else {
                                tracing::debug!("sidecar signalled complete");
                            }
                            break;
                        }
                        Some("error") => {
                            let frame: ErrorFrame = serde_json::from_value(envelope)
                                .context("failed to parse error frame")?;
                            bail!("sidecar error [{}]: {}", frame.code, frame.message);
                        }
                        other => {
                            tracing::warn!("unknown frame type from sidecar: {other:?}");
                        }
                    }
                }
                Message::Close(_) => break,
                Message::Ping(data) => {
                    ws.send(Message::Pong(data))?;
                }
                _ => {}
            }
        }

        Ok(result)
    }
}

