use super::windows;
use super::ToastManager;

use std::path::PathBuf;

use crate::config::GlobalConfig;
use crate::core::{GlobalFilterWorker, LogStore};
use crate::input::{KeyboardBindings, ShortcutAction};
use crate::ui::tabs::{BookmarksView, HighlightsView};
use crate::ui::CrabSession;
use egui::text::LayoutJob;
use egui::{Color32, Id, LayerId, Order, TextStyle};
use std::fmt::Write;

/// Main application
/// Responsibilities:
/// - Main window
/// - File loading
/// - Right now, scoring. Should be moved into `LogView`
/// - Keyboard shortcut processing
pub struct LogCrabApp {
    /// The main log view component
    session: Option<CrabSession>,

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

    /// Pending dropped files to load
    pending_drop_files: Vec<PathBuf>,

    /// Toast notification manager
    toast_manager: ToastManager,
}

impl LogCrabApp {
    pub fn new(cc: &eframe::CreationContext<'_>, file: Option<PathBuf>) -> Self {
        // Load global configuration
        let global_config = GlobalConfig::load();

        // Apply saved theme
        if global_config.bright_mode {
            cc.egui_ctx.set_visuals(egui::Visuals::light());
        } else {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
        }

        let mut app = Self {
            session: None,
            show_anomaly_explanation: false,
            show_shortcuts_window: false,
            shortcut_bindings: KeyboardBindings::load(&global_config),
            global_config,
            pending_rebind: None,
            pending_drop_files: Vec::new(),
            toast_manager: ToastManager::new(cc.egui_ctx.clone()),
        };

        // Load initial file if provided via command line
        if let Some(file) = file {
            if file.exists() {
                app.start_new_session();
                app.add_file_to_session(file, cc.egui_ctx.clone());
            } else {
                app.toast_manager
                    .show_error(format!("File not found: {}", file.display()));
            }
        }
        app
    }

    pub fn start_new_session(&mut self) {
        // Create a new store for this file
        let store = LogStore::new();
        self.session = Some(CrabSession::new(store));
    }

    /// Add a file to the current session
    fn add_file_to_session(&mut self, mut path: PathBuf, ctx: egui::Context) {
        if let Some(ref mut session) = self.session {
            // Check if this is a .crab session file
            if path.to_string_lossy().ends_with(".crab") {
                path = PathBuf::from(path.to_string_lossy().trim_end_matches(".crab"));

                if path.exists() {
                    log::info!("Loading log file from .crab session: {}", path.display());
                } else {
                    let err_msg = format!("File not found: {}", path.display());
                    log::error!("{err_msg}");
                    self.toast_manager.show_error(err_msg);
                    return;
                }
            }
            let file_name = path
                .file_name()
                .map_or_else(|| "file".to_string(), |n| n.to_string_lossy().to_string());
            let toast_handle = self
                .toast_manager
                .create_progress_toast(file_name, "Starting...");

            session.add_file(path, ctx, toast_handle);
        }
    }

    /// Show file dialog and load selected file
    fn open_file_dialog(&mut self, ctx: &egui::Context) {
        let mut dialog = rfd::FileDialog::new()
            .add_filter("Log Files", &["log", "txt", "dlt", "crab"])
            .add_filter("All Files", &["*"]);

        if let Some(ref dir) = self.global_config.last_log_directory {
            dialog = dialog.set_directory(dir);
        }

        if let Some(paths) = dialog.pick_files() {
            if let Some(first) = paths.first() {
                if let Some(parent) = first.parent() {
                    self.global_config.last_log_directory = Some(parent.to_path_buf());
                    let _ = self.global_config.save();
                }
            }

            self.start_new_session();
            for path in paths {
                self.add_file_to_session(path, ctx.clone());
            }
        }
    }

    /// Show file dialog and add selected file(s) to the current workspace
    fn add_file_dialog(&mut self, ctx: &egui::Context) {
        let mut dialog = rfd::FileDialog::new()
            .add_filter("Log Files", &["log", "txt", "dlt", "crab"])
            .add_filter("All Files", &["*"]);

        if let Some(ref dir) = self.global_config.last_log_directory {
            dialog = dialog.set_directory(dir);
        }

        if let Some(paths) = dialog.pick_files() {
            // Remember the directory from the first file
            if let Some(first) = paths.first() {
                if let Some(parent) = first.parent() {
                    self.global_config.last_log_directory = Some(parent.to_path_buf());
                    let _ = self.global_config.save();
                }
            }

            for path in paths {
                self.add_file_to_session(path, ctx.clone());
            }
        }
    }

    /// Process multiple dropped files
    /// - If no session exists, first log file is loaded as main file
    /// - If session exists, additional log files are added to the workspace
    /// - All .crab-filters files are imported
    fn process_dropped_files(&mut self, files: Vec<PathBuf>, ctx: &egui::Context) {
        let mut log_files: Vec<PathBuf> = Vec::new();
        let mut filter_files: Vec<PathBuf> = Vec::new();

        for path in files {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

            if ext == "crab-filters" {
                filter_files.push(path);
            } else {
                log_files.push(path);
            }
        }

        // If no session exists, the first file creates the session
        // Otherwise, all files are added to the existing workspace
        if self.session.is_none() {
            self.start_new_session();
        }

        for path in log_files {
            log::info!("Adding dropped file to workspace: {}", path.display());
            self.add_file_to_session(path, ctx.clone());
        }

        // Import filter files if we have a log view
        if !filter_files.is_empty() {
            if let Some(ref mut log_view) = self.session {
                for path in &filter_files {
                    log::info!("Importing dropped filter file: {}", path.display());
                    match log_view.import_filters(path) {
                        Ok(count) => {
                            log::info!("Imported {count} filters from {}", path.display());
                        }
                        Err(e) => {
                            log::error!("Failed to import filters from {}: {e}", path.display());
                            self.toast_manager.show_error(format!(
                                "Failed to import {}: {e}",
                                path.file_name().map_or_else(
                                    || "filters".to_string(),
                                    |n| n.to_string_lossy().to_string()
                                )
                            ));
                        }
                    }
                }
            } else {
                log::warn!(
                    "Cannot import filter files - no log file is open. Open a log file first."
                );
                self.toast_manager
                    .show_warning("Cannot import filters - open a log file first");
            }
        }
    }

    /// Render top menu bar
    fn render_menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.menu_button("File", |ui| {
            if ui.button("Open Log File...").clicked() {
                self.open_file_dialog(ctx);
                ui.close();
            }

            if self.session.is_some() && ui.button("Add File to session...").clicked() {
                self.add_file_dialog(ctx);
                ui.close();
            }

            ui.separator();

            if let Some(ref mut log_view) = &mut self.session {
                if ui.button("Export Filters...").clicked() {
                    let mut dialog = rfd::FileDialog::new()
                        .add_filter("Crab Filters", &["crab-filters"])
                        .add_filter("All Files", &["*"])
                        .set_file_name("filters.crab-filters");

                    if let Some(ref dir) = self.global_config.last_filters_directory {
                        dialog = dialog.set_directory(dir);
                    }

                    if let Some(path) = dialog.save_file() {
                        if let Some(parent) = path.parent() {
                            self.global_config.last_filters_directory = Some(parent.to_path_buf());
                            let _ = self.global_config.save();
                        }
                        match log_view.export_filters(&path) {
                            Ok(()) => log::info!("Filters exported successfully"),
                            Err(e) => log::error!("Failed to export filters: {e}"),
                        }
                    }
                    ui.close();
                }
                if ui.button("Import Filters...").clicked() {
                    let mut dialog = rfd::FileDialog::new()
                        .add_filter("Crab Filters", &["crab-filters"])
                        .add_filter("All Files", &["*"]);

                    if let Some(ref dir) = self.global_config.last_filters_directory {
                        dialog = dialog.set_directory(dir);
                    }

                    if let Some(paths) = dialog.pick_files() {
                        // Remember the directory from the first file
                        if let Some(first) = paths.first() {
                            if let Some(parent) = first.parent() {
                                self.global_config.last_filters_directory =
                                    Some(parent.to_path_buf());
                                let _ = self.global_config.save();
                            }
                        }
                        for path in paths {
                            match log_view.import_filters(&path) {
                                Ok(count) => {
                                    log::info!("Imported {count} filters from {}", path.display())
                                }
                                Err(e) => log::error!(
                                    "Failed to import filters from {}: {e}",
                                    path.display()
                                ),
                            }
                        }
                    }
                    ui.close();
                }
                ui.separator();
            }

            if ui.button("Quit").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });

        ui.menu_button("View", |ui| {
            if let Some(ref mut log_view) = &mut self.session {
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

                if ui.button("Add Highlights Tab").clicked() {
                    log_view
                        .dock_state
                        .push_to_focused_leaf(Box::new(HighlightsView::new()));
                    ui.close();
                }

                ui.separator();
            }

            if ui
                .checkbox(
                    &mut self.global_config.hide_epoch_in_histogram,
                    "Hide January 1st from Histogram",
                )
                .changed()
            {
                // Save config when changed
                if let Err(e) = self.global_config.save() {
                    log::error!("Failed to save config: {e}");
                }
            }

            ui.separator();

            if ui
                .checkbox(&mut self.global_config.bright_mode, "Bright Mode")
                .changed()
            {
                // Apply theme change
                if self.global_config.bright_mode {
                    ctx.set_visuals(egui::Visuals::light());
                } else {
                    ctx.set_visuals(egui::Visuals::dark());
                }
                // Save config when changed
                if let Err(e) = self.global_config.save() {
                    log::error!("Failed to save config: {e}");
                }
            }
        });

        ui.menu_button("Help", |ui| {
            if ui.button("Anomaly Score Calculation").clicked() {
                self.show_anomaly_explanation = true;
                ui.close();
            }
            if ui.button("Keyboard Shortcuts").clicked() {
                self.show_shortcuts_window = true;
                ui.close();
            }
        });
    }

    /// Render bottom status panel
    fn render_status_panel(ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
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
        profiling::scope!("central_panel");

        // Preview hovering files
        Self::preview_files_being_dropped(ctx);

        // Collect dropped files (store for later processing)
        ctx.input(|i| {
            for file in &i.raw.dropped_files {
                if let Some(path) = &file.path {
                    self.pending_drop_files.push(path.clone());
                }
            }
        });

        if let Some(ref mut log_view) = self.session {
            log_view.render(ui, &mut self.global_config);
        } else {
            ui.vertical_centered(|ui| {
                ui.add_space(100.0);
                ui.heading("Welcome to LogCrab 🦀");
                ui.add_space(20.0);
                ui.add_space(40.0);

                if ui.button("Open Log File").clicked() {
                    self.open_file_dialog(ctx);
                }
            });
        }
    }

    /// Preview hovering files - shows overlay when dragging files over window
    fn preview_files_being_dropped(ctx: &egui::Context) {
        if !ctx.input(|i| i.raw.hovered_files.is_empty()) {
            let text = ctx.input(|i| {
                let mut text = "Drop to open:\n".to_owned();
                for file in &i.raw.hovered_files {
                    if let Some(path) = &file.path {
                        let _ = write!(text, "\n{}", path.display());
                    }
                }
                text
            });

            let painter =
                ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("file_drop_target")));
            let screen_rect = ctx.content_rect();
            painter.rect_filled(screen_rect, 0.0, Color32::from_black_alpha(192));

            let font = TextStyle::Heading.resolve(&ctx.style());
            let mut layout_job =
                LayoutJob::simple(text, font, Color32::WHITE, screen_rect.width() - 40.0);
            layout_job.wrap.max_width = screen_rect.width() - 40.0;

            let galley = painter.layout_job(layout_job);
            let text_pos = screen_rect.center() - galley.rect.size() / 2.0;
            painter.galley(text_pos, galley, Color32::WHITE);
        }
    }

    /// Process keyboard shortcuts and execute actions
    fn process_keyboard_input(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        profiling::scope!("process_keyboard_input");
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

        if let Some(ref mut log_view) = self.session {
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
                    self.open_file_dialog(ctx);
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
        profiling::scope!("raw_input_hook");
        self.process_keyboard_input(ctx, raw_input);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        profiling::function_scope!();

        // Process pending dropped files
        if !self.pending_drop_files.is_empty() {
            profiling::scope!("process_dropped_files");
            let files = std::mem::take(&mut self.pending_drop_files);
            self.process_dropped_files(files, ctx);
        }

        {
            profiling::scope!("top_panel");
            egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    self.render_menu_bar(ui, ctx);
                });
            });
        }

        {
            profiling::scope!("bottom_panel");
            egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
                Self::render_status_panel(ui);
            });
        }

        {
            profiling::scope!("central_panel_show");
            egui::CentralPanel::default().show(ctx, |ui| {
                self.render_central_panel(ui, ctx);
            });
        }

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

        // Show toast notifications
        self.toast_manager.show(ctx);

        profiling::finish_frame!();
    }
}
