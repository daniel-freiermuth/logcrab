use crate::config::GlobalConfig;
use crate::input::{KeyboardBindings, ShortcutAction};

/// Render the keyboard shortcuts configuration window
pub fn render_shortcuts_window(
    ctx: &egui::Context,
    open: &mut bool,
    shortcut_bindings: &mut KeyboardBindings,
    pending_rebind: &mut Option<ShortcutAction>,
    global_config: &mut GlobalConfig,
) {
    egui::Window::new("⌨ Keyboard Shortcuts")
        .open(open)
        .default_width(480.0)
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.add_space(5.0);

            // Configurable keys section
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("⚙ Keyboard Bindings")
                        .strong()
                        .size(13.0)
                        .color(egui::Color32::from_rgb(100, 150, 255)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(egui::RichText::new("↺ Reset").size(10.0))
                        .clicked()
                    {
                        *shortcut_bindings = KeyboardBindings::default();
                        *pending_rebind = None;
                        // Save the reset bindings
                        shortcut_bindings.save_to_config(global_config);
                        let _ = global_config.save();
                    }
                });
            });
            ui.add_space(6.0);

            // Iterate over all shortcut actions
            for (i, action) in ShortcutAction::all().iter().enumerate() {
                if i > 0 {
                    ui.add_space(8.0);
                }

                ui.horizontal(|ui| {
                    ui.add_space(10.0);

                    // Get the key binding string directly
                    let key_text = shortcut_bindings.get_shortcut(*action).to_string();

                    let badge_color = if *pending_rebind == Some(*action) {
                        egui::Color32::from_rgb(255, 200, 100)
                    } else {
                        ui.visuals().code_bg_color
                    };

                    egui::Frame::new()
                        .fill(badge_color)
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .corner_radius(egui::CornerRadius::same(4))
                        .stroke(egui::Stroke::new(1.0, ui.visuals().window_stroke.color))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(&key_text).size(13.0).strong());
                        });

                    ui.add_space(8.0);

                    // Action info
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(action.name()).strong());
                        ui.label(
                            egui::RichText::new(action.description())
                                .size(10.0)
                                .color(ui.visuals().weak_text_color()),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // JumpToTop and JumpToBottom are hardcoded (gg/G) and cannot be rebound
                        let is_rebindable = !matches!(
                            action,
                            ShortcutAction::JumpToTop | ShortcutAction::JumpToBottom
                        );

                        if *pending_rebind == Some(*action) {
                            ui.colored_label(
                                egui::Color32::from_rgb(255, 200, 100),
                                egui::RichText::new("⌛ Press any key...").strong(),
                            );
                            if ui.button("✖ Cancel").clicked() {
                                *pending_rebind = None;
                            }
                        } else if is_rebindable {
                            if ui
                                .button(egui::RichText::new("🔧 Rebind").size(11.0))
                                .clicked()
                            {
                                *pending_rebind = Some(*action);
                            }
                        } else {
                            ui.label(
                                egui::RichText::new("(hardcoded)")
                                    .size(10.0)
                                    .color(ui.visuals().weak_text_color()),
                            );
                        }
                    });
                });
            }

            ui.add_space(4.0);
        });
}
