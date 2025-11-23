use egui_dock::TabViewer;

use crate::ui::LogView;

/// Type of tab content in the dock system
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabType {
    Filter(usize),
    Bookmarks,
}

/// Tab content for dock system
#[derive(PartialEq)]
pub struct TabContent {
    pub tab_type: TabType,
    pub title: String,
}

/// TabViewer implementation for dock system
pub struct LogCrabTabViewer<'a> {
    pub log_view: &'a mut LogView,
    pub add_tab_after: &'a mut Option<egui_dock::NodeIndex>,
    pub focus_search_next_frame: &'a mut Option<usize>,
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
    }
}
