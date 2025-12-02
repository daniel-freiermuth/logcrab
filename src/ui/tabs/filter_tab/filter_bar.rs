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

use crate::{config::GlobalConfig, ui::tabs::filter_tab::filter_state::FilterState};

/// Events emitted by the filter bar
#[derive(Debug, Clone)]
pub enum FilterInternalEvent {
    SearchChanged,
    CaseInsensitiveToggled,
    FavoriteSelected {
        search_text: String,
        case_insensitive: bool,
    },
    FilterNameEditRequested,
    FavoriteToggled,
}

/// Reusable filter search bar component with internal state for inline editing
pub struct FilterBar {
    editing_favorite: bool,
    temp_favorite_name: String,
}

impl FilterBar {
    pub fn new() -> Self {
        Self {
            editing_favorite: false,
            temp_favorite_name: String::new(),
        }
    }

    pub fn save_favorite_name(&mut self, filter: &FilterState, global_config: &mut GlobalConfig) {
        let new_name = self.temp_favorite_name.clone();
        if let Some(fav) = global_config
            .favorite_filters
            .iter_mut()
            .find(|f| f.matches(filter))
        {
            fav.name = new_name.clone();
            log::info!("Updated favorite name to: '{}'", new_name);
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
    ) -> Vec<FilterInternalEvent> {
        let mut events = Vec::new();

        ui.horizontal(|ui| {
            if ui
                .small_button("✏")
                .on_hover_text("Edit filter name")
                .clicked()
            {
                events.push(FilterInternalEvent::FilterNameEditRequested);
            }

            ui.color_edit_button_srgba(&mut filter.color)
                .on_hover_text("Choose highlight color for this filter");

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

            // Dropdown menu for favorites OR inline textbox for editing favorite name
            if !global_config.favorite_filters.is_empty() {
                // Find the matching favorite for the current filter

                if self.editing_favorite && current_favorite.is_some() {
                    // Show inline textbox for editing favorite name
                    let text_edit_id = ui.id().with("favorite_name_edit");
                    let text_response = ui.add(
                        egui::TextEdit::singleline(&mut self.temp_favorite_name)
                            .desired_width(150.0)
                            .id(text_edit_id),
                    );

                    // Auto-focus when starting to edit
                    if !text_response.has_focus() {
                        text_response.request_focus();
                    }

                    // Finish editing on Enter or Escape
                    if text_response.has_focus() {
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            self.save_favorite_name(filter, global_config);
                            self.editing_favorite = false;
                        } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            self.editing_favorite = false;
                        }
                    }

                    // Also finish editing if clicking outside
                    if text_response.lost_focus() && !ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        if !self.temp_favorite_name.is_empty() {
                            self.save_favorite_name(filter, global_config);
                        }
                        self.editing_favorite = false;
                    }
                } else {
                    // Show dropdown as normal
                    // Show current favorite name if this filter matches one, otherwise show "⭐ Favorites"
                    let selected_text = if let Some(fav) = current_favorite {
                        format!("⭐ {}", fav.display_name())
                    } else {
                        "⭐ Favorites".to_string()
                    };

                    let combo_response =
                        egui::ComboBox::from_id_salt(format!("favorites_{}", filter_uuid))
                            .selected_text(&selected_text)
                            .width(150.0)
                            .show_ui(ui, |ui| {
                                for fav in &global_config.favorite_filters {
                                    if ui.selectable_label(false, fav.display_name()).clicked() {
                                        events.push(FilterInternalEvent::FavoriteSelected {
                                            search_text: fav.search_text.clone(),
                                            case_insensitive: fav.case_insensitive,
                                        });
                                    }
                                }
                            });

                    // If a favorite is selected and user double-clicks on the dropdown, start editing
                    if let Some(fav) = current_favorite {
                        if combo_response.response.double_clicked() {
                            self.editing_favorite = true;
                            self.temp_favorite_name = fav.name.clone();
                        }
                    }
                }
            }

            // Search input with ID for Ctrl+L focusing
            let search_id = ui.id().with("search_input");
            let search_response = ui.add(
                egui::TextEdit::singleline(&mut filter.search_text)
                    .hint_text("Enter regex pattern (e.g., ERROR|FATAL, \\d+\\.\\d+\\.\\d+\\.\\d+)")
                    .desired_width(300.0)
                    .id(search_id),
            );

            // Focus search input if requested by Ctrl+L
            if should_focus_search {
                search_response.request_focus();
            }

            // If Enter is pressed in the search input, surrender focus
            if search_response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ui.memory_mut(|mem| mem.surrender_focus(search_id));
            }

            if search_response.changed() {
                events.push(FilterInternalEvent::SearchChanged);
            }

            // Checkbox
            let checkbox_response = ui.checkbox(&mut filter.case_insensitive, "Aa");

            if checkbox_response.changed() {
                events.push(FilterInternalEvent::CaseInsensitiveToggled);
            }

            // Display regex validation status
            match &filter.search_regex {
                Ok(_) => ui.colored_label(Color32::GREEN, "✓"),
                Err(err) => ui.colored_label(Color32::RED, format!("❌ {}", err)),
            }
        });

        events
    }
}
