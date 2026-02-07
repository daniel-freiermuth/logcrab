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

//! Generic task worker with deduplication.
//!
//! Provides a background thread that executes tasks, keeping only the latest
//! task per dedup key. Older pending tasks for the same key are discarded.

use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

type Task = Box<dyn FnOnce() + Send>;

/// Handle to submit tasks to the worker.
///
/// Clone this to submit from multiple places.
/// When all handles are dropped, the worker thread exits gracefully.
pub struct TaskWorkerHandle<D> {
    request_tx: Sender<(D, Task)>,
}

impl<D> Clone for TaskWorkerHandle<D> {
    fn clone(&self) -> Self {
        Self {
            request_tx: self.request_tx.clone(),
        }
    }
}

impl<D: Send + 'static> TaskWorkerHandle<D> {
    /// Submit a task. If a task with the same dedup_key is pending, it's replaced.
    pub fn submit<F>(&self, dedup_key: D, work: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let _ = self.request_tx.send((dedup_key, Box::new(work)));
    }
}

/// Single-threaded task worker with deduplication.
///
/// Tasks are identified by a dedup key. If multiple tasks arrive for the same
/// key before execution starts, only the latest is executed.
pub struct TaskWorker<D> {
    handle: TaskWorkerHandle<D>,
    _thread: thread::JoinHandle<()>,
}

impl<D> TaskWorker<D>
where
    D: Ord + Clone + Send + 'static,
{
    /// Create a new task worker with a background thread.
    #[must_use]
    pub fn new() -> Self {
        let (request_tx, request_rx) = channel();

        let thread = thread::spawn(move || {
            Self::worker_loop(request_rx);
        });

        Self {
            handle: TaskWorkerHandle { request_tx },
            _thread: thread,
        }
    }

    /// Get a handle to submit tasks to this worker.
    pub fn handle(&self) -> TaskWorkerHandle<D> {
        self.handle.clone()
    }

    fn worker_loop(request_rx: Receiver<(D, Task)>) {
        let mut pending: BTreeMap<D, Task> = BTreeMap::new();

        // Drain all available requests, keeping only latest per key
        let drain = |pending: &mut BTreeMap<D, Task>, rx: &Receiver<(D, Task)>| {
            while let Ok((key, work)) = rx.try_recv() {
                pending.insert(key, work);
            }
        };

        // Main loop - exits when all senders are dropped
        while let Ok((key, work)) = request_rx.recv() {
            pending.insert(key, work);
            drain(&mut pending, &request_rx);

            while let Some((_key, task)) = pending.pop_first() {
                task();
                drain(&mut pending, &request_rx);
            }
        }
    }
}

impl<D: Ord + Clone + Send + 'static> Default for TaskWorker<D> {
    fn default() -> Self {
        Self::new()
    }
}
