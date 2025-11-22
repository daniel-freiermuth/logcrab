use crate::input::{KeyboardBindings, ShortcutAction};

/// Render the keyboard shortcuts configuration window
pub fn render_shortcuts_window(
    ctx: &egui::Context,
    open: &mut bool,
    shortcut_bindings: &mut KeyboardBindings,
    pending_rebind: &mut Option<ShortcutAction>,
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
                    }
                });
            });
            ui.add_space(6.0);

            // Iterate over all shortcut actions
            let actions = [
                ShortcutAction::MoveUp,
                ShortcutAction::MoveDown,
                ShortcutAction::ToggleBookmark,
                ShortcutAction::FocusSearch,
                ShortcutAction::NewFilterTab,
                ShortcutAction::NewBookmarksTab,
                ShortcutAction::CloseTab,
                ShortcutAction::JumpToTop,
                ShortcutAction::JumpToBottom,
                ShortcutAction::FocusPaneLeft,
                ShortcutAction::FocusPaneRight,
                ShortcutAction::FocusPaneUp,
                ShortcutAction::FocusPaneDown,
            ];

            for (i, action) in actions.iter().enumerate() {
                if i > 0 {
                    ui.add_space(8.0);
                }

                ui.horizontal(|ui| {
                    ui.add_space(10.0);

                    // Helper function to format keyboard shortcut
                    let format_shortcut = |shortcut: &egui::KeyboardShortcut| -> String {
                        let modifiers_text = if shortcut.modifiers.ctrl {
                            "Ctrl+"
                        } else if shortcut.modifiers.shift {
                            "Shift+"
                        } else if shortcut.modifiers.alt {
                            "Alt+"
                        } else if shortcut.modifiers.mac_cmd {
                            "Cmd+"
                        } else {
                            ""
                        };
                        format!("{}{:?}", modifiers_text, shortcut.logical_key)
                    };

                    // All bindings are now KeyboardShortcuts
                    let key_text = match action {
                        ShortcutAction::MoveUp => {
                            format_shortcut(&shortcut_bindings.get_shortcut(ShortcutAction::MoveUp))
                        }
                        ShortcutAction::MoveDown => format_shortcut(
                            &shortcut_bindings.get_shortcut(ShortcutAction::MoveDown),
                        ),
                        ShortcutAction::ToggleBookmark => format_shortcut(
                            &shortcut_bindings.get_shortcut(ShortcutAction::ToggleBookmark),
                        ),
                        ShortcutAction::FocusSearch => format_shortcut(
                            &shortcut_bindings.get_shortcut(ShortcutAction::FocusSearch),
                        ),
                        ShortcutAction::NewFilterTab => format_shortcut(
                            &shortcut_bindings.get_shortcut(ShortcutAction::NewFilterTab),
                        ),
                        ShortcutAction::NewBookmarksTab => format_shortcut(
                            &shortcut_bindings.get_shortcut(ShortcutAction::NewBookmarksTab),
                        ),
                        ShortcutAction::CloseTab => format_shortcut(
                            &shortcut_bindings.get_shortcut(ShortcutAction::CloseTab),
                        ),
                        ShortcutAction::JumpToTop => "gg".to_string(),
                        ShortcutAction::JumpToBottom => "G".to_string(),
                        ShortcutAction::FocusPaneLeft => format_shortcut(
                            &shortcut_bindings.get_shortcut(ShortcutAction::FocusPaneLeft),
                        ),
                        ShortcutAction::FocusPaneDown => format_shortcut(
                            &shortcut_bindings.get_shortcut(ShortcutAction::FocusPaneDown),
                        ),
                        ShortcutAction::FocusPaneUp => format_shortcut(
                            &shortcut_bindings.get_shortcut(ShortcutAction::FocusPaneUp),
                        ),
                        ShortcutAction::FocusPaneRight => format_shortcut(
                            &shortcut_bindings.get_shortcut(ShortcutAction::FocusPaneRight),
                        ),
                    };

                    let badge_color = if *pending_rebind == Some(*action) {
                        egui::Color32::from_rgb(255, 200, 100)
                    } else {
                        ui.visuals().code_bg_color
                    };

                    egui::Frame::none()
                        .fill(badge_color)
                        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                        .rounding(egui::Rounding::same(4.0))
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
