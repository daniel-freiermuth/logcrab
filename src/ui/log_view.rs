use crate::parser::line::LogLine;
use egui::{Color32, RichText, Ui, text::LayoutJob, TextFormat};
use egui_extras::{TableBuilder, Column};
use regex::Regex;

pub struct LogView {
    pub lines: Vec<LogLine>,
    pub min_score_filter: f64,
    pub search_text: String,
    pub search_regex: Option<Regex>,
    pub regex_error: Option<String>,
    // Cache for filtered/visible lines - only rebuild when search/filter changes
    filtered_indices: Vec<usize>,
    filter_dirty: bool,
}

impl LogView {
    pub fn new() -> Self {
        LogView {
            lines: Vec::new(),
            min_score_filter: 0.0,
            search_text: String::new(),
            search_regex: None,
            regex_error: None,
            filtered_indices: Vec::new(),
            filter_dirty: true,
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
            match Regex::new(&self.search_text) {
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
    fn rebuild_filtered_indices(&mut self) {
        self.filtered_indices.clear();
        self.filtered_indices.reserve(self.lines.len() / 10); // Rough estimate
        
        for (idx, line) in self.lines.iter().enumerate() {
            if line.anomaly_score >= self.min_score_filter && self.matches_search(line) {
                self.filtered_indices.push(idx);
            }
        }
        self.filter_dirty = false;
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
        
        // Rebuild filtered indices if needed
        if self.filter_dirty {
            self.rebuild_filtered_indices();
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
        
        // Virtual scrolling table - only renders visible rows!
        let available_height = ui.available_height();
        
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .id_salt("log_table") // Add ID so egui persists column widths
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(60.0).resizable(true).clip(true))   // Line number
            .column(Column::initial(110.0).resizable(true).clip(true))  // Timestamp
            .column(Column::initial(40.0).resizable(true).clip(true))   // Level
            .column(Column::remainder().clip(true))                      // Message (fills remaining space, clips overflow)
            .column(Column::initial(70.0).resizable(true).clip(true))   // Score
            .min_scrolled_height(available_height)
            .max_scroll_height(available_height)
            .header(20.0, |mut header| {
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
                    
                    let color = score_to_color(line.anomaly_score);
                    
                    // Line number
                    row.col(|ui| {
                        ui.label(RichText::new(format!("{}", line.line_number)).color(color));
                    });
                    
                    // Timestamp
                    row.col(|ui| {
                        let timestamp_str = if let Some(ts) = line.timestamp {
                            ts.format("%H:%M:%S%.3f").to_string()
                        } else {
                            "-".to_string()
                        };
                        ui.label(self.highlight_matches(&timestamp_str, color));
                    });
                    
                    // Message (don't truncate, let it clip)
                    row.col(|ui| {
                        ui.label(self.highlight_matches(&line.message, color));
                    });
                    
                    // Anomaly score
                    row.col(|ui| {
                        ui.label(RichText::new(format!("{:.1}", line.anomaly_score))
                            .strong()
                            .color(color));
                    });
                });
            });
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
