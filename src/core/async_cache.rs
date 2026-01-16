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

//! Async cache for expensive computations with stale-while-revalidate semantics.
//!
//! Combines a [`TaskWorkerHandle`] with a `watch` channel to provide a simple API
//! for cached async computations with automatic deduplication.

use crate::core::task_worker::TaskWorkerHandle;
use tokio::sync::watch;

/// Cache for async-computed values with automatic background computation.
///
/// Provides stale-while-revalidate semantics:
/// - Always returns the latest computed value
/// - Tracks what key that value corresponds to
/// - Automatically submits work to background thread when needed
/// - Deduplicates requests via the task worker
pub struct AsyncCache<D, K, V> {
    dedup_key: D,
    current: Option<(K, V)>,
    pending_key: Option<K>,
    rx: watch::Receiver<Option<(K, V)>>,
    tx: watch::Sender<Option<(K, V)>>,
}

impl<D, K, V> AsyncCache<D, K, V>
where
    D: Clone,
    K: Eq + Clone,
    V: Clone,
{
    /// Create a new empty cache with the given dedup key.
    ///
    /// The dedup key identifies this cache to the task worker for deduplication.
    #[must_use]
    pub fn new(dedup_key: D) -> Self {
        let (tx, rx) = watch::channel(None);
        Self {
            dedup_key,
            current: None,
            pending_key: None,
            rx,
            tx,
        }
    }

    /// Poll for updates from the background task and update local cache.
    ///
    /// Call this at the start of each frame to receive any completed results.
    pub fn poll(&mut self) {
        // borrow_and_update marks the value as seen
        if let Some(new_value) = self.rx.borrow_and_update().clone() {
            self.current = Some(new_value);
            self.pending_key = None;
        }
    }

    /// Get the current cached value, if any.
    pub fn get(&self) -> Option<&(K, V)> {
        self.current.as_ref()
    }

    /// Check if the current cached value matches the given key.
    pub fn is_valid(&self, key: &K) -> bool {
        self.current.as_ref().is_some_and(|(k, _)| k == key)
    }

    /// Ensure a value is computed for the given key.
    ///
    /// If the key is already cached or pending, this is a no-op.
    /// Otherwise, submits the work closure to the background worker.
    pub fn ensure_computed<F>(&mut self, key: K, worker: &TaskWorkerHandle<D>, compute: F)
    where
        D: Send + 'static,
        K: Send + Sync + 'static,
        V: Send + Sync + 'static,
        F: FnOnce() -> V + Send + 'static,
    {
        // Already have it?
        if self.is_valid(&key) {
            return;
        }
        // Already pending?
        if self.pending_key.as_ref() == Some(&key) {
            return;
        }

        self.pending_key = Some(key.clone());
        let tx = self.tx.clone();
        let dedup = self.dedup_key.clone();

        worker.submit(dedup, move || {
            let value = compute();
            let _ = tx.send(Some((key, value)));
        });
    }
}
