use crate::parser::{parse_line, line::LogLine};
use crate::anomaly::{create_default_scorer, normalize_scores};
use crate::ui::LogView;
use std::path::PathBuf;
use std::fs::File;
use std::io::{BufRead, BufReader};

pub struct LogOwlApp {
    log_view: LogView,
    current_file: Option<PathBuf>,
    status_message: String,
    is_loading: bool,
    load_progress: f32,
}

impl LogOwlApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        LogOwlApp {
            log_view: LogView::new(),
            current_file: None,
            status_message: "Ready. Open a log file to begin.".to_string(),
            is_loading: false,
            load_progress: 0.0,
        }
    }
    
    pub fn load_file(&mut self, path: PathBuf) {
        self.is_loading = true;
        self.load_progress = 0.0;
        self.status_message = format!("Loading {}...", path.display());
        
        match self.process_file(&path) {
            Ok(lines) => {
                self.log_view.set_lines(lines);
                self.current_file = Some(path.clone());
                self.status_message = format!("Loaded {} successfully with {} lines", 
                    path.display(), 
                    self.log_view.lines.len());
            }
            Err(e) => {
                self.status_message = format!("Error loading file: {}", e);
            }
        }
        
        self.is_loading = false;
        self.load_progress = 1.0;
    }
    
    fn process_file(&mut self, path: &PathBuf) -> Result<Vec<LogLine>, std::io::Error> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        
        let mut scorer = create_default_scorer();
        let mut lines = Vec::new();
        let mut raw_scores = Vec::new();
        
        // First pass: parse and score
        for (idx, line_result) in reader.lines().enumerate() {
            let raw_line = line_result?;
            if raw_line.trim().is_empty() {
                continue;
            }
            
            let mut log_line = parse_line(raw_line, idx + 1);
            
            // Score before updating (key requirement!)
            let score = scorer.score(&log_line);
            log_line.anomaly_score = score;
            raw_scores.push(score);
            
            // Update scorer state
            scorer.update(&log_line);
            
            lines.push(log_line);
            
            // Update progress every 1000 lines
            if idx % 1000 == 0 {
                self.load_progress = 0.5 * (idx as f32 / lines.len().max(1) as f32);
            }
        }
        
        // Second pass: normalize scores to 0-100
        let normalized_scores = normalize_scores(&raw_scores);
        for (line, &norm_score) in lines.iter_mut().zip(normalized_scores.iter()) {
            line.anomaly_score = norm_score;
        }
        
        Ok(lines)
    }
}

impl eframe::App for LogOwlApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open Log File...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Log Files", &["log", "txt"])
                            .add_filter("All Files", &["*"])
                            .pick_file() 
                        {
                            self.load_file(path);
                        }
                        ui.close_menu();
                    }
                    
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                
                ui.menu_button("Help", |ui| {
                    if ui.button("About").clicked() {
                        self.status_message = "LogOwl - Log Anomaly Explorer v0.1.0".to_string();
                        ui.close_menu();
                    }
                });
            });
        });
        
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status_message);
                
                if self.is_loading {
                    ui.separator();
                    let progress_bar = egui::ProgressBar::new(self.load_progress)
                        .show_percentage();
                    ui.add(progress_bar);
                }
            });
        });
        
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.log_view.lines.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.heading("Welcome to LogOwl 🦉");
                    ui.add_space(20.0);
                    ui.label("An intelligent log anomaly explorer");
                    ui.add_space(40.0);
                    
                    if ui.button("Open Log File").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Log Files", &["log", "txt"])
                            .add_filter("All Files", &["*"])
                            .pick_file() 
                        {
                            self.load_file(path);
                        }
                    }
                    
                    ui.add_space(40.0);
                    ui.label("Features:");
                    ui.label("• Handles large log files (100-500 MB)");
                    ui.label("• Color-coded anomaly visualization");
                    ui.label("• Supports Android logcat and generic log formats");
                    ui.label("• Real-time anomaly scoring");
                });
            } else {
                self.log_view.render(ui);
            }
        });
    }
}
