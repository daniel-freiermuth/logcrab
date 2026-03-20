// LogCrab - GPL-3.0-or-later
// Copyright (C) 2026 Daniel Freiermuth

use chrono::{DateTime, Local, TimeZone};

/// Per-source calibration window state.
///
/// Stored as `#[serde(skip)]` inside each typed `FileState`.  Created directly by
/// `egui_render_context_menu` when the user clicks "Calibrate Time Here"; driven
/// each frame by `LineType::egui_render_file_state`.
#[derive(Debug, Clone)]
pub struct CalibrationWindow {
    target_time_str: String,
    focus_requested: bool,
    is_dlt: bool,
    calculated_time: Option<DateTime<Local>>,
    original_time: DateTime<Local>,
    apply_to_all_apps: bool,
}

impl CalibrationWindow {
    pub fn new(
        current_time: DateTime<Local>,
        is_dlt: bool,
        calculated_time: Option<DateTime<Local>>,
        original_time: DateTime<Local>,
    ) -> Self {
        Self {
            target_time_str: current_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            focus_requested: false,
            is_dlt,
            calculated_time,
            original_time,
            apply_to_all_apps: false,
        }
    }

    /// Render the calibration window.
    ///
    /// Returns:
    /// - `Ok(Some((target_time, apply_to_all_apps)))` — user confirmed
    /// - `Ok(None)` — window still open
    /// - `Err(())` — user cancelled
    pub fn render(&mut self, ui: &egui::Ui) -> Result<Option<(DateTime<Local>, bool)>, ()> {
        let mut result = Ok(None);

        let title = if self.is_dlt {
            "\u{23F1} Calibrate DLT Time"
        } else {
            "\u{23F1} Calibrate Time"
        };

        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                let (current_label, original_label) = if self.is_dlt {
                    ("Derived from monotonic:", "Storage timestamp:")
                } else {
                    ("Current time:", "Original time:")
                };

                if let Some(calc_time) = self.calculated_time {
                    ui.horizontal(|ui| {
                        ui.label(current_label);
                        ui.label(calc_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string());
                        if ui.button("Use for calibration").clicked() {
                            self.target_time_str =
                                calc_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
                        }
                    });
                }

                ui.horizontal(|ui| {
                    ui.label(original_label);
                    ui.label(self.original_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string());
                    if ui.button("Use for calibration").clicked() {
                        self.target_time_str =
                            self.original_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
                    }
                });

                ui.add_space(10.0);

                ui.label("Set the target timestamp for this log entry:");
                ui.label("Format: YYYY-MM-DD HH:MM:SS.mmm");
                ui.add_space(5.0);

                let response = ui.text_edit_singleline(&mut self.target_time_str);

                if !self.focus_requested {
                    response.request_focus();
                    self.focus_requested = true;
                }

                let parsed_time = self.parse_time();
                match &parsed_time {
                    Ok(dt) => {
                        ui.label(format!(
                            "\u{2713} Valid: {}",
                            dt.format("%Y-%m-%d %H:%M:%S%.3f %z")
                        ));
                    }
                    Err(e) => {
                        ui.colored_label(egui::Color32::RED, format!("\u{2717} {e}"));
                    }
                }

                ui.add_space(10.0);

                if self.is_dlt {
                    ui.checkbox(&mut self.apply_to_all_apps, "Apply to all applications");
                    ui.add_space(5.0);
                }

                let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
                let enter_submitted =
                    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let escape_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));

                ui.horizontal(|ui| {
                    let sync_enabled = parsed_time.is_ok();
                    let should_sync = ui
                        .add_enabled(sync_enabled, egui::Button::new("Sync"))
                        .clicked()
                        || (sync_enabled && (enter_pressed || enter_submitted));
                    let should_cancel = ui.button("Cancel").clicked() || escape_pressed;

                    if should_sync {
                        if let Ok(target_time) = parsed_time {
                            result = Ok(Some((target_time, self.apply_to_all_apps)));
                        }
                    }
                    if should_cancel {
                        result = Err(());
                    }
                });
            });
        result
    }

    fn parse_time(&self) -> Result<DateTime<Local>, String> {
        use chrono::NaiveDateTime;

        let s = self.target_time_str.trim();

        if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.3f") {
            return Local
                .from_local_datetime(&naive)
                .single()
                .ok_or_else(|| "Ambiguous or invalid local time".to_string());
        }

        if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return Local
                .from_local_datetime(&naive)
                .single()
                .ok_or_else(|| "Ambiguous or invalid local time".to_string());
        }

        if let Ok(naive) =
            NaiveDateTime::parse_from_str(&format!("{s} 00:00:00"), "%Y-%m-%d %H:%M:%S")
        {
            return Local
                .from_local_datetime(&naive)
                .single()
                .ok_or_else(|| "Ambiguous or invalid local time".to_string());
        }

        Err("Invalid format. Use: YYYY-MM-DD HH:MM:SS.mmm".to_string())
    }
}
