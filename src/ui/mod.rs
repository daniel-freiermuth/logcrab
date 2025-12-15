pub mod app;
pub mod filter_highlight;
pub mod log_view;
pub mod session_state;
pub mod tabs;
pub mod toasts;
pub mod windows;

pub use log_view::CrabSession;
pub use toasts::{ProgressToastHandle, ToastManager};

use egui::Color32;

/// Default color palette for filters and highlights.
/// Colors are chosen to be distinct and visible on both dark and light backgrounds.
pub const DEFAULT_PALETTE: [Color32; 8] = [
    Color32::YELLOW,
    Color32::LIGHT_BLUE,
    Color32::LIGHT_GREEN,
    Color32::from_rgb(255, 200, 150), // Light orange
    Color32::from_rgb(255, 150, 255), // Light magenta
    Color32::from_rgb(150, 255, 255), // Light cyan
    Color32::from_rgb(255, 150, 150), // Light red
    Color32::from_rgb(200, 200, 255), // Light purple
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneDirection {
    Left,
    Right,
    Up,
    Down,
}
