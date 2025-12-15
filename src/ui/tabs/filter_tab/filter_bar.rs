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

use egui::{Color32, Ui};

use crate::{
    config::GlobalConfig,
    ui::{log_view::LogViewState, tabs::filter_tab::filter_state::FilterState},
};

/// Events emitted by the filter bar
#[derive(Debug, Clone)]
pub enum FilterInternalEvent {
    SearchChanged,
    CaseInsensitiveToggled,
    FavoriteSelected {
        search_text: String,
        case_sensitive: bool,
    },
    FilterNameEditRequested,
    FavoriteToggled,
    /// Histogram or globally visible toggle changed
    DisplaySettingsChanged,
}

/// Reusable filter search bar component with internal state for inline editing
pub struct FilterBar {
    editing_favorite: bool,
    temp_favorite_name: String,
    /// Track if we've already requested focus for the current editing session
    favorite_focus_requested: bool,
    /// Current position in history (None = not browsing, Some(0) = most recent, etc.)
    history_index: Option<usize>,
    /// Temporary storage for the text being edited before entering history mode
    pre_history_text: String,
}

impl FilterBar {
    pub const fn new() -> Self {
        Self {
            editing_favorite: false,
            temp_favorite_name: String::new(),
            favorite_focus_requested: false,
            history_index: None,
            pre_history_text: String::new(),
        }
    }

    pub fn save_favorite_name(&mut self, filter: &FilterState, global_config: &mut GlobalConfig) {
        let new_name = self.temp_favorite_name.clone();
        if let Some(fav) = global_config
            .favorite_filters
            .iter_mut()
            .find(|f| f.matches(filter))
        {
            fav.name.clone_from(&new_name);
            log::info!("Updated favorite name to: '{new_name}'");
        }
        let _ = global_config.save();
    }

    /// Render the filter bar UI
    ///
    /// Returns events that occurred during rendering
    pub fn render(
        &mut self,
        ui: &mut Ui,
        filter: &mut FilterState,
        filter_uuid: usize,
        global_config: &mut GlobalConfig,
        should_focus_search: bool,
        log_view_state: &mut LogViewState,
    ) -> Vec<FilterInternalEvent> {
        profiling::scope!("FilterBar::render");

        let mut events = Vec::new();

        ui.horizontal(|ui| {
            Self::render_edit_button(ui, &mut events);
            self.render_globally_visible_toggle(ui, filter, &mut events);
            self.render_histogram_toggle(ui, filter, &mut events);
            Self::render_color_picker(ui, filter);
            Self::render_favorite_toggle(ui, filter, global_config, &mut events);
            self.render_favorites_dropdown(ui, filter, filter_uuid, global_config, &mut events);
            self.render_search_input(ui, filter, should_focus_search, log_view_state, &mut events);
            self.render_case_checkbox(ui, filter, &mut events);
            Self::render_validation_status(ui, filter);
        });

        events
    }

    fn render_edit_button(ui: &mut Ui, events: &mut Vec<FilterInternalEvent>) {
        if ui
            .small_button("✏")
            .on_hover_text("Edit filter name")
            .clicked()
        {
            events.push(FilterInternalEvent::FilterNameEditRequested);
        }
    }

    fn render_color_picker(ui: &mut Ui, filter: &mut FilterState) {
        ui.color_edit_button_srgba(&mut filter.color)
            .on_hover_text("Choose highlight color for this filter");
    }

    fn render_favorite_toggle(
        ui: &mut Ui,
        filter: &FilterState,
        global_config: &GlobalConfig,
        events: &mut Vec<FilterInternalEvent>,
    ) {
        let current_favorite = global_config
            .favorite_filters
            .iter()
            .find(|fav| fav.matches(filter));
        if ui
            .toggle_value(&mut current_favorite.is_some(), "⭐")
            .on_hover_text("Toggle favorite filter")
            .clicked()
        {
            events.push(FilterInternalEvent::FavoriteToggled);
        }
    }

    fn render_favorites_dropdown(
        &mut self,
        ui: &mut Ui,
        filter: &mut FilterState,
        filter_uuid: usize,
        global_config: &mut GlobalConfig,
        events: &mut Vec<FilterInternalEvent>,
    ) {
        if global_config.favorite_filters.is_empty() {
            return;
        }

        let current_favorite = global_config
            .favorite_filters
            .iter()
            .find(|fav| fav.matches(filter));

        if self.editing_favorite && current_favorite.is_some() {
            self.render_favorite_editor(ui, filter, global_config);
        } else {
            self.render_favorite_selector(ui, filter_uuid, global_config, current_favorite, events);
        }
    }

    fn render_favorite_editor(
        &mut self,
        ui: &mut Ui,
        filter: &FilterState,
        global_config: &mut GlobalConfig,
    ) {
        let text_edit_id = ui.id().with("favorite_name_edit");
        let text_response = ui.add(
            egui::TextEdit::singleline(&mut self.temp_favorite_name)
                .desired_width(150.0)
                .id(text_edit_id),
        );

        // Only request focus once when entering editing mode
        if !self.favorite_focus_requested {
            text_response.request_focus();
            self.favorite_focus_requested = true;
        }

        if text_response.has_focus() {
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.save_favorite_name(filter, global_config);
                self.editing_favorite = false;
                self.favorite_focus_requested = false;
            } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.editing_favorite = false;
                self.favorite_focus_requested = false;
            }
        }

        if text_response.lost_focus() && !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            if !self.temp_favorite_name.is_empty() {
                self.save_favorite_name(filter, global_config);
            }
            self.editing_favorite = false;
            self.favorite_focus_requested = false;
        }
    }

    fn render_favorite_selector(
        &mut self,
        ui: &mut Ui,
        filter_uuid: usize,
        global_config: &GlobalConfig,
        current_favorite: Option<&crate::config::FavoriteFilter>,
        events: &mut Vec<FilterInternalEvent>,
    ) {
        let selected_text = current_favorite.map_or_else(
            || "⭐ Favorites".to_string(),
            |fav| format!("⭐ {}", fav.display_name()),
        );

        let combo_response = egui::ComboBox::from_id_salt(format!("favorites_{filter_uuid}"))
            .selected_text(&selected_text)
            .width(150.0)
            .show_ui(ui, |ui| {
                for fav in &global_config.favorite_filters {
                    if ui.selectable_label(false, fav.display_name()).clicked() {
                        events.push(FilterInternalEvent::FavoriteSelected {
                            search_text: fav.search_text.clone(),
                            case_sensitive: fav.case_sensitive,
                        });
                    }
                }
            });

        if let Some(fav) = current_favorite {
            if combo_response.response.double_clicked() {
                self.editing_favorite = true;
                self.favorite_focus_requested = false; // Reset so we request focus in the next frame
                self.temp_favorite_name.clone_from(&fav.name);
            }
        }
    }

    fn render_search_input(
        &mut self,
        ui: &mut Ui,
        filter: &mut FilterState,
        should_focus_search: bool,
        log_view_state: &mut LogViewState,
        events: &mut Vec<FilterInternalEvent>,
    ) {
        let search_id = ui.id().with("search_input");
        let search_response = ui.add(
            egui::TextEdit::singleline(&mut filter.search_text)
                .hint_text("Enter regex pattern (e.g., ERROR|FATAL, \\d+\\.\\d+\\.\\d+\\.\\d+)")
                .desired_width(300.0)
                .id(search_id),
        );

        if should_focus_search {
            search_response.request_focus();
        }

        self.handle_history_navigation(
            ui,
            &search_response,
            search_id,
            filter,
            log_view_state,
            events,
        );

        if search_response.lost_focus() {
            self.history_index = None;
            log_view_state.add_to_filter_history(filter.search_text.clone());
        }

        if search_response.changed() {
            self.history_index = None;
            events.push(FilterInternalEvent::SearchChanged);
        }
    }

    fn handle_history_navigation(
        &mut self,
        ui: &mut Ui,
        search_response: &egui::Response,
        search_id: egui::Id,
        filter: &mut FilterState,
        log_view_state: &LogViewState,
        events: &mut Vec<FilterInternalEvent>,
    ) {
        if !search_response.has_focus() {
            return;
        }

        let filter_history = &log_view_state.filter_history;
        let up_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
        let down_pressed = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));

        if up_pressed && !filter_history.is_empty() {
            self.navigate_backward(filter, filter_history, events);
        } else if down_pressed {
            self.navigate_forward(filter, filter_history, events);
        }

        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            ui.memory_mut(|mem| mem.surrender_focus(search_id));
        }
    }

    fn navigate_backward(
        &mut self,
        filter: &mut FilterState,
        filter_history: &[String],
        events: &mut Vec<FilterInternalEvent>,
    ) {
        let new_index = match self.history_index {
            None => {
                self.pre_history_text.clone_from(&filter.search_text);
                usize::from(!(filter_history[0] != filter.search_text || filter_history.len() == 1))
            }
            Some(idx) => (idx + 1).min(filter_history.len() - 1),
        };
        self.history_index = Some(new_index);
        filter.search_text.clone_from(&filter_history[new_index]);
        events.push(FilterInternalEvent::SearchChanged);
    }

    fn navigate_forward(
        &mut self,
        filter: &mut FilterState,
        filter_history: &[String],
        events: &mut Vec<FilterInternalEvent>,
    ) {
        if let Some(idx) = self.history_index {
            if idx == 0 {
                filter.search_text.clone_from(&self.pre_history_text);
                self.history_index = None;
            } else {
                self.history_index = Some(idx - 1);
                filter.search_text.clone_from(&filter_history[idx - 1]);
            }
            events.push(FilterInternalEvent::SearchChanged);
        }
    }

    fn render_case_checkbox(
        &self,
        ui: &mut Ui,
        filter: &mut FilterState,
        events: &mut Vec<FilterInternalEvent>,
    ) {
        let toggle_response = ui
            .toggle_value(&mut filter.case_sensitive, "Aa")
            .on_hover_text("Toggle case insensitive matching");
        if toggle_response.changed() {
            events.push(FilterInternalEvent::CaseInsensitiveToggled);
        }
    }

    fn render_globally_visible_toggle(
        &self,
        ui: &mut Ui,
        filter: &mut FilterState,
        events: &mut Vec<FilterInternalEvent>,
    ) {
        if ui
            .toggle_value(&mut filter.globally_visible, "👁")
            .on_hover_text("Show highlights from this filter in all tabs")
            .changed()
        {
            events.push(FilterInternalEvent::DisplaySettingsChanged);
        }
    }

    fn render_histogram_toggle(
        &self,
        ui: &mut Ui,
        filter: &mut FilterState,
        events: &mut Vec<FilterInternalEvent>,
    ) {
        if ui
            .toggle_value(&mut filter.show_in_histogram, "📊")
            .on_hover_text("Show filter matches as vertical lines in histogram")
            .changed()
        {
            events.push(FilterInternalEvent::DisplaySettingsChanged);
        }
    }

    fn render_validation_status(ui: &mut Ui, filter: &FilterState) {
        match &filter.search_regex {
            Ok(_) => ui.colored_label(Color32::GREEN, "✓"),
            Err(err) => ui.colored_label(Color32::RED, format!("❌ {err}")),
        };
    }
}
