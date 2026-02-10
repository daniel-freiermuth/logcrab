// LogCrab - GPL-3.0-or-later
// This file is part of LogCrab.
//
// Copyright (C) 2026 Daniel Freiermuth
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

use crate::config::GlobalConfig;
use keybinds::Keybinds;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Wrapper for egui key event that can be converted to keybinds `KeyInput`
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
            } => Ok(Self {
                key,
                mods: modifiers,
            }),
            egui::Event::Copy
            | egui::Event::Cut
            | egui::Event::Paste(_)
            | egui::Event::Text(_)
            | egui::Event::Key { .. }
            | egui::Event::PointerMoved(_)
            | egui::Event::MouseMoved(_)
            | egui::Event::PointerButton { .. }
            | egui::Event::PointerGone
            | egui::Event::Zoom(_)
            | egui::Event::Rotate(_)
            | egui::Event::Ime(_)
            | egui::Event::Touch { .. }
            | egui::Event::MouseWheel { .. }
            | egui::Event::WindowFocused(_)
            | egui::Event::AccessKitActionRequest(_)
            | egui::Event::Screenshot { .. } => Err(()),
        }
    }
}

impl From<EguiKeyEvent<'_>> for keybinds::KeyInput {
    fn from(key_event: EguiKeyEvent<'_>) -> Self {
        let is_letter_key = is_letter_key(*key_event.key);
        let kb_key = map_egui_key_to_kb_key(*key_event.key, key_event.mods.shift);
        let kb_mods = map_modifiers(*key_event.mods, is_letter_key);

        Self::new(kb_key, kb_mods)
    }
}

const fn is_letter_key(key: egui::Key) -> bool {
    matches!(
        key,
        egui::Key::A
            | egui::Key::B
            | egui::Key::C
            | egui::Key::D
            | egui::Key::E
            | egui::Key::F
            | egui::Key::G
            | egui::Key::H
            | egui::Key::I
            | egui::Key::J
            | egui::Key::K
            | egui::Key::L
            | egui::Key::M
            | egui::Key::N
            | egui::Key::O
            | egui::Key::P
            | egui::Key::Q
            | egui::Key::R
            | egui::Key::S
            | egui::Key::T
            | egui::Key::U
            | egui::Key::V
            | egui::Key::W
            | egui::Key::X
            | egui::Key::Y
            | egui::Key::Z
    )
}

#[allow(clippy::cognitive_complexity)] // Many branches trigger false positive
const fn map_egui_key_to_kb_key(key: egui::Key, shift: bool) -> keybinds::Key {
    use keybinds::Key;

    match key {
        // Letters - use shift state to determine case
        egui::Key::A => Key::Char(if shift { 'A' } else { 'a' }),
        egui::Key::B => Key::Char(if shift { 'B' } else { 'b' }),
        egui::Key::C => Key::Char(if shift { 'C' } else { 'c' }),
        egui::Key::D => Key::Char(if shift { 'D' } else { 'd' }),
        egui::Key::E => Key::Char(if shift { 'E' } else { 'e' }),
        egui::Key::F => Key::Char(if shift { 'F' } else { 'f' }),
        egui::Key::G => Key::Char(if shift { 'G' } else { 'g' }),
        egui::Key::H => Key::Char(if shift { 'H' } else { 'h' }),
        egui::Key::I => Key::Char(if shift { 'I' } else { 'i' }),
        egui::Key::J => Key::Char(if shift { 'J' } else { 'j' }),
        egui::Key::K => Key::Char(if shift { 'K' } else { 'k' }),
        egui::Key::L => Key::Char(if shift { 'L' } else { 'l' }),
        egui::Key::M => Key::Char(if shift { 'M' } else { 'm' }),
        egui::Key::N => Key::Char(if shift { 'N' } else { 'n' }),
        egui::Key::O => Key::Char(if shift { 'O' } else { 'o' }),
        egui::Key::P => Key::Char(if shift { 'P' } else { 'p' }),
        egui::Key::Q => Key::Char(if shift { 'Q' } else { 'q' }),
        egui::Key::R => Key::Char(if shift { 'R' } else { 'r' }),
        egui::Key::S => Key::Char(if shift { 'S' } else { 's' }),
        egui::Key::T => Key::Char(if shift { 'T' } else { 't' }),
        egui::Key::U => Key::Char(if shift { 'U' } else { 'u' }),
        egui::Key::V => Key::Char(if shift { 'V' } else { 'v' }),
        egui::Key::W => Key::Char(if shift { 'W' } else { 'w' }),
        egui::Key::X => Key::Char(if shift { 'X' } else { 'x' }),
        egui::Key::Y => Key::Char(if shift { 'Y' } else { 'y' }),
        egui::Key::Z => Key::Char(if shift { 'Z' } else { 'z' }),
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
        // Function keys - map to unicode private use area chars
        egui::Key::F1 => Key::Char('\u{E001}'),
        egui::Key::F2 => Key::Char('\u{E002}'),
        egui::Key::F3 => Key::Char('\u{E003}'),
        egui::Key::F4 => Key::Char('\u{E004}'),
        egui::Key::F5 => Key::Char('\u{E005}'),
        egui::Key::F6 => Key::Char('\u{E006}'),
        egui::Key::F7 => Key::Char('\u{E007}'),
        egui::Key::F8 => Key::Char('\u{E008}'),
        egui::Key::F9 => Key::Char('\u{E009}'),
        egui::Key::F10 => Key::Char('\u{E00A}'),
        egui::Key::F11 => Key::Char('\u{E00B}'),
        egui::Key::F12 => Key::Char('\u{E00C}'),
        // For unsupported keys, return a placeholder
        egui::Key::Insert
        | egui::Key::Copy
        | egui::Key::Cut
        | egui::Key::Paste
        | egui::Key::Colon
        | egui::Key::Comma
        | egui::Key::Backslash
        | egui::Key::Slash
        | egui::Key::Pipe
        | egui::Key::Questionmark
        | egui::Key::Exclamationmark
        | egui::Key::OpenBracket
        | egui::Key::CloseBracket
        | egui::Key::OpenCurlyBracket
        | egui::Key::CloseCurlyBracket
        | egui::Key::Backtick
        | egui::Key::Minus
        | egui::Key::Period
        | egui::Key::Plus
        | egui::Key::Equals
        | egui::Key::Semicolon
        | egui::Key::Quote
        | egui::Key::F13
        | egui::Key::F14
        | egui::Key::F15
        | egui::Key::F16
        | egui::Key::F17
        | egui::Key::F18
        | egui::Key::F19
        | egui::Key::F20
        | egui::Key::F21
        | egui::Key::F22
        | egui::Key::F23
        | egui::Key::F24
        | egui::Key::F25
        | egui::Key::F26
        | egui::Key::F27
        | egui::Key::F28
        | egui::Key::F29
        | egui::Key::F30
        | egui::Key::F31
        | egui::Key::F32
        | egui::Key::F33
        | egui::Key::F34
        | egui::Key::F35
        | egui::Key::BrowserBack => Key::Char('\0'),
    }
}

fn map_modifiers(mods: egui::Modifiers, is_letter_key: bool) -> keybinds::Mods {
    use keybinds::Mods;

    let mut kb_mods = Mods::empty();

    if mods.command {
        kb_mods |= Mods::CTRL;
    }

    // Don't add shift for letter keys - we've already encoded it in the character case
    // Only add shift for non-letter keys where it matters
    if mods.shift && !is_letter_key {
        kb_mods |= Mods::SHIFT;
    }
    if mods.alt {
        kb_mods |= Mods::ALT;
    }

    kb_mods
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
    PageUp,
    PageDown,
    OpenFile,
    FocusPaneLeft,
    FocusPaneDown,
    FocusPaneUp,
    FocusPaneRight,
    CycleTab,
    ReverseCycleTab,
    RenameFilter,
}

impl ShortcutAction {
    /// Get all shortcut actions
    pub const fn all() -> &'static [Self] {
        &[
            Self::MoveUp,
            Self::MoveDown,
            Self::ToggleBookmark,
            Self::FocusSearch,
            Self::NewFilterTab,
            Self::NewBookmarksTab,
            Self::CloseTab,
            Self::JumpToTop,
            Self::JumpToBottom,
            Self::PageUp,
            Self::PageDown,
            Self::OpenFile,
            Self::FocusPaneLeft,
            Self::FocusPaneDown,
            Self::FocusPaneUp,
            Self::FocusPaneRight,
            Self::CycleTab,
            Self::ReverseCycleTab,
            Self::RenameFilter,
        ]
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::MoveUp => "Move Selection Up",
            Self::MoveDown => "Move Selection Down",
            Self::ToggleBookmark => "Toggle Bookmark",
            Self::FocusSearch => "Focus Search Input",
            Self::NewFilterTab => "New Filter Tab",
            Self::NewBookmarksTab => "New Bookmarks Tab",
            Self::CloseTab => "Close Current Tab",
            Self::JumpToTop => "Jump to Top",
            Self::JumpToBottom => "Jump to Bottom",
            Self::PageUp => "Page Up",
            Self::PageDown => "Page Down",
            Self::OpenFile => "Open File",
            Self::FocusPaneLeft => "Focus Pane Left",
            Self::FocusPaneDown => "Focus Pane Down",
            Self::FocusPaneUp => "Focus Pane Up",
            Self::FocusPaneRight => "Focus Pane Right",
            Self::CycleTab => "Cycle to Next Tab",
            Self::ReverseCycleTab => "Cycle to Previous Tab",
            Self::RenameFilter => "Rename Filter",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::MoveUp => "Move to the previous log line in the active view",
            Self::MoveDown => "Move to the next log line in the active view",
            Self::ToggleBookmark => "Add or remove a bookmark on the selected line",
            Self::FocusSearch => "Jump to the search input field (filter tabs only). Press Enter to return focus to logs.",
            Self::NewFilterTab => "Create a new filter tab with search focused",
            Self::NewBookmarksTab => "Create a new bookmarks tab next to the current tab",
            Self::CloseTab => "Close the currently active tab",
            Self::JumpToTop => "Jump to the first log line (Vim-style: gg)",
            Self::JumpToBottom => "Jump to the last log line (Vim-style: Shift+G)",
            Self::PageUp => "Jump up by one page of log lines",
            Self::PageDown => "Jump down by one page of log lines",
            Self::OpenFile => "Open a file dialog to load a new log file",
            Self::FocusPaneLeft => "Move focus to the pane on the left (Vim-style: Shift+H)",
            Self::FocusPaneDown => "Move focus to the pane below (Vim-style: Shift+J)",
            Self::FocusPaneUp => "Move focus to the pane above (Vim-style: Shift+K)",
            Self::FocusPaneRight => "Move focus to the pane on the right (Vim-style: Shift+L)",
            Self::CycleTab => "Cycle to the next tab in the active pane",
            Self::ReverseCycleTab => "Cycle to the previous tab in the active pane",
            Self::RenameFilter => "Open rename dialog for the current filter tab",
        }
    }

    pub const fn default_binding(self) -> &'static str {
        match self {
            Self::MoveUp => "k",
            Self::MoveDown => "j",
            Self::ToggleBookmark => "Space",
            Self::FocusSearch => "Ctrl+l",
            Self::NewFilterTab => "Ctrl+t",
            Self::NewBookmarksTab => "Ctrl+b",
            Self::CloseTab => "Ctrl+w",
            Self::JumpToTop => "g g",
            Self::JumpToBottom => "G", // Uppercase G (Shift+G in egui)
            Self::PageUp => "PageUp",
            Self::PageDown => "PageDown",
            Self::OpenFile => "Ctrl+o",
            Self::FocusPaneLeft => "H", // Uppercase letters for Vim-style pane navigation
            Self::FocusPaneDown => "J",
            Self::FocusPaneUp => "K",
            Self::FocusPaneRight => "L",
            Self::CycleTab => "Ctrl+Tab",
            Self::ReverseCycleTab => "Ctrl+Shift+Tab",
            Self::RenameFilter => "\u{E002}", // F2
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
        let bindings = if config.shortcuts.is_empty() {
            log::info!("No custom keyboard shortcuts found, using defaults");

            // Use defaults for all actions
            let mut bindings = HashMap::new();
            for action in ShortcutAction::all() {
                bindings.insert(*action, action.default_binding().to_string());
            }
            bindings
        } else {
            log::info!(
                "Loading {} keyboard shortcuts from config",
                config.shortcuts.len()
            );
            config.shortcuts.clone()
        };

        let dispatcher = Self::rebuild_dispatcher(&bindings);
        Self {
            dispatcher,
            bindings,
        }
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
        config.shortcuts.clone_from(&self.bindings);
        log::info!("Saved {} keyboard shortcuts to config", self.bindings.len());
    }

    /// Get the shortcut string for a specific action
    pub fn get_shortcut(&self, action: ShortcutAction) -> &str {
        self.bindings
            .get(&action)
            .map_or("", std::string::String::as_str)
    }

    /// Set the shortcut for a specific action
    pub fn set_shortcut(
        &mut self,
        action: ShortcutAction,
        shortcut_str: &str,
    ) -> Result<(), String> {
        // Validate the shortcut string by parsing it as a KeySeq
        shortcut_str
            .parse::<keybinds::KeySeq>()
            .map_err(|e| format!("Invalid keybind: {e}"))?;

        // Update the bindings map
        self.bindings.insert(action, shortcut_str.to_string());

        // Rebuild the entire dispatcher from the updated bindings
        self.dispatcher = Self::rebuild_dispatcher(&self.bindings);

        Ok(())
    }

    /// Process input from egui and return actions to execute
    /// Returns (actions to execute, events to consume, `shortcuts_changed` flag)
    pub fn process_input(
        &mut self,
        raw_input: &egui::RawInput,
        pending_rebind: &mut Option<ShortcutAction>,
    ) -> (Vec<ShortcutAction>, Vec<usize>, bool) {
        let mut actions = Vec::new();
        let mut events_to_consume = Vec::new();
        let mut shortcuts_changed = false;

        for (idx, event) in raw_input.events.iter().enumerate() {
            // Try to convert event to our key wrapper - only succeeds for pressed key events
            if let Ok(key_event) = EguiKeyEvent::try_from(event) {
                // Handle rebinding mode first
                if let Some(action) = pending_rebind.take() {
                    let key_input: keybinds::KeyInput = key_event.into();
                    let shortcut_str = format!("{key_input}");
                    if self.set_shortcut(action, &shortcut_str).is_ok() {
                        shortcuts_changed = true;
                    }
                    events_to_consume.push(idx);
                    continue;
                }

                // Convert egui key to keybinds format and dispatch
                if let Some(shortcut_action) = self.dispatcher.dispatch(key_event) {
                    actions.push(*shortcut_action);
                    // Mark this event for consumption
                    events_to_consume.push(idx);
                }
            }
        }

        (actions, events_to_consume, shortcuts_changed)
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
        Self {
            dispatcher,
            bindings,
        }
    }
}
