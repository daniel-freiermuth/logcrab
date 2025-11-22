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
use crate::parser::{parse_line, line::LogLine};
use crate::anomaly::{create_default_scorer, normalize_scores};
use crate::ui::LogView;
use std::path::PathBuf;
use std::fs::File;
use std::io::Read;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};

enum LoadMessage {
    Progress(f32, String),
    Complete(Vec<LogLine>, PathBuf),
    Error(String),
}

#[derive(Debug, Clone)]
enum TabType {
    Context,
    Filter(usize),
    Bookmarks,
}

struct TabContent {
    tab_type: TabType,
    title: String,
}

pub struct LogCrabApp {
    log_view: LogView,
    current_file: Option<PathBuf>,
    status_message: String,
    is_loading: bool,
    load_progress: f32,
    load_receiver: Option<Receiver<LoadMessage>>,
    initial_file: Option<PathBuf>,
    dock_state: DockState<TabContent>,
    show_anomaly_explanation: bool,
    add_tab_after: Option<egui_dock::NodeIndex>,
    #[cfg(feature = "cpu-profiling")]
    show_profiler: bool,
}

impl LogCrabApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, file: Option<PathBuf>) -> Self {
        // Initialize dock state with default layout
        let mut dock_state = DockState::new(vec![
            TabContent {
                tab_type: TabType::Context,
                title: "Context View".to_string(),
            }
        ]);
        
        // Add two filter tabs by default
        let [_context, _filters] = dock_state.main_surface_mut().split_right(
            NodeIndex::root(),
            0.3,
            vec![
                TabContent {
                    tab_type: TabType::Filter(0),
                    title: "Filter 1".to_string(),
                },
                TabContent {
                    tab_type: TabType::Filter(1),
                    title: "Filter 2".to_string(),
                },
            ],
        );
        
        LogCrabApp {
            log_view: LogView::new(),
            current_file: None,
            status_message: if file.is_some() {
                "Loading file...".to_string()
            } else {
                "Ready. Open a log file to begin.".to_string()
            },
            is_loading: false,
            load_progress: 0.0,
            load_receiver: None,
            initial_file: file,
            dock_state,
            show_anomaly_explanation: false,
            add_tab_after: None,
            #[cfg(feature = "cpu-profiling")]
            show_profiler: false,
        }
    }
    
    pub fn load_file(&mut self, path: PathBuf, ctx: egui::Context) {
        self.is_loading = true;
        self.load_progress = 0.0;
        self.status_message = format!("Loading {}...", path.display());
        
        let (tx, rx) = channel();
        self.load_receiver = Some(rx);
        
        thread::spawn(move || {
            Self::process_file_background(path, tx, ctx);
        });
    }
    
    fn process_file_background(path: PathBuf, tx: Sender<LoadMessage>, ctx: egui::Context) {
        // Get file size for progress tracking
        let metadata = std::fs::metadata(&path);
        if let Err(e) = metadata {
            let _ = tx.send(LoadMessage::Error(format!("Cannot read file: {}", e)));
            return;
        }
        let file_size = metadata.unwrap().len() as f32;
        
        let file = File::open(&path);
        if let Err(e) = file {
            let _ = tx.send(LoadMessage::Error(format!("Cannot open file: {}", e)));
            return;
        }
        
        // Read file with lossy UTF-8 conversion to handle non-UTF8 characters
        let mut file = file.unwrap();
        let mut buffer = Vec::new();
        if let Err(e) = file.read_to_end(&mut buffer) {
            let _ = tx.send(LoadMessage::Error(format!("Cannot read file: {}", e)));
            return;
        }
        
        // Convert to UTF-8 with lossy conversion (replaces invalid UTF-8 with � character)
        let content = String::from_utf8_lossy(&buffer);
        
        let mut scorer = create_default_scorer();
        let mut lines = Vec::new();
        let mut raw_scores = Vec::new();
        
        let mut bytes_read: usize = 0;
        
        // First pass: parse and score
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_scope!("parse_and_score");
        
        let mut file_line_number = 0;
        for line_buffer in content.lines() {
            file_line_number += 1;
            bytes_read += line_buffer.len() + 1; // +1 for newline
            
            // Update progress based on bytes read (first 80% of total progress)
            if file_line_number % 500 == 0 {
                let progress = 0.8 * (bytes_read as f32 / file_size).min(1.0);
                let _ = tx.send(LoadMessage::Progress(
                    progress,
                    format!("Loading {}... ({} lines)", path.display(), lines.len()),
                ));
                ctx.request_repaint();
            }
            
            if line_buffer.trim().is_empty() {
                continue;
            }
            
            let log_line = match parse_line(line_buffer.to_string(), file_line_number) {
                Some(line) => line,
                None => continue, // Skip lines without timestamp
            };
            
            let mut log_line = log_line;
            
            // Score before updating (key requirement!)
            let score = scorer.score(&log_line);
            log_line.anomaly_score = score;
            raw_scores.push(score);
            
            // Update scorer state
            scorer.update(&log_line);
            
            lines.push(log_line);
        }
        
        let _ = tx.send(LoadMessage::Progress(0.8, format!("Normalizing scores for {}...", path.display())));
        ctx.request_repaint();
        let _ = tx.send(LoadMessage::Progress(0.8, format!("Normalizing scores for {}...", path.display())));
        ctx.request_repaint();
        
        // Second pass: normalize scores to 0-100
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_scope!("normalize_scores");
        
        let normalized_scores = normalize_scores(&raw_scores);
        
        let _ = tx.send(LoadMessage::Progress(0.9, format!("Finalizing {}...", path.display())));
        ctx.request_repaint();
        
        // Debug: print score statistics
        if !raw_scores.is_empty() {
            let min_raw = raw_scores.iter().copied().fold(f64::INFINITY, f64::min);
            let max_raw = raw_scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let avg_raw: f64 = raw_scores.iter().sum::<f64>() / raw_scores.len() as f64;
            eprintln!("Score stats - Raw: min={:.3}, max={:.3}, avg={:.3}", min_raw, max_raw, avg_raw);
            
            let min_norm = normalized_scores.iter().copied().fold(f64::INFINITY, f64::min);
            let max_norm = normalized_scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let avg_norm: f64 = normalized_scores.iter().sum::<f64>() / normalized_scores.len() as f64;
            eprintln!("Score stats - Normalized: min={:.3}, max={:.3}, avg={:.3}", min_norm, max_norm, avg_norm);
        }
        
        for (line, &norm_score) in lines.iter_mut().zip(normalized_scores.iter()) {
            line.anomaly_score = norm_score;
        }
        
        let _ = tx.send(LoadMessage::Complete(lines, path));
        ctx.request_repaint();
    }
}

// TabViewer implementation for dock system
struct LogCrabTabViewer<'a> {
    log_view: &'a mut LogView,
    add_tab_after: &'a mut Option<egui_dock::NodeIndex>,
}

impl<'a> TabViewer for LogCrabTabViewer<'a> {
    type Tab = TabContent;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        // For filter tabs, check if there's a custom name
        if let TabType::Filter(index) = &tab.tab_type {
            if let Some(custom_name) = self.log_view.get_filter_name(*index) {
                return custom_name.into();
            }
        }
        (&tab.title).into()
    }
    
    fn context_menu(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab, _surface: egui_dock::SurfaceIndex, _node: egui_dock::NodeIndex) {
        // Only allow renaming filter tabs
        if let TabType::Filter(index) = &tab.tab_type {
            ui.label("Filter Tab");
            ui.separator();
            
            if ui.button("✏ Rename").clicked() {
                // Will be handled in the main UI
                ui.close_menu();
            }
            
            if ui.button("🗑 Clear Name").clicked() {
                self.log_view.set_filter_name(*index, None);
                ui.close_menu();
            }
        }
    }

    fn add_popup(&mut self, ui: &mut egui::Ui, _surface: egui_dock::SurfaceIndex, node: egui_dock::NodeIndex) {
        ui.set_min_width(120.0);
        if ui.button("➕ Filter Tab").clicked() {
            *self.add_tab_after = Some(node);
            ui.close_menu();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match &tab.tab_type {
            TabType::Context => {
                self.log_view.render_context(ui);
            }
            TabType::Filter(index) => {
                self.log_view.render_filter(ui, *index);
            }
            TabType::Bookmarks => {
                self.log_view.render_bookmarks(ui);
            }
        }
    }
}

impl eframe::App for LogCrabApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_function!();
        
        // Load initial file if provided via command line
        if let Some(file) = self.initial_file.take() {
            if file.exists() {
                self.load_file(file, ctx.clone());
            } else {
                self.status_message = format!("Error: File not found: {}", file.display());
            }
        }
        
        // Check for messages from background thread
        let mut should_clear_receiver = false;
        if let Some(ref rx) = self.load_receiver {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    LoadMessage::Progress(progress, status) => {
                        self.load_progress = progress;
                        self.status_message = status;
                    }
                    LoadMessage::Complete(lines, path) => {
                        self.log_view.set_lines(lines);
                        let additional_filters = self.log_view.set_bookmarks_file(path.clone());
                        
                        // Create tabs for any additional filters loaded from the crab file
                        for i in 0..additional_filters {
                            let filter_index = 2 + i; // First 2 filters already have tabs
                            self.dock_state.push_to_focused_leaf(TabContent {
                                tab_type: TabType::Filter(filter_index),
                                title: format!("Filter {}", filter_index + 1),
                            });
                        }
                        
                        self.current_file = Some(path.clone());
                        self.status_message = format!("Loaded {} successfully with {} lines", 
                            path.display(), 
                            self.log_view.lines.len());
                        self.is_loading = false;
                        self.load_progress = 1.0;
                        should_clear_receiver = true;
                    }
                    LoadMessage::Error(err) => {
                        self.status_message = err;
                        self.is_loading = false;
                        self.load_progress = 0.0;
                        should_clear_receiver = true;
                    }
                }
            }
        }
        if should_clear_receiver {
            self.load_receiver = None;
        }
        
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open Log File...").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Log Files", &["log", "txt"])
                            .add_filter("All Files", &["*"])
                            .pick_file() 
                        {
                            self.load_file(path, ctx.clone());
                        }
                        ui.close_menu();
                    }
                    
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                
                ui.menu_button("View", |ui| {
                    if ui.button("Add Filter Tab").clicked() {
                        let filter_index = self.log_view.filter_count();
                        self.log_view.add_filter();
                        self.dock_state.push_to_focused_leaf(TabContent {
                            tab_type: TabType::Filter(filter_index),
                            title: format!("Filter {}", filter_index + 1),
                        });
                        ui.close_menu();
                    }
                    
                    if ui.button("Add Bookmarks Tab").clicked() {
                        self.dock_state.push_to_focused_leaf(TabContent {
                            tab_type: TabType::Bookmarks,
                            title: "Bookmarks".to_string(),
                        });
                        ui.close_menu();
                    }
                    
                    if ui.button("Add Context Tab").clicked() {
                        self.dock_state.push_to_focused_leaf(TabContent {
                            tab_type: TabType::Context,
                            title: "Context View".to_string(),
                        });
                        ui.close_menu();
                    }
                });
                
                ui.menu_button("Help", |ui| {
                    if ui.button("About").clicked() {
                        self.status_message = "LogCrab 🦀 - Log Anomaly Explorer v0.1.0".to_string();
                        ui.close_menu();
                    }
                    
                    if ui.button("Anomaly Score Calculation").clicked() {
                        self.show_anomaly_explanation = true;
                        ui.close_menu();
                    }
                });
                
                #[cfg(feature = "cpu-profiling")]
                ui.menu_button("Profiling", |ui| {
                    ui.checkbox(&mut self.show_profiler, "Show CPU Profiler");
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
            #[cfg(feature = "cpu-profiling")]
            puffin::profile_scope!("central_panel");
            
                    if self.log_view.lines.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.heading("Welcome to LogCrab 🦀");
                    ui.add_space(20.0);
                    ui.add_space(40.0);
                    
                    if ui.button("Open Log File").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Log Files", &["log", "txt"])
                            .add_filter("All Files", &["*"])
                            .pick_file() 
                        {
                            self.load_file(path, ctx.clone());
                        }
                    }
                });
            } else {
                // Use dock area for VS Code-like draggable/tiling layout
                DockArea::new(&mut self.dock_state)
                    .show_inside(ui, &mut LogCrabTabViewer {
                        log_view: &mut self.log_view,
                        add_tab_after: &mut self.add_tab_after,
                    });
            }
        });
        
        // Handle add tab request
        if let Some(node) = self.add_tab_after.take() {
            let filter_index = self.log_view.filter_count();
            self.log_view.add_filter();
            self.dock_state.set_focused_node_and_surface((egui_dock::SurfaceIndex::main(), node));
            self.dock_state.push_to_focused_leaf(TabContent {
                tab_type: TabType::Filter(filter_index),
                title: format!("Filter {}", filter_index + 1),
            });
        }
        
        // Anomaly score explanation window
        if self.show_anomaly_explanation {
            egui::Window::new("Anomaly Score Calculation")
                .collapsible(false)
                .resizable(true)
                .default_width(700.0)
                .open(&mut self.show_anomaly_explanation)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.heading("How Anomaly Scores are Calculated");
                        ui.add_space(10.0);
                        
                        ui.label("LogCrab uses a multi-component scoring system to identify interesting, unusual, or problematic log lines. Each line receives a score from 0-100, where higher scores indicate higher anomaly.");
                        ui.add_space(15.0);
                        
                        ui.heading("Scoring Components:");
                        ui.add_space(10.0);
                        
                        // Rarity Scorer
                        ui.label(egui::RichText::new("1. Rarity Scorer (Weight: 3.0)").strong().color(egui::Color32::from_rgb(100, 200, 255)));
                        ui.indent("rarity", |ui| {
                            ui.label("• Scores based on template rarity (inverse frequency)");
                            ui.label("• Never-seen-before messages score 1.0 (maximum)");
                            ui.label("• Score = √(1 - frequency) where frequency = count/total");
                            ui.label("• Rare messages get higher scores than common ones");
                            ui.label("• Example: A unique error gets 1.0, while a repeated 'INFO: started' gets ~0.1");
                        });
                        ui.add_space(10.0);
                        
                        // Keyword Scorer
                        ui.label(egui::RichText::new("2. Keyword Scorer (Weight: 2.5)").strong().color(egui::Color32::from_rgb(100, 200, 255)));
                        ui.indent("keyword", |ui| {
                            ui.label("• Detects important keywords indicating issues");
                            ui.label("• ERROR/EXCEPTION/FATAL/CRASH/PANIC → score 1.0");
                            ui.label("• FAIL/FAILED/TIMEOUT/DENIED → score 0.8");
                            ui.label("• WARN/WARNING/ALERT → score 0.6");
                            ui.label("• ISSUE/PROBLEM/UNABLE/INVALID → score 0.4");
                            ui.label("• Case-insensitive pattern matching");
                        });
                        ui.add_space(10.0);
                        
                        // Temporal Scorer
                        ui.label(egui::RichText::new("3. Temporal Scorer (Weight: 2.0)").strong().color(egui::Color32::from_rgb(100, 200, 255)));
                        ui.indent("temporal", |ui| {
                            ui.label("• Analyzes time-based patterns with a 30-second window");
                            ui.label("• Recency component: Long gaps since last occurrence → higher score");
                            ui.label("  - Never seen in tracking: +0.7");
                            ui.label("  - Gap > 30 seconds: +0.5");
                            ui.label("  - Gap < 30 seconds: scaled 0.0-0.3 based on gap length");
                            ui.label("• Burst detection: High activity bursts → +0.3");
                            ui.label("  - Triggered when >100 events and >10 events/second");
                        });
                        ui.add_space(10.0);
                        
                        // Entropy Scorer
                        ui.label(egui::RichText::new("4. Entropy Scorer (Weight: 1.5)").strong().color(egui::Color32::from_rgb(100, 200, 255)));
                        ui.indent("entropy", |ui| {
                            ui.label("• Measures information content using Shannon entropy");
                            ui.label("• Entropy = -Σ(p × log₂(p)) where p = character frequency");
                            ui.label("• Tracks running average of entropy and message length");
                            ui.label("• Score based on deviation from average:");
                            ui.label("  - entropy_deviation = |entropy - avg_entropy| / avg_entropy");
                            ui.label("  - length_deviation = |length - avg_length| / avg_length");
                            ui.label("  - final_score = (entropy_deviation + length_deviation) / 2");
                            ui.label("• Unusual messages (very short/long or random) score higher");
                        });
                        ui.add_space(15.0);
                        
                        ui.separator();
                        ui.add_space(10.0);
                        
                        ui.heading("Final Score Calculation:");
                        ui.add_space(10.0);
                        
                        ui.label("1. Each scorer produces a raw score (0.0 - 1.0)");
                        ui.label("2. Raw scores are weighted and summed:");
                        ui.indent("formula", |ui| {
                            ui.label("raw_score = (rarity × 3.0) + (keyword × 2.5) + (temporal × 2.0) + (entropy × 1.5)");
                        });
                        ui.label("3. All raw scores are normalized to 0-100 range:");
                        ui.indent("normalize", |ui| {
                            ui.label("normalized = ((score - min_score) / (max_score - min_score)) × 100");
                        });
                        ui.add_space(10.0);
                        
                        ui.separator();
                        ui.add_space(10.0);
                        
                        ui.heading("Color Coding:");
                        ui.add_space(5.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("■").color(egui::Color32::from_rgb(255, 50, 50)));
                            ui.label("Red (80-100): High anomaly - crashes, errors, rare events");
                        });
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("■").color(egui::Color32::from_rgb(255, 140, 0)));
                            ui.label("Orange (60-79): Medium anomaly - warnings, failures");
                        });
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("■").color(egui::Color32::from_rgb(255, 200, 200)));
                            ui.label("Pink (30-59): Low anomaly - slightly unusual patterns");
                        });
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("■").color(egui::Color32::WHITE));
                            ui.label("White (0-29): Normal - common, expected log lines");
                        });
                        
                        ui.add_space(15.0);
                        ui.separator();
                        ui.add_space(10.0);
                        
                        ui.label(egui::RichText::new("Note:").strong());
                        ui.label("Scores are calculated during file loading in a single pass. The scorer learns patterns as it processes lines sequentially, so later lines benefit from more context.");
                    });
                });
        }
        
        #[cfg(feature = "cpu-profiling")]
        {
            puffin::GlobalProfiler::lock().new_frame();
            
            if self.show_profiler {
                puffin_egui::profiler_window(ctx);
            }
        }
    }
}
