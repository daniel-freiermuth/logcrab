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
pub use filter_tab::{FilterView, FilterViewEvent};

use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::TabViewer;

use crate::config::GlobalConfig;
use crate::ui::LogView;

/// Type of tab content in the dock system
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabType {
    Filter(usize),
    Bookmarks(BookmarksView),
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
    pub focus_search_next_frame: &'a mut Option<usize>,
    pub global_config: &'a mut GlobalConfig,
    pub filter_to_remove: &'a mut Option<usize>,
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

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        // Render content
        match &mut tab.tab_type {
            TabType::Filter(index) => {
                // If Ctrl+L was pressed for this filter, set flag before rendering
                if *self.focus_search_next_frame == Some(*index) {
                    self.log_view.focus_search_input(*index);
                    *self.focus_search_next_frame = None;
                }

                self.log_view.render_filter(ui, *index, self.global_config);
            }
            TabType::Bookmarks(bookmarks_view) => {
                bookmarks_view.render_bookmarks(ui, self.log_view);
            }
        }
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> OnCloseResponse {
        // When closing a filter tab, mark it for removal
        // We can't remove it here because we need to update all other tabs' indices
        if let TabType::Filter(index) = &tab.tab_type {
            *self.filter_to_remove = Some(*index);
        }
        // Return Close to allow the tab to be closed
        OnCloseResponse::Close
    }
}
