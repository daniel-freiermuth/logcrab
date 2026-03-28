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
        Self {
            temp_host: config.sidecar_host.clone(),
            temp_port: config.sidecar_port.to_string(),
            connection_status: ConnectionStatus::Unknown,
            available_models: Vec::new(),
            models_loading: false,
            models_error: None,
        }
    }

    /// Render the sidecar settings window.
    ///
    /// Returns `Ok(true)` if settings were saved, `Err(())` if the window should close.
    pub fn render(&mut self, ui: &mut Ui, config: &mut GlobalConfig) -> Result<bool, ()> {
        let mut should_close = false;
        let mut should_save = false;

        ui.heading("Sidecar Settings");
        ui.separator();

        ui.label("Configure the LogBERT sidecar server for ML-based anomaly detection");
        ui.add_space(10.0);

        // Server configuration
        ui.group(|ui| {
            ui.label(RichText::new("Server Configuration").strong());
            ui.add_space(5.0);

            ui.horizontal(|ui| {
                ui.label("Host:");
                ui.text_edit_singleline(&mut self.temp_host);
            });

            ui.horizontal(|ui| {
                ui.label("Port:");
                ui.text_edit_singleline(&mut self.temp_port);
            });

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
                    ui.label("No models found in checkpoints directory");
                } else {
                    ui.label("Select model:");

                    let current_selected = config.selected_model.as_deref().unwrap_or("");

                    egui::ComboBox::from_label("")
                        .selected_text(if current_selected.is_empty() {
                            "Select a model...".to_string()
                        } else {
                            self.available_models
                                .iter()
                                .find(|m| m.name == current_selected)
                                .map_or_else(
                                    || current_selected.to_string(),
                                    |m| m.display_name.clone(),
                                )
                        })
                        .show_ui(ui, |ui| {
                            for model in &self.available_models {
                                if ui
                                    .selectable_label(
                                        config.selected_model.as_deref() == Some(&model.name),
                                        &model.display_name,
                                    )
                                    .clicked()
                                {
                                    config.selected_model = Some(model.name.clone());
                                    config.selected_model_path = Some(model.model_path.clone());
                                    config.selected_vocab_path = Some(model.vocab_path.clone());
                                }
                            }
                        });

                    let selected_model_info = config.selected_model.as_ref().and_then(|selected| {
                        self.available_models
                            .iter()
                            .find(|m| &m.name == selected)
                            .cloned()
                    });

                    if let Some(model) = selected_model_info {
                        ui.add_space(5.0);
                        ui.label(RichText::new("Model Details:").weak());
                        ui.label(format!("Path: {}", model.model_path));
                        ui.label(format!("Vocab: {}", model.vocab_path));
                    }
                }
            });

            ui.add_space(10.0);
        }

        // Enable/Disable scoring options
        ui.checkbox(
            &mut config.use_sidecar_scoring,
            "Enable sidecar anomaly scoring",
        );
        ui.checkbox(
            &mut config.color_by_ml_score,
            "Color logs by ML anomaly score",
        );

        ui.add_space(10.0);

        // Buttons
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                if self.apply_settings(config) {
                    should_save = true;
                    should_close = true;
                }
            }

            if ui.button("Cancel").clicked() {
                should_close = true;
            }
        });

        if should_close {
            Err(())
        } else {
            Ok(should_save)
        }
    }

    fn test_connection(&mut self) {
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
