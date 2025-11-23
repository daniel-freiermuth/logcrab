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
use keybinds::Keybinds;
use std::collections::HashMap;

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
    CycleTab,
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
            ShortcutAction::CycleTab => "Cycle to Next Tab",
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
            ShortcutAction::JumpToBottom => "Jump to the last log line (Vim-style: Shift+G)",
            ShortcutAction::FocusPaneLeft => "Move focus to the pane on the left (Vim-style: Shift+H)",
            ShortcutAction::FocusPaneDown => "Move focus to the pane below (Vim-style: Shift+J)",
            ShortcutAction::FocusPaneUp => "Move focus to the pane above (Vim-style: Shift+K)",
            ShortcutAction::FocusPaneRight => "Move focus to the pane on the right (Vim-style: Shift+L)",
            ShortcutAction::CycleTab => "Cycle to the next tab in the active pane",
        }
    }

    pub fn default_binding(&self) -> &'static str {
        match self {
            ShortcutAction::MoveUp => "k",
            ShortcutAction::MoveDown => "j",
            ShortcutAction::ToggleBookmark => "Space",
            ShortcutAction::FocusSearch => "Ctrl+l",
            ShortcutAction::NewFilterTab => "Ctrl+t",
            ShortcutAction::NewBookmarksTab => "Ctrl+b",
            ShortcutAction::CloseTab => "Ctrl+w",
            ShortcutAction::JumpToTop => "g g",
            ShortcutAction::JumpToBottom => "G",  // Uppercase G (Shift+G in egui)
            ShortcutAction::FocusPaneLeft => "H",  // Uppercase letters for Vim-style pane navigation
            ShortcutAction::FocusPaneDown => "J",
            ShortcutAction::FocusPaneUp => "K",
            ShortcutAction::FocusPaneRight => "L",
            ShortcutAction::CycleTab => "Ctrl+PageDown",
        }
    }
}


/// Manages keyboard bindings and processes input events
pub struct KeyboardBindings {
    /// The keybinds dispatcher
    dispatcher: Keybinds<ShortcutAction>,
    /// Store current bindings as strings for display/rebinding
    bindings: HashMap<ShortcutAction, String>,
}

impl KeyboardBindings {
    /// Get the shortcut string for a specific action
    pub fn get_shortcut(&self, action: ShortcutAction) -> &str {
        self.bindings.get(&action).map(|s| s.as_str()).unwrap_or("")
    }

    /// Set the shortcut for a specific action
    pub fn set_shortcut(&mut self, action: ShortcutAction, shortcut_str: &str) -> Result<(), String> {
        // Try to parse and bind the new shortcut
        self.dispatcher.bind(shortcut_str, action)
            .map_err(|e| format!("Invalid keybind: {}", e))?;
        
        // Update our stored binding
        self.bindings.insert(action, shortcut_str.to_string());
        Ok(())
    }

    /// Process input from egui and return actions to execute
    pub fn process_input(
        &mut self,
        input: &egui::InputState,
        pending_rebind: &mut Option<ShortcutAction>,
        active_filter_index: Option<usize>,
    ) -> Vec<InputAction> {
        let mut actions = Vec::new();

        // If rebinding in progress, capture first pressed key
        if let Some(action) = *pending_rebind {
            if let Some(event) = input.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key, pressed: true, modifiers, ..
                } => Some((key, modifiers)),
                _ => None,
            }) {
                // Format the key combo as a string for keybinds
                let shortcut_str = Self::format_key_combo(*event.0, *event.1);
                let _ = self.set_shortcut(action, &shortcut_str);
                *pending_rebind = None;
            }
            return actions; // Don't process other actions while rebinding
        }

        // Convert egui events to keybinds format and dispatch
        for event in &input.events {
            if let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            {
                // Convert egui key to keybinds format
                if let Some(key_input) = Self::egui_to_keybinds(*key, *modifiers) {
                    if let Some(shortcut_action) = self.dispatcher.dispatch(key_input) {
                        // Convert ShortcutAction to InputAction
                        Self::action_to_input(shortcut_action, active_filter_index, &mut actions);
                    }
                }
            }
        }

        actions
    }

    /// Convert a ShortcutAction to an InputAction
    fn action_to_input(
        action: &ShortcutAction,
        active_filter_index: Option<usize>,
        actions: &mut Vec<InputAction>,
    ) {
        match action {
            ShortcutAction::MoveUp => actions.push(InputAction::MoveSelection(-1)),
            ShortcutAction::MoveDown => actions.push(InputAction::MoveSelection(1)),
            ShortcutAction::ToggleBookmark => actions.push(InputAction::ToggleBookmark),
            ShortcutAction::FocusSearch => {
                if let Some(idx) = active_filter_index {
                    actions.push(InputAction::FocusSearch(idx));
                }
            }
            ShortcutAction::NewFilterTab => actions.push(InputAction::NewFilterTab),
            ShortcutAction::NewBookmarksTab => actions.push(InputAction::NewBookmarksTab),
            ShortcutAction::CloseTab => actions.push(InputAction::CloseTab),
            ShortcutAction::JumpToTop => actions.push(InputAction::JumpToTop),
            ShortcutAction::JumpToBottom => actions.push(InputAction::JumpToBottom),
            ShortcutAction::FocusPaneLeft => actions.push(InputAction::NavigatePane(PaneDirection::Left)),
            ShortcutAction::FocusPaneDown => actions.push(InputAction::NavigatePane(PaneDirection::Down)),
            ShortcutAction::FocusPaneUp => actions.push(InputAction::NavigatePane(PaneDirection::Up)),
            ShortcutAction::FocusPaneRight => actions.push(InputAction::NavigatePane(PaneDirection::Right)),
            ShortcutAction::CycleTab => actions.push(InputAction::CycleTab),
        }
    }

    /// Convert egui Key and Modifiers to keybinds KeyInput
    fn egui_to_keybinds(key: egui::Key, mods: egui::Modifiers) -> Option<keybinds::KeyInput> {
        use keybinds::{Key, Mods};

        // Helper to check if a key is a letter
        let is_letter_key = matches!(
            key,
            egui::Key::A | egui::Key::B | egui::Key::C | egui::Key::D | egui::Key::E |
            egui::Key::F | egui::Key::G | egui::Key::H | egui::Key::I | egui::Key::J |
            egui::Key::K | egui::Key::L | egui::Key::M | egui::Key::N | egui::Key::O |
            egui::Key::P | egui::Key::Q | egui::Key::R | egui::Key::S | egui::Key::T |
            egui::Key::U | egui::Key::V | egui::Key::W | egui::Key::X | egui::Key::Y |
            egui::Key::Z
        );

        // Map egui key to keybinds key
        // NOTE: egui reports both 'g' and 'G' (Shift+G) as Key::G
        // We need to check the shift modifier to determine the actual character
        let kb_key = match key {
            // Letters - use shift state to determine case
            egui::Key::A => Key::Char(if mods.shift { 'A' } else { 'a' }),
            egui::Key::B => Key::Char(if mods.shift { 'B' } else { 'b' }),
            egui::Key::C => Key::Char(if mods.shift { 'C' } else { 'c' }),
            egui::Key::D => Key::Char(if mods.shift { 'D' } else { 'd' }),
            egui::Key::E => Key::Char(if mods.shift { 'E' } else { 'e' }),
            egui::Key::F => Key::Char(if mods.shift { 'F' } else { 'f' }),
            egui::Key::G => Key::Char(if mods.shift { 'G' } else { 'g' }),
            egui::Key::H => Key::Char(if mods.shift { 'H' } else { 'h' }),
            egui::Key::I => Key::Char(if mods.shift { 'I' } else { 'i' }),
            egui::Key::J => Key::Char(if mods.shift { 'J' } else { 'j' }),
            egui::Key::K => Key::Char(if mods.shift { 'K' } else { 'k' }),
            egui::Key::L => Key::Char(if mods.shift { 'L' } else { 'l' }),
            egui::Key::M => Key::Char(if mods.shift { 'M' } else { 'm' }),
            egui::Key::N => Key::Char(if mods.shift { 'N' } else { 'n' }),
            egui::Key::O => Key::Char(if mods.shift { 'O' } else { 'o' }),
            egui::Key::P => Key::Char(if mods.shift { 'P' } else { 'p' }),
            egui::Key::Q => Key::Char(if mods.shift { 'Q' } else { 'q' }),
            egui::Key::R => Key::Char(if mods.shift { 'R' } else { 'r' }),
            egui::Key::S => Key::Char(if mods.shift { 'S' } else { 's' }),
            egui::Key::T => Key::Char(if mods.shift { 'T' } else { 't' }),
            egui::Key::U => Key::Char(if mods.shift { 'U' } else { 'u' }),
            egui::Key::V => Key::Char(if mods.shift { 'V' } else { 'v' }),
            egui::Key::W => Key::Char(if mods.shift { 'W' } else { 'w' }),
            egui::Key::X => Key::Char(if mods.shift { 'X' } else { 'x' }),
            egui::Key::Y => Key::Char(if mods.shift { 'Y' } else { 'y' }),
            egui::Key::Z => Key::Char(if mods.shift { 'Z' } else { 'z' }),
            // Numbers
            egui::Key::Num0 => Key::Char('0'),
            egui::Key::Num1 => Key::Char('1'),
            egui::Key::Num2 => Key::Char('2'),
            egui::Key::Num3 => Key::Char('3'),
            egui::Key::Num4 => Key::Char('4'),
            egui::Key::Num5 => Key::Char('5'),
            egui::Key::Num6 => Key::Char('6'),
            egui::Key::Num7 => Key::Char('7'),
            egui::Key::Num8 => Key::Char('8'),
            egui::Key::Num9 => Key::Char('9'),
            // Special keys
            egui::Key::Space => Key::Char(' '),
            egui::Key::Enter => Key::Enter,
            egui::Key::Tab => Key::Tab,
            egui::Key::Backspace => Key::Backspace,
            egui::Key::Delete => Key::Delete,
            egui::Key::ArrowUp => Key::Up,
            egui::Key::ArrowDown => Key::Down,
            egui::Key::ArrowLeft => Key::Left,
            egui::Key::ArrowRight => Key::Right,
            egui::Key::Home => Key::Home,
            egui::Key::End => Key::End,
            egui::Key::PageUp => Key::PageUp,
            egui::Key::PageDown => Key::PageDown,
            egui::Key::Escape => Key::Esc,
            _ => return None,
        };

        // Map modifiers - but DON'T include shift for letter keys since we've already
        // handled it by choosing uppercase/lowercase in the character itself
        let mut kb_mods = Mods::empty();
        
        // On Mac, Cmd is the primary modifier; on other platforms, Ctrl is
        #[cfg(target_os = "macos")]
        {
            if mods.mac_cmd || mods.command {
                kb_mods |= Mods::CMD;
            }
            if mods.ctrl {
                kb_mods |= Mods::CTRL;
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            if mods.ctrl {
                kb_mods |= Mods::CTRL;
            }
        }
        
        // Don't add shift for letter keys - we've already encoded it in the character case
        // Only add shift for non-letter keys where it matters
        if mods.shift && !is_letter_key {
            kb_mods |= Mods::SHIFT;
        }
        if mods.alt {
            kb_mods |= Mods::ALT;
        }

        Some(keybinds::KeyInput::new(kb_key, kb_mods))
    }

    /// Format a key combination as a string for display
    fn format_key_combo(key: egui::Key, mods: egui::Modifiers) -> String {
        let mut parts = Vec::new();
        
        if mods.ctrl {
            parts.push("Ctrl");
        }
        if mods.alt {
            parts.push("Alt");
        }
        if mods.shift {
            parts.push("Shift");
        }
        if mods.mac_cmd || mods.command {
            parts.push("Cmd");
        }

        let key_str = match key {
            egui::Key::Space => "Space",
            egui::Key::Enter => "Enter",
            egui::Key::ArrowUp => "Up",
            egui::Key::ArrowDown => "Down",
            egui::Key::ArrowLeft => "Left",
            egui::Key::ArrowRight => "Right",
            egui::Key::PageUp => "PageUp",
            egui::Key::PageDown => "PageDown",
            egui::Key::Home => "Home",
            egui::Key::End => "End",
            egui::Key::Escape => "Esc",
            egui::Key::Tab => "Tab",
            egui::Key::Backspace => "Backspace",
            egui::Key::Delete => "Delete",
            _ => {
                // For letter/number keys, format as single char
                let key_name = format!("{:?}", key);
                parts.push(&key_name);
                return parts.join("+");
            }
        };
        
        parts.push(key_str);
        parts.join("+")
    }
}

impl Default for KeyboardBindings {
    fn default() -> Self {
        let mut dispatcher = Keybinds::default();
        let mut bindings = HashMap::new();

        // Bind all default shortcuts
        let all_actions = [
            ShortcutAction::MoveUp,
            ShortcutAction::MoveDown,
            ShortcutAction::ToggleBookmark,
            ShortcutAction::FocusSearch,
            ShortcutAction::NewFilterTab,
            ShortcutAction::NewBookmarksTab,
            ShortcutAction::CloseTab,
            ShortcutAction::JumpToTop,
            ShortcutAction::JumpToBottom,
            ShortcutAction::FocusPaneLeft,
            ShortcutAction::FocusPaneDown,
            ShortcutAction::FocusPaneUp,
            ShortcutAction::FocusPaneRight,
            ShortcutAction::CycleTab,
        ];

        for action in all_actions {
            let binding = action.default_binding();
            if let Ok(()) = dispatcher.bind(binding, action) {
                bindings.insert(action, binding.to_string());
            }
        }

        // Also bind arrow keys for movement (in addition to j/k)
        let _ = dispatcher.bind("Up", ShortcutAction::MoveUp);
        let _ = dispatcher.bind("Down", ShortcutAction::MoveDown);

        Self { dispatcher, bindings }
    }
}
