use crate::ui::log_view::CrabSession;

/// Render the time offset configuration window
pub fn render_time_offset_window(ctx: &egui::Context, open: &mut bool, session: &CrabSession) {
    egui::Window::new("Configure Source Time Offsets")
        .collapsible(false)
        .resizable(true)
        .default_width(500.0)
        .open(open)
        .show(ctx, |ui| {
            ui.label("Adjust time offsets for each log source to align timestamps from different time zones.");
            ui.label("Positive values shift timestamps forward, negative values shift them backward.");
            ui.add_space(10.0);

            let sources = session.state.store.get_source_info();

            if sources.is_empty() {
                ui.label("No sources loaded.");
                return;
            }

            egui::Grid::new("time_offset_grid")
                .num_columns(3)
                .striped(true)
                .spacing([20.0, 8.0])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Source").strong());
                    ui.label(egui::RichText::new("Offset (seconds)").strong());
                    ui.label(egui::RichText::new("Offset (hours)").strong());
                    ui.end_row();

                    for source in &sources {
                        ui.label(&source.name);

                        // Get current offset
                        let mut offset = source.time_offset_secs;

                        // Create a DragValue for seconds
                        let response = ui.add(
                            egui::DragValue::new(&mut offset)
                                .speed(1.0)
                                .range(i64::MIN..=i64::MAX)
                                .suffix(" s"),
                        );

                        if response.changed() {
                            session.state.store.set_source_time_offset(source.index, offset);
                        }

                        // Show hours equivalent
                        let hours = offset as f64 / 3600.0;
                        ui.label(format!("{hours:+.2} h"));

                        ui.end_row();
                    }
                });

            ui.add_space(15.0);

            // Quick offset buttons
            ui.horizontal(|ui| {
                ui.label("Quick adjustments:");
                if ui.button("+1 hour to all").clicked() {
                    for source in &sources {
                        let current = source.time_offset_secs;
                        session.state.store.set_source_time_offset(source.index, current + 3600);
                    }
                }
                if ui.button("-1 hour to all").clicked() {
                    for source in &sources {
                        let current = source.time_offset_secs;
                        session.state.store.set_source_time_offset(source.index, current - 3600);
                    }
                }
                if ui.button("Reset all to 0").clicked() {
                    for source in &sources {
                        session.state.store.set_source_time_offset(source.index, 0);
                    }
                }
            });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(5.0);

            ui.label(egui::RichText::new("Note:").strong());
            ui.label("Changes are applied immediately and affect the merged view order.");
            ui.label("Time offsets are saved with your session (.crab file).");
        });
}
