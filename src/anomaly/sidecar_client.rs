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
use std::sync::mpsc;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingCorpus {
    pub filter_profile: String,
    pub description: String,
    /// Filetype slug → normalisation version the training data was built with.
    pub normalization_versions: HashMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkPolicy {
    pub recommended_lines_per_chunk: usize,
    pub max_lines_per_chunk: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputInfo {
    pub score_kind: String,
    pub higher_is_more_anomalous: bool,
    pub supports_explanations: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

// ── Manual classification ─────────────────────────────────────────────────────

/// Label applied by the user when manually classifying a log sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleLabel {
    Benign,
    Anomalous,
}

impl SampleLabel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Benign => "benign",
            Self::Anomalous => "anomalous",
        }
    }
}

impl std::fmt::Display for SampleLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Serialize)]
struct SubmitSampleRequest<'a> {
    api_version: &'static str,
    model_id: &'a str,
    label: SampleLabel,
    classified_line_number: usize,
    lines: &'a [InputLine],
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

// ── Explain session types ─────────────────────────────────────────────────────

/// How much the model attended to context line `line_number` when scoring the
/// target.  Only lines with non-zero weight are returned by the sidecar.
#[derive(Debug, Clone)]
pub struct AttentionEntry {
    pub line_number: usize,
    pub weight: f32,
}

/// A template the model predicted was likely at the masked position,
/// together with its softmax probability.
#[derive(Debug, Clone)]
pub struct TemplateEntry {
    pub template: String,
    pub probability: f32,
}

/// Result of a single explain request.
#[derive(Debug, Clone)]
pub struct ExplainResult {
    pub target_line_number: usize,
    /// `false` when the target was filtered out by the model's corpus filter.
    pub target_in_corpus: bool,
    /// Cross-entropy loss matching the score-stream value; `None` when not in corpus.
    pub target_score: Option<f64>,
    pub target_is_unk: bool,
    pub target_is_rare: bool,
    /// Sparse attention entries — only in-corpus context lines with weight > 0.
    /// Already sorted by weight descending by the sidecar.
    pub attention: Vec<AttentionEntry>,
    /// Top-K templates predicted at the masked position, sorted by probability descending.
    pub top_templates: Vec<TemplateEntry>,
}

// Private deserialization helpers.
#[derive(Deserialize)]
struct AttentionEntryRaw {
    line_number: usize,
    weight: f64,
}

#[derive(Deserialize)]
struct TemplateEntryRaw {
    template: String,
    probability: f64,
}

#[derive(Deserialize)]
struct ExplanationFrame {
    target_line_number: usize,
    target_in_corpus: bool,
    target_score: Option<f64>,
    target_is_unk: bool,
    target_is_rare: bool,
    attention: Vec<AttentionEntryRaw>,
    top_templates: Vec<TemplateEntryRaw>,
}

/// Status returned by [`ExplainSession::poll_status`].
pub enum ExplainPollStatus {
    /// The result is not ready yet.
    Pending,
    /// A result arrived.
    Ready(ExplainResult),
    /// The background WebSocket thread has exited; no further results will arrive.
    Dead,
}

/// Handle to the explain phase of a live score-stream WebSocket session.
///
/// The WebSocket connection is kept alive in a background thread.
/// Dropping this struct closes the connection and exits the thread.
pub struct ExplainSession {
    request_tx: mpsc::SyncSender<usize>,
    result_rx: mpsc::Receiver<ExplainResult>,
}

impl ExplainSession {
    fn spawn(ws: tungstenite::WebSocket<TcpStream>) -> Self {
        let (req_tx, req_rx) = mpsc::sync_channel::<usize>(1);
        let (res_tx, res_rx) = mpsc::sync_channel::<ExplainResult>(4);
        std::thread::spawn(move || Self::run_loop(ws, req_rx, res_tx));
        Self { request_tx: req_tx, result_rx: res_rx }
    }

    /// Request an explanation for `target_line_number`.
    /// Returns `false` if the session has already ended or the queue is full.
    pub fn request(&self, target_line_number: usize) -> bool {
        self.request_tx.try_send(target_line_number).is_ok()
    }

    /// Poll for a completed explanation without blocking.
    pub fn try_recv(&self) -> Option<ExplainResult> {
        self.result_rx.try_recv().ok()
    }

    /// Poll the session and return a rich status so callers can detect when
    /// the background WebSocket thread has exited.
    pub fn poll_status(&self) -> ExplainPollStatus {
        match self.result_rx.try_recv() {
            Ok(result) => ExplainPollStatus::Ready(result),
            Err(mpsc::TryRecvError::Empty) => ExplainPollStatus::Pending,
            Err(mpsc::TryRecvError::Disconnected) => ExplainPollStatus::Dead,
        }
    }

    fn run_loop(
        mut ws: tungstenite::WebSocket<TcpStream>,
        req_rx: mpsc::Receiver<usize>,
        res_tx: mpsc::SyncSender<ExplainResult>,
    ) {
        // Short read timeout so we can interleave WebSocket keepalive handling
        // with waiting for explain requests from the UI thread.
        let _ = ws.get_ref().set_read_timeout(Some(Duration::from_secs(5)));

        loop {
            // Poll for a pending explain request while servicing WebSocket
            // control frames (Ping→Pong, Close) that arrive in the meantime.
            let target_ln = loop {
                match req_rx.try_recv() {
                    Ok(ln) => break ln,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        // All senders dropped — send close frame and exit.
                        let _ = ws.send(Message::Text(r#"{"type":"close"}"#.into()));
                        return;
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                }
                // No request yet — service incoming WebSocket frames.
                match ws.read() {
                    Ok(Message::Ping(data)) => {
                        let _ = ws.send(Message::Pong(data));
                    }
                    Ok(Message::Close(_)) => return,
                    Ok(_) => {} // unexpected data frame while idle — ignore
                    Err(tungstenite::Error::Io(e))
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        // Read timeout elapsed — go back and poll the channel.
                    }
                    Err(_) => return, // connection lost
                }
            };

            let frame = serde_json::json!({
                "type": "explain",
                "target_line_number": target_ln,
            });
            if ws.send(Message::Text(frame.to_string().into())).is_err() {
                break;
            }

            // Read until we get the explanation response for this request.
            loop {
                match ws.read() {
                    Ok(Message::Text(text)) => {
                        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                            break;
                        };
                        if v["type"] == "explanation" {
                            if let Ok(f) = serde_json::from_value::<ExplanationFrame>(v) {
                                let result = ExplainResult {
                                    target_line_number: f.target_line_number,
                                    target_in_corpus: f.target_in_corpus,
                                    target_score: f.target_score,
                                    target_is_unk: f.target_is_unk,
                                    target_is_rare: f.target_is_rare,
                                    attention: f.attention.into_iter().map(|e| AttentionEntry {
                                        line_number: e.line_number,
                                        weight: e.weight as f32,
                                    }).collect(),
                                    top_templates: f.top_templates.into_iter().map(|e| TemplateEntry {
                                        template: e.template,
                                        probability: e.probability as f32,
                                    }).collect(),
                                };
                                let _ = res_tx.send(result);
                            }
                            break;
                        }
                        // Ignore other frame types (e.g. stray warnings).
                    }
                    Ok(Message::Ping(data)) => {
                        let _ = ws.send(Message::Pong(data));
                    }
                    Ok(Message::Close(_)) | Err(_) => return,
                    _ => {}
                }
            }
        }
    }
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

    /// `POST /v1/samples` — submit a manually labelled log sample for future training.
    ///
    /// Uploads all `lines` from the source that contains `classified_line_number`, tagged
    /// with `label`.  The sidecar stores the data under
    /// `{uploads_dir}/{model_id}/{label}/{timestamp}_{uuid}.ndjson`.
    pub fn submit_sample(
        &self,
        model_id: &str,
        label: SampleLabel,
        classified_line_number: usize,
        lines: &[InputLine],
    ) -> Result<()> {
        let url = format!("http://{}:{}/v1/samples", self.host, self.port);
        let body = SubmitSampleRequest { api_version: "1", model_id, label, classified_line_number, lines };
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .context("submit_sample request failed")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let msg = resp.text().unwrap_or_default();
            bail!("submit_sample returned {status}: {msg}");
        }
        Ok(())
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
        let (result, _ws) = self.open_score_stream_ws(model_id, normalization_versions, lines, &mut |_, _| {})?;
        Ok(result)
    }

    /// Like [`score_stream`], but keeps the WebSocket open for on-demand attention
    /// explanations after scoring completes.
    ///
    /// Returns the scores plus an [`ExplainSession`] that the caller can use to
    /// request per-line attention weights without re-sending the input lines.
    /// The WebSocket connection is held alive in a background thread until the
    /// `ExplainSession` is dropped.
    pub fn score_stream_with_explain(
        &self,
        model_id: &str,
        normalization_versions: &HashMap<&str, u32>,
        lines: &[InputLine],
    ) -> Result<(ScoreStreamResult, ExplainSession)> {
        let (result, ws) = self.open_score_stream_ws(model_id, normalization_versions, lines, &mut |_, _| {})?;
        Ok((result, ExplainSession::spawn(ws)))
    }

    /// Like [`score_stream_with_explain`], but invokes `on_scores` after each
    /// `scores` frame arrives from the sidecar. This lets the caller
    /// incrementally apply results (e.g. update the UI store) as GPU batches
    /// complete on the server side.
    ///
    /// The callback receives `(&ScoreStreamResult, lines_total)` where
    /// `lines_total` is the total number of input lines for progress calculation.
    pub fn score_stream_streaming(
        &self,
        model_id: &str,
        normalization_versions: &HashMap<&str, u32>,
        lines: &[InputLine],
        on_scores: &mut dyn FnMut(&ScoreStreamResult, usize),
    ) -> Result<(ScoreStreamResult, ExplainSession)> {
        let total = lines.len();
        let (result, ws) = self.open_score_stream_ws(
            model_id,
            normalization_versions,
            lines,
            &mut |result, _| on_scores(result, total),
        )?;
        Ok((result, ExplainSession::spawn(ws)))
    }

    /// Internal: open a WebSocket, run the full scoring protocol, and return
    /// both the results and the still-open WebSocket (ready for the explain phase).
    ///
    /// `on_frame` is called after each `scores` frame is processed, receiving
    /// a reference to the accumulated result so far and the chunk_index.
    fn open_score_stream_ws(
        &self,
        model_id: &str,
        normalization_versions: &HashMap<&str, u32>,
        lines: &[InputLine],
        on_frame: &mut dyn FnMut(&ScoreStreamResult, usize),
    ) -> Result<(ScoreStreamResult, tungstenite::WebSocket<TcpStream>)> {
        let addr = format!("{}:{}", self.host, self.port);
        tracing::info!("Connecting to sidecar at {addr}");
        let tcp = TcpStream::connect(&addr)
            .with_context(|| format!("TCP connect to {addr} failed"))?;
        // No write timeout: the OS handles flow control; we block freely.
        // During the send phase we use a short read timeout (1 ms) so that
        // ws.read() acts like try_read() — returning immediately when there is
        // nothing yet — letting us interleave sends and reads without threads
        // or a Mutex.  After all frames are sent we switch to 600 s so we wait
        // patiently for the server to finish GPU scoring.
        tcp.set_write_timeout(None)?;
        // Leave read timeout unset for the handshake; we'll configure it below.

        let ws_url = format!("ws://{}:{}/v1/score-stream", self.host, self.port);
        let (mut ws, _) = tungstenite::client(ws_url, tcp)
            .context("WebSocket handshake failed")?;
        let n_chunks = (lines.len() + LINES_PER_CHUNK - 1) / LINES_PER_CHUNK;
        tracing::info!("WebSocket handshake done — sending {} lines ({n_chunks} chunks)", lines.len());

        // 1 ms non-blocking read during the send phase (acts like try_read).
        // Switched to 600 s once all frames are sent.
        ws.get_ref().set_read_timeout(Some(Duration::from_millis(1)))?;

        // ── Send phase: stream all frames, opportunistically reading responses ─
        // tungstenite preserves internal read state across WouldBlock/TimedOut
        // errors, so it is safe to call read() speculatively between sends.
        let mut result = ScoreStreamResult::default();

        let start = StartFrame::new(model_id, normalization_versions);
        ws.send(Message::Text(serde_json::to_string(&start)?.into()))?;
        tracing::debug!("sent start frame");

        for (chunk_index, chunk) in lines.chunks(LINES_PER_CHUNK).enumerate() {
            let frame = LinesFrame { type_: "lines", chunk_index, lines: chunk };
            ws.send(Message::Text(serde_json::to_string(&frame)?.into()))?;
            if chunk_index % 500 == 499 {
                tracing::info!("sent {}/{n_chunks} chunks", chunk_index + 1);
            }
            // Opportunistic read: consume any scores frames the server has
            // already produced while we were uploading.
            Self::drain_nonblocking(&mut ws, &mut result, on_frame)?;
        }

        ws.send(Message::Text(serde_json::to_string(&EndFrame { type_: "end" })?.into()))?;
        tracing::info!("all {n_chunks} chunks + end frame sent — waiting for complete");

        // ── Read phase: switch to long timeout and consume remaining responses ─
        ws.get_ref().set_read_timeout(Some(Duration::from_secs(600)))?;

        loop {
            match ws.read()? {
                Message::Text(text) => {
                    if Self::handle_text(&text, &mut result, on_frame)? == ReadAction::Break {
                        break;
                    }
                }
                Message::Close(_) => break,
                Message::Ping(data) => ws.send(Message::Pong(data))?,
                _ => {}
            }
        }

        Ok((result, ws))
    }

    /// Drain any immediately-available inbound WebSocket messages, ignoring
    /// `WouldBlock`/`TimedOut` (i.e. nothing ready yet).
    fn drain_nonblocking(
        ws: &mut tungstenite::WebSocket<TcpStream>,
        result: &mut ScoreStreamResult,
        on_frame: &mut dyn FnMut(&ScoreStreamResult, usize),
    ) -> Result<()> {
        loop {
            match ws.read() {
                Ok(Message::Text(text)) => {
                    Self::handle_text(&text, result, on_frame)?;
                }
                Ok(Message::Ping(data)) => ws.send(Message::Pong(data))?,
                Ok(Message::Close(_)) => bail!("sidecar closed connection during send phase"),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    return Ok(()); // nothing ready — continue sending
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Parse and act on a text WebSocket frame.  Returns `Break` when a
    /// `complete` frame is received (only expected in the read phase).
    fn handle_text(
        text: &str,
        result: &mut ScoreStreamResult,
        on_frame: &mut dyn FnMut(&ScoreStreamResult, usize),
    ) -> Result<ReadAction> {
        let envelope: serde_json::Value =
            serde_json::from_str(text).context("invalid JSON from sidecar")?;
        match envelope["type"].as_str() {
            Some("accepted") => {
                if let Ok(f) = serde_json::from_value::<AcceptedFrame>(envelope) {
                    tracing::info!(run_id = %f.run_id, "sidecar accepted the run");
                } else {
                    tracing::info!("sidecar accepted the run");
                }
            }
            Some("scores") => {
                let frame: ScoresFrame = serde_json::from_value(envelope)
                    .context("failed to parse scores frame")?;
                let chunk_index = frame.chunk_index;
                let newly_scored = frame.scored.len();
                for s in frame.scored {
                    result.scored.insert(
                        s.line_id.line_number,
                        ScoreEntry {
                            score: s.score,
                            target_is_unk: s.target_is_unk,
                            target_is_rare: s.target_is_rare,
                        },
                    );
                }
                let _ = frame.filtered; // absent from `scored`; callers assign score 0.0
                tracing::info!(
                    "scores frame #{chunk_index}: +{newly_scored} scored, {} total so far",
                    result.scored.len()
                );
                on_frame(result, chunk_index);
            }
            Some("warning") => {
                let frame: WarningFrame = serde_json::from_value(envelope)
                    .context("failed to parse warning frame")?;
                tracing::warn!("sidecar warning [{}]: {}", frame.code, frame.message);
                result.warnings.push(format!("[{}] {}", frame.code, frame.message));
            }
            Some("complete") => {
                if let Ok(f) = serde_json::from_value::<CompleteFrame>(envelope) {
                    tracing::info!(
                        lines_received = f.summary.lines_received,
                        lines_scored = f.summary.lines_scored,
                        lines_filtered = f.summary.lines_filtered,
                        "sidecar signalled complete",
                    );
                } else {
                    tracing::info!("sidecar signalled complete");
                }
                return Ok(ReadAction::Break);
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
        Ok(ReadAction::Continue)
    }
}

#[derive(PartialEq)]
enum ReadAction {
    Continue,
    Break,
}