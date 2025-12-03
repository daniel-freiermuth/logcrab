pub struct ChangeFilternameWindow {
    new_name: String,
}

impl ChangeFilternameWindow {
    pub fn new(initial_name: String) -> Self {
        Self {
            new_name: initial_name,
        }
    }

    /// Render the change filter name window
    ///
    /// Returns `Ok(Some(new_name))` if the name was changed,
    /// Ok(None) if the window is still open,
    /// Err(()) if the operation was cancelled.
    pub fn render(&mut self, ui: &mut egui::Ui) -> Result<Option<String>, ()> {
        let mut result = Ok(None);
        egui::Window::new("Rename Filter")
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label("Enter filter name:");
                let response = ui.text_edit_singleline(&mut self.new_name);

                // Request focus on first frame
                if !response.has_focus() {
                    response.request_focus();
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
                        result = Ok(Some(self.new_name.clone()));
                    }
                    if should_cancel {
                        result = Err(());
                    }
                });
            });
        result
    }
}
