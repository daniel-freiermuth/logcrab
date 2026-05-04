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

use crate::anomaly::sidecar_client::{ModelInfo, SidecarClient};
use crate::config::GlobalConfig;
use egui::{Color32, RichText, Ui};

pub struct SidecarSettingsWindow {
    temp_host: String,
    temp_port: String,
    connection_status: ConnectionStatus,
    available_models: Vec<ModelInfo>,
    models_loading: bool,
    models_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum ConnectionStatus {
    Unknown,
    Connected,
    Failed(String),
}

impl SidecarSettingsWindow {
    pub fn open_with_config(config: &GlobalConfig) -> Self {
        let mut window = Self {
            temp_host: config.sidecar_host.clone(),
            temp_port: config.sidecar_port.to_string(),
            connection_status: ConnectionStatus::Unknown,
            available_models: Vec::new(),
            models_loading: false,
            models_error: None,
        };
        // Probe the connection immediately so the UI is pre-populated on open.
        window.test_connection();
        if window.connection_status == ConnectionStatus::Connected {
            window.load_models();
        }
        window
    }

    /// Render the sidecar settings window.
    ///
    /// Returns `true` when config was changed and should be persisted.
    pub fn render(&mut self, ui: &mut Ui, config: &mut GlobalConfig) -> bool {
        let mut changed = false;

        ui.heading("Sidecar Settings");
        ui.separator();

        ui.label("Configure the LogBERT sidecar server for ML-based anomaly detection");
        ui.add_space(10.0);

        // Server configuration
        ui.group(|ui| {
            ui.label(RichText::new("Server Configuration").strong());
            ui.add_space(5.0);

            let host_changed = ui.horizontal(|ui| {
                ui.label("Host:");
                ui.text_edit_singleline(&mut self.temp_host)
            }).inner.changed();

            let port_changed = ui.horizontal(|ui| {
                ui.label("Port:");
                ui.text_edit_singleline(&mut self.temp_port)
            }).inner.changed();

            if host_changed || port_changed {
                if self.apply_settings(config) {
                    changed = true;
                }
            }

            ui.add_space(5.0);

            // Test connection button
            ui.horizontal(|ui| {
                if ui.button("Test Connection").clicked() {
                    self.test_connection();
                }

                match &self.connection_status {
                    ConnectionStatus::Unknown => {}
                    ConnectionStatus::Connected => {
                        ui.colored_label(Color32::GREEN, "✓ Connected");

                        if self.available_models.is_empty() && !self.models_loading {
                            self.load_models();
                        }
                    }
                    ConnectionStatus::Failed(error) => {
                        ui.colored_label(Color32::RED, format!("✗ {error}"));
                    }
                }
            });
        });

        ui.add_space(10.0);

        // Model selection (only when connected)
        if self.connection_status == ConnectionStatus::Connected {
            let prev_model = config.selected_model.clone();

            ui.group(|ui| {
                ui.label(RichText::new("Model Selection").strong());
                ui.add_space(5.0);

                if self.models_loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Loading available models...");
                    });
                } else if let Some(error) = &self.models_error {
                    ui.colored_label(Color32::RED, format!("Error loading models: {error}"));
                    if ui.button("Retry").clicked() {
                        self.load_models();
                    }
                } else if self.available_models.is_empty() {
                    ui.label("No models available");
                } else {
                    ui.label("Select model:");

                    let current_id = config.selected_model.as_deref().unwrap_or("");

                    egui::ComboBox::from_label("")
                        .selected_text(if current_id.is_empty() {
                            "Select a model...".to_string()
                        } else {
                            self.available_models
                                .iter()
                                .find(|m| m.id == current_id)
                                .map_or_else(|| current_id.to_string(), |m| m.name.clone())
                        })
                        .show_ui(ui, |ui| {
                            for model in &self.available_models {
                                // Show compatibility indicator alongside the name.
                                let norm_versions =
                                    crate::core::log_store::all_normalization_versions();
                                let compatible =
                                    is_normalization_compatible(model, &norm_versions);
                                let label = if compatible {
                                    model.name.clone()
                                } else {
                                    format!("⚠ {}", model.name)
                                };
                                if ui
                                    .selectable_label(
                                        config.selected_model.as_deref() == Some(&model.id),
                                        label,
                                    )
                                    .clicked()
                                {
                                    config.selected_model = Some(model.id.clone());
                                }
                            }
                        });

                    // Details for the selected model
                    if let Some(model) = config.selected_model.as_ref().and_then(|id| {
                        self.available_models.iter().find(|m| &m.id == id)
                    }) {
                        ui.add_space(5.0);
                        ui.label(RichText::new("Model Details:").weak());
                        ui.label(format!("Architecture: {}", model.architecture));
                        ui.label(format!("Version: {}", model.version));
                        ui.label(format!("Status: {}", model.status));

                        let norm_versions = crate::core::log_store::all_normalization_versions();
                        let mismatches = normalization_mismatches(model, &norm_versions);
                        if mismatches.is_empty() {
                            ui.colored_label(Color32::GREEN, "✓ Normalisation versions match");
                        } else {
                            ui.add_space(3.0);
                            ui.colored_label(Color32::YELLOW, "⚠ Normalisation version mismatch:");
                            for (slug, trained_on, current) in &mismatches {
                                ui.label(format!(
                                    "  {slug}: trained on v{trained_on}, frontend is v{current}"
                                ));
                            }
                            ui.label(RichText::new(
                                "Scores may be less accurate for affected file types.",
                            ).weak());
                        }
                    }
                }
            });

            if config.selected_model != prev_model {
                changed = true;
            }

            ui.add_space(10.0);
        }

        changed
    }

    fn test_connection(&mut self) {
        self.available_models.clear();
        self.models_error = None;
        let Ok(port) = self.temp_port.parse::<u16>() else {
            self.connection_status =
                ConnectionStatus::Failed("Invalid port number".to_string());
            return;
        };

        match SidecarClient::connect(&self.temp_host, port) {
            Ok(client) => match client.health_check() {
                Ok(_) => {
                    self.connection_status = ConnectionStatus::Connected;
                }
                Err(e) => {
                    self.connection_status =
                        ConnectionStatus::Failed(format!("Health check failed: {e}"));
                }
            },
            Err(e) => {
                self.connection_status =
                    ConnectionStatus::Failed(format!("Connection failed: {e}"));
            }
        }
    }

    fn load_models(&mut self) {
        self.models_loading = true;
        self.models_error = None;

        let Ok(port) = self.temp_port.parse::<u16>() else {
            self.models_error = Some("Invalid port number".to_string());
            self.models_loading = false;
            return;
        };

        match SidecarClient::connect(&self.temp_host, port) {
            Ok(client) => match client.list_models() {
                Ok(models) => {
                    self.available_models = models;
                    self.models_loading = false;
                }
                Err(e) => {
                    self.models_error = Some(format!("Failed to fetch models: {e}"));
                    self.models_loading = false;
                }
            },
            Err(e) => {
                self.models_error = Some(format!("Connection failed: {e}"));
                self.models_loading = false;
            }
        }
    }

    fn apply_settings(&self, config: &mut GlobalConfig) -> bool {
        if let Ok(port) = self.temp_port.parse::<u16>() {
            config.sidecar_host.clone_from(&self.temp_host);
            config.sidecar_port = port;
            true
        } else {
            false
        }
    }
}

/// Returns `true` when every filetype in the model's `normalization_versions`
/// map matches the frontend's current version.
fn is_normalization_compatible(
    model: &ModelInfo,
    frontend_versions: &std::collections::HashMap<&str, u32>,
) -> bool {
    normalization_mismatches(model, frontend_versions).is_empty()
}

/// Returns `(slug, trained_on_version, frontend_version)` for every mismatch.
fn normalization_mismatches(
    model: &ModelInfo,
    frontend_versions: &std::collections::HashMap<&str, u32>,
) -> Vec<(String, u32, u32)> {
    model
        .training_corpus
        .normalization_versions
        .iter()
        .filter_map(|(slug, &trained_on)| {
            let current = *frontend_versions.get(slug.as_str()).unwrap_or(&1);
            if current != trained_on {
                Some((slug.clone(), trained_on, current))
            } else {
                None
            }
        })
        .collect()
}

