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

use super::actions::{InputAction, PaneDirection};
use std::time::Instant;

/// Keyboard shortcut actions that can be rebound
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutAction {
    MoveUp,
    MoveDown,
    ToggleBookmark,
    FocusSearch,
    NewFilterTab,
    NewBookmarksTab,
    CloseTab,
    JumpToTop,
    JumpToBottom,
    FocusPaneLeft,
    FocusPaneDown,
    FocusPaneUp,
    FocusPaneRight,
}

impl ShortcutAction {
    pub fn name(&self) -> &'static str {
        match self {
            ShortcutAction::MoveUp => "Move Selection Up",
            ShortcutAction::MoveDown => "Move Selection Down",
            ShortcutAction::ToggleBookmark => "Toggle Bookmark",
            ShortcutAction::FocusSearch => "Focus Search Input",
            ShortcutAction::NewFilterTab => "New Filter Tab",
            ShortcutAction::NewBookmarksTab => "New Bookmarks Tab",
            ShortcutAction::CloseTab => "Close Current Tab",
            ShortcutAction::JumpToTop => "Jump to Top",
            ShortcutAction::JumpToBottom => "Jump to Bottom",
            ShortcutAction::FocusPaneLeft => "Focus Pane Left",
            ShortcutAction::FocusPaneDown => "Focus Pane Down",
            ShortcutAction::FocusPaneUp => "Focus Pane Up",
            ShortcutAction::FocusPaneRight => "Focus Pane Right",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            ShortcutAction::MoveUp => "Move to the previous log line in the active view",
            ShortcutAction::MoveDown => "Move to the next log line in the active view",
            ShortcutAction::ToggleBookmark => "Add or remove a bookmark on the selected line",
            ShortcutAction::FocusSearch => "Jump to the search input field (filter tabs only). Press Enter to return focus to logs.",
            ShortcutAction::NewFilterTab => "Create a new filter tab with search focused",
            ShortcutAction::NewBookmarksTab => "Create a new bookmarks tab next to the current tab",
            ShortcutAction::CloseTab => "Close the currently active tab",
            ShortcutAction::JumpToTop => "Jump to the first log line (Vim-style: gg)",
            ShortcutAction::JumpToBottom => "Jump to the last log line (Vim-style: G)",
            ShortcutAction::FocusPaneLeft => "Move focus to the pane on the left (Vim-style: Shift+H)",
            ShortcutAction::FocusPaneDown => "Move focus to the pane below (Vim-style: Shift+J)",
            ShortcutAction::FocusPaneUp => "Move focus to the pane above (Vim-style: Shift+K)",
            ShortcutAction::FocusPaneRight => "Move focus to the pane on the right (Vim-style: Shift+L)",
        }
    }
}

/// Manages keyboard bindings and processes input events
pub struct KeyboardBindings {
    move_up: egui::KeyboardShortcut,
    move_down: egui::KeyboardShortcut,
    toggle_bookmark: egui::KeyboardShortcut,
    focus_search: egui::KeyboardShortcut,
    new_filter_tab: egui::KeyboardShortcut,
    new_bookmarks_tab: egui::KeyboardShortcut,
    close_tab: egui::KeyboardShortcut,
    focus_pane_left: egui::KeyboardShortcut,
    focus_pane_down: egui::KeyboardShortcut,
    focus_pane_up: egui::KeyboardShortcut,
    focus_pane_right: egui::KeyboardShortcut,

    // State for Vim-style gg detection
    last_g_press_time: Option<Instant>,
}

impl KeyboardBindings {
    /// Get the shortcut for a specific action
    pub fn get_shortcut(&self, action: ShortcutAction) -> egui::KeyboardShortcut {
        match action {
            ShortcutAction::MoveUp => self.move_up,
            ShortcutAction::MoveDown => self.move_down,
            ShortcutAction::ToggleBookmark => self.toggle_bookmark,
            ShortcutAction::FocusSearch => self.focus_search,
            ShortcutAction::NewFilterTab => self.new_filter_tab,
            ShortcutAction::NewBookmarksTab => self.new_bookmarks_tab,
            ShortcutAction::CloseTab => self.close_tab,
            ShortcutAction::FocusPaneLeft => self.focus_pane_left,
            ShortcutAction::FocusPaneDown => self.focus_pane_down,
            ShortcutAction::FocusPaneUp => self.focus_pane_up,
            ShortcutAction::FocusPaneRight => self.focus_pane_right,
            // Hardcoded shortcuts don't have rebindable keys
            ShortcutAction::JumpToTop | ShortcutAction::JumpToBottom => {
                egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::G)
            }
        }
    }

    /// Set the shortcut for a specific action
    pub fn set_shortcut(&mut self, action: ShortcutAction, shortcut: egui::KeyboardShortcut) {
        match action {
            ShortcutAction::MoveUp => self.move_up = shortcut,
            ShortcutAction::MoveDown => self.move_down = shortcut,
            ShortcutAction::ToggleBookmark => self.toggle_bookmark = shortcut,
            ShortcutAction::FocusSearch => self.focus_search = shortcut,
            ShortcutAction::NewFilterTab => self.new_filter_tab = shortcut,
            ShortcutAction::NewBookmarksTab => self.new_bookmarks_tab = shortcut,
            ShortcutAction::CloseTab => self.close_tab = shortcut,
            ShortcutAction::FocusPaneLeft => self.focus_pane_left = shortcut,
            ShortcutAction::FocusPaneDown => self.focus_pane_down = shortcut,
            ShortcutAction::FocusPaneUp => self.focus_pane_up = shortcut,
            ShortcutAction::FocusPaneRight => self.focus_pane_right = shortcut,
            // Hardcoded shortcuts cannot be changed
            ShortcutAction::JumpToTop | ShortcutAction::JumpToBottom => {}
        }
    }

    /// Process input and return actions to execute
    /// Returns None if rebinding is in progress and a key was captured
    pub fn process_input(
        &mut self,
        input: &egui::InputState,
        pending_rebind: &mut Option<ShortcutAction>,
        active_filter_index: Option<usize>,
    ) -> Vec<InputAction> {
        let mut actions = Vec::new();

        // If rebinding in progress, capture first pressed key
        if let Some(action) = *pending_rebind {
            if let Some(event_key) = input.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key, pressed: true, ..
                } => Some(*key),
                _ => None,
            }) {
                let shortcut = egui::KeyboardShortcut::new(input.modifiers, event_key);
                self.set_shortcut(action, shortcut);
                *pending_rebind = None;
            }
            return actions; // Don't process other actions while rebinding
        }

        // New filter tab (Ctrl+T by default)
        if input.modifiers.matches_exact(self.new_filter_tab.modifiers)
            && input.key_pressed(self.new_filter_tab.logical_key)
        {
            actions.push(InputAction::NewFilterTab);
        }

        // New bookmarks tab (Ctrl+B by default)
        if input
            .modifiers
            .matches_exact(self.new_bookmarks_tab.modifiers)
            && input.key_pressed(self.new_bookmarks_tab.logical_key)
        {
            actions.push(InputAction::NewBookmarksTab);
        }

        // Close tab (Ctrl+W by default)
        if input.modifiers.matches_exact(self.close_tab.modifiers)
            && input.key_pressed(self.close_tab.logical_key)
        {
            actions.push(InputAction::CloseTab);
        }

        // Focus search input (only works in filter tabs)
        if input.modifiers.matches_exact(self.focus_search.modifiers)
            && input.key_pressed(self.focus_search.logical_key)
        {
            if let Some(idx) = active_filter_index {
                actions.push(InputAction::FocusSearch(idx));
            }
        }

        // Arrow keys always work (hardcoded, not configurable)
        if input.key_pressed(egui::Key::ArrowUp) {
            actions.push(InputAction::MoveSelection(-1));
        }
        if input.key_pressed(egui::Key::ArrowDown) {
            actions.push(InputAction::MoveSelection(1));
        }

        // Configurable movement bindings (default: j/k vim-style)
        if input.modifiers.matches_exact(self.move_up.modifiers)
            && input.key_pressed(self.move_up.logical_key)
        {
            actions.push(InputAction::MoveSelection(-1));
        }
        if input.modifiers.matches_exact(self.move_down.modifiers)
            && input.key_pressed(self.move_down.logical_key)
        {
            actions.push(InputAction::MoveSelection(1));
        }

        // Toggle bookmark (configurable, default: Space)
        if input
            .modifiers
            .matches_exact(self.toggle_bookmark.modifiers)
            && input.key_pressed(self.toggle_bookmark.logical_key)
        {
            actions.push(InputAction::ToggleBookmark);
        }

        // Pane navigation (default: HJKL vim-style)
        if input
            .modifiers
            .matches_exact(self.focus_pane_left.modifiers)
            && input.key_pressed(self.focus_pane_left.logical_key)
        {
            actions.push(InputAction::NavigatePane(PaneDirection::Left));
        }
        if input
            .modifiers
            .matches_exact(self.focus_pane_down.modifiers)
            && input.key_pressed(self.focus_pane_down.logical_key)
        {
            actions.push(InputAction::NavigatePane(PaneDirection::Down));
        }
        if input.modifiers.matches_exact(self.focus_pane_up.modifiers)
            && input.key_pressed(self.focus_pane_up.logical_key)
        {
            actions.push(InputAction::NavigatePane(PaneDirection::Up));
        }
        if input
            .modifiers
            .matches_exact(self.focus_pane_right.modifiers)
            && input.key_pressed(self.focus_pane_right.logical_key)
        {
            actions.push(InputAction::NavigatePane(PaneDirection::Right));
        }

        // Vim-style navigation: gg (jump to top) and G (jump to bottom)
        if input.key_pressed(egui::Key::G) {
            let now = Instant::now();

            if input.modifiers.shift {
                // Shift+G: Jump to bottom
                actions.push(InputAction::JumpToBottom);
                self.last_g_press_time = None;
            } else {
                // g without shift: Check for gg (double g)
                if let Some(last_press) = self.last_g_press_time {
                    if now.duration_since(last_press).as_millis() < 500 {
                        actions.push(InputAction::JumpToTop);
                        self.last_g_press_time = None;
                    } else {
                        self.last_g_press_time = Some(now);
                    }
                } else {
                    self.last_g_press_time = Some(now);
                }
            }
        } else {
            // Clear gg state if any other key is pressed
            if input
                .events
                .iter()
                .any(|e| matches!(e, egui::Event::Key { pressed: true, .. }))
            {
                self.last_g_press_time = None;
            }
        }

        actions
    }
}

impl Default for KeyboardBindings {
    fn default() -> Self {
        KeyboardBindings {
            move_up: egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::K),
            move_down: egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::J),
            toggle_bookmark: egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Space),
            focus_search: egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::L),
            new_filter_tab: egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::T),
            new_bookmarks_tab: egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::B),
            close_tab: egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::W),
            focus_pane_left: egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::H),
            focus_pane_down: egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::J),
            focus_pane_up: egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::K),
            focus_pane_right: egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::L),
            last_g_press_time: None,
        }
    }
}
