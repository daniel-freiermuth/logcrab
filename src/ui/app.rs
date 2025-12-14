use super::windows;
use super::ToastManager;

use std::path::PathBuf;

use crate::config::GlobalConfig;
use crate::core::{LogFileLoader, LogStore};
use crate::input::{KeyboardBindings, ShortcutAction};
use crate::ui::tabs::filter_tab::filter_state::GlobalFilterWorker;
use crate::ui::tabs::BookmarksView;
use crate::ui::LogView;
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
    log_view: Option<LogView>,

    /// Currently loaded file path
    current_file: Option<PathBuf>,

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

    /// Pending dropped file to load
    pending_drop_file: Option<PathBuf>,

    /// Toast notification manager
    toast_manager: ToastManager,

    /// Whether to show the CPU profiler window
    #[cfg(feature = "cpu-profiling")]
    show_profiler: bool,
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
            log_view: None,
            current_file: None,
            show_anomaly_explanation: false,
            show_shortcuts_window: false,
            shortcut_bindings: KeyboardBindings::load(&global_config),
            global_config,
            pending_rebind: None,
            pending_drop_file: None,
            toast_manager: ToastManager::new(cc.egui_ctx.clone()),
            #[cfg(feature = "cpu-profiling")]
            show_profiler: false,
        };

        // Load initial file if provided via command line
        if let Some(file) = file {
            if file.exists() {
                app.load_file(file, cc.egui_ctx.clone());
            } else {
                app.toast_manager
                    .show_error(format!("File not found: {}", file.display()));
            }
        }
        app
    }

    pub fn load_file(&mut self, mut path: PathBuf, ctx: egui::Context) {
        // Check if this is a .crab session file
        if path.to_string_lossy().ends_with(".crab") {
            path = PathBuf::from(path.to_string_lossy().trim_end_matches(".crab"));

            if path.exists() {
                log::info!("Loading log file from .crab session: {}", path.display());
            } else {
                let err_msg = format!("File not found: {}", path.display());
                log::error!("{}", err_msg);
                self.toast_manager.show_error(err_msg);
                return;
            }
        }

        self.current_file = Some(path.clone());
        self.update_window_title(&ctx);

        // Create a new store for this file
        let store = LogStore::new();

        // Create a progress toast handle and pass it to the loader
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());
        let toast_handle = self
            .toast_manager
            .create_progress_toast(file_name, "Starting...");

        let source = LogFileLoader::load_async(path.clone(), ctx, toast_handle);
        store.add_source(source);

        // Create LogView immediately - it will show lines as they stream in
        let mut crab_path = path.clone();
        crab_path.set_file_name(format!(
            "{}.crab",
            path.file_name().unwrap().to_string_lossy()
        ));
        self.log_view = Some(LogView::new(store, crab_path));
    }

    /// Show file dialog and load selected file
    fn open_file_dialog(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Log Files", &["log", "txt", "dlt", "crab"])
            .add_filter("All Files", &["*"])
            .pick_file()
        {
            self.load_file(path, ctx.clone());
        }
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

    /// Render top menu bar
    fn render_menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.menu_button("File", |ui| {
            if ui.button("Open Log File...").clicked() {
                self.open_file_dialog(ctx);
                ui.close();
            }

            ui.separator();

            if let Some(ref mut log_view) = &mut self.log_view {
                if ui.button("Export Filters...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Crab Filters", &["crab-filters"])
                        .add_filter("All Files", &["*"])
                        .set_file_name("filters.crab-filters")
                        .save_file()
                    {
                        match log_view.export_filters(&path) {
                            Ok(()) => log::info!("Filters exported successfully"),
                            Err(e) => log::error!("Failed to export filters: {e}"),
                        }
                    }
                    ui.close();
                }

                if ui.button("Import Filters...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Crab Filters", &["crab-filters"])
                        .add_filter("All Files", &["*"])
                        .pick_file()
                    {
                        match log_view.import_filters(&path) {
                            Ok(count) => log::info!("Successfully imported {count} filters"),
                            Err(e) => log::error!("Failed to import filters: {e}"),
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
                    log::error!("Failed to save config: {}", e);
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
                    log::error!("Failed to save config: {}", e);
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

        #[cfg(feature = "cpu-profiling")]
        ui.menu_button("Profiling", |ui| {
            ui.checkbox(&mut self.show_profiler, "Show CPU Profiler");
        });
    }

    /// Render bottom status panel
    fn render_status_panel(&mut self, ui: &mut egui::Ui) {
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
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_scope!("central_panel");

        // Preview hovering files
        Self::preview_files_being_dropped(ctx);

        // Collect dropped files (store for later processing)
        self.pending_drop_file = ctx.input(|i| {
            if let Some(f) = i.raw.dropped_files.first() {
                f.path.clone()
            } else {
                None
            }
        });

        if let Some(ref mut log_view) = self.log_view {
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
        self.process_keyboard_input(ctx, raw_input);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(feature = "cpu-profiling")]
        puffin::profile_function!();

        // Process pending dropped file
        if let Some(path) = self.pending_drop_file.take() {
            log::info!("Loading dropped file: {}", path.display());
            self.load_file(path, ctx.clone());
        }

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

        // Show toast notifications
        self.toast_manager.show(ctx);

        #[cfg(feature = "cpu-profiling")]
        {
            puffin::GlobalProfiler::lock().new_frame();

            if self.show_profiler {
                puffin_egui::profiler_window(ctx);
            }
        }
    }
}
