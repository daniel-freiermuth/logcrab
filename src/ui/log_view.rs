use crate::parser::line::LogLine;
use egui::{Color32, RichText, ScrollArea, Ui, text::LayoutJob, TextFormat, FontId};
use regex::Regex;

pub struct LogView {
    pub lines: Vec<LogLine>,
    pub min_score_filter: f64,
    pub search_text: String,
    pub search_regex: Option<Regex>,
    pub regex_error: Option<String>,
}

impl LogView {
    pub fn new() -> Self {
        LogView {
            lines: Vec::new(),
            min_score_filter: 0.0,
            search_text: String::new(),
            search_regex: None,
            regex_error: None,
        }
    }
    
    pub fn set_lines(&mut self, lines: Vec<LogLine>) {
        self.lines = lines;
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
    }
    
    fn matches_search(&self, line: &LogLine) -> bool {
        if let Some(ref regex) = self.search_regex {
            // Search in message, tag, and raw line
            regex.is_match(&line.message) ||
            line.tag.as_ref().map(|t| regex.is_match(t)).unwrap_or(false) ||
            regex.is_match(&line.raw)
        } else {
            true // No search active, everything matches
        }
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
        
        // Stats
        let total_lines = self.lines.len();
        let visible_lines = self.lines.iter()
            .filter(|line| line.anomaly_score >= self.min_score_filter && self.matches_search(line))
            .count();
        
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
        
        // Scrollable log view
        ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                egui::Grid::new("log_grid")
                    .striped(true)
                    .spacing([10.0, 4.0])
                    .show(ui, |ui| {
                        // Header
                        ui.label(RichText::new("Line").strong());
                        ui.label(RichText::new("Timestamp").strong());
                        ui.label(RichText::new("Lvl").strong());
                        ui.label(RichText::new("PID").strong());
                        ui.label(RichText::new("Tag").strong());
                        ui.label(RichText::new("Message").strong());
                        ui.label(RichText::new("Score").strong());
                        ui.end_row();
                        
                        // Log lines
                        for line in &self.lines {
                            if line.anomaly_score < self.min_score_filter {
                                continue;
                            }
                            
                            // Check if line matches search
                            let matches_search = self.matches_search(line);
                            if !matches_search {
                                continue;
                            }
                            
                            let color = score_to_color(line.anomaly_score);
                            
                            // Line number
                            ui.label(RichText::new(format!("{}", line.line_number)).color(color));
                            
                            // Timestamp
                            let timestamp_str = if let Some(ts) = line.timestamp {
                                ts.format("%H:%M:%S%.3f").to_string()
                            } else {
                                "-".to_string()
                            };
                            ui.label(self.highlight_matches(&timestamp_str, color));
                            
                            // Level
                            ui.label(RichText::new(line.level.to_str())
                                .color(level_color(&line.level)));
                            
                            // PID
                            let pid_str = line.pid.map(|p| p.to_string()).unwrap_or("-".to_string());
                            ui.label(self.highlight_matches(&pid_str, color));
                            
                            // Tag
                            let tag_str = line.tag.as_deref().unwrap_or("-");
                            ui.label(self.highlight_matches(tag_str, color));
                            
                            // Message (truncate if too long, but highlight matches)
                            let message = if line.message.len() > 120 {
                                format!("{}...", &line.message[..120])
                            } else {
                                line.message.clone()
                            };
                            ui.label(self.highlight_matches(&message, color));
                            
                            // Anomaly score
                            ui.label(RichText::new(format!("{:.1}", line.anomaly_score))
                                .strong()
                                .color(color));
                            
                            ui.end_row();
                        }
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

/// Get color for log level
fn level_color(level: &crate::parser::line::LogLevel) -> Color32 {
    use crate::parser::line::LogLevel;
    match level {
        LogLevel::Verbose => Color32::GRAY,
        LogLevel::Debug => Color32::LIGHT_BLUE,
        LogLevel::Info => Color32::GREEN,
        LogLevel::Warning => Color32::YELLOW,
        LogLevel::Error => Color32::LIGHT_RED,
        LogLevel::Fatal => Color32::RED,
        LogLevel::Unknown => Color32::GRAY,
    }
}
