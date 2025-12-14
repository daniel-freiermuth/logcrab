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

//! Toast notification system with thread-safe handles for background operations.
//!
//! The loader thread can own a `ProgressToastHandle` and update it directly.
//! The `ToastManager` renders all active handles each frame.

use egui::{Align2, Color32, Margin};
use egui_toast::{Toast, ToastKind, ToastOptions, Toasts};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

/// How long a dismissed toast stays visible (in seconds)
const DISMISSED_TOAST_LINGER_SECS: f32 = 3.0;

/// Shared state for a progress toast, updated by the handle, read by the renderer
#[derive(Debug, Clone)]
pub struct ProgressToastState {
    /// Title shown at the top of the toast
    pub title: String,
    /// Current progress (0.0 to 1.0), None for indeterminate
    pub progress: Option<f32>,
    /// Status message
    pub message: String,
    /// When the toast was dismissed (None if still active)
    pub dismissed_at: Option<Instant>,
    /// Optional error message (will show error style)
    pub error: Option<String>,
}

impl Default for ProgressToastState {
    fn default() -> Self {
        Self {
            title: "Loading".to_string(),
            progress: Some(0.0),
            message: String::new(),
            dismissed_at: None,
            error: None,
        }
    }
}

impl ProgressToastState {
    /// Check if this toast should be removed (dismissed and linger time elapsed)
    pub fn should_remove(&self) -> bool {
        self.dismissed_at
            .map(|t| t.elapsed().as_secs_f32() > DISMISSED_TOAST_LINGER_SECS)
            .unwrap_or(false)
    }
}

/// A thread-safe handle to a progress toast.
///
/// Can be sent to background threads and used to update the toast.
/// When dropped, the toast is automatically dismissed.
#[derive(Clone)]
pub struct ProgressToastHandle {
    state: Arc<RwLock<ProgressToastState>>,
    ctx: egui::Context,
}

impl ProgressToastHandle {
    fn new(ctx: egui::Context, title: String, message: String) -> Self {
        Self {
            state: Arc::new(RwLock::new(ProgressToastState {
                title,
                message,
                progress: Some(0.0),
                dismissed_at: None,
                error: None,
            })),
            ctx,
        }
    }

    /// Update the progress and message
    pub fn update(&self, progress: f32, message: impl Into<String>) {
        if let Ok(mut state) = self.state.write() {
            state.progress = Some(progress);
            state.message = message.into();
        }
        self.ctx.request_repaint();
    }

    /// Change the title (e.g., from "Loading" to "Scoring")
    pub fn set_title(&self, title: impl Into<String>) {
        if let Ok(mut state) = self.state.write() {
            state.title = title.into();
        }
        self.ctx.request_repaint();
    }

    /// Mark as error (will show error styling)
    pub fn set_error(&self, error: impl Into<String>) {
        if let Ok(mut state) = self.state.write() {
            state.error = Some(error.into());
        }
        self.ctx.request_repaint();
    }

    /// Dismiss the toast (it will linger for a few seconds before disappearing)
    pub fn dismiss(&self) {
        if let Ok(mut state) = self.state.write() {
            if state.dismissed_at.is_none() {
                state.dismissed_at = Some(Instant::now());
            }
        }
        self.ctx.request_repaint();
    }
}

impl Drop for ProgressToastHandle {
    fn drop(&mut self) {
        // Only dismiss if this is the last reference
        if Arc::strong_count(&self.state) <= 1 {
            // self.dismiss();
        }
    }
}

/// Manages toast notifications for the app
pub struct ToastManager {
    /// egui-toast manager for simple toasts (errors, success)
    toasts: Toasts,
    /// Active progress toast handles
    progress_handles: Arc<Mutex<Vec<Arc<RwLock<ProgressToastState>>>>>,
    /// egui context for repaints
    ctx: egui::Context,
}

impl ToastManager {
    /// Create a new ToastManager with the egui context already set
    pub fn new(ctx: egui::Context) -> Self {
        let toasts = Toasts::new()
            .anchor(Align2::RIGHT_BOTTOM, (-10.0, -40.0))
            .direction(egui::Direction::BottomUp);

        Self {
            toasts,
            progress_handles: Arc::new(Mutex::new(Vec::new())),
            ctx,
        }
    }

    /// Create a new progress toast and return a handle.
    /// The handle can be sent to background threads.
    pub fn create_progress_toast(
        &mut self,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> ProgressToastHandle {
        let ctx = self.ctx.clone();
        let handle = ProgressToastHandle::new(ctx, title.into(), message.into());

        // Store reference to state for rendering
        if let Ok(mut handles) = self.progress_handles.lock() {
            handles.push(Arc::clone(&handle.state));
        }

        handle
    }

    /// Show an error toast (auto-dismisses after timeout)
    #[allow(dead_code)]
    pub fn show_error(&mut self, message: impl Into<String>) {
        self.toasts.add(Toast {
            text: message.into().into(),
            kind: ToastKind::Error,
            options: ToastOptions::default()
                .duration_in_seconds(8.0)
                .show_progress(true),
            ..Default::default()
        });
    }

    /// Show a success toast (auto-dismisses after timeout)
    #[allow(dead_code)]
    pub fn show_success(&mut self, message: impl Into<String>) {
        self.toasts.add(Toast {
            text: message.into().into(),
            kind: ToastKind::Success,
            options: ToastOptions::default()
                .duration_in_seconds(3.0)
                .show_progress(true),
            ..Default::default()
        });
    }

    /// Render all toasts - call this in the update loop
    pub fn show(&mut self, ctx: &egui::Context) {
        // Render progress toasts manually (not using egui-toast for these)
        self.render_progress_toasts(ctx);

        // Render simple toasts (errors, success) via egui-toast
        self.toasts.show(ctx);
    }

    fn render_progress_toasts(&mut self, ctx: &egui::Context) {
        // Clean up dismissed handles and collect active ones
        let active_states: Vec<ProgressToastState> = {
            let mut handles = self.progress_handles.lock().unwrap();
            // Remove toasts that have been dismissed long enough
            handles.retain(|state| state.read().map(|s| !s.should_remove()).unwrap_or(false));
            handles
                .iter()
                .filter_map(|state| state.read().ok().map(|s| s.clone()))
                .collect()
        };

        if active_states.is_empty() {
            return;
        }

        // Calculate position for progress toasts (above the simple toasts area)
        #[allow(deprecated)]
        let screen_rect = ctx.input(|i| i.screen_rect());
        let toast_width = 300.0;
        let toast_margin = 10.0;
        let bottom_offset = 40.0; // Space for status bar

        for (idx, state) in active_states.iter().enumerate() {
            let toast_height = if state.progress.is_some() {
                100.0
            } else {
                80.0
            };
            let y_offset = bottom_offset + (idx as f32) * (toast_height + toast_margin);

            let pos = egui::pos2(
                screen_rect.right() - toast_width - toast_margin,
                screen_rect.bottom() - y_offset - toast_height,
            );

            egui::Area::new(egui::Id::new(format!("progress_toast_{idx}")))
                .fixed_pos(pos)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    self.render_single_progress_toast(ui, state);
                });
        }
    }

    fn render_single_progress_toast(&self, ui: &mut egui::Ui, state: &ProgressToastState) {
        let is_error = state.error.is_some();

        let fill = if is_error {
            Color32::from_rgb(80, 20, 20)
        } else {
            ui.visuals().window_fill
        };

        egui::Frame::default()
            .fill(fill)
            .stroke(ui.visuals().window_stroke)
            .inner_margin(Margin::same(12))
            .corner_radius(8.0)
            .shadow(egui::epaint::Shadow {
                offset: [0, 2],
                blur: 8,
                spread: 0,
                color: Color32::from_black_alpha(60),
            })
            .show(ui, |ui| {
                ui.set_min_width(280.0);

                ui.horizontal(|ui| {
                    if !is_error {
                        ui.spinner();
                        ui.add_space(8.0);
                    }
                    ui.strong(&state.title);
                });

                ui.add_space(6.0);

                // Show error or message
                if let Some(ref error) = state.error {
                    ui.colored_label(Color32::from_rgb(255, 100, 100), error);
                } else {
                    // Truncate long messages for display
                    let display_message = if state.message.len() > 50 {
                        format!(
                            "...{}",
                            &state.message[state.message.len().saturating_sub(47)..]
                        )
                    } else {
                        state.message.clone()
                    };
                    ui.label(&display_message);
                }

                // Progress bar (only if we have determinate progress)
                if let Some(progress) = state.progress {
                    ui.add_space(6.0);
                    let progress_bar = egui::ProgressBar::new(progress)
                        .show_percentage()
                        .fill(Color32::from_rgb(100, 180, 100));
                    ui.add(progress_bar);
                }
            });
    }
}
