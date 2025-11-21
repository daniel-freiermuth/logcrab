use crate::parser::line::LogLine;
use egui::{Color32, RichText, Ui, text::LayoutJob, TextFormat};
use egui_extras::{TableBuilder, Column};
use regex::{Regex, RegexBuilder};
use chrono::{DateTime, Local};

pub struct LogView {
    pub lines: Vec<LogLine>,
    pub min_score_filter: f64,
    pub search_text: String,
    pub search_regex: Option<Regex>,
    pub regex_error: Option<String>,
    pub case_insensitive: bool,
    // Cache for filtered/visible lines - only rebuild when search/filter changes
    filtered_indices: Vec<usize>,
    filter_dirty: bool,
    // Selected line tracking
    selected_line_index: Option<usize>,
    selected_timestamp: Option<DateTime<Local>>,
    // Track last rendered selection to detect changes for each view
    last_rendered_selection_context: Option<usize>,
    last_rendered_selection_filtered: Option<usize>,
}

impl LogView {
    pub fn new() -> Self {
        LogView {
            lines: Vec::new(),
            min_score_filter: 0.0,
            search_text: String::new(),
            search_regex: None,
            regex_error: None,
            case_insensitive: false,
            filtered_indices: Vec::new(),
            filter_dirty: true,
            selected_line_index: None,
            selected_timestamp: None,
            last_rendered_selection_context: None,
            last_rendered_selection_filtered: None,
        }
    }
    
    pub fn set_lines(&mut self, lines: Vec<LogLine>) {
        self.lines = lines;
        self.filter_dirty = true;
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
            // Search in message and raw line
            regex.is_match(&line.message) ||
            regex.is_match(&line.raw)
        } else {
            true // No search active, everything matches
        }
    }
    
    /// Rebuild the filtered indices cache when filter/search changes
    fn rebuild_filtered_indices(&mut self) -> Option<usize> {
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_function!();
        
        self.filtered_indices.clear();
        self.filtered_indices.reserve(self.lines.len() / 10); // Rough estimate
        
        for (idx, line) in self.lines.iter().enumerate() {
            if line.anomaly_score >= self.min_score_filter && self.matches_search(line) {
                self.filtered_indices.push(idx);
            }
        }
        self.filter_dirty = false;
        
        // If we had a selected line, try to find it in the new filtered set
        if let Some(selected_line_idx) = self.selected_line_index {
            // First, try to find the exact same line in the filtered results
            if let Some(position) = self.filtered_indices.iter().position(|&idx| idx == selected_line_idx) {
                return Some(position);
            }
            
            // If the exact line is not in the filtered results, find closest by timestamp
            if let Some(selected_ts) = self.selected_timestamp {
                return self.find_closest_timestamp_index(selected_ts);
            }
        }
        
        None
    }
    
    /// Find the index in filtered_indices that is closest to the given timestamp
    fn find_closest_timestamp_index(&self, target_ts: DateTime<Local>) -> Option<usize> {
        if self.filtered_indices.is_empty() {
            return None;
        }
        
        let mut closest_idx = 0;
        let mut min_diff = i64::MAX;
        
        for (filtered_idx, &line_idx) in self.filtered_indices.iter().enumerate() {
            if let Some(line_ts) = self.lines[line_idx].timestamp {
                let diff = (line_ts.timestamp() - target_ts.timestamp()).abs();
                if diff < min_diff {
                    min_diff = diff;
                    closest_idx = filtered_idx;
                }
            }
        }
        
        Some(closest_idx)
    }
    
    /// Create a LayoutJob with highlighted matches
    fn highlight_matches(&self, text: &str, base_color: Color32) -> LayoutJob {
        let mut job = LayoutJob::default();
        
        if let Some(ref regex) = self.search_regex {
            let mut last_end = 0;
            
            // Find all matches and highlight them
            for mat in regex.find_iter(text) {
                // Add text before match (normal color)
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
                
                // Add matched text (highlighted)
                job.append(
                    mat.as_str(),
                    0.0,
                    TextFormat {
                        color: Color32::BLACK,
                        background: Color32::YELLOW,
                        ..Default::default()
                    },
                );
                
                last_end = mat.end();
            }
            
            // Add remaining text after last match
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
            // No search, just use base color
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
    
    pub fn render(&mut self, ui: &mut Ui) {
        ui.heading("Log Anomaly Explorer");
        
        ui.separator();
        
        // Search bar
        ui.horizontal(|ui| {
            ui.label("🔍 Search (regex):");
            let search_response = ui.add(
                egui::TextEdit::singleline(&mut self.search_text)
                    .hint_text("Enter regex pattern (e.g., ERROR|FATAL, \\d+\\.\\d+\\.\\d+\\.\\d+)")
                    .desired_width(400.0)
            );
            
            if search_response.changed() {
                self.update_search_regex();
            }
            
            // Case insensitive checkbox
            let case_changed = ui.checkbox(&mut self.case_insensitive, "Case insensitive").changed();
            if case_changed {
                self.update_search_regex();
            }
            
            if ui.button("Clear").clicked() {
                self.search_text.clear();
                self.update_search_regex();
            }
            
            // Show regex error if any
            if let Some(ref error) = self.regex_error {
                ui.colored_label(Color32::RED, format!("❌ {}", error));
            } else if self.search_regex.is_some() {
                ui.colored_label(Color32::GREEN, "✓ Valid regex");
            }
        });
        
        ui.separator();
        
        // Rebuild filtered indices if needed and get scroll target
        let mut scroll_to_row = if self.filter_dirty {
            self.rebuild_filtered_indices()
        } else {
            None
        };
        
        // Check if selection changed since last render of filtered view
        if scroll_to_row.is_none() && self.selected_line_index.is_some() {
            if self.last_rendered_selection_filtered != self.selected_line_index {
                // Selection changed, find it in filtered view
                if let Some(selected_idx) = self.selected_line_index {
                    if let Some(position) = self.filtered_indices.iter().position(|&idx| idx == selected_idx) {
                        scroll_to_row = Some(position);
                    }
                }
            }
        }
        
        // Stats
        let total_lines = self.lines.len();
        let visible_lines = self.filtered_indices.len();
        
        ui.horizontal(|ui| {
            ui.label(format!("Total lines: {}", total_lines));
            ui.separator();
            ui.label(format!("Visible: {}", visible_lines));
            if self.search_regex.is_some() {
                ui.separator();
                ui.colored_label(Color32::LIGHT_BLUE, format!("🔍 {} matches", visible_lines));
            }
        });
        
        ui.separator();
        
        // Wrap table in horizontal scroll area
        egui::ScrollArea::horizontal()
            .id_source("filtered_scroll")
            .show(ui, |ui| {
                #[cfg(feature = "cpu-profiling")]
                puffin::profile_scope!("filtered_table");
                
                // Virtual scrolling table - only renders visible rows!
                let mut table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(false)
                    .sense(egui::Sense::click())
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::initial(60.0).resizable(true).clip(true))   // Line number
                    .column(Column::initial(110.0).resizable(true).clip(true))  // Timestamp
                    .column(Column::remainder().resizable(true).clip(true))      // Message (fills remaining space, clips overflow)
                    .column(Column::initial(70.0).resizable(true).clip(true));  // Score
        
                // Scroll to target row if filter changed or selection changed from context view
                if let Some(row_idx) = scroll_to_row {
                    table = table.scroll_to_row(row_idx, Some(egui::Align::Center));
                    self.last_rendered_selection_filtered = self.selected_line_index;
                }
        
                table.header(20.0, |mut header| {
                header.col(|ui| {
                    ui.strong("Line");
                });
                header.col(|ui| {
                    ui.strong("Timestamp");
                });
                header.col(|ui| {
                    ui.strong("Message");
                });
                header.col(|ui| {
                    ui.strong("Score");
                });
            })
            .body(|body| {
                // CRITICAL: Only renders visible rows!
                // egui_extras handles virtualization automatically
                body.rows(18.0, visible_lines, |mut row| {
                    let row_index = row.index();
                    let line_idx = self.filtered_indices[row_index];
                    let line = &self.lines[line_idx];
                    
                    // Extract all needed data before entering closures to avoid borrow checker issues
                    let is_selected = self.selected_line_index == Some(line_idx);
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
                    
                    // Track if row was clicked in any column
                    let mut row_clicked = false;
                    
                    // Line number
                    row.col(|ui| {
                        // Highlight background if selected
                        if is_selected {
                            let rect = ui.available_rect_before_wrap();
                            ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(60, 60, 80));
                        }
                        
                        let text = RichText::new(format!("▶ {}", line_number)).color(color);
                        let text = if is_selected { text.strong() } else { RichText::new(format!("{}", line_number)).color(color) };
                        ui.label(text);
                        
                        // Add invisible button covering entire cell
                        if ui.interact(ui.max_rect(), ui.id().with(line_idx), egui::Sense::click()).clicked() {
                            row_clicked = true;
                        }
                    });
                    
                    // Timestamp
                    row.col(|ui| {
                        // Highlight background if selected
                        if is_selected {
                            let rect = ui.available_rect_before_wrap();
                            ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(60, 60, 80));
                        }
                        
                        let job = self.highlight_matches(&timestamp_str, color);
                        ui.label(job);
                        
                        // Add invisible button covering entire cell
                        if ui.interact(ui.max_rect(), ui.id().with(line_idx).with("ts"), egui::Sense::click()).clicked() {
                            row_clicked = true;
                        }
                    });
                    
                    // Message (don't truncate, let it clip)
                    row.col(|ui| {
                        // Highlight background if selected
                        if is_selected {
                            let rect = ui.available_rect_before_wrap();
                            ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(60, 60, 80));
                        }
                        
                        let job = self.highlight_matches(&message, color);
                        ui.label(job);
                        
                        // Add invisible button covering entire cell
                        if ui.interact(ui.max_rect(), ui.id().with(line_idx).with("msg"), egui::Sense::click()).clicked() {
                            row_clicked = true;
                        }
                    });
                    
                    // Anomaly score
                    row.col(|ui| {
                        // Highlight background if selected
                        if is_selected {
                            let rect = ui.available_rect_before_wrap();
                            ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(60, 60, 80));
                        }
                        
                        let text = RichText::new(format!("{:.1}", anomaly_score))
                            .strong()
                            .color(color);
                        ui.label(text);
                        
                        // Add invisible button covering entire cell
                        if ui.interact(ui.max_rect(), ui.id().with(line_idx).with("score"), egui::Sense::click()).clicked() {
                            row_clicked = true;
                        }
                    });
                    
                    // Update selection after all columns processed
                    if row_clicked {
                        self.selected_line_index = Some(line_idx);
                        self.selected_timestamp = timestamp;
                    }
                });
            });
        }); // End of ScrollArea
    }
    
    /// Render context view - shows unfiltered lines around the selected line
    pub fn render_context(&mut self, ui: &mut Ui) {
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_function!();
        
        ui.heading("Context View");
        ui.label("Unfiltered view centered on selected line");
        ui.separator();
        
        // If no line is selected, show message
        let Some(selected_idx) = self.selected_line_index else {
            ui.vertical_centered(|ui| {
                ui.add_space(100.0);
                ui.label("No line selected");
                ui.label("Click on a line in the main view to see context");
            });
            return;
        };
        
        // Show context: 50 lines before and after selected line
        const CONTEXT_LINES: usize = 50;
        let start_idx = selected_idx.saturating_sub(CONTEXT_LINES);
        let end_idx = (selected_idx + CONTEXT_LINES + 1).min(self.lines.len());
        
        // Calculate which index in the context view is the selected line
        let selected_position = selected_idx - start_idx;
        let context_line_count = end_idx - start_idx;
        
        ui.label(format!("Showing lines {} - {} (selected: {})", 
            self.lines[start_idx].line_number,
            self.lines[end_idx - 1].line_number,
            self.lines[selected_idx].line_number
        ));
        ui.separator();
        
        // Wrap table in horizontal scroll area
        egui::ScrollArea::horizontal()
            .id_source("context_scroll")
            .show(ui, |ui| {
                #[cfg(feature = "cpu-profiling")]
                puffin::profile_scope!("context_table");
                
                // Virtual scrolling table for context
                // Check if selection changed since last render
                let selection_changed = self.last_rendered_selection_context != Some(selected_idx);
        
                let mut table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(false)
                    .sense(egui::Sense::click())
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::initial(60.0).resizable(true).clip(true))   // Line number
                    .column(Column::initial(110.0).resizable(true).clip(true))  // Timestamp
                    .column(Column::remainder().resizable(true).clip(true))      // Message
                    .column(Column::initial(70.0).resizable(true).clip(true));  // Score
        
                // Only scroll to selected line when selection changes
                if selection_changed {
                    table = table.scroll_to_row(selected_position, Some(egui::Align::Center));
                    self.last_rendered_selection_context = Some(selected_idx);
                }
        
                        table.header(20.0, |mut header| {
                    header.col(|ui| {
                        ui.strong("Line");
                    });
                    header.col(|ui| {
                        ui.strong("Timestamp");
                    });
                    header.col(|ui| {
                        ui.strong("Message");
                    });
                    header.col(|ui| {
                        ui.strong("Score");
                    });
                })
                .body(|body| {
                    body.rows(18.0, context_line_count, |mut row| {
                        let row_index = row.index();
                        let line_idx = start_idx + row_index;
                        let line = &self.lines[line_idx];
                        
                        let is_selected = line_idx == selected_idx;
                        let color = score_to_color(line.anomaly_score);
                        
                        // Line number
                        row.col(|ui| {
                            if is_selected {
                                let rect = ui.available_rect_before_wrap();
                                ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            
                            let text = if is_selected {
                                RichText::new(format!("▶ {}", line.line_number)).color(color).strong()
                            } else {
                                RichText::new(format!("{}", line.line_number)).color(color)
                            };
                            ui.label(text);
                            
                            if ui.interact(ui.max_rect(), ui.id().with(line_idx), egui::Sense::click()).clicked() {
                                self.selected_line_index = Some(line_idx);
                                self.selected_timestamp = line.timestamp;
                            }
                        });
                        
                        // Timestamp
                        row.col(|ui| {
                            if is_selected {
                                let rect = ui.available_rect_before_wrap();
                                ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            
                            let timestamp_str = if let Some(ts) = line.timestamp {
                                ts.format("%H:%M:%S%.3f").to_string()
                            } else {
                                "-".to_string()
                            };
                            ui.label(RichText::new(timestamp_str).color(color));
                            
                            if ui.interact(ui.max_rect(), ui.id().with(line_idx).with("ts"), egui::Sense::click()).clicked() {
                                self.selected_line_index = Some(line_idx);
                                self.selected_timestamp = line.timestamp;
                            }
                        });
                        
                        // Message
                        row.col(|ui| {
                            if is_selected {
                                let rect = ui.available_rect_before_wrap();
                                ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            
                            ui.label(RichText::new(&line.message).color(color));
                            
                            if ui.interact(ui.max_rect(), ui.id().with(line_idx).with("msg"), egui::Sense::click()).clicked() {
                                self.selected_line_index = Some(line_idx);
                                self.selected_timestamp = line.timestamp;
                            }
                        });
                        
                        // Anomaly score
                        row.col(|ui| {
                            if is_selected {
                                let rect = ui.available_rect_before_wrap();
                                ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(60, 60, 80));
                            }
                            
                            let text = RichText::new(format!("{:.1}", line.anomaly_score))
                                .strong()
                                .color(color);
                            ui.label(text);
                            
                            if ui.interact(ui.max_rect(), ui.id().with(line_idx).with("score"), egui::Sense::click()).clicked() {
                                self.selected_line_index = Some(line_idx);
                                self.selected_timestamp = line.timestamp;
                            }
                        });
                    });
                });
            }); // End of ScrollArea
    }
}

impl Default for LogView {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert anomaly score (0-100) to color gradient
fn score_to_color(score: f64) -> Color32 {
    // White (0) -> Pink (30) -> Orange (60) -> Red (100)
    if score < 30.0 {
        // White to light pink
        let ratio = score / 30.0;
        Color32::from_rgb(
            255,
            (255.0 - ratio * 30.0) as u8,
            (255.0 - ratio * 30.0) as u8,
        )
    } else if score < 60.0 {
        // Pink to orange
        let ratio = (score - 30.0) / 30.0;
        Color32::from_rgb(
            255,
            (225.0 - ratio * 60.0) as u8,
            (225.0 - ratio * 125.0) as u8,
        )
    } else {
        // Orange to red
        let ratio = (score - 60.0) / 40.0;
        Color32::from_rgb(
            255,
            (165.0 - ratio * 165.0) as u8,
            (100.0 - ratio * 100.0) as u8,
        )
    }
}
