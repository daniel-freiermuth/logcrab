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

mod highlight_state;

pub use highlight_state::HighlightState;

use std::sync::Arc;

use egui::{Color32, RichText, Ui};

use crate::config::GlobalConfig;
use crate::core::{LogStore, SavedFilter};
use crate::input::ShortcutAction;
use crate::ui::filter_highlight::FilterHighlight;
use crate::ui::log_view::LogViewState;
use crate::ui::tabs::filter_tab::HistogramMarker;
use crate::ui::tabs::LogCrabTab;
use crate::ui::DEFAULT_PALETTE;

/// Tab for managing highlight rules
#[derive(Default)]
pub struct HighlightsView {
    /// Counter for assigning colors and names to new highlights
    monotonic_counter: usize,
    /// Which highlight index is currently being edited (inline name editing)
    editing_name_index: Option<usize>,
    /// Whether we've already requested focus for the current edit session
    focus_requested: bool,
}

impl HighlightsView {
    pub fn new() -> Self {
        Self::default()
    }

    fn next_color_and_name(&mut self) -> (Color32, String) {
        let color = DEFAULT_PALETTE[self.monotonic_counter % DEFAULT_PALETTE.len()];
        let name = format!("Highlight {}", self.monotonic_counter + 1);
        self.monotonic_counter += 1;
        (color, name)
    }

    fn render_highlight_row(
        ui: &mut Ui,
        highlight: &mut HighlightState,
        index: usize,
        is_editing_name: bool,
        should_focus: bool,
    ) -> Vec<HighlightRowAction> {
        let mut actions = Vec::new();

        ui.horizontal(|ui| {
            // Enable/disable toggle
            if ui
                .toggle_value(&mut highlight.enabled, "👁")
                .on_hover_text("Enable/disable this highlight")
                .changed()
            {
                actions.push(HighlightRowAction::Modified);
            }

            // Histogram toggle
            if ui
                .toggle_value(&mut highlight.show_in_histogram, "📊")
                .on_hover_text("Show matches as markers in timeline")
                .changed()
            {
                actions.push(HighlightRowAction::Modified);
            }

            // Name: clickable label or editable field
            if is_editing_name {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut highlight.name)
                        .desired_width(120.0)
                        .hint_text("Name..."),
                );
                // Request focus only on the first frame
                if should_focus {
                    response.request_focus();
                    actions.push(HighlightRowAction::FocusRequested);
                }
                // Cancel on Escape (don't save)
                else if ui.input(|i| i.key_pressed(egui::Key::Escape))
                    || response.lost_focus()
                    || !response.has_focus()
                {
                    actions.push(HighlightRowAction::StopEditingName);
                } else if response.changed() {
                    actions.push(HighlightRowAction::Modified);
                }
            } else {
                // Show as clickable label
                let name_display = if highlight.name.is_empty() {
                    "(unnamed)"
                } else {
                    &highlight.name
                };
                if ui
                    .add(egui::Label::new(name_display).sense(egui::Sense::click()))
                    .on_hover_text("Click to edit name")
                    .clicked()
                {
                    actions.push(HighlightRowAction::StartEditingName(index));
                }
            }

            // Color picker
            let mut color_arr = highlight.color.to_array();
            if ui
                .color_edit_button_srgba_unmultiplied(&mut color_arr)
                .changed()
            {
                highlight.color = Color32::from_rgba_unmultiplied(
                    color_arr[0],
                    color_arr[1],
                    color_arr[2],
                    color_arr[3],
                );
                actions.push(HighlightRowAction::Modified);
            }

            // Search text input
            let response = ui.add(
                egui::TextEdit::singleline(&mut highlight.search.search_text)
                    .desired_width(300.0)
                    .hint_text("Search pattern..."),
            );
            if response.changed() {
                actions.push(HighlightRowAction::Modified);
            }

            // Case sensitivity toggle
            let case_label = if highlight.search.case_sensitive {
                RichText::new("Aa").strong()
            } else {
                RichText::new("Aa")
            };
            if ui
                .toggle_value(&mut highlight.search.case_sensitive, case_label)
                .on_hover_text("Case sensitive")
                .changed()
            {
                actions.push(HighlightRowAction::Modified);
            }

            // Show validation status
            match &highlight.search.get_regex() {
                Ok(_) if !highlight.search.search_text.is_empty() => {
                    ui.colored_label(Color32::GREEN, "✓");
                }
                Err(err) => {
                    ui.colored_label(Color32::RED, format!("❌ {err}"));
                }
                _ => {}
            }

            // Convert to filter button
            if ui
                .button("into Filter")
                .on_hover_text("Convert this highlight to a filter tab")
                .clicked()
            {
                actions.push(HighlightRowAction::ConvertToFilter(index));
            }

            // Delete button
            if ui
                .button(RichText::new("🗑").color(Color32::from_rgb(200, 80, 80)))
                .on_hover_text("Delete this highlight")
                .clicked()
            {
                actions.push(HighlightRowAction::Delete(index));
            }
        });

        actions
    }
}

#[derive(Debug, Clone)]
enum HighlightRowAction {
    Modified,
    StartEditingName(usize),
    /// Stop editing and save (Enter or focus lost)
    StopEditingName,
    /// Focus was requested for the text field
    FocusRequested,
    /// Convert this highlight to a filter tab
    ConvertToFilter(usize),
    Delete(usize),
}

impl LogCrabTab for HighlightsView {
    fn title(&mut self) -> egui::WidgetText {
        "🎨 Highlights".into()
    }

    fn render(
        &mut self,
        ui: &mut Ui,
        data_state: &mut LogViewState,
        _global_config: &mut GlobalConfig,
        _all_filter_highlights: &[FilterHighlight],
        _histogram_markers: &[HistogramMarker],
    ) {
        profiling::scope!("HighlightsView::render");

        ui.vertical(|ui| {
            ui.add_space(4.0);

            // Header
            ui.horizontal(|ui| {
                if ui.button("➕ Add Highlight").clicked() {
                    let (color, name) = self.next_color_and_name();
                    data_state.highlights.push(HighlightState::new(name, color));
                    data_state.modified = true;
                }
            });

            ui.separator();

            if data_state.highlights.is_empty() {
                ui.label("No highlights configured. Click 'Add Highlight' to create one.");
            } else {
                // Render each highlight row
                let mut actions = Vec::new();
                for (index, highlight) in data_state.highlights.iter_mut().enumerate() {
                    let is_editing = self.editing_name_index == Some(index);
                    let should_focus = is_editing && !self.focus_requested;
                    let row_actions =
                        Self::render_highlight_row(ui, highlight, index, is_editing, should_focus);
                    actions.extend(row_actions);
                    ui.add_space(2.0);
                }

                // Process actions
                for action in actions {
                    match action {
                        HighlightRowAction::Modified => {
                            data_state.modified = true;
                        }
                        HighlightRowAction::ConvertToFilter(index) => {
                            // Request conversion - will be handled by LogView
                            data_state.pending_highlight_to_filter = Some(index);
                        }
                        HighlightRowAction::Delete(index) => {
                            // Clear editing state if we're deleting the item being edited
                            if self.editing_name_index == Some(index) {
                                self.editing_name_index = None;
                                self.focus_requested = false;
                            } else if let Some(editing_idx) = self.editing_name_index {
                                // Adjust editing index if we deleted an item before it
                                if index < editing_idx {
                                    self.editing_name_index = Some(editing_idx - 1);
                                }
                            }
                            data_state.highlights.remove(index);
                            data_state.modified = true;
                        }
                        HighlightRowAction::StartEditingName(index) => {
                            self.editing_name_index = Some(index);
                            self.focus_requested = false; // Reset so we request focus on next frame
                        }
                        HighlightRowAction::StopEditingName => {
                            self.editing_name_index = None;
                            self.focus_requested = false;
                        }
                        HighlightRowAction::FocusRequested => {
                            self.focus_requested = true;
                        }
                    }
                }
            }
        });
    }

    fn process_events(
        &mut self,
        _actions: &[ShortcutAction],
        _data_state: &mut LogViewState,
    ) -> bool {
        false
    }

    fn try_into_stored_filter(&self) -> Option<SavedFilter> {
        None // Highlights tab doesn't store as a filter
    }

    fn get_filter_highlight(&self) -> Option<FilterHighlight> {
        None // Highlights are stored separately in LogViewState
    }

    fn get_histogram_marker(&mut self, _store: &Arc<LogStore>) -> Option<HistogramMarker> {
        None // Highlights provide their markers via LogViewState
    }
}
