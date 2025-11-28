// LogCrab - GPL-3.0-or-later
// This file is part of LogCrab.
//
// Copyright (C) 2025 Daniel Freiermuth
//
// LogCrab is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// LogCrab is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with LogCrab.  If not, see <https://www.gnu.org/licenses/>.

pub mod bookmarks_tab;
pub mod filter_tab;
pub mod navigation;

pub use bookmarks_tab::BookmarksView;
pub use filter_tab::FilterView;

use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::TabViewer;

use crate::config::GlobalConfig;
use crate::input::ShortcutAction;
use crate::ui::LogView;

pub trait LogCrabTab {
    fn title(&mut self) -> egui::WidgetText;
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        data_state: &mut LogView,
        global_config: &mut GlobalConfig,
    );
    fn process_events(&mut self, actions: &[ShortcutAction], data_state: &mut LogView);
    fn on_close(&mut self, _filter_to_remove: &mut Option<usize>) -> OnCloseResponse {
        OnCloseResponse::Close
    }
    fn filter_got_removed(&mut self, _filter_index: usize) {
        // Default implementation does nothing
    }
}

/// Pending tab addition request from the add button
#[derive(Debug, Clone)]
pub enum PendingTabAdd {
    Filter,
    Bookmarks,
}

/// TabViewer implementation for dock system
pub struct LogCrabTabViewer<'a> {
    pub log_view: &'a mut LogView,
    pub global_config: &'a mut GlobalConfig,
    pub filter_to_remove: &'a mut Option<usize>,
    pub pending_tab_add: &'a mut Option<PendingTabAdd>,
}

impl TabViewer for LogCrabTabViewer<'_> {
    type Tab = Box<dyn LogCrabTab>;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        tab.render(ui, self.log_view, self.global_config);
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> OnCloseResponse {
        tab.on_close(self.filter_to_remove)
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        [false, false]
    }

    fn add_popup(
        &mut self,
        ui: &mut egui::Ui,
        _surface: egui_dock::SurfaceIndex,
        _node: egui_dock::NodeIndex,
    ) {
        ui.set_min_width(150.0);

        if ui.button("➕ Filter Tab").clicked() {
            *self.pending_tab_add = Some(PendingTabAdd::Filter);
            ui.close();
        }

        if ui.button("⭐ Bookmarks Tab").clicked() {
            *self.pending_tab_add = Some(PendingTabAdd::Bookmarks);
            ui.close();
        }
    }
}
