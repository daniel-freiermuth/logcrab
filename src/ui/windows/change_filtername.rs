pub enum RenameFilterResult {
    Saved(String),
    Pending,
    Cancelled,
}

pub struct ChangeFilternameWindow {
    new_name: String,
    focus_requested: bool,
}

impl ChangeFilternameWindow {
    #[must_use]
    pub const fn new(initial_name: String) -> Self {
        Self {
            new_name: initial_name,
            focus_requested: false,
        }
    }

    /// Render the change filter name window.
    #[must_use]
    pub fn render(&mut self, ui: &egui::Ui) -> RenameFilterResult {
        let mut result = RenameFilterResult::Pending;
        egui::Window::new("Rename Filter")
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label("Enter filter name:");
                let response = ui.text_edit_singleline(&mut self.new_name);

                // Request focus on first frame only
                if !self.focus_requested {
                    response.request_focus();
                    self.focus_requested = true;
                }

                // Check if Enter was pressed (even if field still has focus)
                let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
                // Or if focus was lost by pressing Enter
                let enter_submitted =
                    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let escape_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));

                ui.horizontal(|ui| {
                    let should_save =
                        ui.button("Save").clicked() || enter_pressed || enter_submitted;
                    let should_cancel = ui.button("Cancel").clicked() || escape_pressed;

                    if should_save {
                        result = RenameFilterResult::Saved(self.new_name.clone());
                    }
                    if should_cancel {
                        result = RenameFilterResult::Cancelled;
                    }
                });
            });
        result
    }
}
