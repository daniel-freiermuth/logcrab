use super::windows;

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use crate::config::GlobalConfig;
use crate::core::{LoadMessage, LogFileLoader};
use crate::input::{KeyboardBindings, ShortcutAction};
use crate::ui::tabs::filter_tab::filter_state::GlobalFilterWorker;
use crate::ui::tabs::BookmarksView;
use crate::ui::LogView;

/// Main application
/// Responsibilities:
/// - Main window
/// - File loading
/// - Right now, scoring. Should be moved into LogView
/// - Keyboard shortcut processing
pub struct LogCrabApp {
    /// The main log view component
    log_view: Option<LogView>,

    /// Currently loaded file path
    current_file: Option<PathBuf>,

    /// Status message shown in the bottom panel
    status_message: String,

    /// Whether a file is currently being loaded
    is_loading: bool,

    /// Progress of the current load operation (0.0 to 1.0)
    load_progress: f32,

    /// Receiver for background loading messages
    load_receiver: Option<Receiver<LoadMessage>>,

    /// Initial file to load from command line
    initial_file: Option<PathBuf>,

    /// Whether to show the anomaly explanation window
    show_anomaly_explanation: bool,

    /// Whether to show the keyboard shortcuts window
    show_shortcuts_window: bool,

    /// Global configuration (shortcuts, favorites, etc.)
    global_config: GlobalConfig,

    /// Keyboard shortcut bindings
    shortcut_bindings: KeyboardBindings,

    /// Pending key rebind action
    pending_rebind: Option<ShortcutAction>,

    /// Whether to show the CPU profiler window
    #[cfg(feature = "cpu-profiling")]
    show_profiler: bool,
}

impl LogCrabApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, file: Option<PathBuf>) -> Self {
        // Load global configuration
        let global_config = GlobalConfig::load();

        LogCrabApp {
            log_view: None,
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
            show_anomaly_explanation: false,
            show_shortcuts_window: false,
            shortcut_bindings: KeyboardBindings::load(&global_config),
            global_config,
            pending_rebind: None,
            #[cfg(feature = "cpu-profiling")]
            show_profiler: false,
        }
    }

    pub fn load_file(&mut self, path: PathBuf, ctx: egui::Context) {
        self.log_view = None;
        self.current_file = Some(path.clone());
        self.update_window_title(&ctx);
        self.is_loading = true;
        self.load_progress = 0.0;
        self.status_message = format!("Loading {}...", path.display());

        let rx = LogFileLoader::load_async(path, ctx);
        self.load_receiver = Some(rx);
    }

    /// Update window title to show current file
    fn update_window_title(&self, ctx: &egui::Context) {
        let title = if let Some(ref path) = self.current_file {
            format!(
                "LogCrab - {}",
                path.file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
            )
        } else {
            "LogCrab - Log Anomaly Explorer".to_string()
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }

    /// Process background file loading messages
    fn process_file_loading(&mut self, ctx: &egui::Context) {
        let mut should_clear_receiver = false;
        while let Some(msg) = self
            .load_receiver
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            match msg {
                LoadMessage::Progress(progress, status) => {
                    self.load_progress = progress;
                    self.status_message = status;
                }
                LoadMessage::Complete(lines, path) => {
                    let n_lines = lines.len();
                    self.status_message = format!(
                        "Loaded {} lines - calculating anomaly scores in background...",
                        n_lines
                    );
                    self.is_loading = false;
                    self.load_progress = 0.0;

                    if n_lines > 0 {
                        let crab_path = path.with_extension("crab");
                        self.log_view = Some(LogView::new(lines, crab_path));
                    }
                    self.update_window_title(ctx);
                    // Keep receiver open for scoring progress
                }
                LoadMessage::ScoringProgress(progress, status) => {
                    self.load_progress = progress;
                    self.status_message = status;
                }
                LoadMessage::ScoringComplete(lines) => {
                    let n_lines = lines.len();
                    if let Some(ref mut log_view) = self.log_view {
                        log_view.state.lines = lines;
                    }
                    self.status_message =
                        format!("Ready. {} lines loaded with anomaly scores", n_lines);
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
        if should_clear_receiver {
            self.load_receiver = None;
        }
    }

    /// Render top menu bar
    fn render_menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.menu_button("File", |ui| {
            if ui.button("Open Log File...").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Log Files", &["log", "txt", "dlt"])
                    .add_filter("All Files", &["*"])
                    .pick_file()
                {
                    self.load_file(path, ctx.clone());
                }
                ui.close();
            }

            if ui.button("Quit").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });

        ui.menu_button("View", |ui| {
            if let Some(ref mut log_view) = &mut self.log_view {
                if ui.button("Add Filter Tab").clicked() {
                    log_view.add_filter_view(false, None);
                    ui.close();
                }

                if ui.button("Add Bookmarks Tab").clicked() {
                    log_view
                        .dock_state
                        .push_to_focused_leaf(Box::new(BookmarksView::default()));
                    ui.close();
                }
            }
        });

        ui.menu_button("Help", |ui| {
            if ui.button("About").clicked() {
                self.status_message = "LogCrab 🦀 - Log Anomaly Explorer v0.1.0".to_string();
                ui.close();
            }

            if ui.button("Anomaly Score Calculation").clicked() {
                self.show_anomaly_explanation = true;
                ui.close();
            }
            if ui.button("Keyboard Shortcuts").clicked() {
                self.show_shortcuts_window = true;
                ui.close();
            }
        });

        #[cfg(feature = "cpu-profiling")]
        ui.menu_button("Profiling", |ui| {
            ui.checkbox(&mut self.show_profiler, "Show CPU Profiler");
        });
    }

    /// Render bottom status panel
    fn render_status_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(&self.status_message);

            if self.is_loading {
                ui.separator();
                let progress_bar = egui::ProgressBar::new(self.load_progress).show_percentage();
                ui.add(progress_bar);
            }

            // Show filtering indicator if any filter is currently processing
            if GlobalFilterWorker::get()
                .is_filtering
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                ui.separator();
                ui.spinner();
                ui.label("Filtering...");
            }
        });
    }

    /// Render central content area with dock layout
    fn render_central_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_scope!("central_panel");

        if let Some(ref mut log_view) = self.log_view {
            log_view.render(ui, &mut self.global_config);
        } else {
            ui.vertical_centered(|ui| {
                ui.add_space(100.0);
                ui.heading("Welcome to LogCrab 🦀");
                ui.add_space(20.0);
                ui.add_space(40.0);

                if self.is_loading {
                    // Show prominent loading indicator instead of button
                    ui.add_space(20.0);
                    ui.spinner();
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(&self.status_message)
                            .size(16.0)
                            .strong(),
                    );
                    ui.add_space(10.0);
                    let progress_bar = egui::ProgressBar::new(self.load_progress)
                        .show_percentage()
                        .desired_width(400.0);
                    ui.add(progress_bar);
                } else if ui.button("Open Log File").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Log Files", &["log", "txt", "dlt"])
                        .add_filter("All Files", &["*"])
                        .pick_file()
                    {
                        self.load_file(path, ctx.clone());
                    }
                }
            });
        }
    }

    /// Process keyboard shortcuts and execute actions
    fn process_keyboard_input(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // Skip keyboard shortcuts if text input is focused AND no modifiers are pressed
        // This allows shortcuts like Ctrl+w to work even in text fields
        let has_modifiers = raw_input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key { modifiers, .. } if modifiers.ctrl || modifiers.alt || modifiers.command
            )
        });

        if ctx.wants_keyboard_input() && !has_modifiers {
            return;
        }

        let (actions, events_to_remove, shortcuts_changed) = self
            .shortcut_bindings
            .process_input(raw_input, &mut self.pending_rebind);

        // Save shortcuts if they were changed
        if shortcuts_changed {
            self.shortcut_bindings
                .save_to_config(&mut self.global_config);
            let _ = self.global_config.save();
        }

        if let Some(ref mut log_view) = self.log_view {
            log_view.process_keyboard_input(&actions);
        }

        for action in actions {
            match action {
                ShortcutAction::ToggleBookmark => {}
                ShortcutAction::FocusSearch => {}
                ShortcutAction::NewFilterTab => {}
                ShortcutAction::NewBookmarksTab => {}
                ShortcutAction::CloseTab => {}
                ShortcutAction::CycleTab => {}
                ShortcutAction::ReverseCycleTab => {}
                ShortcutAction::JumpToTop => {}
                ShortcutAction::JumpToBottom => {}
                ShortcutAction::PageUp => {}
                ShortcutAction::PageDown => {}
                ShortcutAction::OpenFile => {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("log", &["log", "txt", "dlt"])
                        .pick_file()
                    {
                        self.load_file(path, ctx.clone());
                    }
                }
                ShortcutAction::RenameFilter => {}
                ShortcutAction::MoveUp => {}
                ShortcutAction::MoveDown => {}
                ShortcutAction::FocusPaneLeft => {}
                ShortcutAction::FocusPaneDown => {}
                ShortcutAction::FocusPaneUp => {}
                ShortcutAction::FocusPaneRight => {}
            }
        }

        // Remove consumed events in reverse order
        for idx in events_to_remove.into_iter().rev() {
            raw_input.events.remove(idx);
        }
    }
}

impl eframe::App for LogCrabApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.process_keyboard_input(ctx, raw_input);
    }

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
        self.process_file_loading(ctx);

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                self.render_menu_bar(ui, ctx);
            });
        });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            self.render_status_panel(ui);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_central_panel(ui, ctx);
        });

        // Show windows
        if self.show_anomaly_explanation {
            windows::render_anomaly_explanation(ctx, &mut self.show_anomaly_explanation);
        }

        if self.show_shortcuts_window {
            windows::render_shortcuts_window(
                ctx,
                &mut self.show_shortcuts_window,
                &mut self.shortcut_bindings,
                &mut self.pending_rebind,
                &mut self.global_config,
            );
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
