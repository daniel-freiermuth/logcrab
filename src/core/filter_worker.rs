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

//! Background worker for filter computations.
//!
//! This module provides a worker thread that processes filter requests
//! from both `FilterState` and `HighlightState`, avoiding duplicate threading logic.
//!
//! The worker is owned by the application and shuts down gracefully when dropped.

use crate::core::log_store::{StoreID, StoreVersion};
use crate::core::queue_map::QueueMap;
use crate::core::LogStore;
use fancy_regex::Regex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Cancels a filter request when a newer search snapshot supersedes it.
#[derive(Clone)]
pub struct FilterCancellation(Arc<AtomicBool>);

impl FilterCancellation {
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

impl Default for FilterCancellation {
    fn default() -> Self {
        Self::new()
    }
}

/// Let short-lived snapshots complete so live search remains responsive while typing.
const SUPERSEDED_SEARCH_GRACE: Duration = Duration::from_millis(300);

fn should_abort_superseded_search(is_superseded: bool, elapsed: Duration) -> bool {
    is_superseded && elapsed >= SUPERSEDED_SEARCH_GRACE
}

fn should_abort_filter(request: &FilterRequest, started_at: Instant) -> bool {
    should_abort_superseded_search(request.cancellation.is_cancelled(), started_at.elapsed())
}

/// Schedules interactive filter-tab searches ahead of background highlight searches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterRequestPriority {
    Interactive,
    Background,
}

/// Request to compute filtered indices in background
#[derive(Clone)]
pub struct FilterRequest {
    pub filter_id: usize, // Unique identifier for each filter/highlight instance
    pub regex: Regex,
    pub exclude_regex: Option<Regex>,
    pub store: Arc<LogStore>, // Shared read-only access to log store
    pub result_tx: Sender<FilterResult>, // Each filter has its own result channel
    /// The search text this request was made for (for result tracking)
    pub search_text: String,
    /// The exclude text this request was made for (for result tracking)
    pub exclude_text: String,
    /// Whether case sensitivity was enabled (for result tracking)
    pub case_sensitive: bool,
    /// Whether to deduplicate exact matches (same timestamp, source, message)
    pub hide_duplicates: bool,
    /// Cancels this snapshot after the live-search grace period if it is superseded.
    pub cancellation: FilterCancellation,
    /// Controls whether this request may preempt background work.
    pub priority: FilterRequestPriority,
}

/// Result from background filtering
pub struct FilterResult {
    pub filtered_indices: Arc<Vec<StoreID>>,
    /// The search text these indices were computed for
    pub search_text: String,
    /// The exclude text these indices were computed for
    pub exclude_text: String,
    /// Whether case sensitivity was enabled
    pub case_sensitive: bool,
    /// Whether deduplication was applied
    pub hide_duplicates: bool,
    /// The `LogStore` version these indices were computed for
    pub store_version: StoreVersion,
}

/// Coordinates active work with newly submitted interactive requests.
struct FilterWorkerScheduler {
    active_cancellation: Mutex<Option<FilterCancellation>>,
}

impl FilterWorkerScheduler {
    fn cancel_active(&self) {
        match self.active_cancellation.lock() {
            Ok(active) => {
                if let Some(cancellation) = active.as_ref() {
                    cancellation.cancel();
                }
            }
            Err(error) => tracing::error!("Filter scheduler lock poisoned: {error}"),
        }
    }

    fn activate(&self, cancellation: FilterCancellation) {
        match self.active_cancellation.lock() {
            Ok(mut active) => *active = Some(cancellation),
            Err(error) => tracing::error!("Filter scheduler lock poisoned: {error}"),
        }
    }

    fn deactivate(&self) {
        match self.active_cancellation.lock() {
            Ok(mut active) => *active = None,
            Err(error) => tracing::error!("Filter scheduler lock poisoned: {error}"),
        }
    }
}

/// Clears the active request even when filtering exits early.
struct ActiveFilterRequest {
    scheduler: Arc<FilterWorkerScheduler>,
}

impl ActiveFilterRequest {
    fn new(scheduler: &Arc<FilterWorkerScheduler>, cancellation: FilterCancellation) -> Self {
        scheduler.activate(cancellation);
        Self {
            scheduler: Arc::clone(scheduler),
        }
    }
}

impl Drop for ActiveFilterRequest {
    fn drop(&mut self) {
        self.scheduler.deactivate();
    }
}

/// Handle to send filter requests to the background worker.
///
/// Clone this to send requests from multiple places.
/// When all handles are dropped, the worker thread exits gracefully.
#[derive(Clone)]
pub struct FilterWorkerHandle {
    request_tx: Sender<FilterRequest>,
    pub is_filtering: Arc<AtomicBool>,
    scheduler: Arc<FilterWorkerScheduler>,
}

impl FilterWorkerHandle {
    /// Send a filter request to the background worker.
    pub fn send_request(&self, request: FilterRequest) {
        if request.priority == FilterRequestPriority::Interactive {
            self.scheduler.cancel_active();
        }
        let _ = self.request_tx.send(request);
    }
}

/// Background filter worker that processes filter requests.
///
/// When dropped, the sender channel closes and the worker thread exits.
pub struct FilterWorker {
    /// Handle for sending requests (can be cloned and shared)
    handle: FilterWorkerHandle,
    /// Thread handle (joined on drop for clean shutdown)
    _thread: std::thread::JoinHandle<()>,
}

impl Default for FilterWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterWorker {
    /// Create a new filter worker with a background thread.
    #[must_use]
    pub fn new() -> Self {
        let (request_tx, request_rx) = channel::<FilterRequest>();
        let is_filtering = Arc::new(AtomicBool::new(false));
        let is_filtering_copy = Arc::clone(&is_filtering);
        let scheduler = Arc::new(FilterWorkerScheduler {
            active_cancellation: Mutex::new(None),
        });
        let worker_scheduler = Arc::clone(&scheduler);

        let thread = std::thread::spawn(move || {
            Self::worker_loop(&request_rx, &is_filtering_copy, &worker_scheduler);
        });

        Self {
            handle: FilterWorkerHandle {
                request_tx,
                is_filtering,
                scheduler,
            },
            _thread: thread,
        }
    }

    /// Get a handle to send requests to this worker.
    /// The handle can be cloned and shared across the application.
    #[must_use]
    pub fn handle(&self) -> FilterWorkerHandle {
        self.handle.clone()
    }

    fn enqueue_request(
        request: FilterRequest,
        interactive_requests: &mut QueueMap<usize, FilterRequest>,
        background_requests: &mut QueueMap<usize, FilterRequest>,
    ) {
        let filter_id = request.filter_id;
        let pending_requests = if request.priority == FilterRequestPriority::Interactive {
            interactive_requests
        } else {
            background_requests
        };
        if !pending_requests.insert(filter_id, request) {
            tracing::trace!("Coalescing request for filter {filter_id}");
        }
    }

    /// Background worker loop that processes filter requests.
    fn worker_loop(
        request_rx: &Receiver<FilterRequest>,
        is_filtering: &Arc<AtomicBool>,
        scheduler: &Arc<FilterWorkerScheduler>,
    ) {
        profiling::function_scope!();

        tracing::debug!("Filter worker thread started");

        let mut interactive_requests = QueueMap::new();
        let mut background_requests = QueueMap::new();

        let drain_pending =
            |interactive: &mut QueueMap<usize, FilterRequest>,
             background: &mut QueueMap<usize, FilterRequest>| {
                while let Ok(request) = request_rx.try_recv() {
                    Self::enqueue_request(request, interactive, background);
                }
            };

        // Main processing loop - exits when all senders are dropped
        while let Ok(first_request) = request_rx.recv() {
            is_filtering.store(true, Ordering::Relaxed);
            profiling::scope!("process_filter_request");
            Self::enqueue_request(
                first_request,
                &mut interactive_requests,
                &mut background_requests,
            );

            // Collect any additional pending requests
            drain_pending(&mut interactive_requests, &mut background_requests);

            while let Some((filter_id, request)) = interactive_requests
                .pop_front()
                .or_else(|| background_requests.pop_front())
            {
                let _active_request =
                    ActiveFilterRequest::new(scheduler, request.cancellation.clone());
                drain_pending(&mut interactive_requests, &mut background_requests);
                if !interactive_requests.is_empty() {
                    request.cancellation.cancel();
                }
                profiling::scope!("process_single_filter");
                tracing::trace!("Processing filter request (search: '{:?}')", request.regex);

                let store_version = request.store.version();
                let started_at = Instant::now();
                let should_cancel = || should_abort_filter(&request, started_at);
                let filtered_indices = {
                    profiling::scope!("filter_lines");

                    let Some(filtered_indices) = request.store.get_matching_ids(
                        |display_msg, raw| {
                            let matches_include =
                                request.regex.is_match(display_msg).unwrap_or(false)
                                    || request.regex.is_match(raw).unwrap_or(false);

                            if !matches_include {
                                return false;
                            }

                            request.exclude_regex.as_ref().is_none_or(|exclude_regex| {
                                let matches_exclude =
                                    exclude_regex.is_match(display_msg).unwrap_or(false)
                                        || exclude_regex.is_match(raw).unwrap_or(false);
                                !matches_exclude
                            })
                        },
                        &should_cancel,
                    ) else {
                        tracing::trace!("Discarding superseded filter {filter_id}");
                        drain_pending(&mut interactive_requests, &mut background_requests);
                        continue;
                    };
                    filtered_indices
                };

                // Apply deduplication if requested (serial pass after parallel regex filter)
                let filtered_indices = if request.hide_duplicates {
                    profiling::scope!("dedup_filter");
                    let mut seen = std::collections::HashSet::new();
                    let mut deduplicated = Vec::with_capacity(filtered_indices.len());
                    for id in filtered_indices {
                        if should_cancel() {
                            break;
                        }
                        if let (Some(ts), Some(line)) = (
                            request.store.adjusted_timestamp(&id),
                            request.store.get_by_id(&id),
                        ) {
                            let key = (
                                ts.timestamp_nanos_opt().unwrap_or(0),
                                id.source_id(),
                                line.message,
                            );
                            if seen.insert(key) {
                                deduplicated.push(id);
                            }
                        } else {
                            deduplicated.push(id);
                        }
                    }
                    if should_cancel() {
                        tracing::trace!("Discarding superseded filter {filter_id}");
                        drain_pending(&mut interactive_requests, &mut background_requests);
                        continue;
                    }
                    deduplicated
                } else {
                    filtered_indices
                };

                if should_cancel() {
                    tracing::trace!("Discarding superseded filter {filter_id}");
                    drain_pending(&mut interactive_requests, &mut background_requests);
                    continue;
                }

                tracing::trace!(
                    "Filter {} complete: {} matches",
                    filter_id,
                    filtered_indices.len(),
                );

                let result = FilterResult {
                    filtered_indices: Arc::new(filtered_indices),
                    search_text: request.search_text.clone(),
                    exclude_text: request.exclude_text.clone(),
                    case_sensitive: request.case_sensitive,
                    hide_duplicates: request.hide_duplicates,
                    store_version,
                };

                // Send result back to the specific filter (ignore errors if filter is gone)
                {
                    profiling::scope!("send_result");

                    let _ = request.result_tx.send(result);
                }

                // Check one more time if a newer request arrived during processing
                drain_pending(&mut interactive_requests, &mut background_requests);
            }
            is_filtering.store(false, Ordering::Relaxed);
        }
        tracing::debug!("Filter worker thread shutting down (channel closed)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::log_store::{DataSourceVariant, SourceData};
    use crate::filetype::dmesg::{DmesgFileType, DmesgLogLine};
    use chrono::{Local, TimeZone};
    use std::sync::RwLock;

    /// Never matches `SLOW_LINE`, but fancy-regex's backtracking VM explores an
    /// exponential number of `(a|aa)` splits before failing, so scanning
    /// `SLOW_LINE_COUNT` lines takes far longer than `SUPERSEDED_SEARCH_GRACE` even in
    /// release builds on many cores, while each line stays short enough for the
    /// per-line cancellation check to abort promptly.
    const SLOW_PATTERN: &str = r"^(a|aa)+\1b";
    const SLOW_LINE: &str = "aaaaaaaaaaaaaaaaaaaaaaaa";
    const SLOW_LINE_COUNT: usize = 8192;

    /// Generous upper bound for results that should arrive; never reached on success.
    const RESULT_TIMEOUT: Duration = Duration::from_secs(30);

    struct TestStore {
        store: Arc<LogStore>,
        /// Holds the `.crab` sidecar created by `SourceData::new`.
        _dir: tempfile::TempDir,
    }

    fn store_with_lines(lines: &[&str]) -> TestStore {
        let dir = tempfile::tempdir().expect("tempdir");
        let warnings = crate::ui::ToastManager::new(egui::Context::default()).sender();
        let (source, _, _) = SourceData::<DmesgFileType>::new(
            dir.path().join("test.log"),
            Arc::new(RwLock::new(())),
            &warnings,
        );
        source.append_lines(
            lines
                .iter()
                .enumerate()
                .map(|(index, line)| {
                    let seconds = i64::try_from(index).expect("line index fits in i64");
                    let timestamp = Local
                        .timestamp_opt(seconds, 0)
                        .single()
                        .expect("valid timestamp");
                    DmesgLogLine::new((*line).to_owned(), timestamp, (*line).to_owned(), index)
                })
                .collect(),
        );
        let store = LogStore::new();
        store.add_source(DataSourceVariant::from(Arc::new(source)));
        TestStore { store, _dir: dir }
    }

    fn slow_store() -> TestStore {
        store_with_lines(&vec![SLOW_LINE; SLOW_LINE_COUNT])
    }

    fn request(
        filter_id: usize,
        pattern: &str,
        store: &Arc<LogStore>,
        priority: FilterRequestPriority,
        result_tx: &Sender<FilterResult>,
    ) -> FilterRequest {
        FilterRequest {
            filter_id,
            regex: Regex::new(pattern).expect("valid test regex"),
            exclude_regex: None,
            store: Arc::clone(store),
            result_tx: result_tx.clone(),
            search_text: pattern.to_owned(),
            exclude_text: String::new(),
            case_sensitive: false,
            hide_duplicates: false,
            cancellation: FilterCancellation::new(),
            priority,
        }
    }

    struct WorkerHarness {
        handle: FilterWorkerHandle,
        request_rx: Receiver<FilterRequest>,
        scheduler: Arc<FilterWorkerScheduler>,
    }

    fn worker_harness() -> WorkerHarness {
        let (request_tx, request_rx) = channel();
        let scheduler = Arc::new(FilterWorkerScheduler {
            active_cancellation: Mutex::new(None),
        });
        WorkerHarness {
            handle: FilterWorkerHandle {
                request_tx,
                is_filtering: Arc::new(AtomicBool::new(false)),
                scheduler: Arc::clone(&scheduler),
            },
            request_rx,
            scheduler,
        }
    }

    impl WorkerHarness {
        /// Processes everything already queued, then returns because every handle is dropped.
        fn run_queued_to_shutdown(self) -> (Arc<AtomicBool>, Arc<FilterWorkerScheduler>) {
            let is_filtering = Arc::clone(&self.handle.is_filtering);
            drop(self.handle);
            FilterWorker::worker_loop(&self.request_rx, &is_filtering, &self.scheduler);
            (is_filtering, self.scheduler)
        }

        /// Runs the worker loop on its own thread, like `FilterWorker::new`.
        fn spawn(self) -> (FilterWorkerHandle, std::thread::JoinHandle<()>) {
            let is_filtering = Arc::clone(&self.handle.is_filtering);
            let scheduler = self.scheduler;
            let request_rx = self.request_rx;
            let thread = std::thread::spawn(move || {
                FilterWorker::worker_loop(&request_rx, &is_filtering, &scheduler);
            });
            (self.handle, thread)
        }
    }

    fn has_active_request(scheduler: &FilterWorkerScheduler) -> bool {
        scheduler
            .active_cancellation
            .lock()
            .expect("scheduler lock")
            .is_some()
    }

    fn wait_until_request_is_active(scheduler: &FilterWorkerScheduler) {
        let deadline = Instant::now() + RESULT_TIMEOUT;
        while !has_active_request(scheduler) {
            assert!(
                Instant::now() < deadline,
                "worker never started the request"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Search texts of all delivered results, in delivery order.
    fn delivered_searches(result_rx: &Receiver<FilterResult>) -> Vec<String> {
        result_rx
            .try_iter()
            .map(|result| result.search_text)
            .collect()
    }

    fn fast_store() -> TestStore {
        store_with_lines(&["alpha one", "beta two", "gamma three"])
    }

    #[test]
    fn interactive_requests_run_before_already_queued_background_requests() {
        let test_store = fast_store();
        let (result_tx, result_rx) = channel();
        let harness = worker_harness();
        for (filter_id, pattern, priority) in [
            (1, "alpha", FilterRequestPriority::Background),
            (2, "beta", FilterRequestPriority::Background),
            (3, "gamma", FilterRequestPriority::Interactive),
        ] {
            harness.handle.send_request(request(
                filter_id,
                pattern,
                &test_store.store,
                priority,
                &result_tx,
            ));
        }

        let (is_filtering, scheduler) = harness.run_queued_to_shutdown();

        assert_eq!(delivered_searches(&result_rx), ["gamma", "alpha", "beta"]);
        assert!(!is_filtering.load(Ordering::Relaxed));
        assert!(!has_active_request(&scheduler));
    }

    #[test]
    fn superseded_request_finishing_within_grace_period_still_delivers_result() {
        let test_store = fast_store();
        let (result_tx, result_rx) = channel();
        let harness = worker_harness();
        let superseded = request(
            1,
            "alpha",
            &test_store.store,
            FilterRequestPriority::Interactive,
            &result_tx,
        );
        let superseded_cancellation = superseded.cancellation.clone();
        harness.handle.send_request(superseded);
        harness.handle.send_request(request(
            2,
            "beta",
            &test_store.store,
            FilterRequestPriority::Interactive,
            &result_tx,
        ));

        harness.run_queued_to_shutdown();

        assert!(
            superseded_cancellation.is_cancelled(),
            "a request dequeued while interactive work is pending must be marked superseded"
        );
        let results: Vec<FilterResult> = result_rx.try_iter().collect();
        let searches: Vec<&str> = results.iter().map(|r| r.search_text.as_str()).collect();
        assert_eq!(searches, ["alpha", "beta"]);
        assert_eq!(results[0].filtered_indices.len(), 1);
    }

    #[test]
    fn superseded_request_exceeding_grace_period_is_discarded() {
        let test_store = slow_store();
        let (result_tx, result_rx) = channel();
        let harness = worker_harness();
        harness.handle.send_request(request(
            1,
            SLOW_PATTERN,
            &test_store.store,
            FilterRequestPriority::Interactive,
            &result_tx,
        ));
        harness.handle.send_request(request(
            2,
            "a",
            &test_store.store,
            FilterRequestPriority::Interactive,
            &result_tx,
        ));

        let (is_filtering, scheduler) = harness.run_queued_to_shutdown();

        assert_eq!(delivered_searches(&result_rx), ["a"]);
        assert!(!is_filtering.load(Ordering::Relaxed));
        assert!(
            !has_active_request(&scheduler),
            "discarded requests must not stay registered as active"
        );
    }

    #[test]
    fn interactive_send_preempts_running_interactive_request() {
        let test_store = slow_store();
        let (result_tx, result_rx) = channel();
        let harness = worker_harness();
        let scheduler = Arc::clone(&harness.scheduler);
        let (handle, thread) = harness.spawn();
        let running = request(
            1,
            SLOW_PATTERN,
            &test_store.store,
            FilterRequestPriority::Interactive,
            &result_tx,
        );
        let running_cancellation = running.cancellation.clone();
        handle.send_request(running);
        wait_until_request_is_active(&scheduler);

        handle.send_request(request(
            2,
            "a",
            &test_store.store,
            FilterRequestPriority::Interactive,
            &result_tx,
        ));
        let first_result = result_rx
            .recv_timeout(RESULT_TIMEOUT)
            .expect("preempting request must deliver a result");

        drop(handle);
        thread.join().expect("worker thread panicked");
        assert!(running_cancellation.is_cancelled());
        assert_eq!(first_result.search_text, "a");
        assert_eq!(first_result.filtered_indices.len(), SLOW_LINE_COUNT);
        assert_eq!(
            delivered_searches(&result_rx),
            Vec::<String>::new(),
            "the preempted request must not deliver a result"
        );
    }

    #[test]
    fn superseded_searches_finish_within_grace_period() {
        assert!(!should_abort_superseded_search(
            true,
            Duration::from_millis(299)
        ));
        assert!(should_abort_superseded_search(
            true,
            SUPERSEDED_SEARCH_GRACE
        ));
        assert!(!should_abort_superseded_search(
            false,
            SUPERSEDED_SEARCH_GRACE
        ));
    }

    #[test]
    fn filter_cancellation_marks_the_replaced_snapshot() {
        let cancellation = FilterCancellation::new();
        assert!(!cancellation.is_cancelled());
        cancellation.cancel();
        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn interactive_requests_cancel_active_work() {
        let scheduler = FilterWorkerScheduler {
            active_cancellation: Mutex::new(None),
        };
        let cancellation = FilterCancellation::new();
        scheduler.activate(cancellation.clone());

        scheduler.cancel_active();

        assert!(cancellation.is_cancelled());
    }
}
