mod navigation;
mod tabs;
mod windows;

pub use tabs::{TabContent, TabType};

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use egui_dock::{DockArea, DockState, Node};

use crate::core::{LoadMessage, LogFileLoader};
use crate::input::{InputAction, KeyboardBindings, PaneDirection, ShortcutAction};
use crate::ui::LogView;

use tabs::LogCrabTabViewer;

/// Main application state
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

    /// Dock state for VS Code-like tiling layout
    dock_state: DockState<TabContent>,

    /// Whether to show the anomaly explanation window
    show_anomaly_explanation: bool,

    /// Request to add a tab after a specific node
    add_tab_after: Option<egui_dock::NodeIndex>,

    /// Whether to show the keyboard shortcuts window
    show_shortcuts_window: bool,

    /// Keyboard shortcut bindings
    shortcut_bindings: KeyboardBindings,

    /// Pending key rebind action
    pending_rebind: Option<ShortcutAction>,

    /// Request to focus search input in a specific filter tab
    focus_search_next_frame: Option<usize>,

    /// Request to create a new filter tab
    request_new_filter_tab: bool,

    /// Request to create a new bookmarks tab
    request_new_bookmarks_tab: bool,

    /// Request to navigate to a neighboring pane
    navigate_pane_direction: Option<PaneDirection>,

    /// Whether to show the CPU profiler window
    #[cfg(feature = "cpu-profiling")]
    show_profiler: bool,
}

impl LogCrabApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, file: Option<PathBuf>) -> Self {
        // Initialize dock state with two filter tabs by default
        let dock_state = DockState::new(vec![
            TabContent {
                tab_type: TabType::Filter(0),
                title: "Filter 1".to_string(),
            },
            TabContent {
                tab_type: TabType::Filter(1),
                title: "Filter 2".to_string(),
            },
        ]);

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
            show_shortcuts_window: false,
            shortcut_bindings: KeyboardBindings::default(), // Vim-style (j/k) by default
            pending_rebind: None,
            focus_search_next_frame: None,
            request_new_filter_tab: false,
            request_new_bookmarks_tab: false,
            navigate_pane_direction: None,
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

    /// Process background file loading messages
    fn process_file_loading(&mut self, _ctx: &egui::Context) {
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
                        self.status_message = format!(
                            "Loaded {} successfully with {} lines",
                            path.display(),
                            self.log_view.lines.len()
                        );
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
    }

    /// Render top menu bar
    fn render_menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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
            if ui.button("Keyboard Shortcuts").clicked() {
                self.show_shortcuts_window = true;
                ui.close_menu();
            }
        });

        #[cfg(feature = "cpu-profiling")]
        ui.menu_button("Profiling", |ui| {
            ui.checkbox(&mut self.show_profiler, "Show CPU Profiler");
        });
    }

    /// Render bottom status panel
    fn render_status_panel(&self, ui: &mut egui::Ui) {
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

        if self.log_view.lines.is_empty() {
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
            DockArea::new(&mut self.dock_state).show_inside(
                ui,
                &mut LogCrabTabViewer {
                    log_view: &mut self.log_view,
                    add_tab_after: &mut self.add_tab_after,
                    focus_search_next_frame: &mut self.focus_search_next_frame,
                },
            );
        }
    }

    /// Handle post-frame tab operations
    fn handle_tab_operations(&mut self) {
        // Handle add tab request
        if let Some(node) = self.add_tab_after.take() {
            let filter_index = self.log_view.filter_count();
            self.log_view.add_filter();
            self.dock_state
                .set_focused_node_and_surface((egui_dock::SurfaceIndex::main(), node));
            self.dock_state.push_to_focused_leaf(TabContent {
                tab_type: TabType::Filter(filter_index),
                title: format!("Filter {}", filter_index + 1),
            });
        }

        // Handle new filter tab request (Ctrl+T)
        if self.request_new_filter_tab {
            self.request_new_filter_tab = false;
            let filter_index = self.log_view.filter_count();
            self.log_view.add_filter();

            self.dock_state.push_to_focused_leaf(TabContent {
                tab_type: TabType::Filter(filter_index),
                title: format!("Filter {}", filter_index + 1),
            });

            // Focus the search input in the new tab
            self.focus_search_next_frame = Some(filter_index);
        }

        // Handle new bookmarks tab request
        if self.request_new_bookmarks_tab {
            self.request_new_bookmarks_tab = false;

            self.dock_state.push_to_focused_leaf(TabContent {
                tab_type: TabType::Bookmarks,
                title: "Bookmarks".to_string(),
            });
        }
    }

    /// Process keyboard shortcuts and execute actions
    fn process_keyboard_input(&mut self, ctx: &egui::Context) {
        // Global keyboard shortcut handling (navigation). Skip if a text input wants keyboard.
        if ctx.wants_keyboard_input() {
            return;
        }

        // Get the currently focused tab directly from dock state
        let focused_tab = self
            .dock_state
            .find_active_focused()
            .map(|(_, tab)| tab.tab_type.clone());
        let active_filter_index = match &focused_tab {
            Some(TabType::Filter(idx)) => Some(*idx),
            _ => None,
        };

        let actions = ctx.input(|i| {
            self.shortcut_bindings
                .process_input(i, &mut self.pending_rebind, active_filter_index)
        });

        // Execute all generated actions
        for action in actions {
            match action {
                InputAction::MoveSelection(delta) => {
                    if let Some(TabType::Filter(idx)) = focused_tab {
                        self.log_view.move_selection_in_filter(idx, delta);
                    }
                }
                InputAction::ToggleBookmark => {
                    self.log_view.toggle_bookmark_for_selected();
                }
                InputAction::FocusSearch(idx) => {
                    self.focus_search_next_frame = Some(idx);
                }
                InputAction::NewFilterTab => {
                    self.request_new_filter_tab = true;
                }
                InputAction::NewBookmarksTab => {
                    self.request_new_bookmarks_tab = true;
                }
                InputAction::CloseTab => {
                    // Close the currently focused/active tab (the one the user is viewing)
                    // focused_leaf() returns which pane has keyboard focus
                    if let Some((surface_idx, node_idx)) = self.dock_state.focused_leaf() {
                        let tree = &self.dock_state[surface_idx];
                        
                        // Each pane (leaf node) can have multiple tabs, but only one is "active" (visible).
                        // The 'active' field tells us which tab is currently displayed in this pane.
                        if let Node::Leaf { active, .. } = &tree[node_idx] {
                            self.dock_state.remove_tab((surface_idx, node_idx, *active));
                        }
                    }
                }
                InputAction::JumpToTop => {
                    if let Some(TabType::Filter(idx)) = focused_tab {
                        self.log_view.jump_to_top_in_filter(idx);
                    }
                }
                InputAction::JumpToBottom => {
                    if let Some(TabType::Filter(idx)) = focused_tab {
                        self.log_view.jump_to_bottom_in_filter(idx);
                    }
                }
                InputAction::NavigatePane(direction) => {
                    self.navigate_pane_direction = Some(direction);
                }
            }
        }
    }

    /// Handle pane navigation (Shift+HJKL)
    fn handle_pane_navigation(&mut self) {
        if let Some(direction) = self.navigate_pane_direction.take() {
            let tree = self.dock_state.main_surface_mut();

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
        self.process_file_loading(ctx);

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                self.render_menu_bar(ui, ctx);
            });
        });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            self.render_status_panel(ui);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_central_panel(ui, ctx);
        });

        // Handle post-frame operations
        self.handle_tab_operations();

        // Process keyboard input
        self.process_keyboard_input(ctx);

        // Handle pane navigation
        self.handle_pane_navigation();

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
