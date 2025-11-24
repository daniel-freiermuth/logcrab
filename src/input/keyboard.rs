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
use crate::config::GlobalConfig;
use keybinds::Keybinds;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Wrapper for egui key event that can be converted to keybinds KeyInput
struct EguiKeyEvent<'a> {
    key: &'a egui::Key,
    mods: &'a egui::Modifiers,
}

impl<'a> TryFrom<&'a egui::Event> for EguiKeyEvent<'a> {
    type Error = ();

    fn try_from(event: &'a egui::Event) -> Result<Self, Self::Error> {
        match event {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => Ok(Self { key, mods: modifiers }),
            _ => Err(()),
        }
    }
}

impl From<EguiKeyEvent<'_>> for keybinds::KeyInput {
    fn from(key_event: EguiKeyEvent<'_>) -> Self {
        use keybinds::{Key, Mods};

        // Helper to check if a key is a letter
        let is_letter_key = matches!(
            key_event.key,
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
        let kb_key = match key_event.key {
            // Letters - use shift state to determine case
            egui::Key::A => Key::Char(if key_event.mods.shift { 'A' } else { 'a' }),
            egui::Key::B => Key::Char(if key_event.mods.shift { 'B' } else { 'b' }),
            egui::Key::C => Key::Char(if key_event.mods.shift { 'C' } else { 'c' }),
            egui::Key::D => Key::Char(if key_event.mods.shift { 'D' } else { 'd' }),
            egui::Key::E => Key::Char(if key_event.mods.shift { 'E' } else { 'e' }),
            egui::Key::F => Key::Char(if key_event.mods.shift { 'F' } else { 'f' }),
            egui::Key::G => Key::Char(if key_event.mods.shift { 'G' } else { 'g' }),
            egui::Key::H => Key::Char(if key_event.mods.shift { 'H' } else { 'h' }),
            egui::Key::I => Key::Char(if key_event.mods.shift { 'I' } else { 'i' }),
            egui::Key::J => Key::Char(if key_event.mods.shift { 'J' } else { 'j' }),
            egui::Key::K => Key::Char(if key_event.mods.shift { 'K' } else { 'k' }),
            egui::Key::L => Key::Char(if key_event.mods.shift { 'L' } else { 'l' }),
            egui::Key::M => Key::Char(if key_event.mods.shift { 'M' } else { 'm' }),
            egui::Key::N => Key::Char(if key_event.mods.shift { 'N' } else { 'n' }),
            egui::Key::O => Key::Char(if key_event.mods.shift { 'O' } else { 'o' }),
            egui::Key::P => Key::Char(if key_event.mods.shift { 'P' } else { 'p' }),
            egui::Key::Q => Key::Char(if key_event.mods.shift { 'Q' } else { 'q' }),
            egui::Key::R => Key::Char(if key_event.mods.shift { 'R' } else { 'r' }),
            egui::Key::S => Key::Char(if key_event.mods.shift { 'S' } else { 's' }),
            egui::Key::T => Key::Char(if key_event.mods.shift { 'T' } else { 't' }),
            egui::Key::U => Key::Char(if key_event.mods.shift { 'U' } else { 'u' }),
            egui::Key::V => Key::Char(if key_event.mods.shift { 'V' } else { 'v' }),
            egui::Key::W => Key::Char(if key_event.mods.shift { 'W' } else { 'w' }),
            egui::Key::X => Key::Char(if key_event.mods.shift { 'X' } else { 'x' }),
            egui::Key::Y => Key::Char(if key_event.mods.shift { 'Y' } else { 'y' }),
            egui::Key::Z => Key::Char(if key_event.mods.shift { 'Z' } else { 'z' }),
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
            // For unsupported keys, return a placeholder
            _ => Key::Char('\0'),
        };

        // Map modifiers - but DON'T include shift for letter keys since we've already
        // handled it by choosing uppercase/lowercase in the character itself
        let mut kb_mods = Mods::empty();
        
        // On Mac, Cmd is the primary modifier; on other platforms, Ctrl is
        #[cfg(target_os = "macos")]
        {
            if key_event.mods.mac_cmd || key_event.mods.command {
                kb_mods |= Mods::CMD;
            }
            if key_event.mods.ctrl {
                kb_mods |= Mods::CTRL;
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            if key_event.mods.ctrl {
                kb_mods |= Mods::CTRL;
            }
        }
        
        // Don't add shift for letter keys - we've already encoded it in the character case
        // Only add shift for non-letter keys where it matters
        if key_event.mods.shift && !is_letter_key {
            kb_mods |= Mods::SHIFT;
        }
        if key_event.mods.alt {
            kb_mods |= Mods::ALT;
        }

        keybinds::KeyInput::new(kb_key, kb_mods)
    }
}


/// Keyboard shortcut actions that can be rebound
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    ReverseCycleTab,
}

impl ShortcutAction {
    /// Get all shortcut actions
    pub const fn all() -> &'static [ShortcutAction] {
        &[
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
            ShortcutAction::ReverseCycleTab,
        ]
    }

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
            ShortcutAction::ReverseCycleTab => "Cycle to Previous Tab",
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
            ShortcutAction::ReverseCycleTab => "Cycle to the previous tab in the active pane",
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
            ShortcutAction::CycleTab => "Ctrl+Tab",
            ShortcutAction::ReverseCycleTab => "Ctrl+Shift+Tab",
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
    /// Load shortcuts from global config
    pub fn load(config: &GlobalConfig) -> Self {
        let bindings = if !config.shortcuts.is_empty() {
            log::info!("Loading {} keyboard shortcuts from config", config.shortcuts.len());
            config.shortcuts.clone()
        } else {
            log::info!("No custom keyboard shortcuts found, using defaults");
            
            // Use defaults for all actions
            let mut bindings = HashMap::new();
            for action in ShortcutAction::all() {
                bindings.insert(*action, action.default_binding().to_string());
            }
            bindings
        };
        
        let dispatcher = Self::rebuild_dispatcher(&bindings);
        Self { dispatcher, bindings }
    }

    /// Rebuild the dispatcher from the current bindings
    fn rebuild_dispatcher(bindings: &HashMap<ShortcutAction, String>) -> Keybinds<ShortcutAction> {
        let mut dispatcher = Keybinds::default();
        
        // Bind all shortcuts from the bindings map
        for (action, binding) in bindings {
            let _ = dispatcher.bind(binding, *action);
        }
        
        // Also bind arrow keys for movement (in addition to j/k)
        let _ = dispatcher.bind("Up", ShortcutAction::MoveUp);
        let _ = dispatcher.bind("Down", ShortcutAction::MoveDown);
        
        dispatcher
    }

    /// Save shortcuts to global config
    pub fn save_to_config(&self, config: &mut GlobalConfig) {
        config.shortcuts = self.bindings.clone();
        log::info!("Saved {} keyboard shortcuts to config", self.bindings.len());
    }

    /// Get the shortcut string for a specific action
    pub fn get_shortcut(&self, action: ShortcutAction) -> &str {
        self.bindings.get(&action).map(|s| s.as_str()).unwrap_or("")
    }

    /// Set the shortcut for a specific action
    pub fn set_shortcut(&mut self, action: ShortcutAction, shortcut_str: &str) -> Result<(), String> {
        // Validate the shortcut string by parsing it as a KeySeq
        shortcut_str.parse::<keybinds::KeySeq>()
            .map_err(|e| format!("Invalid keybind: {}", e))?;
        
        // Update the bindings map
        self.bindings.insert(action, shortcut_str.to_string());
        
        // Rebuild the entire dispatcher from the updated bindings
        self.dispatcher = Self::rebuild_dispatcher(&self.bindings);
        
        Ok(())
    }

    /// Process input from egui and return actions to execute
    /// Returns (actions to execute, events to consume, shortcuts_changed flag)
    pub fn process_input(
        &mut self,
        raw_input: &egui::RawInput,
        pending_rebind: &mut Option<ShortcutAction>,
        active_filter_index: Option<usize>,
    ) -> (Vec<InputAction>, Vec<usize>, bool) {
        let mut actions = Vec::new();
        let mut events_to_consume = Vec::new();
        let mut shortcuts_changed = false;
        
        for (idx, event) in raw_input.events.iter().enumerate() {
            // Try to convert event to our key wrapper - only succeeds for pressed key events
            if let Ok(key_event) = EguiKeyEvent::try_from(event) {
                // Handle rebinding mode first
                if let Some(action) = pending_rebind.take() {
                    let key_input: keybinds::KeyInput = key_event.into();
                    let shortcut_str = format!("{}", key_input);
                    if self.set_shortcut(action, &shortcut_str).is_ok() {
                        shortcuts_changed = true;
                    }
                    events_to_consume.push(idx);
                    continue;
                }

                // Convert egui key to keybinds format and dispatch
                if let Some(shortcut_action) = self.dispatcher.dispatch(key_event) {
                    // Convert ShortcutAction to InputAction
                    Self::action_to_input(shortcut_action, active_filter_index, &mut actions);
                    // Mark this event for consumption
                    events_to_consume.push(idx);
                }
            }
        }
        
        (actions, events_to_consume, shortcuts_changed)
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
            ShortcutAction::ReverseCycleTab => actions.push(InputAction::ReverseCycleTab),
        }
    }
}

impl Default for KeyboardBindings {
    fn default() -> Self {
        let mut bindings = HashMap::new();

        // Bind all default shortcuts
        for action in ShortcutAction::all() {
            bindings.insert(*action, action.default_binding().to_string());
        }

        let dispatcher = Self::rebuild_dispatcher(&bindings);
        Self { dispatcher, bindings }
    }
}
