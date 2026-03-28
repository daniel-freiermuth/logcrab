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

//! Sidecar client for LogBERT anomaly detection.
//!
//! Manages communication with the Python sidecar service.
//! Handles health checks and scoring requests.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const DEFAULT_PORT: u16 = 8765;
const DEFAULT_HOST: &str = "127.0.0.1";

#[derive(Debug, Serialize)]
struct BatchLogRequest {
    logs: Vec<String>,
    score_type: String,
    model_path: String,
    vocab_path: String,
}

#[derive(Debug, Deserialize)]
struct ScoreResponse {
    scores: Vec<f32>,
    #[allow(dead_code)]
    device_used: String,
}

#[derive(Debug, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub model_loaded: bool,
    pub device: String,
    pub context_size: i32,
    pub current_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub display_name: String,
    pub model_path: String,
    pub vocab_path: String,
    pub directory: String,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    models: Vec<ModelInfo>,
}

/// Client for LogBERT sidecar service
pub struct SidecarClient {
    base_url: String,
    client: reqwest::blocking::Client,
}

impl SidecarClient {
    /// Default host for the sidecar server
    pub const fn default_host() -> &'static str {
        DEFAULT_HOST
    }

    /// Default port for the sidecar server
    pub const fn default_port() -> u16 {
        DEFAULT_PORT
    }

    /// Connect to an already-running sidecar server.
    pub fn connect(host: &str, port: u16) -> Result<Self> {
        let base_url = format!("http://{host}:{port}");

        // Long timeout for scoring large files (up to 10 minutes)
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()?;

        Ok(Self { base_url, client })
    }

    /// Check sidecar health.
    pub fn health_check(&self) -> Result<HealthResponse> {
        let url = format!("{}/health", self.base_url);
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            anyhow::bail!("Health check failed: {}", response.status());
        }

        let health: HealthResponse = response.json()?;
        Ok(health)
    }

    /// List available models from the sidecar.
    pub fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let url = format!("{}/models", self.base_url);
        let response = self.client.get(&url).send()?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to list models: {}", response.status());
        }

        let models_response: ModelsResponse = response.json()?;
        Ok(models_response.models)
    }

    /// Score a batch of logs.
    ///
    /// Returns a vector of anomaly scores (one per log line).
    pub fn score_batch(
        &self,
        logs: &[String],
        model_path: &str,
        vocab_path: &str,
    ) -> Result<Vec<f32>> {
        let url = format!("{}/score_batch", self.base_url);

        let request = BatchLogRequest {
            logs: logs.to_vec(),
            score_type: "entropy_weighted".to_string(),
            model_path: model_path.to_string(),
            vocab_path: vocab_path.to_string(),
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .context("Failed to send batch score request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().unwrap_or_default();
            anyhow::bail!("Batch score request failed: {status} - {error_text}");
        }

        let score_response: ScoreResponse = response.json()?;

        Ok(score_response.scores)
    }
}
