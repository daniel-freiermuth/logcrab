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
use crate::state::FilterState;

/// Favorite filter for quick selection
pub struct FavoriteFilter {
    pub search_text: String,
    pub case_insensitive: bool,
}

/// Events emitted by the filter bar
#[derive(Debug, Clone)]
pub enum FilterBarEvent {
    SearchChanged,
    CaseInsensitiveToggled,
    ClearClicked,
    FavoriteSelected { search_text: String, case_insensitive: bool },
}

/// Reusable filter search bar component
pub struct FilterBar;

impl FilterBar {
    /// Render the filter bar UI
    /// 
    /// Returns events that occurred during rendering
    pub fn render(
        ui: &mut Ui,
        filter: &mut FilterState,
        filter_index: usize,
        favorites: &[FavoriteFilter],
    ) -> Vec<FilterBarEvent> {
        let mut events = Vec::new();
        
        ui.horizontal(|ui| {
            ui.label("🔍 Search (regex):");
            
            // Dropdown menu for favorites
            if !favorites.is_empty() {
                egui::ComboBox::from_id_source(format!("favorites_{}", filter_index))
                    .selected_text("⭐ Favorites")
                    .width(100.0)
                    .show_ui(ui, |ui| {
                        for fav in favorites {
                            if ui.selectable_label(false, &fav.search_text).clicked() {
                                events.push(FilterBarEvent::FavoriteSelected {
                                    search_text: fav.search_text.clone(),
                                    case_insensitive: fav.case_insensitive,
                                });
                            }
                        }
                    });
            }
            
            // Search input with ID for Ctrl+L focusing
            let search_id = ui.id().with("search_input");
            let mut search_response = ui.add(
                egui::TextEdit::singleline(&mut filter.search_text)
                    .hint_text("Enter regex pattern (e.g., ERROR|FATAL, \\d+\\.\\d+\\.\\d+\\.\\d+)")
                    .desired_width(300.0)
                    .id(search_id)
            );
            
            // Focus search input if requested by Ctrl+L
            if filter.should_focus_search {
                search_response.request_focus();
                filter.should_focus_search = false;
            }
            
            // If Enter is pressed in the search input, surrender focus
            if search_response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ui.memory_mut(|mem| mem.surrender_focus(search_id));
            }
            
            if search_response.changed() {
                events.push(FilterBarEvent::SearchChanged);
            }
            
            // Checkbox
            let checkbox_response = ui.checkbox(&mut filter.case_insensitive, "Case insensitive");
            
            if checkbox_response.changed() {
                events.push(FilterBarEvent::CaseInsensitiveToggled);
            }
            
            if ui.button("Clear").clicked() {
                events.push(FilterBarEvent::ClearClicked);
            }
            
            // Display regex validation status
            if let Some(ref error) = filter.regex_error {
                ui.colored_label(Color32::RED, format!("❌ {}", error));
            } else if filter.search_regex.is_some() {
                ui.colored_label(Color32::GREEN, "✓ Valid regex");
            }
        });
        
        events
    }
}
