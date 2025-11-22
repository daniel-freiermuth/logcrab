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
use crate::parser::line::LogLine;
use crate::state::FilterState;
use crate::ui::components::{FilterBar, FavoriteFilter, FilterBarEvent, Histogram, HistogramClickEvent, LogTable, LogTableEvent};
use chrono::DateTime;
use std::collections::HashMap;

/// Events that can be emitted by the filter view
#[derive(Debug, Clone)]
pub enum FilterViewEvent {
    LineSelected { line_index: usize, timestamp: Option<DateTime<chrono::Local>> },
    BookmarkToggled { line_index: usize },
    FilterNameEditRequested,
    FavoriteToggled,
}

/// Orchestrates the filter view UI using reusable components
pub struct FilterView;

impl FilterView {
    /// Render a complete filter view
    /// 
    /// Returns events that occurred during rendering
    pub fn render(
        ui: &mut Ui,
        lines: &[LogLine],
        filter: &mut FilterState,
        filter_index: usize,
        all_filters: &[FilterState],
        selected_line_index: Option<usize>,
        selected_timestamp: Option<DateTime<chrono::Local>>,
        bookmarked_lines: &HashMap<usize, String>,
        min_score_filter: f64,
    ) -> Vec<FilterViewEvent> {
        let mut events = Vec::new();
        
        // Display custom name if set, otherwise default
        let display_name = filter.name.clone()
            .unwrap_or_else(|| format!("Filter View {}", filter_index + 1));
        ui.heading(&display_name);
        
        // Name and Star buttons
        ui.horizontal(|ui| {
            // Edit name button
            if ui.small_button("✏").on_hover_text("Edit filter name").clicked() {
                events.push(FilterViewEvent::FilterNameEditRequested);
            }
            
            let star_text = if filter.is_favorite { "⭐" } else { "☆" };
            if ui.button(star_text).on_hover_text("Toggle favorite filter").clicked() {
                events.push(FilterViewEvent::FavoriteToggled);
            }
        });
        
        ui.separator();
        
        // Collect favorite filters from all filters
        let favorites: Vec<FavoriteFilter> = all_filters.iter()
            .filter(|f| f.is_favorite && !f.search_text.is_empty())
            .map(|f| FavoriteFilter {
                search_text: f.search_text.clone(),
                case_insensitive: f.case_insensitive,
            })
            .collect();
        
        // Render filter bar
        let filter_bar_events = FilterBar::render(ui, filter, filter_index, &favorites);
        
        // Handle filter bar events
        for event in filter_bar_events {
            match event {
                FilterBarEvent::SearchChanged => {
                    filter.update_search_regex();
                }
                FilterBarEvent::CaseInsensitiveToggled => {
                    filter.update_search_regex();
                }
                FilterBarEvent::ClearClicked => {
                    filter.search_text.clear();
                    filter.update_search_regex();
                }
                FilterBarEvent::FavoriteSelected { search_text, case_insensitive } => {
                    filter.search_text = search_text;
                    filter.case_insensitive = case_insensitive;
                    filter.update_search_regex();
                }
            }
        }
        
        ui.separator();
        
        // Rebuild filtered indices if needed
        let mut scroll_to_row = if filter.filter_dirty {
            filter.rebuild_filtered_indices(
                lines,
                min_score_filter,
                selected_line_index,
                selected_timestamp,
            )
        } else {
            None
        };
        
        // Check if selection changed
        if scroll_to_row.is_none() && selected_line_index.is_some() {
            if filter.last_rendered_selection != selected_line_index {
                if let Some(selected_idx) = selected_line_index {
                    if let Some(position) = filter.filtered_indices.iter().position(|&idx| idx == selected_idx) {
                        scroll_to_row = Some(position);
                    } else {
                        // Line not in filtered results - try to find closest by timestamp
                        if let Some(selected_ts) = selected_timestamp {
                            if let Some(closest_pos) = filter.find_closest_timestamp_index(lines, selected_ts) {
                                scroll_to_row = Some(closest_pos);
                            }
                        }
                        // Mark as processed so we don't keep checking on every render
                        filter.last_rendered_selection = selected_line_index;
                    }
                }
            }
        }
        
        // Stats
        let total_lines = lines.len();
        let visible_lines = filter.filtered_indices.len();
        
        ui.horizontal(|ui| {
            ui.label(format!("Total lines: {}", total_lines));
            ui.separator();
            ui.label(format!("Visible: {}", visible_lines));
            if filter.search_regex.is_some() {
                ui.separator();
                ui.colored_label(Color32::LIGHT_BLUE, format!("🔍 {} matches", visible_lines));
            }
        });
        
        ui.separator();
        
        // Render histogram
        if let Some(hist_event) = Histogram::render(ui, lines, &filter.filtered_indices, selected_line_index) {
            events.push(FilterViewEvent::LineSelected {
                line_index: hist_event.line_index,
                timestamp: hist_event.timestamp,
            });
        }
        
        ui.separator();
        
        // Render log table
        let table_events = LogTable::render(
            ui,
            lines,
            filter,
            filter_index,
            selected_line_index,
            bookmarked_lines,
            scroll_to_row,
        );
        
        // Handle table events
        for event in table_events {
            match event {
                LogTableEvent::LineClicked { line_index, timestamp } => {
                    events.push(FilterViewEvent::LineSelected { line_index, timestamp });
                }
                LogTableEvent::BookmarkToggled { line_index } => {
                    events.push(FilterViewEvent::BookmarkToggled { line_index });
                }
            }
        }
        
        events
    }
}
