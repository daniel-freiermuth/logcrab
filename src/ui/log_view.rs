use crate::parser::line::LogLine;
use egui::{Color32, RichText, ScrollArea, Ui};

pub struct LogView {
    pub lines: Vec<LogLine>,
    pub min_score_filter: f64,
}

impl LogView {
    pub fn new() -> Self {
        LogView {
            lines: Vec::new(),
            min_score_filter: 0.0,
        }
    }
    
    pub fn set_lines(&mut self, lines: Vec<LogLine>) {
        self.lines = lines;
    }
    
    pub fn render(&mut self, ui: &mut Ui) {
        ui.heading("Log Anomaly Explorer");
        
        ui.separator();
        
        // Stats
        let total_lines = self.lines.len();
        let visible_lines = self.lines.iter()
            .filter(|line| line.anomaly_score >= self.min_score_filter)
            .count();
        
        ui.horizontal(|ui| {
            ui.label(format!("Total lines: {}", total_lines));
            ui.separator();
            ui.label(format!("Visible: {}", visible_lines));
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
                            
                            let color = score_to_color(line.anomaly_score);
                            
                            // Line number
                            ui.label(RichText::new(format!("{}", line.line_number))
                                .color(color));
                            
                            // Timestamp
                            let timestamp_str = if let Some(ts) = line.timestamp {
                                ts.format("%H:%M:%S%.3f").to_string()
                            } else {
                                "-".to_string()
                            };
                            ui.label(RichText::new(timestamp_str).color(color));
                            
                            // Level
                            ui.label(RichText::new(line.level.to_str())
                                .color(level_color(&line.level)));
                            
                            // PID
                            let pid_str = line.pid.map(|p| p.to_string()).unwrap_or("-".to_string());
                            ui.label(RichText::new(pid_str).color(color));
                            
                            // Tag
                            let tag_str = line.tag.as_deref().unwrap_or("-");
                            ui.label(RichText::new(tag_str).color(color));
                            
                            // Message (truncate if too long)
                            let message = if line.message.len() > 120 {
                                format!("{}...", &line.message[..120])
                            } else {
                                line.message.clone()
                            };
                            ui.label(RichText::new(message).color(color));
                            
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
