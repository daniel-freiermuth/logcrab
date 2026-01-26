use chrono::{DateTime, Local, TimeZone};

/// Scope of DLT time synchronization
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DltSyncScope {
    /// Sync all DLT entries in the file
    EntireFile,
    /// Sync only entries matching the reference ECU
    PerEcu,
    /// Sync only entries matching both ECU and Context
    PerEcuAndContext,
}

pub struct SyncDltTimeWindow {
    target_time_str: String,
    sync_scope: DltSyncScope,
}

impl SyncDltTimeWindow {
    pub fn new(storage_time: DateTime<Local>) -> Self {
        Self {
            target_time_str: storage_time.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            sync_scope: DltSyncScope::EntireFile,
        }
    }

    /// Render the sync DLT time window
    ///
    /// Returns `Ok(Some((target_time, scope)))` if the user confirmed the sync,
    /// Ok(None) if the window is still open,
    /// Err(()) if the operation was cancelled.
    pub fn render(
        &mut self,
        ui: &egui::Ui,
    ) -> Result<Option<(DateTime<Local>, DltSyncScope)>, ()> {
        let mut result = Ok(None);

        egui::Window::new("⏱ Sync DLT Time")
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label("Set the target timestamp for this log entry:");
                ui.label("Format: YYYY-MM-DD HH:MM:SS.mmm");
                ui.add_space(5.0);

                let response = ui.text_edit_singleline(&mut self.target_time_str);

                // Request focus on first frame
                if !response.has_focus() {
                    response.request_focus();
                }

                // Parse the time string and show validation
                let parsed_time = self.parse_time();
                match &parsed_time {
                    Ok(dt) => {
                        ui.label(format!(
                            "✓ Valid: {}",
                            dt.format("%Y-%m-%d %H:%M:%S%.3f %z")
                        ));
                    }
                    Err(e) => {
                        ui.colored_label(egui::Color32::RED, format!("✗ {e}"));
                    }
                }

                ui.add_space(10.0);

                // Sync scope selection
                ui.label("Synchronization scope:");
                ui.radio_value(
                    &mut self.sync_scope,
                    DltSyncScope::EntireFile,
                    "Entire file (all ECUs and Contexts)",
                );
                ui.radio_value(
                    &mut self.sync_scope,
                    DltSyncScope::PerEcu,
                    "Only this ECU (all Contexts)",
                );
                ui.radio_value(
                    &mut self.sync_scope,
                    DltSyncScope::PerEcuAndContext,
                    "Only this ECU and Context",
                );

                ui.add_space(10.0);

                // Check if Enter was pressed
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
                            result = Ok(Some((target_time, self.sync_scope)));
                        }
                    }
                    if should_cancel {
                        result = Err(());
                    }
                });
            });
        result
    }

    /// Parse the time string with multiple format attempts
    fn parse_time(&self) -> Result<DateTime<Local>, String> {
        use chrono::NaiveDateTime;

        let s = self.target_time_str.trim();

        // Try parsing with milliseconds: "YYYY-MM-DD HH:MM:SS.mmm"
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.3f") {
            return Local
                .from_local_datetime(&naive)
                .single()
                .ok_or_else(|| "Ambiguous or invalid local time".to_string());
        }

        // Try parsing without milliseconds: "YYYY-MM-DD HH:MM:SS"
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return Local
                .from_local_datetime(&naive)
                .single()
                .ok_or_else(|| "Ambiguous or invalid local time".to_string());
        }

        // Try parsing with just date: "YYYY-MM-DD"
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
