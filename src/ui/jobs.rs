// LogCrab - GPL-3.0-or-later
// This file is part of LogCrab.
//
// Copyright (C) 2026 Daniel Freiermuth
//
// LogCrab is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Thread-safe tracking for finite, user-visible background work.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

/// An immutable view of an active job for UI rendering.
#[derive(Debug, Clone)]
pub struct JobSnapshot {
    /// Stable identifier used to request cancellation.
    pub id: u64,
    /// User-facing operation name.
    pub title: String,
    /// Current phase or progress detail.
    pub message: String,
    /// Completion ratio when the job can measure it.
    pub progress: Option<f32>,
    /// Whether this job supports cooperative cancellation.
    pub cancellable: bool,
    /// Whether cooperative cancellation has been requested.
    pub cancelling: bool,
    cancel_requested: Arc<AtomicBool>,
    ctx: egui::Context,
}

impl JobSnapshot {
    /// Request cooperative cancellation when this job supports it.
    pub fn request_cancel(&self) {
        if self.cancellable {
            self.cancel_requested.store(true, Ordering::Relaxed);
            self.ctx.request_repaint();
        }
    }
}

#[derive(Debug)]
struct JobState {
    id: u64,
    title: String,
    message: String,
    progress: Option<f32>,
    cancellable: bool,
    cancel_requested: Arc<AtomicBool>,
}

#[derive(Debug, Default)]
struct JobRegistry {
    next_id: u64,
    jobs: Vec<JobState>,
}

/// Owns the active-job registry rendered by the notification center.
#[derive(Clone, Debug, Default)]
pub struct JobManager {
    registry: Arc<Mutex<JobRegistry>>,
    ctx: egui::Context,
}

impl JobManager {
    /// Create an empty registry that repaints `ctx` when job state changes.
    #[must_use]
    pub fn new(ctx: egui::Context) -> Self {
        Self {
            registry: Arc::default(),
            ctx,
        }
    }

    /// Register a new active job with cooperative cancellation.
    #[must_use]
    pub fn start(&self, title: impl Into<String>, message: impl Into<String>) -> JobHandle {
        self.start_with_cancellation(title.into(), message.into(), true)
    }

    /// Register an active job that reports progress but cannot be interrupted.
    #[must_use]
    pub fn start_non_cancellable(
        &self,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> JobHandle {
        self.start_with_cancellation(title.into(), message.into(), false)
    }

    fn start_with_cancellation(
        &self,
        title: String,
        message: String,
        cancellable: bool,
    ) -> JobHandle {
        let (id, cancel_requested) = {
            let mut registry = self
                .registry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let id = registry.next_id;
            registry.next_id = registry.next_id.wrapping_add(1);
            let cancel_requested = Arc::new(AtomicBool::new(false));
            registry.jobs.push(JobState {
                id,
                title,
                message,
                progress: Some(0.0),
                cancellable,
                cancel_requested: Arc::clone(&cancel_requested),
            });
            drop(registry);
            (id, cancel_requested)
        };
        JobHandle {
            id,
            registry: Arc::clone(&self.registry),
            cancellable,
            cancel_requested,
            ctx: self.ctx.clone(),
        }
    }

    /// Return active jobs without ever waiting for a worker-held registry lock.
    ///
    /// `None` means the caller should retain its prior UI snapshot for this frame.
    #[must_use]
    pub fn try_snapshots(&self) -> Option<Vec<JobSnapshot>> {
        self.registry.try_lock().ok().map(|registry| {
            registry
                .jobs
                .iter()
                .map(|job| JobSnapshot {
                    id: job.id,
                    title: job.title.clone(),
                    message: job.message.clone(),
                    progress: job.progress,
                    cancellable: job.cancellable,
                    cancelling: job.cancel_requested.load(Ordering::Relaxed),
                    cancel_requested: Arc::clone(&job.cancel_requested),
                    ctx: self.ctx.clone(),
                })
                .collect()
        })
    }
}

/// Thread-safe worker-side handle for one active job.
#[derive(Clone, Debug)]
pub struct JobHandle {
    id: u64,
    registry: Arc<Mutex<JobRegistry>>,
    cancellable: bool,
    cancel_requested: Arc<AtomicBool>,
    ctx: egui::Context,
}

impl JobHandle {
    /// Report whether this worker should stop at its next cancellation checkpoint.
    #[must_use]
    pub fn is_cancel_requested(&self) -> bool {
        self.cancel_requested.load(Ordering::Relaxed)
    }

    /// Whether this job accepts a cancellation request.
    #[must_use]
    pub const fn is_cancellable(&self) -> bool {
        self.cancellable
    }

    /// Request cancellation and wake the UI to show the cancelling state.
    pub fn request_cancel(&self) {
        if self.cancellable {
            self.cancel_requested.store(true, Ordering::Relaxed);
            self.ctx.request_repaint();
        }
    }

    /// Register a sibling job that shares this job's cancellation request.
    #[must_use]
    pub fn spawn_sibling(&self, title: impl Into<String>, message: impl Into<String>) -> Self {
        let id = {
            let mut registry = self
                .registry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let id = registry.next_id;
            registry.next_id = registry.next_id.wrapping_add(1);
            registry.jobs.push(JobState {
                id,
                title: title.into(),
                message: message.into(),
                progress: Some(0.0),
                cancellable: self.cancellable,
                cancel_requested: Arc::clone(&self.cancel_requested),
            });
            id
        };
        Self {
            id,
            registry: Arc::clone(&self.registry),
            cancellable: self.cancellable,
            cancel_requested: Arc::clone(&self.cancel_requested),
            ctx: self.ctx.clone(),
        }
    }
    /// Update the notification-center representation and schedule a repaint.
    pub fn update(
        &self,
        title: impl Into<String>,
        progress: Option<f32>,
        message: impl Into<String>,
    ) {
        if let Ok(mut registry) = self.registry.lock() {
            if let Some(job) = registry.jobs.iter_mut().find(|job| job.id == self.id) {
                job.title = title.into();
                job.progress = progress;
                job.message = message.into();
            }
        }
        self.ctx.request_repaint();
    }

    /// Remove this job from the active-job registry and schedule a repaint.
    pub fn finish(&self) {
        if let Ok(mut registry) = self.registry.lock() {
            registry.jobs.retain(|job| job.id != self.id);
        }
        self.ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::JobManager;

    #[test]
    fn cancellation_is_visible_to_the_worker_handle() {
        let manager = JobManager::new(egui::Context::default());
        let handle = manager.start("Loading example.log", "Starting…");
        let job = manager
            .try_snapshots()
            .expect("uncontended registry is readable")
            .into_iter()
            .next()
            .expect("started job is visible");

        job.request_cancel();

        assert!(handle.is_cancel_requested());
    }

    #[test]
    fn finishing_removes_the_job() {
        let manager = JobManager::new(egui::Context::default());
        let handle = manager.start("Loading example.log", "Starting…");

        handle.finish();

        assert!(manager
            .try_snapshots()
            .expect("uncontended registry is readable")
            .is_empty());
    }

    #[test]
    fn sibling_jobs_share_cancellation_and_finish_independently() {
        let manager = JobManager::new(egui::Context::default());
        let parent = manager.start("Loading example.log", "Starting…");
        let sibling = parent.spawn_sibling("ML scoring", "Connecting…");
        let job = manager
            .try_snapshots()
            .expect("uncontended registry is readable")
            .into_iter()
            .find(|job| job.title == "ML scoring")
            .expect("sibling job is visible");

        job.request_cancel();
        sibling.finish();

        assert!(parent.is_cancel_requested());
        assert_eq!(
            manager
                .try_snapshots()
                .expect("uncontended registry is readable")
                .len(),
            1
        );
    }

    #[test]
    fn non_cancellable_jobs_ignore_cancellation_requests() {
        let manager = JobManager::new(egui::Context::default());
        let handle = manager.start_non_cancellable("Saving session", "Writing file…");
        let job = manager
            .try_snapshots()
            .expect("uncontended registry is readable")
            .into_iter()
            .next()
            .expect("started job is visible");

        job.request_cancel();

        assert!(!job.cancellable);
        assert!(!handle.is_cancel_requested());
    }
}
