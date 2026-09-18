// LogCrab - GPL-3.0-or-later
// This file is part of LogCrab.
//
// Copyright (C) 2026 Daniel Freiermuth
//
// LogCrab is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Bottom-right notification overlays and the active-job notification center.

use crate::ui::{JobHandle, JobSnapshot};
use egui::{Color32, Margin};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

const MAX_VISIBLE_TOASTS: usize = 5;
const JOB_PROGRESS_WIDTH: f32 = 220.0;

/// Shared state for a progress job, updated by its worker and read by the UI.
#[derive(Debug, Clone)]
pub struct ProgressToastState {
    /// User-facing operation name.
    pub title: String,
    /// Completion ratio when known.
    pub progress: Option<f32>,
    /// Current operation detail.
    pub message: String,
    /// Completion time, if the work is no longer active.
    pub dismissed_at: Option<Instant>,
    /// Error detail, if the job failed.
    pub error: Option<String>,
    /// Whether this job currently appears as an overlay toast.
    pub visible: bool,
    job: Option<JobHandle>,
}

impl ProgressToastState {
    #[must_use]
    const fn is_active(&self) -> bool {
        self.dismissed_at.is_none()
    }
}

#[derive(Debug, Clone, Copy)]
enum NotificationKind {
    Error,
    Success,
}

#[derive(Debug)]
struct Notification {
    id: u64,
    message: String,
    kind: NotificationKind,
}

#[derive(Debug)]
struct PendingNotification {
    message: String,
    kind: NotificationKind,
}

/// Thread-safe producer for one-shot success and error notifications.
#[derive(Clone)]
pub struct ToastSender {
    queue: Arc<Mutex<Vec<PendingNotification>>>,
    ctx: egui::Context,
}

impl ToastSender {
    /// Queue an error notification for the next UI frame.
    pub fn send(&self, message: impl Into<String>) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.push(PendingNotification {
                message: message.into(),
                kind: NotificationKind::Error,
            });
        }
        self.ctx.request_repaint();
    }

    /// Queue a success notification for the next UI frame.
    pub fn send_success(&self, message: impl Into<String>) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.push(PendingNotification {
                message: message.into(),
                kind: NotificationKind::Success,
            });
        }
        self.ctx.request_repaint();
    }
}

/// Thread-safe worker handle for a job's progress notification.
#[derive(Clone)]
pub struct ProgressToastHandle {
    state: Arc<RwLock<ProgressToastState>>,
    progress_handles: Arc<Mutex<Vec<Arc<RwLock<ProgressToastState>>>>>,
    pending_notifications: Arc<Mutex<Vec<PendingNotification>>>,
    ctx: egui::Context,
}

impl ProgressToastHandle {
    fn new(
        ctx: egui::Context,
        progress_handles: Arc<Mutex<Vec<Arc<RwLock<ProgressToastState>>>>>,
        pending_notifications: Arc<Mutex<Vec<PendingNotification>>>,
        title: String,
        message: String,
    ) -> Self {
        let state = Arc::new(RwLock::new(ProgressToastState {
            title,
            message,
            progress: Some(0.0),
            dismissed_at: None,
            error: None,
            visible: true,
            job: None,
        }));
        if let Ok(mut handles) = progress_handles.lock() {
            handles.push(Arc::clone(&state));
        }
        Self {
            state,
            progress_handles,
            pending_notifications,
            ctx,
        }
    }

    /// Create a related job that shares cancellation with this job.
    #[must_use]
    pub fn spawn_sibling(&self, title: impl Into<String>, message: impl Into<String>) -> Self {
        let title = title.into();
        let message = message.into();
        let sibling = Self::new(
            self.ctx.clone(),
            Arc::clone(&self.progress_handles),
            Arc::clone(&self.pending_notifications),
            title.clone(),
            message.clone(),
        );
        if let Some(job) = self.state.read().ok().and_then(|state| state.job.clone()) {
            sibling.track_job(job.spawn_sibling(title, message))
        } else {
            sibling
        }
    }

    /// Associate this progress operation with a cancellable job.
    #[must_use]
    pub fn track_job(self, job: JobHandle) -> Self {
        if let Ok(mut state) = self.state.write() {
            state.job = Some(job);
        }
        self
    }

    /// Check whether the user requested cooperative cancellation.
    #[must_use]
    pub fn is_cancel_requested(&self) -> bool {
        self.state
            .read()
            .ok()
            .and_then(|state| state.job.clone())
            .is_some_and(|job| job.is_cancel_requested())
    }

    /// Update progress and message.
    pub fn update(&self, progress: f32, message: impl Into<String>) {
        let message = message.into();
        let (title, job) = if let Ok(mut state) = self.state.write() {
            state.progress = Some(progress);
            state.message.clone_from(&message);
            (state.title.clone(), state.job.clone())
        } else {
            return;
        };
        if let Some(job) = job {
            job.update(title, Some(progress), message);
        }
        self.ctx.request_repaint();
    }

    /// Change the operation title while retaining its current progress detail.
    pub fn set_title(&self, title: impl Into<String>) {
        let title = title.into();
        let (progress, message, job) = if let Ok(mut state) = self.state.write() {
            state.title.clone_from(&title);
            (state.progress, state.message.clone(), state.job.clone())
        } else {
            return;
        };
        if let Some(job) = job {
            job.update(title, progress, message);
        }
        self.ctx.request_repaint();
    }

    /// Mark the operation as failed.
    pub fn set_error(&self, error: impl Into<String>) {
        if let Ok(mut state) = self.state.write() {
            state.error = Some(error.into());
        }
        self.ctx.request_repaint();
    }

    /// Mark the operation complete, remove its job row, and preserve any error.
    pub fn dismiss(&self) {
        let (job, error) = self.state.write().map_or_else(
            |_| (None, None),
            |mut state| {
                state.dismissed_at = Some(Instant::now());
                (state.job.clone(), state.error.take())
            },
        );
        if let Some(job) = job {
            job.finish();
        }
        if let Some(message) = error {
            if let Ok(mut notifications) = self.pending_notifications.lock() {
                notifications.push(PendingNotification {
                    message,
                    kind: NotificationKind::Error,
                });
            }
        }
        self.ctx.request_repaint();
    }
}

/// Bottom-right notification surface and notification/job manager.
pub struct ToastManager {
    progress_handles: Arc<Mutex<Vec<Arc<RwLock<ProgressToastState>>>>>,
    pending_notifications: Arc<Mutex<Vec<PendingNotification>>>,
    notifications: Vec<Notification>,
    next_notification_id: u64,
    center_open: bool,
    close_center_when_jobs_finish: bool,
    ctx: egui::Context,
}

impl ToastManager {
    #[must_use]
    pub fn new(ctx: egui::Context) -> Self {
        Self {
            progress_handles: Arc::new(Mutex::new(Vec::new())),
            pending_notifications: Arc::new(Mutex::new(Vec::new())),
            notifications: Vec::new(),
            next_notification_id: 0,
            center_open: false,
            close_center_when_jobs_finish: false,
            ctx,
        }
    }

    /// Start a visible progress notification. Call [`ProgressToastHandle::track_job`]
    /// before passing it to an active background operation.
    #[must_use]
    pub fn create_progress_toast(
        &self,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> ProgressToastHandle {
        ProgressToastHandle::new(
            self.ctx.clone(),
            Arc::clone(&self.progress_handles),
            Arc::clone(&self.pending_notifications),
            title.into(),
            message.into(),
        )
    }

    /// Return a thread-safe one-shot notification producer.
    #[must_use]
    pub fn sender(&self) -> ToastSender {
        ToastSender {
            queue: Arc::clone(&self.pending_notifications),
            ctx: self.ctx.clone(),
        }
    }

    /// Add an error notification on the UI thread.
    pub fn show_error(&mut self, message: impl Into<String>) {
        self.push_notification(message.into(), NotificationKind::Error);
    }

    /// Add a success notification on the UI thread.
    pub fn show_success(&mut self, message: impl Into<String>) {
        self.push_notification(message.into(), NotificationKind::Success);
    }

    /// Render the bottom-right overlays and notification/job manager.
    pub fn show(&mut self, ctx: &egui::Context, jobs: &[JobSnapshot]) {
        if self.close_center_when_jobs_finish && jobs.is_empty() {
            self.center_open = false;
            self.close_center_when_jobs_finish = false;
        }
        self.drain_pending_notifications();
        self.render_overlays(ctx, jobs);
        self.render_notification_center(ctx, jobs);
    }

    fn push_notification(&mut self, message: String, kind: NotificationKind) {
        let id = self.next_notification_id;
        self.next_notification_id = self.next_notification_id.wrapping_add(1);
        self.notifications.push(Notification { id, message, kind });
    }

    fn drain_pending_notifications(&mut self) {
        let pending: Vec<PendingNotification> = self
            .pending_notifications
            .lock()
            .map(|mut queue| queue.drain(..).collect())
            .unwrap_or_default();
        for notification in pending {
            self.push_notification(notification.message, notification.kind);
        }
    }

    fn progress_states(&self) -> Vec<(ProgressToastState, Arc<RwLock<ProgressToastState>>)> {
        let Ok(mut handles) = self.progress_handles.lock() else {
            return Vec::new();
        };
        handles.retain(|state| state.read().is_ok_and(|state| state.is_active()));
        handles
            .iter()
            .filter_map(|handle| {
                handle
                    .read()
                    .ok()
                    .map(|state| (state.clone(), Arc::clone(handle)))
            })
            .collect()
    }

    fn render_overlays(&mut self, ctx: &egui::Context, jobs: &[JobSnapshot]) {
        enum Overlay {
            Progress(ProgressToastState, Arc<RwLock<ProgressToastState>>),
            Notification(u64, String, NotificationKind),
        }

        if self.center_open {
            return;
        }
        let progress = self.progress_states();
        let active_jobs = jobs.len();
        let mut overlays: Vec<Overlay> = progress
            .into_iter()
            .filter(|(state, _)| state.visible)
            .map(|(state, handle)| Overlay::Progress(state, handle))
            .collect();
        overlays.extend(self.notifications.iter().map(|notification| {
            Overlay::Notification(
                notification.id,
                notification.message.clone(),
                notification.kind,
            )
        }));

        let overflow = overlays.len().saturating_sub(MAX_VISIBLE_TOASTS);
        let mut dismiss_notifications = Vec::new();
        for (index, overlay) in overlays.into_iter().take(MAX_VISIBLE_TOASTS).enumerate() {
            match overlay {
                Overlay::Progress(state, handle) => {
                    if Self::render_progress_overlay(ctx, index, &state) {
                        if let Ok(mut state) = handle.write() {
                            state.visible = false;
                        }
                    }
                }
                Overlay::Notification(id, message, kind) => {
                    if Self::render_notification_overlay(ctx, index, &message, kind) {
                        dismiss_notifications.push(id);
                    }
                }
            }
        }
        if !dismiss_notifications.is_empty() {
            self.notifications
                .retain(|notification| !dismiss_notifications.contains(&notification.id));
        }

        if active_jobs > 0 || overflow > 0 {
            let text = match (active_jobs, overflow) {
                (0, more) => format!("{more} more notifications"),
                (jobs, 0) => format!("Jobs ({jobs})"),
                (jobs, more) => format!("Jobs ({jobs}) · {more} more"),
            };
            let screen_rect = ctx.content_rect();
            egui::Area::new(egui::Id::new("notification_center_button"))
                .fixed_pos(egui::pos2(
                    screen_rect.right() - 330.0,
                    screen_rect.bottom() - 30.0,
                ))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if active_jobs > 0
                            && ui
                                .button("Hide all jobs")
                                .on_hover_text("Fold all active job overlays")
                                .clicked()
                        {
                            self.fold_all_jobs();
                        }
                        if ui.button(text).clicked() {
                            self.center_open = true;
                            self.close_center_when_jobs_finish = active_jobs > 0;
                        }
                    });
                });
        }
    }

    fn render_progress_overlay(
        ctx: &egui::Context,
        index: usize,
        state: &ProgressToastState,
    ) -> bool {
        let screen_rect = ctx.content_rect();
        let height = 106.0;
        let pos = egui::pos2(
            screen_rect.right() - 310.0,
            (index as f32).mul_add(-(height + 8.0), screen_rect.bottom() - 42.0 - height),
        );
        let mut fold = false;
        egui::Area::new(egui::Id::new(("job_toast", index)))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::default()
                    .fill(ui.visuals().window_fill)
                    .stroke(ui.visuals().window_stroke)
                    .inner_margin(Margin::same(10))
                    .corner_radius(6.0)
                    .show(ui, |ui| {
                        ui.set_min_width(280.0);
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.strong(&state.title);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    fold = ui
                                        .small_button("×")
                                        .on_hover_text("Fold job toast")
                                        .clicked();
                                    if let Some(job) = &state.job {
                                        if !job.is_cancellable() {
                                            ui.small("Cannot cancel");
                                        } else if job.is_cancel_requested() {
                                            ui.add_enabled(false, egui::Button::new("Cancelling…"));
                                        } else if ui.small_button("Cancel").clicked() {
                                            job.request_cancel();
                                        }
                                    }
                                },
                            );
                        });
                        if let Some(error) = &state.error {
                            ui.colored_label(Color32::from_rgb(255, 100, 100), error);
                        } else {
                            ui.label(&state.message);
                        }
                        if let Some(progress) = state.progress {
                            ui.add(egui::ProgressBar::new(progress).show_percentage());
                        }
                    });
            });
        fold
    }

    fn render_notification_overlay(
        ctx: &egui::Context,
        index: usize,
        message: &str,
        kind: NotificationKind,
    ) -> bool {
        let screen_rect = ctx.content_rect();
        let height = 76.0;
        let pos = egui::pos2(
            screen_rect.right() - 310.0,
            (index as f32).mul_add(-(height + 8.0), screen_rect.bottom() - 42.0 - height),
        );
        let mut dismiss = false;
        egui::Area::new(egui::Id::new(("notification_toast", index)))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::default()
                    .fill(ui.visuals().window_fill)
                    .stroke(ui.visuals().window_stroke)
                    .inner_margin(Margin::same(10))
                    .corner_radius(6.0)
                    .show(ui, |ui| {
                        ui.set_min_width(280.0);
                        ui.horizontal(|ui| {
                            let color = match kind {
                                NotificationKind::Error => Color32::from_rgb(255, 100, 100),
                                NotificationKind::Success => Color32::from_rgb(100, 200, 120),
                            };
                            ui.colored_label(
                                color,
                                match kind {
                                    NotificationKind::Error => "Error",
                                    NotificationKind::Success => "Success",
                                },
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    dismiss =
                                        ui.small_button("×").on_hover_text("Dismiss").clicked();
                                },
                            );
                        });
                        ui.label(message);
                    });
            });
        dismiss
    }

    fn render_notification_center(&mut self, ctx: &egui::Context, jobs: &[JobSnapshot]) {
        if !self.center_open {
            return;
        }

        let screen_rect = ctx.content_rect();
        let mut open = true;
        egui::Window::new("Notifications")
            .id(egui::Id::new("notification_center"))
            .default_pos(egui::pos2(
                screen_rect.right() - 450.0,
                screen_rect.bottom() - 400.0,
            ))
            .default_size(egui::vec2(430.0, 360.0))
            .min_size(egui::vec2(360.0, 240.0))
            .resizable(true)
            .open(&mut open)
            .show(ctx, |ui| {
                if !jobs.is_empty() {
                    ui.horizontal(|ui| {
                        ui.heading(format!("Jobs ({})", jobs.len()));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let cancellable =
                                jobs.iter().any(|job| job.cancellable && !job.cancelling);
                            if ui
                                .add_enabled(cancellable, egui::Button::new("Cancel all"))
                                .clicked()
                            {
                                for job in jobs.iter().filter(|job| job.cancellable) {
                                    job.request_cancel();
                                }
                            }
                            if ui.button("Fold all").clicked() {
                                self.fold_all_jobs();
                            }
                        });
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            for job in jobs {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.vertical(|ui| {
                                        ui.strong(&job.title);
                                        ui.small(&job.message);
                                        if let Some(progress) = job.progress {
                                            ui.add(
                                                egui::ProgressBar::new(progress)
                                                    .desired_width(JOB_PROGRESS_WIDTH)
                                                    .show_percentage(),
                                            );
                                        }
                                    });
                                    if !job.cancellable {
                                        ui.small("Cannot cancel");
                                    } else if job.cancelling {
                                        ui.add_enabled(false, egui::Button::new("Cancelling…"));
                                    } else if ui.button("Cancel").clicked() {
                                        job.request_cancel();
                                    }
                                });
                                ui.separator();
                            }
                        });
                }

                if !self.notifications.is_empty() {
                    ui.heading("Notifications");
                    let mut dismiss = Vec::new();
                    egui::ScrollArea::vertical()
                        .max_height(160.0)
                        .show(ui, |ui| {
                            for notification in &self.notifications {
                                ui.horizontal(|ui| {
                                    ui.label(&notification.message);
                                    if ui.small_button("×").on_hover_text("Dismiss").clicked() {
                                        dismiss.push(notification.id);
                                    }
                                });
                            }
                        });
                    self.notifications
                        .retain(|notification| !dismiss.contains(&notification.id));
                }
            });
        self.center_open = open;
        if !open {
            self.close_center_when_jobs_finish = false;
        }
    }

    fn fold_all_jobs(&self) {
        if let Ok(handles) = self.progress_handles.lock() {
            for handle in handles.iter() {
                if let Ok(mut state) = handle.write() {
                    if state.job.is_some() {
                        state.visible = false;
                    }
                }
            }
        }
        self.ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::ToastManager;
    use crate::ui::JobManager;

    #[test]
    fn sibling_progress_jobs_share_cancellation() {
        let ctx = egui::Context::default();
        let job_manager = JobManager::new(ctx.clone());
        let toast_manager = ToastManager::new(ctx);
        let parent = toast_manager
            .create_progress_toast("Loading example.log", "Starting…")
            .track_job(job_manager.start("Loading example.log", "Starting…"));
        let sibling = parent.spawn_sibling("ML scoring", "Connecting…");
        let job = job_manager
            .try_snapshots()
            .expect("uncontended registry is readable")
            .into_iter()
            .find(|job| job.title == "ML scoring")
            .expect("sibling job is visible");

        job.request_cancel();

        assert!(parent.is_cancel_requested());
        assert!(sibling.is_cancel_requested());
    }

    #[test]
    fn folding_all_jobs_keeps_work_running_but_hides_its_overlay() {
        let ctx = egui::Context::default();
        let job_manager = JobManager::new(ctx.clone());
        let toast_manager = ToastManager::new(ctx);
        let job = toast_manager
            .create_progress_toast("Loading example.log", "Starting…")
            .track_job(job_manager.start("Loading example.log", "Starting…"));

        toast_manager.fold_all_jobs();

        assert!(
            !job.state
                .read()
                .expect("toast state remains readable")
                .visible
        );
        assert!(!job.is_cancel_requested());
    }

    #[test]
    fn job_manager_closes_when_its_last_job_finishes() {
        let ctx = egui::Context::default();
        let mut manager = ToastManager::new(ctx.clone());
        manager.center_open = true;
        manager.close_center_when_jobs_finish = true;

        manager.show(&ctx, &[]);

        assert!(!manager.center_open);
        assert!(!manager.close_center_when_jobs_finish);
    }

    #[test]
    fn completed_job_errors_become_dismissable_notifications() {
        let ctx = egui::Context::default();
        let mut manager = ToastManager::new(ctx);
        let job = manager.create_progress_toast("Loading example.log", "Starting…");

        job.set_error("File is unreadable");
        job.dismiss();
        manager.drain_pending_notifications();

        assert_eq!(manager.notifications.len(), 1);
        assert_eq!(manager.notifications[0].message, "File is unreadable");
    }
}
