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
pub mod highlights_tab;
pub mod navigation;

pub use bookmarks_tab::BookmarksView;
pub use filter_tab::FilterView;
pub use highlights_tab::HighlightsView;

use egui_dock::TabViewer;

use crate::config::GlobalConfig;
use crate::input::ShortcutAction;
use crate::ui::log_view::{FilterHighlight, LogViewState, SavedFilter};
use crate::ui::tabs::filter_tab::HistogramMarker;

pub trait LogCrabTab {
    fn title(&mut self) -> egui::WidgetText;
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        data_state: &mut LogViewState,
        global_config: &mut GlobalConfig,
        all_filter_highlights: &[FilterHighlight],
        histogram_markers: &[HistogramMarker],
    );
    fn process_events(&mut self, actions: &[ShortcutAction], data_state: &mut LogViewState)
        -> bool;
    fn try_into_stored_filter(&self) -> Option<SavedFilter>;
    fn get_filter_highlight(&self) -> Option<FilterHighlight>;
    fn get_histogram_marker(&self) -> Option<HistogramMarker>;
    fn context_menu(&mut self, _ui: &mut egui::Ui) {
        // Default implementation does nothing
    }
    /// Get the unique identifier for this tab (for filter tabs)
    fn get_uuid(&self) -> Option<usize> {
        None
    }
}

/// Pending tab addition request from the add button
#[derive(Debug, Clone)]
pub enum PendingTabAdd {
    Filter,
    Bookmarks,
    Highlights,
}

/// `TabViewer` implementation for dock system
pub struct LogCrabTabViewer<'a> {
    pub log_view: &'a mut LogViewState,
    pub global_config: &'a mut GlobalConfig,
    pub pending_tab_add: &'a mut Option<PendingTabAdd>,
    pub all_filter_highlights: &'a [FilterHighlight],
    pub histogram_markers: &'a [HistogramMarker],
}

impl TabViewer for LogCrabTabViewer<'_> {
    type Tab = Box<dyn LogCrabTab>;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        tab.render(
            ui,
            self.log_view,
            self.global_config,
            self.all_filter_highlights,
            self.histogram_markers,
        );
    }

    fn context_menu(
        &mut self,
        ui: &mut egui::Ui,
        tab: &mut Self::Tab,
        _surface: egui_dock::SurfaceIndex,
        _node: egui_dock::NodeIndex,
    ) {
        tab.context_menu(ui);
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

        if ui.button("🎨 Highlights Tab").clicked() {
            *self.pending_tab_add = Some(PendingTabAdd::Highlights);
            ui.close();
        }

        if ui.button("⭐ Bookmarks Tab").clicked() {
            *self.pending_tab_add = Some(PendingTabAdd::Bookmarks);
            ui.close();
        }
    }
}
