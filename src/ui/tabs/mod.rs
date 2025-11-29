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

use std::sync::Arc;

pub use bookmarks_tab::BookmarksView;
pub use filter_tab::FilterView;

use egui_dock::TabViewer;

use crate::config::GlobalConfig;
use crate::input::ShortcutAction;
use crate::parser::line::LogLine;
use crate::ui::log_view::{LogViewState, SavedFilter};

pub trait LogCrabTab {
    fn title(&mut self) -> egui::WidgetText;
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        data_state: &mut LogViewState,
        global_config: &mut GlobalConfig,
    ) -> bool;
    fn process_events(&mut self, actions: &[ShortcutAction], data_state: &mut LogViewState)
        -> bool;
    fn request_filter_update(&mut self, lines: Arc<Vec<LogLine>>);
    fn try_into_stored_filter(&self) -> Option<SavedFilter>;
    fn is_filtering(&mut self) -> bool;
}

/// Pending tab addition request from the add button
#[derive(Debug, Clone)]
pub enum PendingTabAdd {
    Filter,
    Bookmarks,
}

/// TabViewer implementation for dock system
pub struct LogCrabTabViewer<'a> {
    pub log_view: &'a mut LogViewState,
    pub global_config: &'a mut GlobalConfig,
    pub pending_tab_add: &'a mut Option<PendingTabAdd>,
    pub should_save: &'a mut bool,
}

impl TabViewer for LogCrabTabViewer<'_> {
    type Tab = Box<dyn LogCrabTab>;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        if tab.render(ui, self.log_view, self.global_config) {
            *self.should_save = true;
        }
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
