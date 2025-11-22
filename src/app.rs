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
use crate::parser::line::LogLine;
use crate::ui::LogView;
use crate::core::{LogFileLoader, LoadMessage};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use egui_dock::{DockArea, DockState, TabViewer};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum TabType {
    Filter(usize),
    Bookmarks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ShortcutAction {
    MoveUp,
    MoveDown,
    ToggleBookmark,
    FocusSearch,
    NewFilterTab,
    CloseTab,
    JumpToTop,
    JumpToBottom,
    FocusPaneLeft,
    FocusPaneDown,
    FocusPaneUp,
    FocusPaneRight,
}

impl ShortcutAction {
    fn name(&self) -> &'static str {
        match self {
            ShortcutAction::MoveUp => "Move Selection Up",
            ShortcutAction::MoveDown => "Move Selection Down",
            ShortcutAction::ToggleBookmark => "Toggle Bookmark",
            ShortcutAction::FocusSearch => "Focus Search Input",
            ShortcutAction::NewFilterTab => "New Filter Tab",
            ShortcutAction::CloseTab => "Close Current Tab",
            ShortcutAction::JumpToTop => "Jump to Top",
            ShortcutAction::JumpToBottom => "Jump to Bottom",
            ShortcutAction::FocusPaneLeft => "Focus Pane Left",
            ShortcutAction::FocusPaneDown => "Focus Pane Down",
            ShortcutAction::FocusPaneUp => "Focus Pane Up",
            ShortcutAction::FocusPaneRight => "Focus Pane Right",
        }
    }
    
    fn description(&self) -> &'static str {
        match self {
            ShortcutAction::MoveUp => "Move to the previous log line in the active view",
            ShortcutAction::MoveDown => "Move to the next log line in the active view",
            ShortcutAction::ToggleBookmark => "Add or remove a bookmark on the selected line",
            ShortcutAction::FocusSearch => "Jump to the search input field (filter tabs only). Press Enter to return focus to logs.",
            ShortcutAction::NewFilterTab => "Create a new filter tab with search focused",
            ShortcutAction::CloseTab => "Close the currently active tab (filter tabs only)",
            ShortcutAction::JumpToTop => "Jump to the first log line (Vim-style: gg)",
            ShortcutAction::JumpToBottom => "Jump to the last log line (Vim-style: G)",
            ShortcutAction::FocusPaneLeft => "Move focus to the pane on the left (Vim-style: Shift+H)",
            ShortcutAction::FocusPaneDown => "Move focus to the pane below (Vim-style: Shift+J)",
            ShortcutAction::FocusPaneUp => "Move focus to the pane above (Vim-style: Shift+K)",
            ShortcutAction::FocusPaneRight => "Move focus to the pane on the right (Vim-style: Shift+L)",
        }
    }
}

struct ShortcutBindings {
    move_up: egui::KeyboardShortcut,
    move_down: egui::KeyboardShortcut,
    toggle_bookmark: egui::KeyboardShortcut,
    focus_search: egui::KeyboardShortcut,
    new_filter_tab: egui::KeyboardShortcut,
    close_tab: egui::KeyboardShortcut,
    focus_pane_left: egui::KeyboardShortcut,
    focus_pane_down: egui::KeyboardShortcut,
    focus_pane_up: egui::KeyboardShortcut,
    focus_pane_right: egui::KeyboardShortcut,
}

impl Default for ShortcutBindings {
    fn default() -> Self {
        ShortcutBindings {
            move_up: egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::K),             // Vim-style by default
            move_down: egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::J),           // Vim-style by default
            toggle_bookmark: egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Space), // Space by default
            focus_search: egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::L),         // Ctrl+L by default
            new_filter_tab: egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::T),       // Ctrl+T by default
            close_tab: egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::W),            // Ctrl+W by default
            focus_pane_left: egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::H),      // Vim-style by default
            focus_pane_down: egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::J),     // Shift+J by default
            focus_pane_up: egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::K),       // Shift+K by default
            focus_pane_right: egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::L),     // Vim-style by default
        }
    }
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
    // Keyboard shortcut / input state
    active_tab: Option<TabType>,
    show_shortcuts_window: bool,
    shortcut_bindings: ShortcutBindings,
    pending_rebind: Option<ShortcutAction>,
    focus_search_next_frame: Option<usize>,  // Filter index to focus search input on next render
    request_new_filter_tab: bool,  // Request to create a new filter tab
    close_active_tab: bool,  // Request to close the currently active tab
    last_g_press_time: Option<std::time::Instant>,  // Track when 'g' was last pressed for gg detection
    navigate_pane_direction: Option<PaneDirection>,  // Direction to navigate between panes
    #[cfg(feature = "cpu-profiling")]
    show_profiler: bool,
}

#[derive(Debug, Clone, Copy)]
enum PaneDirection {
    Left,
    Right,
    Up,
    Down,
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
            active_tab: None,
            show_shortcuts_window: false,
            shortcut_bindings: ShortcutBindings::default(), // Vim-style (j/k) by default
            pending_rebind: None,
            focus_search_next_frame: None,
            request_new_filter_tab: false,
            close_active_tab: false,
            last_g_press_time: None,
            navigate_pane_direction: None,
            #[cfg(feature = "cpu-profiling")]
            show_profiler: false,
        }
    }
    
    /// Find a neighboring leaf node in the specified direction
    fn find_neighbor(tree: &egui_dock::Tree<TabContent>, current: egui_dock::NodeIndex, direction: PaneDirection) -> Option<egui_dock::NodeIndex> {
        let current_rect = tree[current].rect()?;
        
        // Track best candidate: (NodeIndex, distance, overlap)
        let mut best: Option<(egui_dock::NodeIndex, f32, f32)> = None;
        
        for idx_usize in 0..tree.len() {
            let idx = egui_dock::NodeIndex::from(idx_usize);
            
            if idx == current {
                continue;
            }
            
            let node = &tree[idx];
            if !node.is_leaf() {
                continue;
            }
            
            let candidate_rect = match node.rect() {
                Some(r) => r,
                None => continue,
            };
            
            // Check if candidate is in the correct direction and calculate distance and overlap
            let (interesting_candidate, distance, overlap) = match direction {
                PaneDirection::Left => {
                    // Must be entirely to the left
                    if candidate_rect.max.x > current_rect.min.x {
                        (false, 0.0, 0.0)
                    } else {
                        // Require vertical overlap
                        let overlap = (candidate_rect.max.y.min(current_rect.max.y))
                            - (candidate_rect.min.y.max(current_rect.min.y));
                        if overlap <= 0.0 {
                            (false, 0.0, 0.0)
                        } else {
                            (true, current_rect.min.x - candidate_rect.max.x, overlap)
                        }
                    }
                }
                PaneDirection::Right => {
                    // Must be entirely to the right
                    if candidate_rect.min.x < current_rect.max.x {
                        (false, 0.0, 0.0)
                    } else {
                        // Require vertical overlap
                        let overlap = (candidate_rect.max.y.min(current_rect.max.y))
                            - (candidate_rect.min.y.max(current_rect.min.y));
                        if overlap <= 0.0 {
                            (false, 0.0, 0.0)
                        } else {
                            (true, candidate_rect.min.x - current_rect.max.x, overlap)
                        }
                    }
                }
                PaneDirection::Up => {
                    // Must be entirely above
                    if candidate_rect.max.y > current_rect.min.y {
                        (false, 0.0, 0.0)
                    } else {
                        // Require horizontal overlap
                        let overlap = (candidate_rect.max.x.min(current_rect.max.x))
                            - (candidate_rect.min.x.max(current_rect.min.x));
                        if overlap <= 0.0 {
                            (false, 0.0, 0.0)
                        } else {
                            (true, current_rect.min.y - candidate_rect.max.y, overlap)
                        }
                    }
                }
                PaneDirection::Down => {
                    // Must be entirely below
                    if candidate_rect.min.y < current_rect.max.y {
                        (false, 0.0, 0.0)
                    } else {
                        // Require horizontal overlap
                        let overlap = (candidate_rect.max.x.min(current_rect.max.x))
                            - (candidate_rect.min.x.max(current_rect.min.x));
                        if overlap <= 0.0 {
                            (false, 0.0, 0.0)
                        } else {
                            (true, candidate_rect.min.y - current_rect.max.y, overlap)
                        }
                    }
                }
            };
            
            if !interesting_candidate {
                continue;
            }
            
            // Pick the best candidate: closest distance, then most overlap as tiebreaker
            let is_better = best.map_or(true, |(_, best_dist, best_overlap)| {
                const EPSILON: f32 = 1e-6;
                if (distance - best_dist).abs() < EPSILON {
                    // Distances are essentially equal, use overlap as tiebreaker
                    overlap > best_overlap
                } else {
                    distance < best_dist
                }
            });
            
            if is_better {
                best = Some((idx, distance, overlap));
            }
        }
        
        best.map(|(idx, _, _)| idx)
    }
    
    pub fn load_file(&mut self, path: PathBuf, ctx: egui::Context) {
        self.is_loading = true;
        self.load_progress = 0.0;
        self.status_message = format!("Loading {}...", path.display());
        
        let rx = LogFileLoader::load_async(path, ctx);
        self.load_receiver = Some(rx);
    }
}

// TabViewer implementation for dock system
struct LogCrabTabViewer<'a> {
    log_view: &'a mut LogView,
    add_tab_after: &'a mut Option<egui_dock::NodeIndex>,
    active_tab: &'a mut Option<TabType>,
    focus_search_next_frame: &'a mut Option<usize>,
    close_active_tab: &'a mut bool,
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
    
    fn force_close(&mut self, tab: &mut Self::Tab) -> bool {
        // Close the tab if it's the active tab and close_active_tab flag is set
        if *self.close_active_tab {
            if let Some(ref active) = self.active_tab {
                if &tab.tab_type == active {
                    // Clear both the active tab and the close flag
                    *self.active_tab = None;
                    *self.close_active_tab = false;
                    return true;
                }
            }
        }
        false
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        // Render content
        match &tab.tab_type {
            TabType::Filter(index) => {
                // If Ctrl+L was pressed for this filter, set flag before rendering
                if *self.focus_search_next_frame == Some(*index) {
                    self.log_view.focus_search_input(*index);
                    *self.focus_search_next_frame = None;
                }
                
                self.log_view.render_filter(ui, *index);
            }
            TabType::Bookmarks => {
                self.log_view.render_bookmarks(ui);
            }
        }
        
        // CRITICAL: Only update active_tab if the pointer is CURRENTLY in this UI's bounds
        // AND a click/press just happened in this frame
        // This prevents the last-rendered-tab from always winning
        if ui.ui_contains_pointer() {
            if ui.input(|i| i.pointer.any_pressed()) {
                *self.active_tab = Some(tab.tab_type.clone());
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
                        active_tab: &mut self.active_tab,
                        focus_search_next_frame: &mut self.focus_search_next_frame,
                        close_active_tab: &mut self.close_active_tab,
                    });
                
                // If a tab was just closed, update active_tab to the currently focused tab
                if self.active_tab.is_none() {
                    if let Some((_, tab)) = self.dock_state.find_active_focused() {
                        self.active_tab = Some(tab.tab_type.clone());
                    }
                }
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
        
        // Global keyboard shortcut handling (navigation). Skip if a text input wants keyboard.
        if !ctx.wants_keyboard_input() {
            ctx.input(|i| {
                let mut move_delta: i32 = 0;
                
                // If rebinding in progress, capture first pressed key or keyboard shortcut
                if let Some(action) = self.pending_rebind {
                    if let Some(event_key) = i.events.iter().find_map(|e| match e { 
                        egui::Event::Key { key, pressed: true, .. } => Some(*key), 
                        _ => None 
                    }) {
                        // Capture modifiers + key for all actions
                        let shortcut = egui::KeyboardShortcut::new(i.modifiers, event_key);
                        match action {
                            ShortcutAction::MoveUp => self.shortcut_bindings.move_up = shortcut,
                            ShortcutAction::MoveDown => self.shortcut_bindings.move_down = shortcut,
                            ShortcutAction::ToggleBookmark => self.shortcut_bindings.toggle_bookmark = shortcut,
                            ShortcutAction::FocusSearch => self.shortcut_bindings.focus_search = shortcut,
                            ShortcutAction::NewFilterTab => self.shortcut_bindings.new_filter_tab = shortcut,
                            ShortcutAction::CloseTab => self.shortcut_bindings.close_tab = shortcut,
                            ShortcutAction::FocusPaneLeft => self.shortcut_bindings.focus_pane_left = shortcut,
                            ShortcutAction::FocusPaneDown => self.shortcut_bindings.focus_pane_down = shortcut,
                            ShortcutAction::FocusPaneUp => self.shortcut_bindings.focus_pane_up = shortcut,
                            ShortcutAction::FocusPaneRight => self.shortcut_bindings.focus_pane_right = shortcut,
                            // JumpToTop and JumpToBottom are hardcoded (gg/G), not rebindable
                            ShortcutAction::JumpToTop | ShortcutAction::JumpToBottom => {}
                        }
                        self.pending_rebind = None;
                    }
                } else {
                    // New filter tab (Ctrl+T by default)
                    if i.modifiers.matches_exact(self.shortcut_bindings.new_filter_tab.modifiers) 
                        && i.key_pressed(self.shortcut_bindings.new_filter_tab.logical_key) {
                        self.request_new_filter_tab = true;
                    }
                    
                    // Close tab (Ctrl+W by default)
                    if i.modifiers.matches_exact(self.shortcut_bindings.close_tab.modifiers) 
                        && i.key_pressed(self.shortcut_bindings.close_tab.logical_key) {
                        // Only allow closing filter tabs
                        if let Some(ref active) = self.active_tab {
                            if matches!(active, TabType::Filter(_)) {
                                self.close_active_tab = true;
                            }
                        }
                    }
                    
                    // Focus search input (only works in filter tabs)
                    if i.modifiers.matches_exact(self.shortcut_bindings.focus_search.modifiers) 
                        && i.key_pressed(self.shortcut_bindings.focus_search.logical_key) {
                        if let Some(TabType::Filter(idx)) = &self.active_tab {
                            // Will focus the search input in the next frame during render
                            self.focus_search_next_frame = Some(*idx);
                        }
                    }
                    
                    // Arrow keys always work (hardcoded, not configurable)
                    if i.key_pressed(egui::Key::ArrowUp) { move_delta = -1; }
                    if i.key_pressed(egui::Key::ArrowDown) { move_delta = 1; }
                    
                    // Configurable bindings (default: j/k vim-style)
                    if i.modifiers.matches_exact(self.shortcut_bindings.move_up.modifiers) 
                        && i.key_pressed(self.shortcut_bindings.move_up.logical_key) { 
                        move_delta = -1; 
                    }
                    if i.modifiers.matches_exact(self.shortcut_bindings.move_down.modifiers) 
                        && i.key_pressed(self.shortcut_bindings.move_down.logical_key) { 
                        move_delta = 1; 
                    }
                    
                    // Toggle bookmark (configurable, default: Space)
                    if i.modifiers.matches_exact(self.shortcut_bindings.toggle_bookmark.modifiers) 
                        && i.key_pressed(self.shortcut_bindings.toggle_bookmark.logical_key) {
                        self.log_view.toggle_bookmark_for_selected();
                    }
                    
                    // Pane navigation (default: HJKL vim-style)
                    // H = left, J = down, K = up, L = right
                    if i.modifiers.matches_exact(self.shortcut_bindings.focus_pane_left.modifiers) 
                        && i.key_pressed(self.shortcut_bindings.focus_pane_left.logical_key) {
                        self.navigate_pane_direction = Some(PaneDirection::Left);
                    }
                    if i.modifiers.matches_exact(self.shortcut_bindings.focus_pane_down.modifiers) 
                        && i.key_pressed(self.shortcut_bindings.focus_pane_down.logical_key) {
                        self.navigate_pane_direction = Some(PaneDirection::Down);
                    }
                    if i.modifiers.matches_exact(self.shortcut_bindings.focus_pane_up.modifiers) 
                        && i.key_pressed(self.shortcut_bindings.focus_pane_up.logical_key) {
                        self.navigate_pane_direction = Some(PaneDirection::Up);
                    }
                    if i.modifiers.matches_exact(self.shortcut_bindings.focus_pane_right.modifiers) 
                        && i.key_pressed(self.shortcut_bindings.focus_pane_right.logical_key) {
                        self.navigate_pane_direction = Some(PaneDirection::Right);
                    }
                    
                    // Vim-style navigation: gg (jump to top) and G (jump to bottom)
                    // gg: Press 'g' twice within 500ms
                    if i.key_pressed(egui::Key::G) {
                        let now = std::time::Instant::now();
                        
                        // Check if Shift is held (Shift+G = capital G)
                        if i.modifiers.shift {
                            // Shift+G: Jump to bottom
                            if let Some(active) = &self.active_tab {
                                match active {
                                    TabType::Filter(idx) => {
                                        self.log_view.jump_to_bottom_in_filter(*idx);
                                    }
                                    TabType::Bookmarks => {}
                                }
                            }
                            self.last_g_press_time = None; // Clear gg state
                        } else {
                            // g without shift: Check for gg (double g)
                            if let Some(last_press) = self.last_g_press_time {
                                // If less than 500ms since last 'g', treat as gg (jump to top)
                                if now.duration_since(last_press).as_millis() < 500 {
                                    if let Some(active) = &self.active_tab {
                                        match active {
                                            TabType::Filter(idx) => {
                                                self.log_view.jump_to_top_in_filter(*idx);
                                            }
                                            TabType::Bookmarks => {}
                                        }
                                    }
                                    self.last_g_press_time = None; // Clear after successful gg
                                } else {
                                    // Too much time passed, start new gg sequence
                                    self.last_g_press_time = Some(now);
                                }
                            } else {
                                // First 'g' press, start timing
                                self.last_g_press_time = Some(now);
                            }
                        }
                    } else {
                        // Clear gg state if any other key is pressed
                        if i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. })) {
                            self.last_g_press_time = None;
                        }
                    }
                    
                    // Execute movement if any key was pressed
                    if move_delta != 0 {
                        if let Some(active) = &self.active_tab {
                            match active {
                                TabType::Filter(idx) => {
                                    self.log_view.move_selection_in_filter(*idx, move_delta);
                                }
                                TabType::Bookmarks => {}
                            }
                        }
                    }
                }
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
            self.active_tab = Some(TabType::Filter(filter_index));
        }
        
        // Handle pane navigation (Shift+HJKL)
        if let Some(direction) = self.navigate_pane_direction.take() {
            let tree = self.dock_state.main_surface_mut();
            
            // Get the currently focused node
            if let Some(current_node) = tree.focused_leaf() {
                // Find the neighbor in the specified direction
                let neighbor = Self::find_neighbor(tree, current_node, direction);
                
                // If we found a neighbor, focus it
                if let Some(neighbor_idx) = neighbor {
                    tree.set_focused_node(neighbor_idx);
                    
                    // Update active_tab to match the newly focused tab
                    if let Some((_, tab)) = self.dock_state.find_active_focused() {
                        self.active_tab = Some(tab.tab_type.clone());
                    }
                }
            }
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

        // Keyboard shortcuts window
        if self.show_shortcuts_window {
            egui::Window::new("⌨ Keyboard Shortcuts")
                .open(&mut self.show_shortcuts_window)
                .default_width(480.0)
                .resizable(true)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.add_space(5.0);
                    
                    // Configurable keys section
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("⚙ Keyboard Bindings")
                            .strong()
                            .size(13.0)
                            .color(egui::Color32::from_rgb(100, 150, 255)));
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(egui::RichText::new("↺ Reset").size(10.0)).clicked() {
                                self.shortcut_bindings = ShortcutBindings::default();
                                self.pending_rebind = None;
                            }
                        });
                    });
                    ui.add_space(6.0);
                    
                    // Iterate over all shortcut actions
                    let actions = [
                        ShortcutAction::MoveUp, 
                        ShortcutAction::MoveDown, 
                        ShortcutAction::ToggleBookmark, 
                        ShortcutAction::FocusSearch,
                        ShortcutAction::NewFilterTab,
                        ShortcutAction::CloseTab,
                        ShortcutAction::JumpToTop,
                        ShortcutAction::JumpToBottom,
                        ShortcutAction::FocusPaneLeft,
                        ShortcutAction::FocusPaneRight,
                        ShortcutAction::FocusPaneUp,
                        ShortcutAction::FocusPaneDown,
                    ];
                    
                    for (i, action) in actions.iter().enumerate() {
                        if i > 0 {
                            ui.add_space(8.0);
                        }
                        
                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            
                            // Helper function to format keyboard shortcut
                            let format_shortcut = |shortcut: &egui::KeyboardShortcut| -> String {
                                let modifiers_text = if shortcut.modifiers.ctrl {
                                    "Ctrl+"
                                } else if shortcut.modifiers.shift {
                                    "Shift+"
                                } else if shortcut.modifiers.alt {
                                    "Alt+"
                                } else if shortcut.modifiers.mac_cmd {
                                    "Cmd+"
                                } else {
                                    ""
                                };
                                format!("{}{:?}", modifiers_text, shortcut.logical_key)
                            };
                            
                            // All bindings are now KeyboardShortcuts
                            let key_text = match action {
                                ShortcutAction::MoveUp => format_shortcut(&self.shortcut_bindings.move_up),
                                ShortcutAction::MoveDown => format_shortcut(&self.shortcut_bindings.move_down),
                                ShortcutAction::ToggleBookmark => format_shortcut(&self.shortcut_bindings.toggle_bookmark),
                                ShortcutAction::FocusSearch => format_shortcut(&self.shortcut_bindings.focus_search),
                                ShortcutAction::NewFilterTab => format_shortcut(&self.shortcut_bindings.new_filter_tab),
                                ShortcutAction::CloseTab => format_shortcut(&self.shortcut_bindings.close_tab),
                                ShortcutAction::JumpToTop => "gg".to_string(),
                                ShortcutAction::JumpToBottom => "G".to_string(),
                                ShortcutAction::FocusPaneLeft => format_shortcut(&self.shortcut_bindings.focus_pane_left),
                                ShortcutAction::FocusPaneDown => format_shortcut(&self.shortcut_bindings.focus_pane_down),
                                ShortcutAction::FocusPaneUp => format_shortcut(&self.shortcut_bindings.focus_pane_up),
                                ShortcutAction::FocusPaneRight => format_shortcut(&self.shortcut_bindings.focus_pane_right),
                            };
                            
                            let badge_color = if self.pending_rebind == Some(*action) {
                                egui::Color32::from_rgb(255, 200, 100)
                            } else {
                                ui.visuals().code_bg_color
                            };
                            
                            egui::Frame::none()
                                .fill(badge_color)
                                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                                .rounding(egui::Rounding::same(4.0))
                                .stroke(egui::Stroke::new(1.0, ui.visuals().window_stroke.color))
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new(&key_text).size(13.0).strong());
                                });
                            
                            ui.add_space(8.0);
                            
                            // Action info
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(action.name()).strong());
                                ui.label(egui::RichText::new(action.description())
                                    .size(10.0)
                                    .color(ui.visuals().weak_text_color()));
                            });
                            
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // JumpToTop and JumpToBottom are hardcoded (gg/G) and cannot be rebound
                                let is_rebindable = !matches!(action, ShortcutAction::JumpToTop | ShortcutAction::JumpToBottom);
                                
                                if self.pending_rebind == Some(*action) {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(255, 200, 100),
                                        egui::RichText::new("⌛ Press any key...").strong()
                                    );
                                    if ui.button("✖ Cancel").clicked() {
                                        self.pending_rebind = None;
                                    }
                                } else if is_rebindable {
                                    if ui.button(egui::RichText::new("🔧 Rebind").size(11.0)).clicked() {
                                        self.pending_rebind = Some(*action);
                                    }
                                } else {
                                    ui.label(egui::RichText::new("(hardcoded)")
                                        .size(10.0)
                                        .color(ui.visuals().weak_text_color()));
                                }
                            });
                        });
                    }
                    
                    ui.add_space(4.0);
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
