# Sidecar API V1 Design

This document defines the proposed V1 API between the LogCrab Rust frontend and the Python scoring sidecar.

The goal is to make the contract concrete enough to implement on both sides while staying narrow, boring, and evolvable.

## Status

- Proposed design
- Intended transport: localhost HTTP (health, models) and WebSocket (scoring)
- Companion specification: `docs/sidecar_api_v1.openapi.yaml`
- Companion frame schema: `docs/sidecar_api_v1.frames.schema.json`

## Goals

- Let the user choose between multiple backend-provided models.
- Score the full scoring corpus, not the current UI-visible subset.
- Allow models to define which subset of the loaded corpus they accept.
- Support large log files via chunked transport without changing scoring semantics.
- Return scores keyed by stable line identifiers so results can be merged back into `LogStore` deterministically.
- Keep the V1 protocol small enough to implement and debug easily.

## Non-Goals

- Remote multi-tenant serving.
- Authentication or authorization.
- Resumable jobs after disconnect.
- Generic workflow orchestration.
- A protocol specialized to one model architecture.

## Design Principles

1. The Rust frontend owns loaded log data, line identity, and UI state.
2. The Python sidecar owns model discovery, model-specific corpus acceptance, and scoring.
3. UI filters and scoring filters are different concepts and must not be conflated.
4. One open scoring connection corresponds to one scoring run.
5. Scores are always returned keyed by line identity, never only by position.
6. Transport chunking is an implementation detail and must not change scoring semantics.

## Core Concepts

### Scoring Run

A scoring run is one complete attempt to score one loaded corpus with one selected model.

In V1, a scoring run corresponds to one WebSocket connection. Connection lifetime is run lifetime.

- Opening the connection starts the run.
- Sending `lines` frames feeds input lines.
- The `end` frame signals end-of-input.
- The `complete` response frame signals end-of-run.
- Closing the connection early cancels the run.

### Scoring Corpus

The scoring corpus is the set of loaded lines that participate in scoring for a selected model.

Important:

- The scoring corpus is not influenced by filter tabs, search, or visibility state.
- The scoring corpus may be a subset of loaded lines if the selected model was trained on a restricted corpus.
- The backend is authoritative for model-specific corpus filtering.

### Stable Line Identity

Every line sent to the sidecar must carry a stable ID so the score can be mapped back to the Rust store.

The existing `LineId` direction from the multi-source architecture is a good fit:

```json
{
  "source_id": 0,
  "line_number": 123,
  "timestamp_unix_ms": 1713093201000
}
```

The sidecar must treat this identifier as opaque identity. It may inspect fields for debugging or validation, but it must not invent replacement IDs.

## API Surface

V1 exposes four endpoints:

1. `GET /v1/health`
2. `GET /v1/models`
3. `WS  /v1/score-stream`
4. `POST /v1/samples`

`WS /v1/score-stream` is the primary scoring protocol. `POST /v1/samples` is an optional
training-data collection mechanism.

## Endpoint: `GET /v1/health`

Purpose:

- Liveness check
- Optional readiness signal while models are loading

Example response:

```json
{
  "api_version": "1",
  "status": "ok"
}
```

## Endpoint: `GET /v1/models`

Purpose:

- Let the frontend discover available models.
- Publish each model's input expectations.
- Publish model-owned corpus filter information.
- Publish chunking hints.

Example response:

```json
{
  "api_version": "1",
  "models": [
    {
      "id": "logbert-android-errors",
      "name": "Android Errors (WARN/ERROR) — logcat 2026",
      "architecture": "temporal_logbert",
      "kind": "sequence_anomaly",
      "version": "2026-04-14",
      "status": "ready",
      "input_mode": "ordered_lines",
      "training_corpus": {
        "filter_profile": "android-errors-v1",
        "description": "Android logcat WARN and ERROR lines with parsed message field",
        "normalization_versions": {
          "logcat": 2
        }
      },
      "supported_fields": [
        "message",
        "template_key",
        "timestamp_unix_ms",
        "source_file",
        "filetype"
      ],
      "required_fields": [
        "message"
      ],
      "chunk_policy": {
        "recommended_lines_per_chunk": 2048,
        "max_lines_per_chunk": 65536
      },
      "output": {
        "score_kind": "anomaly",
        "higher_is_more_anomalous": true,
        "supports_explanations": false
      }
    }
  ],
  "filter_profiles": [
    {
      "id": "android-errors-v1",
      "description": "Android logcat WARN and ERROR lines with parsed message field"
    }
  ]
}
```

### Notes

- `filter_profiles` are descriptive metadata, not frontend-owned filtering logic.
- The frontend may show them in the UI to explain what a model will score.
- The backend remains authoritative for what is accepted.
- `training_corpus.normalization_versions` maps each filetype slug to the integer version of
  the logcrab parser/normalizer that was used to produce the training data. The frontend
  declares its own current versions in the `start` frame. If the frontend's version for a
  relevant filetype differs from what the model was trained on, the sidecar emits a `warning`
  frame (code `normalization_version_mismatch`). Because each model within a sidecar can have
  been trained on data from a different parser generation, versions are per-model, not global.

## Endpoint: `WS /v1/score-stream`

### Transport

- Protocol: WebSocket (`ws://127.0.0.1:8765/v1/score-stream`)
- Each WebSocket text message is one JSON object with a `type` discriminator.
- One connection corresponds to exactly one scoring run. Close after `complete` or `error`.
- The server responds to WebSocket ping frames automatically.

### Why WebSocket

- Native message framing: no newline delimiters or byte stream splitting needed.
- True bidirectional: the server can emit `scores` frames while the client is still sending `lines` frames.
- Clean connection lifetime: one run per connection, disconnect cancels the run.
- Debuggable with `wscat` or any WebSocket client.

## Request Frames

The request stream uses exactly three frame types:

1. `start`
2. `lines`
3. `end`

### `start`

Must be the first frame.

Example:

```json
{
  "type": "start",
  "api_version": "1",
  "model_id": "logbert-android-errors",
  "filtering_mode": "backend_authoritative",
  "normalization_versions": {
    "logcat": 2,
    "dlt": 1
  }
}
```

Fields:

- `api_version`: protocol version string
- `model_id`: selected backend model
- `filtering_mode`: must be `backend_authoritative` in V1
- `normalization_versions`: map of filetype slug to the integer version of the normalizer
  (`message()`) currently in use by this frontend build. The sidecar compares these against
  the values in `training_corpus.normalization_versions` for the selected model. A mismatch
  on a filetype that the model actually scores triggers a `warning` frame.

### `lines`

Carries one ordered chunk of candidate lines from the loaded corpus.

Example:

```json
{
  "type": "lines",
  "chunk_index": 0,
  "lines": [
    {
      "line_id": {
        "source_id": 0,
        "line_number": 123,
        "timestamp_unix_ms": 1713093201000
      },
      "message": "ERROR disk full",
      "source_file": "system.log",
      "filetype": "generic"
    },
    {
      "line_id": {
        "source_id": 0,
        "line_number": 124,
        "timestamp_unix_ms": 1713093202000
      },
      "message": "INFO retrying",
      "source_file": "system.log",
      "filetype": "generic"
    }
  ]
}
```

Fields:

- `chunk_index`: strictly increasing zero-based integer
- `lines`: ordered candidate input lines

The frontend should send all loaded candidate lines for the scoring run, not only currently visible lines.

### `end`

Signals that no more request frames will follow.

Example:

```json
{
  "type": "end"
}
```

## Response Frames

The response stream uses five frame types:

1. `accepted`
2. `scores`
3. `warning`
4. `error`
5. `complete`

### `accepted`

Confirms that the run has started and echoes back operational parameters. (This frame type keeps the name `accepted` — it is the handshake acknowledgement, not a per-line outcome.)

Example:

```json
{
  "type": "accepted",
  "api_version": "1",
  "run_id": "run_01hsz2j3p6k4n8m",
  "model_id": "logbert-android-errors",
  "chunk_policy": {
    "recommended_lines_per_chunk": 2048,
    "max_lines_per_chunk": 65536
  }
}
```

`run_id` is diagnostic metadata. It is not a resumable session handle in V1.

### `scores`

Returns per-line outcomes keyed by `line_id`.

Because the backend owns corpus filtering, the response must distinguish scored and filtered lines explicitly.

Each element of `scored` carries:
- `score` — the anomaly score produced by the model
- `score_kind` — always `"anomaly"` in V1
- `target_is_unk` — `true` when the line's template was not in the model vocabulary and the
  `[UNK]` token embedding was used instead. The score is still valid, but carries less signal
  than a fully-known template.

Each element of `filtered` carries a `reason` from the following closed enum:

| `reason` | Meaning |
|---|---|
| `out_of_corpus` | Model's corpus filter rejected this line (e.g. wrong filetype for this model) |
| `empty_after_normalization` | Template normalised to an empty string |

Example:

```json
{
  "type": "scores",
  "chunk_index": 0,
  "scored": [
    {
      "line_id": {
        "source_id": 0,
        "line_number": 123,
        "timestamp_unix_ms": 1713093201000
      },
      "score": 3.421,
      "score_kind": "anomaly",
      "target_is_unk": false
    },
    {
      "line_id": {
        "source_id": 0,
        "line_number": 125,
        "timestamp_unix_ms": 1713093203000
      },
      "score": 8.12,
      "score_kind": "anomaly",
      "target_is_unk": true
    }
  ],
  "filtered": [
    {
      "line_id": {
        "source_id": 0,
        "line_number": 124,
        "timestamp_unix_ms": 1713093202000
      },
      "reason": "out_of_corpus"
    }
  ]
}
```

This is intentionally explicit. A missing score must never be ambiguous.

`target_is_unk` and `filtered` are orthogonal: a line can be scored with `target_is_unk: true`
(the model ran with an UNK embedding), and a line can be filtered for reasons unrelated to
vocabulary coverage.

### `warning`

Carries recoverable issues.

Example:

```json
{
  "type": "warning",
  "code": "line_truncated",
  "message": "12 lines exceeded max token length and were truncated"
}
```

### `error`

Carries terminal run errors.

Example:

```json
{
  "type": "error",
  "code": "unsupported_model",
  "message": "Model 'foo' is not available"
}
```

After an `error` frame, the backend should close the stream.

### `complete`

Marks successful run completion.

Example:

```json
{
  "type": "complete",
  "summary": {
    "lines_received": 20480,
    "lines_scored": 18200,
    "lines_filtered": 2280
  }
}
```

## Filtering Semantics

V1 treats corpus filtering as model-owned and backend-authoritative.

That means:

- The Rust frontend sends the full loaded scoring candidate set.
- The Python sidecar applies the model's corpus filter before inference.
- The backend returns scored lines with scores and filtered lines with explicit reasons.

### Corpus filter

Each model ships with a corpus filter alongside its weights. The filter is a simple predicate
over the input line's fields (typically `filetype`, and optionally message-level heuristics)
that decides whether the line is within the model's intended training domain.

Examples:
- An Android logcat model may filter out any line whose `filetype` is not `logcat` or `bugreport`.
- A kernel model may filter out any line that does not look like a dmesg message.

The corpus filter is not configurable by the end user. It is part of the model artifact.

### Why not use UI filters

UI filters are view concerns.

Scoring filters are model correctness concerns.

Mixing them would make scoring dependent on the currently open tab, which is incorrect and hard to reason about.

### Why not silently filter in the backend

Silent filtering creates ambiguity between:

- line not scored yet
- line filtered out
- line rejected due to bad input
- line missing due to protocol error

The protocol therefore requires filter visibility.

## Ordering and Chunking

### Ordering

- The frontend must preserve line order across the scoring run.
- `chunk_index` must increase monotonically.
- The backend must not reorder lines within a returned outcome set relative to identity mapping.

### Chunking

Chunking is a transport concern, not a scoring-semantic concern.

The scoring run represents one whole corpus scored as a single sequence. Chunks only exist to avoid sending one enormous WebSocket message. There is no overlap between chunks — each line is sent exactly once.

The backend publishes chunking hints via `GET /v1/models` and echoes them in `accepted`. The frontend may choose smaller chunks than recommended. It must not exceed the backend-published `max_lines_per_chunk`.

## Error Semantics

WebSocket upgrade HTTP status codes:

- `101`: upgrade accepted, connection is now a WebSocket
- `400`: invalid upgrade request or unknown path
- `500`: sidecar internal error during upgrade
- `503`: sidecar not ready (model loading or unavailable)

Within an established connection, semantic failures must use `error` frames. The server closes the connection after sending an `error` frame.

## State Machine

Frontend obligations:

1. Call `GET /v1/models`.
2. Let the user choose a model.
3. Open `WS /v1/score-stream`.
4. Send exactly one `start` frame.
5. Send zero or more `lines` frames.
6. Send exactly one `end` frame.
7. Read response frames until `complete`, `error`, or disconnect.

Backend obligations:

1. Reject streams that do not begin with `start`.
2. Validate `model_id`.
3. Process zero or more `lines` frames.
4. Apply authoritative corpus filtering.
5. Return explicit accepted and rejected outcomes.
6. Emit `complete` or `error` exactly once.

## Shared Specification Strategy

V1 should be schema-first.

Recommended approach:

- Define frame schemas in JSON Schema.
- Generate Rust request/response types from the schema where practical.
- Use Pydantic models in Python that mirror the same schema.
- Add contract tests with recorded WebSocket message sequences in CI.

Important nuance:

- Rust can get close to compile-time conformance at the protocol boundary.
- Python will still validate at runtime.
- True cross-language compile-time proof is not realistic, so CI contract tests remain necessary.

## Forward Compatibility

The protocol should follow these rules from day one:

- All endpoints are namespaced under `/v1`.
- Every frame includes a `type` discriminator.
- Unknown fields must be ignored by clients.
- Enums are string-valued.
- Score payloads are self-describing and keyed by identity.

Likely V2 additions that should not require redesign:

- explanations per line
- calibrated score metadata
- model warmup and preload controls
- richer filter profile metadata
- async-only models that buffer more before first output

## Explicitly Deferred From V1

- Background jobs that survive disconnects
- Resume tokens
- Authentication
- File-path based ingestion where Python reads the file directly
- Backend-owned persistent sessions
- Model-specific endpoints such as `/logbert-score`

## Suggested Rust Integration Shape

The Rust client should treat the sidecar as a scoring transport, not a second source of truth.

Suggested internal flow:

1. Load logs into `LogStore`.
2. Discover models.
3. Open one scoring stream for the selected model.
4. Stream loaded lines in stable order.
5. Apply returned scores or rejections by `LineId`.
6. Keep UI filters independent from scoring status.

This fits the multi-source architecture direction where stable IDs are used as the durable join key.

## Data Preprocessing and Training Pipeline

### The Train/Inference Consistency Problem

The model's vocab maps `template_key → integer ID`. If the string passed to
`vocab.template_to_id` at inference time differs from the string that was used
when the vocab was built, every affected token lookup produces `[UNK]` and
scoring silently degrades. There are three independent sources of divergence
that must all be eliminated:

1. **Parsing** — who extracts `message` from the raw bytes.
2. **Normalization** — what regex patterns produce `template_key` from `message`.
3. **Timestamp** — whether calibration offsets are applied before the value is recorded.

### Resolution: `logcrab export`

A `logcrab export` subcommand (a second binary entry point on top of the
existing filetype infrastructure) is the canonical bridge between raw log
files and the training/inference pipeline.

```
logcrab export bugreport.log > data/raw/bugreport.ndjson
```

The output is NDJSON, one JSON object per line:

```json
{"line_number": 1, "timestamp_unix_ms": 1713093201000, "message": "ActivityManager: Start proc com.example.app", "source_file": "bugreport.log", "filetype": "logcat"}
```

This is the **only** parser of raw log files. The Python `parser.py` in the
training repo is dead code and will be removed once all training scripts
consume NDJSON.

### `message` Field Definition

`message` is defined per format:

- **logcat:** `"TAG: text"` — tag and payload only, no PID, TID, or level character.
  This matches what the Python `LogcatParser` used during training and
  is the most information-dense signal for the model.
- **DLT:** `format_body()` — `"{ecu} {session} {app_id} {ctx_id} {type} {payload}"`.
  No config or calibration dependency.
- **generic/dmesg/syslog:** post-timestamp, post-level text — the existing `message()` return value.

The `LineType::message()` contract in the Rust codebase is annotated with the
stability invariant: the returned string must be identical regardless of UI
settings, time offsets, or calibration state.

### `timestamp_unix_ms` Field Definition

`timestamp_unix_ms` is always the **raw, uncalibrated** source-file timestamp.
Calibration and time-offset corrections are UI/display concerns and must not
enter the training or scoring data.

In the Rust export tool and in the Rust code that assembles `lines` frames,
this is produced by calling:

```rust
line.timestamp(&Default::default(), &Default::default())
```

The `LineType::timestamp()` trait contract is annotated with the matching
stability invariant: calling with default config and default file state must
return the raw source-file timestamp. For DLT this means `storage_time`
(no boot-time correction, no `storage_offset_ms`).

### Python Template Normalization

The Python sidecar owns template normalization (`TemplateExtractor.extract_template`).
It applies this function to the `message` field received in every `lines` frame,
producing the `template_key` used for vocab lookup — the same function that was
used when the vocab was built from exported NDJSON.

The `template_key` field on `InputLine` is optional and treated as a debug hint
only. The sidecar does not use it for vocab lookup.

### Updated Training Pipeline

```bash
# 1. Export (run once per file; output is stable and can be committed)
logcrab export bugreport.log > data/raw/bugreport.ndjson

# 2. Build vocabulary
python src/build_vocab.py --logs data/raw/bugreport.ndjson --output data/processed/sequences

# 3. Train
python src/pretrain_sequence.py --logs data/raw/bugreport.ndjson --vocab data/processed/sequences/vocab.pkl
```

The `.ndjson` files are reproducible — the same log file always yields the
same output as long as the Rust parser is unchanged. If a parser bug is fixed,
re-export and retrain.

The export format is also the integration test fixture: a recorded `.ndjson`
file can be replayed against the sidecar to verify scoring without a live
log file.

## Open Questions

These do not block V1, but should be decided during implementation:

- Should rejected lines be stored as `None`, a richer enum, or a side map in Rust?
- How much filter profile detail should be exposed to the user versus kept descriptive only?

---

## Endpoint: `POST /v1/samples`

### Purpose

Allow the user to manually label a log source file as a training sample.  When the user
right-clicks a log line and chooses **Mark as Benign** or **Mark as Anomalous**, the
frontend submits the full source file (as an array of `InputLine` objects) together with
the 1-based display line number of the selected line.  The sidecar stores the data on
disk for later use in model training.

### Prerequisite

The server must be started with `--uploads-dir <dir>`.  If this argument is omitted the
endpoint returns `503 Service Unavailable`.

### Request

`POST /v1/samples`

Content-Type: `application/json`

```json
{
  "api_version": "1",
  "model_id": "logbert-android-errors",
  "label": "benign",
  "classified_line_number": 42,
  "lines": [
    {
      "line_id": { "source_id": 0, "line_number": 0, "timestamp_unix_ms": 1713093200000 },
      "message": "D/SomeTag: normal startup",
      "template_key": "D/SomeTag: normal startup",
      "filetype": "logcat"
    }
  ]
}
```

| Field | Type | Description |
|---|---|---|
| `api_version` | `"1"` | Protocol version |
| `model_id` | string | The model the user was viewing when classifying |
| `label` | `"benign"` \| `"anomalous"` | User-assigned class |
| `classified_line_number` | integer | 1-based display line number of the clicked line |
| `lines` | array of `InputLine` | All lines from the source file, in order |

The `lines` array uses the same `InputLine` format as `WS /v1/score-stream`.

### Response

```json
{
  "api_version": "1",
  "stored": true,
  "sample_id": "20260424T142033Z_a1b2c3d4"
}
```

### Storage Layout

```
{uploads_dir}/
  {model_id}/
    {label}/
      {timestamp}_{uuid8}.ndjson       — one InputLine JSON object per line
      {timestamp}_{uuid8}.meta.json    — metadata
```

The `.meta.json` file contains:

```json
{
  "sample_id": "20260424T142033Z_a1b2c3d4",
  "model_id": "logbert-android-errors",
  "label": "benign",
  "classified_line_number": 42,
  "line_count": 18500,
  "created_at": "2026-04-24T14:20:33.000000+00:00"
}
```

### Error Codes

| HTTP status | Body `error` | Meaning |
|---|---|---|
| `400` | `unsupported_version` | `api_version` is not `"1"` |
| `400` | `missing_model_id` | `model_id` is absent or empty |
| `400` | `invalid_label` | `label` is not `"benign"` or `"anomalous"` |
| `503` | `uploads_not_configured` | Server started without `--uploads-dir` |

### Notes

- The server does **not** validate that `model_id` corresponds to a registered model.
  Samples can be stored for models that are not currently loaded.
- The `classified_line_number` is stored for human reference only; training pipelines
  read it from `.meta.json` to understand which line triggered the label.
- Each upload produces a unique file; concurrent uploads are safe.
- The endpoint is intentionally simple: it stores raw data without any ML inference.
  Training pipelines operate on the stored files offline.
- Should the Rust client use `tungstenite` (sync, matches existing `std::thread` worker pattern) or `tokio-tungstenite` (async)?