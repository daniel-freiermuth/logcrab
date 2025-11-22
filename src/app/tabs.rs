use egui_dock::TabViewer;

use crate::ui::LogView;

/// Type of tab content in the dock system
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabType {
    Filter(usize),
    Bookmarks,
}

/// Tab content for dock system
pub struct TabContent {
    pub tab_type: TabType,
    pub title: String,
}

/// TabViewer implementation for dock system
pub struct LogCrabTabViewer<'a> {
    pub log_view: &'a mut LogView,
    pub add_tab_after: &'a mut Option<egui_dock::NodeIndex>,
    pub active_tab: &'a mut Option<TabType>,
    pub focus_search_next_frame: &'a mut Option<usize>,
    pub close_active_tab: &'a mut bool,
}

impl TabViewer for LogCrabTabViewer<'_> {
    type Tab = TabContent;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        // For filter tabs, check if there's a custom name
        if let TabType::Filter(index) = &tab.tab_type {
            if let Some(custom_name) = self.log_view.get_filter_name(*index) {
                return custom_name.into();
            }
        }
        (&tab.title).into()
    }

    fn context_menu(
        &mut self,
        ui: &mut egui::Ui,
        tab: &mut Self::Tab,
        _surface: egui_dock::SurfaceIndex,
        _node: egui_dock::NodeIndex,
    ) {
        // Only allow renaming filter tabs
        if let TabType::Filter(index) = &tab.tab_type {
            ui.label("Filter Tab");
            ui.separator();

            if ui.button("✏ Rename").clicked() {
                // Will be handled in the main UI
                ui.close_menu();
            }

            if ui.button("🗑 Clear Name").clicked() {
                self.log_view.set_filter_name(*index, None);
                ui.close_menu();
            }
        }
    }

    fn add_popup(
        &mut self,
        ui: &mut egui::Ui,
        _surface: egui_dock::SurfaceIndex,
        node: egui_dock::NodeIndex,
    ) {
        ui.set_min_width(120.0);
        if ui.button("➕ Filter Tab").clicked() {
            *self.add_tab_after = Some(node);
            ui.close_menu();
        }
    }

    fn force_close(&mut self, tab: &mut Self::Tab) -> bool {
        // Close the tab if it's the active tab and close_active_tab flag is set
        if *self.close_active_tab {
            if let Some(ref active) = self.active_tab {
                if &tab.tab_type == active {
                    // Clear both the active tab and the close flag
                    *self.active_tab = None;
                    *self.close_active_tab = false;
                    return true;
                }
            }
        }
        false
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        // Render content
        match &tab.tab_type {
            TabType::Filter(index) => {
                // If Ctrl+L was pressed for this filter, set flag before rendering
                if *self.focus_search_next_frame == Some(*index) {
                    self.log_view.focus_search_input(*index);
                    *self.focus_search_next_frame = None;
                }

                self.log_view.render_filter(ui, *index);
            }
            TabType::Bookmarks => {
                self.log_view.render_bookmarks(ui);
            }
        }

        // CRITICAL: Only update active_tab if the pointer is CURRENTLY in this UI's bounds
        // AND a click/press just happened in this frame
        // This prevents the last-rendered-tab from always winning
        if ui.ui_contains_pointer() && ui.input(|i| i.pointer.any_pressed()) {
            *self.active_tab = Some(tab.tab_type.clone());
        }
    }
}
