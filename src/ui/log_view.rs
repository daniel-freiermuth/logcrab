use crate::parser::line::LogLine;
use egui::{Color32, RichText, Ui, text::LayoutJob, TextFormat};
use egui_extras::{TableBuilder, Column};
use regex::{Regex, RegexBuilder};
use chrono::{DateTime, Local};
use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;
use serde::{Serialize, Deserialize};

/// Named bookmark with optional description
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Bookmark {
    line_index: usize,
    name: String,
    timestamp: Option<DateTime<Local>>,
}

/// Saved filter configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedFilter {
    search_text: String,
    case_insensitive: bool,
    is_favorite: bool,
}

/// .crab file format - stores all session data
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrabFile {
    bookmarks: Vec<Bookmark>,
    filters: Vec<SavedFilter>,
}

impl CrabFile {
    fn new() -> Self {
        CrabFile {
            bookmarks: Vec::new(),
            filters: Vec::new(),
        }
    }
}

/// Represents a single filter view with its own search criteria and cached results
struct FilterView {
    search_text: String,
    search_regex: Option<Regex>,
    regex_error: Option<String>,
    case_insensitive: bool,
    filtered_indices: Vec<usize>,
    filter_dirty: bool,
    last_rendered_selection: Option<usize>,
    highlight_color: Color32,
    is_favorite: bool,
}

impl FilterView {
    fn new(highlight_color: Color32) -> Self {
        FilterView {
            search_text: String::new(),
            search_regex: None,
            regex_error: None,
            case_insensitive: false,
            filtered_indices: Vec::new(),
            filter_dirty: true,
            last_rendered_selection: None,
            highlight_color,
            is_favorite: false,
        }
    }
    
    fn update_search_regex(&mut self) {
        if self.search_text.is_empty() {
            self.search_regex = None;
            self.regex_error = None;
        } else {
            match RegexBuilder::new(&self.search_text)
                .case_insensitive(self.case_insensitive)
                .build()
            {
                Ok(regex) => {
                    self.search_regex = Some(regex);
                    self.regex_error = None;
                }
                Err(e) => {
                    self.search_regex = None;
                    self.regex_error = Some(e.to_string());
                }
            }
        }
        self.filter_dirty = true;
    }
    
    fn matches_search(&self, line: &LogLine) -> bool {
        if let Some(ref regex) = self.search_regex {
            regex.is_match(&line.message) || regex.is_match(&line.raw)
        } else {
            true
        }
    }
    
    fn rebuild_filtered_indices(
        &mut self,
        lines: &[LogLine],
        min_score_filter: f64,
        selected_line_index: Option<usize>,
        selected_timestamp: Option<DateTime<Local>>,
    ) -> Option<usize> {
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_function!();
        
        self.filtered_indices.clear();
        self.filtered_indices.reserve(lines.len() / 10);
        
        for (idx, line) in lines.iter().enumerate() {
            if line.anomaly_score >= min_score_filter && self.matches_search(line) {
                self.filtered_indices.push(idx);
            }
        }
        self.filter_dirty = false;
        
        if let Some(selected_line_idx) = selected_line_index {
            if let Some(position) = self.filtered_indices.iter().position(|&idx| idx == selected_line_idx) {
                return Some(position);
            }
            
            if let Some(selected_ts) = selected_timestamp {
                return self.find_closest_timestamp_index(lines, selected_ts);
            }
        }
        
        None
    }
    
    fn find_closest_timestamp_index(&self, lines: &[LogLine], target_ts: DateTime<Local>) -> Option<usize> {
        if self.filtered_indices.is_empty() {
            return None;
        }
        
        let mut closest_idx = 0;
        let mut min_diff = i64::MAX;
        
        for (filtered_idx, &line_idx) in self.filtered_indices.iter().enumerate() {
            if let Some(line_ts) = lines[line_idx].timestamp {
                let diff = (line_ts.timestamp() - target_ts.timestamp()).abs();
                if diff < min_diff {
                    min_diff = diff;
                    closest_idx = filtered_idx;
                }
            }
        }
        
        Some(closest_idx)
    }
    
    fn highlight_matches(&self, text: &str, base_color: Color32) -> LayoutJob {
        let mut job = LayoutJob::default();
        
        if let Some(ref regex) = self.search_regex {
            let mut last_end = 0;
            
            for mat in regex.find_iter(text) {
                if mat.start() > last_end {
                    job.append(
                        &text[last_end..mat.start()],
                        0.0,
                        TextFormat {
                            color: base_color,
                            ..Default::default()
                        },
                    );
                }
                
                job.append(
                    mat.as_str(),
                    0.0,
                    TextFormat {
                        color: Color32::BLACK,
                        background: self.highlight_color,
                        ..Default::default()
                    },
                );
                
                last_end = mat.end();
            }
            
            if last_end < text.len() {
                job.append(
                    &text[last_end..],
                    0.0,
                    TextFormat {
                        color: base_color,
                        ..Default::default()
                    },
                );
            }
        } else {
            job.append(
                text,
                0.0,
                TextFormat {
                    color: base_color,
                    ..Default::default()
                },
            );
        }
        
        job
    }
}

pub struct LogView {
    pub lines: Vec<LogLine>,
    pub min_score_filter: f64,
    // Multiple filter views
    filters: Vec<FilterView>,
    // Selected line tracking
    selected_line_index: Option<usize>,
    selected_timestamp: Option<DateTime<Local>>,
    // Track last rendered selection for context view
    last_rendered_selection_context: Option<usize>,
    // Bookmarks with names
    bookmarks: HashMap<usize, Bookmark>,
    // .crab file path
    crab_file: Option<PathBuf>,
    // UI state
    show_bookmarks_panel: bool,
    bookmark_name_input: String,
    editing_bookmark: Option<usize>,
}

impl LogView {
    pub fn new() -> Self {
        // Start with 2 filters by default (yellow and light blue highlights)
        let filters = vec![
            FilterView::new(Color32::YELLOW),
            FilterView::new(Color32::LIGHT_BLUE),
        ];
        
        LogView {
            lines: Vec::new(),
            min_score_filter: 0.0,
            filters,
            selected_line_index: None,
            selected_timestamp: None,
            last_rendered_selection_context: None,
            bookmarks: HashMap::new(),
            crab_file: None,
            show_bookmarks_panel: false,
            bookmark_name_input: String::new(),
            editing_bookmark: None,
        }
    }
    
    pub fn add_filter(&mut self) {
        // Cycle through different highlight colors
        let colors = [
            Color32::YELLOW,
            Color32::LIGHT_BLUE,
            Color32::LIGHT_GREEN,
            Color32::from_rgb(255, 200, 150), // Light orange
            Color32::from_rgb(255, 150, 255), // Light magenta
            Color32::from_rgb(150, 255, 255), // Light cyan
        ];
        let color = colors[self.filters.len() % colors.len()];
        self.filters.push(FilterView::new(color));
    }
    
    pub fn remove_filter(&mut self, index: usize) {
        if self.filters.len() > 1 && index < self.filters.len() {
            self.filters.remove(index);
        }
    }
    
    pub fn filter_count(&self) -> usize {
        self.filters.len()
    }
    
    pub fn is_bookmarks_panel_visible(&self) -> bool {
        self.show_bookmarks_panel
    }
    
    pub fn set_lines(&mut self, lines: Vec<LogLine>) {
        self.lines = lines;
        for filter in &mut self.filters {
            filter.filter_dirty = true;
        }
    }
    
    pub fn set_bookmarks_file(&mut self, log_file_path: PathBuf) {
        let crab_path = log_file_path.with_extension("crab");
        self.crab_file = Some(crab_path.clone());
        self.load_crab_file();
    }
    
    fn load_crab_file(&mut self) {
        self.bookmarks.clear();
        
        if let Some(ref path) = self.crab_file {
            if let Ok(file_content) = fs::read_to_string(path) {
                if let Ok(crab_data) = serde_json::from_str::<CrabFile>(&file_content) {
                    // Load bookmarks
                    for bookmark in crab_data.bookmarks {
                        self.bookmarks.insert(bookmark.line_index, bookmark);
                    }
                    
                    // Load favorite filters
                    for (i, saved_filter) in crab_data.filters.iter().enumerate() {
                        if i < self.filters.len() {
                            self.filters[i].search_text = saved_filter.search_text.clone();
                            self.filters[i].case_insensitive = saved_filter.case_insensitive;
                            self.filters[i].is_favorite = saved_filter.is_favorite;
                            self.filters[i].update_search_regex();
                        }
                    }
                }
            }
        }
    }
    
    fn save_crab_file(&self) {
        if let Some(ref path) = self.crab_file {
            let crab_data = CrabFile {
                bookmarks: self.bookmarks.values().cloned().collect(),
                filters: self.filters.iter().map(|f| SavedFilter {
                    search_text: f.search_text.clone(),
                    case_insensitive: f.case_insensitive,
                    is_favorite: f.is_favorite,
                }).collect(),
            };
            
            if let Ok(json) = serde_json::to_string_pretty(&crab_data) {
                let _ = fs::write(path, json);
            }
        }
    }
    
    fn toggle_bookmark(&mut self, line_index: usize) {
        if self.bookmarks.contains_key(&line_index) {
            self.bookmarks.remove(&line_index);
        } else {
            let timestamp = if line_index < self.lines.len() {
                self.lines[line_index].timestamp
            } else {
                None
            };
            
            self.bookmarks.insert(line_index, Bookmark {
                line_index,
                name: format!("Line {}", if line_index < self.lines.len() {
                    self.lines[line_index].line_number.to_string()
                } else {
                    line_index.to_string()
                }),
                timestamp,
            });
        }
        self.save_crab_file();
    }
    
    fn rename_bookmark(&mut self, line_index: usize, new_name: String) {
        if let Some(bookmark) = self.bookmarks.get_mut(&line_index) {
            bookmark.name = new_name;
            self.save_crab_file();
        }
    }
    
    pub fn toggle_bookmarks_panel(&mut self) {
        self.show_bookmarks_panel = !self.show_bookmarks_panel;
    }
    
    /// Render a specific filter view
    pub fn render_filter(&mut self, ui: &mut Ui, filter_index: usize) {
        if filter_index >= self.filters.len() {
            ui.label("Invalid filter index");
            return;
        }
        
        ui.heading(format!("Filter View {}", filter_index + 1));
        
        // Star/Unstar button
        ui.horizontal(|ui| {
            let star_text = if self.filters[filter_index].is_favorite { "⭐" } else { "☆" };
            if ui.button(star_text).on_hover_text("Toggle favorite filter").clicked() {
                self.filters[filter_index].is_favorite = !self.filters[filter_index].is_favorite;
                self.save_crab_file();
            }
        });
        
        ui.separator();
        
        // Search bar
        ui.horizontal(|ui| {
            ui.label("🔍 Search (regex):");
            let search_response = ui.add(
                egui::TextEdit::singleline(&mut self.filters[filter_index].search_text)
                    .hint_text("Enter regex pattern (e.g., ERROR|FATAL, \\d+\\.\\d+\\.\\d+\\.\\d+)")
                    .desired_width(400.0)
            );
            
            if search_response.changed() {
                self.filters[filter_index].update_search_regex();
            }
            
            let case_changed = ui.checkbox(&mut self.filters[filter_index].case_insensitive, "Case insensitive").changed();
            if case_changed {
                self.filters[filter_index].update_search_regex();
            }
            
            if ui.button("Clear").clicked() {
                self.filters[filter_index].search_text.clear();
                self.filters[filter_index].update_search_regex();
            }
            
            if let Some(ref error) = self.filters[filter_index].regex_error {
                ui.colored_label(Color32::RED, format!("❌ {}", error));
            } else if self.filters[filter_index].search_regex.is_some() {
                ui.colored_label(Color32::GREEN, "✓ Valid regex");
            }
        });
        
        ui.separator();
        
        // Rebuild filtered indices if needed
        let mut scroll_to_row = if self.filters[filter_index].filter_dirty {
            self.filters[filter_index].rebuild_filtered_indices(
                &self.lines,
                self.min_score_filter,
                self.selected_line_index,
                self.selected_timestamp,
            )
        } else {
            None
        };
        
        // Check if selection changed
        if scroll_to_row.is_none() && self.selected_line_index.is_some() {
            if self.filters[filter_index].last_rendered_selection != self.selected_line_index {
                if let Some(selected_idx) = self.selected_line_index {
                    if let Some(position) = self.filters[filter_index].filtered_indices.iter().position(|&idx| idx == selected_idx) {
                        scroll_to_row = Some(position);
                    }
                }
            }
        }
        
        // Stats
        let total_lines = self.lines.len();
        let visible_lines = self.filters[filter_index].filtered_indices.len();
        
        ui.horizontal(|ui| {
            ui.label(format!("Total lines: {}", total_lines));
            ui.separator();
            ui.label(format!("Visible: {}", visible_lines));
            if self.filters[filter_index].search_regex.is_some() {
                ui.separator();
                ui.colored_label(Color32::LIGHT_BLUE, format!("🔍 {} matches", visible_lines));
            }
        });
        
        ui.separator();
        
        self.render_filter_table(ui, filter_index, scroll_to_row);
    }
    
    fn render_filter_table(&mut self, ui: &mut Ui, filter_index: usize, scroll_to_row: Option<usize>) {
        let visible_lines = self.filters[filter_index].filtered_indices.len();
        
        egui::ScrollArea::horizontal()
            .id_source(format!("filtered_scroll_{}", filter_index))
            .show(ui, |ui| {
                #[cfg(feature = "cpu-profiling")]
                puffin::profile_scope!("filtered_table");
                
                let mut table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(false)
                    .sense(egui::Sense::click())
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::initial(60.0).resizable(true).clip(true))
                    .column(Column::initial(110.0).resizable(true).clip(true))
                    .column(Column::remainder().resizable(true).clip(true))
                    .column(Column::initial(70.0).resizable(true).clip(true));
                
                if let Some(row_idx) = scroll_to_row {
                    table = table.scroll_to_row(row_idx, Some(egui::Align::Center));
                    self.filters[filter_index].last_rendered_selection = self.selected_line_index;
                }
                
                table.header(20.0, |mut header| {
                    header.col(|ui| { ui.strong("Line"); });
                    header.col(|ui| { ui.strong("Timestamp"); });
                    header.col(|ui| { ui.strong("Message"); });
                    header.col(|ui| { ui.strong("Score"); });
                })
                .body(|body| {
                    body.rows(18.0, visible_lines, |mut row| {
                        let row_index = row.index();
                        let line_idx = self.filters[filter_index].filtered_indices[row_index];
                        let line = &self.lines[line_idx];
                        
                        let is_selected = self.selected_line_index == Some(line_idx);
                        let is_bookmarked = self.bookmarks.contains_key(&line_idx);
                        let color = score_to_color(line.anomaly_score);
                        let line_number = line.line_number;
                        let timestamp_str = if let Some(ts) = line.timestamp {
                            ts.format("%H:%M:%S%.3f").to_string()
                        } else {
                            "-".to_string()
                        };
                        let message = line.message.clone();
                        let anomaly_score = line.anomaly_score;
                        let timestamp = line.timestamp;
                        
                        let mut row_clicked = false;
                        let mut row_right_clicked = false;
                        
                        let bookmark_icon = if is_bookmarked { "★ " } else { "" };
                        let line_text = if is_selected {
                            format!("▶ {}{}", bookmark_icon, line_number)
                        } else {
                            format!("{}{}", bookmark_icon, line_number)
                        };
                        
                        self.render_table_cell(&mut row, filter_index, is_bookmarked, is_selected, &line_text, color, line_idx, "line", &mut row_clicked, &mut row_right_clicked);
                        
                        row.col(|ui| {
                            if is_bookmarked {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            } else if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            
                            let job = self.filters[filter_index].highlight_matches(&timestamp_str, color);
                            ui.label(job);
                            
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with(filter_index).with("ts"), egui::Sense::click());
                            if response.clicked() { row_clicked = true; }
                            if response.secondary_clicked() { row_right_clicked = true; }
                        });
                        
                        row.col(|ui| {
                            if is_bookmarked {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            } else if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            
                            let job = self.filters[filter_index].highlight_matches(&message, color);
                            ui.label(job);
                            
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with(filter_index).with("msg"), egui::Sense::click());
                            if response.clicked() { row_clicked = true; }
                            if response.secondary_clicked() { row_right_clicked = true; }
                        });
                        
                        row.col(|ui| {
                            if is_bookmarked {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            } else if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            
                            let text = RichText::new(format!("{:.1}", anomaly_score)).strong().color(color);
                            ui.label(text);
                            
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with(filter_index).with("score"), egui::Sense::click());
                            if response.clicked() { row_clicked = true; }
                            if response.secondary_clicked() { row_right_clicked = true; }
                        });
                        
                        if row_right_clicked {
                            self.toggle_bookmark(line_idx);
                        } else if row_clicked {
                            self.selected_line_index = Some(line_idx);
                            self.selected_timestamp = timestamp;
                        }
                    });
                });
            });
    }
    
    fn render_table_cell(&self, row: &mut egui_extras::TableRow, _filter_index: usize, is_bookmarked: bool, is_selected: bool, text: &str, color: Color32, line_idx: usize, id_suffix: &str, row_clicked: &mut bool, row_right_clicked: &mut bool) {
        row.col(|ui| {
            if is_bookmarked {
                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
            } else if is_selected {
                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(60, 60, 80));
            }
            
            let text = if is_selected {
                RichText::new(text).color(color).strong()
            } else {
                RichText::new(text).color(color)
            };
            ui.label(text);
            
            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with(id_suffix), egui::Sense::click());
            if response.clicked() { *row_clicked = true; }
            if response.secondary_clicked() { *row_right_clicked = true; }
        });
    }
    
    pub fn render_context(&mut self, ui: &mut Ui) {
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_function!();
        
        ui.heading("Context View");
        ui.label("Unfiltered view centered on selected line");
        ui.separator();
        
        let Some(selected_idx) = self.selected_line_index else {
            ui.vertical_centered(|ui| {
                ui.add_space(100.0);
                ui.label("No line selected");
                ui.label("Click on a line in a filter view to see context");
            });
            return;
        };
        
        const CONTEXT_LINES: usize = 50;
        let start_idx = selected_idx.saturating_sub(CONTEXT_LINES);
        let end_idx = (selected_idx + CONTEXT_LINES + 1).min(self.lines.len());
        
        let selected_position = selected_idx - start_idx;
        let context_line_count = end_idx - start_idx;
        
        ui.label(format!("Showing lines {} - {} (selected: {})", 
            self.lines[start_idx].line_number,
            self.lines[end_idx - 1].line_number,
            self.lines[selected_idx].line_number
        ));
        ui.separator();
        
        let scroll_to_row = if self.last_rendered_selection_context != self.selected_line_index {
            self.last_rendered_selection_context = self.selected_line_index;
            Some(selected_position)
        } else {
            None
        };
        
        egui::ScrollArea::horizontal()
            .id_source("context_scroll")
            .show(ui, |ui| {
                #[cfg(feature = "cpu-profiling")]
                puffin::profile_scope!("context_table");
                
                let mut table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(false)
                    .sense(egui::Sense::click())
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::initial(60.0).resizable(true).clip(true))
                    .column(Column::initial(110.0).resizable(true).clip(true))
                    .column(Column::remainder().resizable(true).clip(true))
                    .column(Column::initial(70.0).resizable(true).clip(true));
                
                if let Some(row_idx) = scroll_to_row {
                    table = table.scroll_to_row(row_idx, Some(egui::Align::Center));
                }
                
                table.header(20.0, |mut header| {
                    header.col(|ui| { ui.strong("Line"); });
                    header.col(|ui| { ui.strong("Timestamp"); });
                    header.col(|ui| { ui.strong("Message"); });
                    header.col(|ui| { ui.strong("Score"); });
                })
                .body(|body| {
                    body.rows(18.0, context_line_count, |mut row| {
                        let row_index = row.index();
                        let line_idx = start_idx + row_index;
                        let line = &self.lines[line_idx];
                        
                        let is_selected = line_idx == selected_idx;
                        let is_bookmarked = self.bookmarks.contains_key(&line_idx);
                        let color = score_to_color(line.anomaly_score);
                        let timestamp = line.timestamp;
                        
                        let timestamp_str = if let Some(ts) = line.timestamp {
                            ts.format("%H:%M:%S%.3f").to_string()
                        } else {
                            "-".to_string()
                        };
                        
                        let mut row_clicked = false;
                        let mut row_right_clicked = false;
                        
                        row.col(|ui| {
                            if is_bookmarked {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            } else if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            
                            let bookmark_icon = if is_bookmarked { "★ " } else { "" };
                            let text = if is_selected {
                                RichText::new(format!("▶ {}{}", bookmark_icon, line.line_number)).color(color).strong()
                            } else {
                                RichText::new(format!("{}{}", bookmark_icon, line.line_number)).color(color)
                            };
                            ui.label(text);
                            
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with("ctx_line"), egui::Sense::click());
                            if response.clicked() { row_clicked = true; }
                            if response.secondary_clicked() { row_right_clicked = true; }
                        });
                        
                        row.col(|ui| {
                            if is_bookmarked {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            } else if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            ui.label(RichText::new(&timestamp_str).color(color));
                            
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with("ctx_ts"), egui::Sense::click());
                            if response.clicked() { row_clicked = true; }
                            if response.secondary_clicked() { row_right_clicked = true; }
                        });
                        
                        row.col(|ui| {
                            if is_bookmarked {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            } else if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            ui.label(RichText::new(&line.message).color(color));
                            
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with("ctx_msg"), egui::Sense::click());
                            if response.clicked() { row_clicked = true; }
                            if response.secondary_clicked() { row_right_clicked = true; }
                        });
                        
                        row.col(|ui| {
                            if is_bookmarked {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(100, 80, 30));
                            } else if is_selected {
                                ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            ui.label(RichText::new(format!("{:.1}", line.anomaly_score)).strong().color(color));
                            
                            let response = ui.interact(ui.max_rect(), ui.id().with(line_idx).with("ctx_score"), egui::Sense::click());
                            if response.clicked() { row_clicked = true; }
                            if response.secondary_clicked() { row_right_clicked = true; }
                        });
                        
                        if row_right_clicked {
                            self.toggle_bookmark(line_idx);
                        } else if row_clicked {
                            self.selected_line_index = Some(line_idx);
                            self.selected_timestamp = timestamp;
                        }
                    });
                });
            });
    }
    
    pub fn render_bookmarks(&mut self, ui: &mut Ui) {
        ui.heading("Bookmarks");
        ui.separator();
        
        if self.bookmarks.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(50.0);
                ui.label("No bookmarks yet");
                ui.label("Right-click on any line to bookmark it");
            });
            return;
        }
        
        let mut bookmarks: Vec<_> = self.bookmarks.values().cloned().collect();
        bookmarks.sort_by_key(|b| b.line_index);
        
        let mut to_delete = None;
        let mut to_jump = None;
        let mut should_save = false;
        
        egui::ScrollArea::vertical().show(ui, |ui| {
            for bookmark in &bookmarks {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        // Click to jump to bookmark
                        if ui.button("→").on_hover_text("Jump to line").clicked() {
                            to_jump = Some((bookmark.line_index, bookmark.timestamp));
                        }
                        
                        // Show line number and timestamp
                        let line_text = if bookmark.line_index < self.lines.len() {
                            format!("Line {} ({})", 
                                self.lines[bookmark.line_index].line_number,
                                if let Some(ts) = bookmark.timestamp {
                                    ts.format("%H:%M:%S").to_string()
                                } else {
                                    "-".to_string()
                                }
                            )
                        } else {
                            format!("Line {}", bookmark.line_index)
                        };
                        ui.label(RichText::new(line_text).strong());
                    });
                    
                    ui.horizontal(|ui| {
                        // Editable name
                        if self.editing_bookmark == Some(bookmark.line_index) {
                            let response = ui.text_edit_singleline(&mut self.bookmark_name_input);
                            if response.lost_focus() || ui.button("✓").clicked() {
                                if !self.bookmark_name_input.is_empty() {
                                    if let Some(b) = self.bookmarks.get_mut(&bookmark.line_index) {
                                        b.name = self.bookmark_name_input.clone();
                                        should_save = true;
                                    }
                                }
                                self.editing_bookmark = None;
                                self.bookmark_name_input.clear();
                            }
                            if ui.button("✖").clicked() {
                                self.editing_bookmark = None;
                                self.bookmark_name_input.clear();
                            }
                        } else {
                            ui.label(format!("📝 {}", bookmark.name));
                            if ui.small_button("✏").on_hover_text("Rename").clicked() {
                                self.editing_bookmark = Some(bookmark.line_index);
                                self.bookmark_name_input = bookmark.name.clone();
                            }
                            if ui.small_button("🗑").on_hover_text("Delete bookmark").clicked() {
                                to_delete = Some(bookmark.line_index);
                            }
                        }
                    });
                    
                    // Show message preview
                    if bookmark.line_index < self.lines.len() {
                        let message = &self.lines[bookmark.line_index].message;
                        let preview = if message.len() > 100 {
                            format!("{}...", &message[..100])
                        } else {
                            message.clone()
                        };
                        ui.label(RichText::new(preview).italics().color(Color32::GRAY));
                    }
                });
                ui.add_space(5.0);
            }
        });
        
        // Apply actions after the scroll area
        if let Some(line_idx) = to_delete {
            self.bookmarks.remove(&line_idx);
            should_save = true;
        }
        
        if let Some((line_idx, timestamp)) = to_jump {
            self.selected_line_index = Some(line_idx);
            self.selected_timestamp = timestamp;
        }
        
        if should_save {
            self.save_crab_file();
        }
    }
}

fn score_to_color(score: f64) -> Color32 {
    if score >= 80.0 {
        Color32::from_rgb(255, 100, 100)
    } else if score >= 60.0 {
        Color32::from_rgb(255, 180, 100)
    } else if score >= 30.0 {
        Color32::from_rgb(255, 200, 200)
    } else {
        Color32::LIGHT_GRAY
    }
}
