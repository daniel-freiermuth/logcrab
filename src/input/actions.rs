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

/// Direction for pane navigation (Vim-style HJKL)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneDirection {
    Left,
    Right,
    Up,
    Down,
}

/// All possible input actions in the application
#[derive(Debug, Clone, PartialEq)]
pub enum InputAction {
    /// Move selection by delta (positive = down, negative = up)
    MoveSelection(i32),

    /// Toggle bookmark on the selected line
    ToggleBookmark,

    /// Focus the search input for a specific filter
    FocusSearch(usize),

    /// Create a new filter tab
    NewFilterTab,

    /// Close the currently active tab
    CloseTab,

    /// Jump to the top of the current view (Vim gg)
    JumpToTop,

    /// Jump to the bottom of the current view (Vim G)
    JumpToBottom,

    /// Navigate to a neighboring pane
    NavigatePane(PaneDirection),
}
