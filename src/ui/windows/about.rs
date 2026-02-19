// LogCrab - GPL-3.0-or-later

/// Version from Cargo.toml
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Git hash embedded at compile time
const GIT_HASH: &str = env!("GIT_HASH");

/// Render the About window
pub fn render_about_window(ctx: &egui::Context, open: &mut bool) {
    egui::Window::new("About LogCrab")
        .collapsible(false)
        .resizable(false)
        .default_width(350.0)
        .open(open)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);

                ui.heading("🦀 LogCrab");
                ui.add_space(5.0);

                ui.label("A polyscopic anomaly explorer");
                ui.add_space(15.0);

                egui::Grid::new("about_grid")
                    .num_columns(2)
                    .spacing([20.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Version:");
                        ui.label(egui::RichText::new(VERSION).strong());
                        ui.end_row();

                        ui.label("Git:");
                        ui.label(egui::RichText::new(GIT_HASH).code());
                        ui.end_row();

                        ui.label("License:");
                        ui.label("GPL-3.0-or-later");
                        ui.end_row();
                    });

                ui.add_space(15.0);
                ui.separator();
                ui.add_space(10.0);

                ui.label("© 2025-2026 Daniel Freiermuth");
                ui.add_space(5.0);

                ui.hyperlink_to(
                    "GitHub Repository",
                    "https://github.com/dan-freiermuth/logcrab",
                );

                ui.add_space(10.0);
            });
        });
}
