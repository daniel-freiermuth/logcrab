use crate::parser::{parse_line, line::LogLine};
use crate::anomaly::{create_default_scorer, normalize_scores};
use crate::ui::LogView;
use std::path::PathBuf;
use std::fs::File;
use std::io::{BufRead, BufReader};
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
        let mut reader = BufReader::new(file.unwrap());
        
        let mut scorer = create_default_scorer();
        let mut lines = Vec::new();
        let mut raw_scores = Vec::new();
        
        let mut bytes_read: usize = 0;
        
        // First pass: parse and score
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_scope!("parse_and_score");
        
        let mut line_buffer = String::new();
        let mut file_line_number = 0;
        loop {
            line_buffer.clear();
            let bytes = match reader.read_line(&mut line_buffer) {
                Ok(b) => b,
                Err(e) => {
                    let _ = tx.send(LoadMessage::Error(format!("Read error: {}", e)));
                    return;
                }
            };
            
            if bytes == 0 {
                break; // EOF
            }
            
            bytes_read += bytes;
            file_line_number += 1;
            
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
            
            let log_line = match parse_line(line_buffer.clone(), file_line_number) {
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
}

impl<'a> TabViewer for LogCrabTabViewer<'a> {
    type Tab = TabContent;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        (&tab.title).into()
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
                        self.log_view.set_bookmarks_file(path.clone());
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
                        self.status_message = "LogCrab - Log Anomaly Explorer v0.1.0".to_string();
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
                    });
            }
        });
        
        #[cfg(feature = "cpu-profiling")]
        {
            puffin::GlobalProfiler::lock().new_frame();
            
            if self.show_profiler {
                puffin_egui::profiler_window(ctx);
            }
        }
    }
}
