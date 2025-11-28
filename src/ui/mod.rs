pub mod app;
pub mod log_view;
pub mod tabs;
pub mod windows;

pub use log_view::LogView;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneDirection {
    Left,
    Right,
    Up,
    Down,
}
