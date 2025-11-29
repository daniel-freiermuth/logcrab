use super::tabs::navigation;
use super::windows;

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use egui_dock::Node;

use crate::config::GlobalConfig;
use crate::core::{LoadMessage, LogFileLoader};
use crate::input::{KeyboardBindings, ShortcutAction};
use crate::ui::tabs::{BookmarksView, PendingTabAdd};
use crate::ui::{LogView, PaneDirection};

/// Main application
pub struct LogCrabApp {
    /// The main log view component
    log_view: LogView,

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

    /// Pending tab add request (set by add button callback)
    pending_tab_add: Option<PendingTabAdd>,

    /// Whether to show the CPU profiler window
    #[cfg(feature = "cpu-profiling")]
    show_profiler: bool,
}

impl LogCrabApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, file: Option<PathBuf>) -> Self {
        // Load global configuration
        let global_config = GlobalConfig::load();

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
            show_anomaly_explanation: false,
            show_shortcuts_window: false,
            shortcut_bindings: KeyboardBindings::load(&global_config),
            global_config,
            pending_rebind: None,
            pending_tab_add: None,
            #[cfg(feature = "cpu-profiling")]
            show_profiler: false,
        }
    }

    pub fn load_file(&mut self, path: PathBuf, ctx: egui::Context) {
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
                    self.log_view.set_lines(lines);
                    self.log_view.set_bookmarks_file(path.clone());

                    self.current_file = Some(path.clone());
                    self.update_window_title(ctx);
                    self.status_message = format!(
                        "Loaded {} lines - calculating anomaly scores in background...",
                        self.log_view.state.lines.len()
                    );
                    self.is_loading = false;
                    self.load_progress = 0.0;
                    // Keep receiver open for scoring progress
                }
                LoadMessage::ScoringProgress(progress, status) => {
                    self.load_progress = progress;
                    self.status_message = status;
                }
                LoadMessage::ScoringComplete(lines) => {
                    self.log_view.set_lines(lines);
                    self.status_message = format!(
                        "Ready. {} lines loaded with anomaly scores",
                        self.log_view.state.lines.len()
                    );
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
            if ui.button("Add Filter Tab").clicked() {
                self.log_view.add_filter_view(false, None);
                ui.close();
            }

            if ui.button("Add Bookmarks Tab").clicked() {
                self.log_view
                    .dock_state
                    .push_to_focused_leaf(Box::new(BookmarksView::default()));
                ui.close();
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
            if self.log_view.is_any_filter_active() {
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

        if self.log_view.state.lines.is_empty() {
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
        } else {
            self.log_view
                .render(ui, &mut self.global_config, &mut self.pending_tab_add);
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
        self.log_view.process_keyboard_input(&actions);

        // Execute all generated actions
        for action in actions {
            match action {
                ShortcutAction::ToggleBookmark => {}
                ShortcutAction::FocusSearch => {}
                ShortcutAction::NewFilterTab => {
                    self.log_view.add_filter_view(true, None);
                }
                ShortcutAction::NewBookmarksTab => {
                    self.log_view
                        .dock_state
                        .push_to_focused_leaf(Box::new(BookmarksView::default()));
                }
                ShortcutAction::CloseTab => {
                    // Close the currently focused/active tab (the one the user is viewing)
                    // focused_leaf() returns which pane has keyboard focus
                    if let Some((surface_idx, node_idx)) = self.log_view.dock_state.focused_leaf() {
                        let tree = &self.log_view.dock_state[surface_idx];

                        // Each pane (leaf node) can have multiple tabs, but only one is "active" (visible).
                        // Get the active tab index from the leaf node
                        if let Node::Leaf(leaf) = &tree[node_idx] {
                            let active = leaf.active;
                            self.log_view
                                .dock_state
                                .remove_tab((surface_idx, node_idx, active));
                        }
                    }
                }
                ShortcutAction::CycleTab => {
                    // Cycle to the next tab in the active pane
                    if let Some((surface_idx, node_idx)) = self.log_view.dock_state.focused_leaf() {
                        let surface = &mut self.log_view.dock_state[surface_idx];

                        // Get the number of tabs and current active tab
                        if let Node::Leaf(leaf) = &mut surface[node_idx] {
                            let tab_count = leaf.tabs.len();
                            if tab_count > 1 {
                                let active = leaf.active;
                                // Cycle to next tab (wrap around to 0 if at the end)
                                let next_tab = (active.0 + 1) % tab_count;
                                leaf.active = egui_dock::TabIndex(next_tab);
                            }
                        }
                    }
                }
                ShortcutAction::ReverseCycleTab => {
                    // Cycle to the previous tab in the active pane
                    if let Some((surface_idx, node_idx)) = self.log_view.dock_state.focused_leaf() {
                        let surface = &mut self.log_view.dock_state[surface_idx];

                        // Get the number of tabs and current active tab
                        if let Node::Leaf(leaf) = &mut surface[node_idx] {
                            let tab_count = leaf.tabs.len();
                            if tab_count > 1 {
                                let active = leaf.active;
                                // Cycle to previous tab (wrap around to last if at the beginning)
                                let prev_tab = if active.0 == 0 {
                                    tab_count - 1
                                } else {
                                    active.0 - 1
                                };
                                leaf.active = egui_dock::TabIndex(prev_tab);
                            }
                        }
                    }
                }
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
                ShortcutAction::FocusPaneLeft => self.navigate_pane(PaneDirection::Left),
                ShortcutAction::FocusPaneDown => self.navigate_pane(PaneDirection::Down),
                ShortcutAction::FocusPaneUp => self.navigate_pane(PaneDirection::Up),
                ShortcutAction::FocusPaneRight => self.navigate_pane(PaneDirection::Right),
            }
        }

        // Remove consumed events in reverse order
        for idx in events_to_remove.into_iter().rev() {
            raw_input.events.remove(idx);
        }
    }

    fn navigate_pane(&mut self, direction: PaneDirection) {
        let tree = self.log_view.dock_state.main_surface_mut();

        // Get the currently focused node
        if let Some(current_node) = tree.focused_leaf() {
            // Find the neighbor in the specified direction
            let neighbor = navigation::find_neighbor(tree, current_node, direction);

            // If we found a neighbor, focus it
            if let Some(neighbor_idx) = neighbor {
                tree.set_focused_node(neighbor_idx);
            }
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

        // Update window title to show current file (if any)
        self.update_window_title(ctx);

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

        // Handle tab addition from add button popup (must be done after DockArea)
        if let Some(tab_type) = self.pending_tab_add.take() {
            match tab_type {
                PendingTabAdd::Filter => {
                    self.log_view.add_filter_view(false, None);
                }
                PendingTabAdd::Bookmarks => {
                    self.log_view
                        .dock_state
                        .push_to_focused_leaf(Box::new(BookmarksView::default()));
                }
            }
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

        #[cfg(feature = "cpu-profiling")]
        {
            puffin::GlobalProfiler::lock().new_frame();

            if self.show_profiler {
                puffin_egui::profiler_window(ctx);
            }
        }
    }
}
