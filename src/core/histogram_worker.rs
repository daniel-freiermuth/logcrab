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

//! Background worker for histogram cache computation.
//!
//! This module provides a worker thread that processes histogram requests,
//! deduplicating rapid successive requests to only process the most recent one.
//!
//! The worker is owned by the application and shuts down gracefully when dropped.

use crate::core::log_store::StoreID;
use crate::core::LogStore;
use chrono::{DateTime, Datelike, Local};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// Number of horizontal time buckets in the histogram
pub const NUM_BUCKETS: usize = 100;

/// Number of vertical buckets for anomaly score distribution
pub const SCORE_BUCKETS: usize = 20;

/// Distribution of anomaly scores within a histogram bucket
#[derive(Debug, Clone, Copy, Default)]
pub struct AnomalyDistribution {
    pub buckets: [usize; SCORE_BUCKETS],
}

/// Request to compute histogram data in background
#[derive(Clone)]
pub struct HistogramRequest {
    /// Unique identifier for the filter this histogram belongs to
    pub filter_id: usize,
    /// The log store to read from
    pub store: Arc<LogStore>,
    /// Filtered indices to compute histogram for
    pub filtered_indices: Vec<StoreID>,
    /// Whether to hide January 1st timestamps (epoch)
    pub hide_epoch: bool,
    /// Store version at request time (for cache validation)
    pub store_version: u64,
    /// Channel to send result back
    pub result_tx: Sender<HistogramResult>,
}

/// Cache key for validating histogram results
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HistogramCacheKey {
    pub store_version: u64,
    pub indices_len: usize,
    pub hide_epoch: bool,
}

/// Result from background histogram computation
#[derive(Clone)]
pub struct HistogramResult {
    /// Filter ID this result belongs to (useful for debugging)
    #[allow(dead_code)]
    pub filter_id: usize,
    /// Cache key for validation
    pub cache_key: HistogramCacheKey,
    /// Filtered indices after epoch removal (if `hide_epoch` is true)
    pub effective_indices: Vec<StoreID>,
    /// Time range of the histogram
    pub time_range: Option<(DateTime<Local>, DateTime<Local>)>,
    /// Bucket size in seconds
    pub bucket_size: f64,
    /// Count per time bucket
    pub buckets: Vec<usize>,
    /// Anomaly score distribution per bucket
    pub anomaly_buckets: Vec<AnomalyDistribution>,
}

/// Handle to send histogram requests to the background worker.
///
/// Clone this to send requests from multiple places.
/// When all handles are dropped, the worker thread exits gracefully.
#[derive(Clone)]
pub struct HistogramWorkerHandle {
    request_tx: Sender<HistogramRequest>,
}

impl HistogramWorkerHandle {
    /// Send a histogram computation request
    pub fn send_request(&self, request: HistogramRequest) {
        let _ = self.request_tx.send(request);
    }
}

/// Background histogram worker that processes requests with deduplication.
///
/// When multiple requests arrive for the same filter, only the most recent
/// one is processed - older requests are discarded.
pub struct HistogramWorker {
    /// Handle for sending requests (can be cloned and shared)
    handle: HistogramWorkerHandle,
    /// Thread handle (joined on drop for clean shutdown)
    _thread: std::thread::JoinHandle<()>,
}

impl HistogramWorker {
    /// Create a new histogram worker with a background thread.
    #[must_use]
    pub fn new() -> Self {
        let (request_tx, request_rx) = channel::<HistogramRequest>();

        let thread = std::thread::spawn(move || {
            Self::worker_loop(&request_rx);
        });

        Self {
            handle: HistogramWorkerHandle { request_tx },
            _thread: thread,
        }
    }

    /// Get a handle to send requests to this worker.
    pub fn handle(&self) -> HistogramWorkerHandle {
        self.handle.clone()
    }

    /// Background worker loop that processes histogram requests.
    fn worker_loop(request_rx: &Receiver<HistogramRequest>) {
        profiling::function_scope!();

        log::debug!("Histogram worker thread started");

        // Persistent HashMap of pending requests per filter
        // This allows us to accumulate and update requests while processing others
        let mut pending_requests: HashMap<usize, HistogramRequest> = HashMap::new();

        // Helper to drain all available requests into the HashMap
        let drain_pending = |pending: &mut HashMap<usize, HistogramRequest>| {
            while let Ok(request) = request_rx.try_recv() {
                let filter_id = request.filter_id;
                if pending.contains_key(&filter_id) {
                    log::trace!("Updating pending histogram request for filter {filter_id}");
                }
                pending.insert(filter_id, request);
            }
        };

        // Main processing loop - exits when all senders are dropped
        while let Ok(first_request) = request_rx.recv() {
            profiling::scope!("process_histogram_request");
            pending_requests.insert(first_request.filter_id, first_request);

            // Collect any additional pending requests
            drain_pending(&mut pending_requests);

            while !pending_requests.is_empty() {
                let first_key = *pending_requests.keys().next().unwrap();
                let request = pending_requests.remove(&first_key).unwrap();
                let filter_id = request.filter_id;

                profiling::scope!("process_single_histogram");
                log::trace!("Processing histogram request for filter {filter_id}");

                let result = Self::compute_histogram(&request);

                log::trace!(
                    "Histogram {} complete: {} effective indices",
                    filter_id,
                    result.effective_indices.len(),
                );

                // Send result back (ignore errors if receiver is gone)
                let _ = request.result_tx.send(result);

                // Check one more time if a newer request arrived during processing
                drain_pending(&mut pending_requests);
            }
        }
        log::debug!("Histogram worker thread shutting down (channel closed)");
    }

    /// Compute histogram data
    fn compute_histogram(request: &HistogramRequest) -> HistogramResult {
        profiling::scope!("HistogramWorker::compute_histogram");

        let store = &request.store;
        let filtered_indices = &request.filtered_indices;
        let hide_epoch = request.hide_epoch;

        // Filter out January 1st timestamps if requested
        let effective_indices: Vec<StoreID> = if hide_epoch {
            profiling::scope!("Histogram::filter_epoch");
            filtered_indices
                .iter()
                .filter_map(|idx| {
                    store.get_by_id(idx).and_then(|line| {
                        let ts = line.timestamp;
                        // Exclude all timestamps that are January 1st (any year)
                        if ts.month0() == 0 && ts.day0() == 0 {
                            None
                        } else {
                            Some(*idx)
                        }
                    })
                })
                .collect()
        } else {
            filtered_indices.clone()
        };

        // Calculate time range
        let time_range = Self::calculate_time_range(store, &effective_indices);

        let (bucket_size, buckets, anomaly_buckets) =
            if let Some((start_time, end_time)) = time_range {
                let time_span = (end_time.timestamp() - start_time.timestamp()).max(1);
                let bucket_size = time_span as f64 / NUM_BUCKETS as f64;

                let (buckets, anomaly_buckets) =
                    Self::create_buckets(store, &effective_indices, start_time, bucket_size);

                (bucket_size, buckets, anomaly_buckets)
            } else {
                (0.0, Vec::new(), Vec::new())
            };

        HistogramResult {
            filter_id: request.filter_id,
            cache_key: HistogramCacheKey {
                store_version: request.store_version,
                indices_len: request.filtered_indices.len(),
                hide_epoch,
            },
            effective_indices,
            time_range,
            bucket_size,
            buckets,
            anomaly_buckets,
        }
    }

    fn calculate_time_range(
        store: &LogStore,
        filtered_indices: &[StoreID],
    ) -> Option<(DateTime<Local>, DateTime<Local>)> {
        profiling::scope!("Histogram::calculate_time_range");
        let first_ts = filtered_indices
            .iter()
            .filter_map(|idx| store.get_by_id(idx))
            .map(|line| line.timestamp)
            .next();
        let last_ts = filtered_indices
            .iter()
            .rev()
            .filter_map(|idx| store.get_by_id(idx))
            .map(|line| line.timestamp)
            .next();

        match (first_ts, last_ts) {
            (Some(start), Some(end)) => Some((start, end)),
            _ => None,
        }
    }

    fn create_buckets(
        store: &LogStore,
        filtered_indices: &[StoreID],
        start_time: DateTime<Local>,
        bucket_size: f64,
    ) -> (Vec<usize>, Vec<AnomalyDistribution>) {
        profiling::scope!("Histogram::create_buckets");
        let mut buckets = vec![0usize; NUM_BUCKETS];
        let mut anomaly_distributions = vec![AnomalyDistribution::default(); NUM_BUCKETS];

        for line_idx in filtered_indices {
            if let Some(line) = store.get_by_id(line_idx) {
                let ts = line.timestamp;
                let bucket_idx = Self::timestamp_to_bucket(ts, start_time, bucket_size);
                buckets[bucket_idx] += 1;

                // Use anomaly_score from the line
                let score = line.anomaly_score / 100.0;
                // Determine which score bucket this falls into
                let score_bucket =
                    ((score * SCORE_BUCKETS as f64).floor() as usize).min(SCORE_BUCKETS - 1);
                anomaly_distributions[bucket_idx].buckets[score_bucket] += 1;
            }
        }

        (buckets, anomaly_distributions)
    }

    fn timestamp_to_bucket(
        ts: DateTime<Local>,
        start_time: DateTime<Local>,
        bucket_size: f64,
    ) -> usize {
        let elapsed = (ts.timestamp() - start_time.timestamp()) as f64;
        ((elapsed / bucket_size) as usize).min(NUM_BUCKETS - 1)
    }
}

impl Default for HistogramWorker {
    fn default() -> Self {
        Self::new()
    }
}
