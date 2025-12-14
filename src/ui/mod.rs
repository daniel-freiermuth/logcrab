pub mod app;
pub mod log_view;
pub mod tabs;
pub mod toasts;
pub mod windows;

pub use log_view::LogView;
pub use toasts::{ProgressToastHandle, ToastManager};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneDirection {
    Left,
    Right,
    Up,
    Down,
}
