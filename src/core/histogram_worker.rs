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
use crate::parser::line::LogLineCore;
use chrono::{DateTime, Local};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

/// Number of horizontal time buckets in the histogram
pub const NUM_BUCKETS: usize = 100;

/// Number of vertical buckets for anomaly score distribution
pub const SCORE_BUCKETS: usize = 20;

/// Distribution of anomaly scores within a histogram bucket
#[derive(Debug, Clone, Copy, Default)]
pub struct AnomalyDistribution {
    pub buckets: [usize; SCORE_BUCKETS], // count per score bucket for a single time bucket
}

/// Request to compute histogram data in background
#[derive(Clone)]
pub struct HistogramRequest {
    pub key: HistogramCacheKey,
    /// Unique identifier for the filter this histogram belongs to
    pub filter_id: usize,
    /// The log store to read from
    pub store: Arc<LogStore>,
    /// Filtered indices to compute histogram for
    pub filtered_indices: Vec<StoreID>,
    /// Optional zoom range - if Some, compute buckets only for this range
    /// If None, compute for full data range
    pub zoom_range: Option<(DateTime<Local>, DateTime<Local>)>,
    /// Channel to send result back
    pub result_tx: Sender<HistogramResult>,
}

/// Cache key for validating histogram results
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HistogramCacheKey {
    pub store_version: u64,
    pub search_str: String,
    pub case_sensitive: bool,
    /// Zoom range in milliseconds (for cache invalidation)
    /// None means full range
    pub zoom_range_ms: Option<(i64, i64)>,
}

/// Result from background histogram computation
#[derive(Clone)]
pub struct HistogramResult {
    /// Cache key for validation
    pub cache_key: HistogramCacheKey,
    pub data: Option<HistogramData>,
}

#[derive(Clone)]
pub struct HistogramData {
    /// Time range of the histogram (may be zoomed subset)
    pub start_time: DateTime<Local>,
    pub end_time: DateTime<Local>,
    /// Full data time range (for zoom reset)
    pub full_start: DateTime<Local>,
    pub full_end: DateTime<Local>,
    /// Count per time bucket
    pub buckets: Vec<usize>,
    /// Anomaly score distribution per bucket
    pub anomaly_buckets: Vec<AnomalyDistribution>,
    /// Filtered indices used for histogram computation
    pub filtered_indices: Vec<StoreID>,
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
        let drain_pending = |map: &mut HashMap<usize, HistogramRequest>| {
            while let Ok(request) = request_rx.try_recv() {
                let filter_id = request.filter_id;
                if map.contains_key(&filter_id) {
                    log::trace!("Updating pending histogram request for filter {filter_id}");
                }
                map.insert(filter_id, request);
            }
        };

        // Main processing loop - exits when all senders are dropped
        while let Ok(first_request) = request_rx.recv() {
            profiling::scope!("process_histogram_request");
            pending_requests.insert(first_request.filter_id, first_request);

            // Collect any additional pending requests
            drain_pending(&mut pending_requests);

            while let Some(&first_key) = pending_requests.keys().next() {
                let request = pending_requests
                    .remove(&first_key)
                    .expect("key exists from keys().next()");
                let filter_id = request.filter_id;

                profiling::scope!("process_single_histogram");
                log::trace!("Processing histogram request for filter {filter_id}");

                let result_channel = request.result_tx.clone();

                let result = Self::compute_histogram(request);

                log::trace!("Histogram {filter_id} complete",);

                // Send result back (ignore errors if receiver is gone)
                let _ = result_channel.send(result);

                // Check one more time if a newer request arrived during processing
                drain_pending(&mut pending_requests);
            }
        }
        log::debug!("Histogram worker thread shutting down (channel closed)");
    }

    /// Compute histogram data
    fn compute_histogram(request: HistogramRequest) -> HistogramResult {
        profiling::scope!("HistogramWorker::compute_histogram");

        let store = &request.store;
        let filtered_indices = request.filtered_indices;

        // Calculate full data range first
        let full_range = Self::calculate_time_range(store, &filtered_indices);

        let Some((full_start, full_end)) = full_range else {
            return HistogramResult {
                cache_key: request.key.clone(),
                data: None,
            };
        };

        // Use zoom range if provided, otherwise use full range
        let (start_time, end_time) = request.zoom_range.unwrap_or((full_start, full_end));

        // Clamp zoom range to actual data bounds
        let start_time = start_time.max(full_start);
        let end_time = end_time.min(full_end);

        if start_time >= end_time {
            return HistogramResult {
                cache_key: request.key.clone(),
                data: None,
            };
        }

        let time_span = end_time - start_time;
        let bucket_size = Duration::from_secs_f64(time_span.as_seconds_f64() / NUM_BUCKETS as f64);

        // Filter indices to only those within the zoom range
        let zoomed_indices: Vec<StoreID> = if request.zoom_range.is_some() {
            profiling::scope!("Histogram::filter_zoom_range");
            filtered_indices
                .iter()
                .filter(|idx| {
                    store.get_by_id(idx).is_some_and(|line| {
                        let ts = line.timestamp();
                        ts >= start_time && ts <= end_time
                    })
                })
                .copied()
                .collect()
        } else {
            filtered_indices.clone()
        };

        let (buckets, anomaly_buckets) =
            Self::create_buckets(store, &zoomed_indices, start_time, bucket_size);

        HistogramResult {
            cache_key: request.key.clone(),
            data: Some(HistogramData {
                start_time,
                end_time,
                full_start,
                full_end,
                buckets,
                anomaly_buckets,
                filtered_indices,
            }),
        }
    }

    fn calculate_time_range(
        store: &LogStore,
        filtered_indices: &[StoreID],
    ) -> Option<(DateTime<Local>, DateTime<Local>)> {
        profiling::scope!("Histogram::calculate_time_range");
        // TODO first?
        let first_ts = filtered_indices
            .iter()
            .filter_map(|idx| store.get_by_id(idx))
            .map(|line| line.timestamp())
            .next();
        let last_ts = filtered_indices
            .iter()
            .rev()
            .filter_map(|idx| store.get_by_id(idx))
            .map(|line| line.timestamp())
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
        bucket_size: Duration,
    ) -> (Vec<usize>, Vec<AnomalyDistribution>) {
        profiling::scope!("Histogram::create_buckets");
        let mut buckets = vec![0usize; NUM_BUCKETS];
        let mut anomaly_distributions = vec![AnomalyDistribution::default(); NUM_BUCKETS];

        // possible optimization: par_iter
        for line_idx in filtered_indices {
            if let Some(line) = store.get_by_id(line_idx) {
                let ts = line.timestamp();
                let bucket_idx = Self::timestamp_to_bucket(ts, start_time, bucket_size);
                buckets[bucket_idx] += 1;

                // Use anomaly_score from the line
                let line_score = line.anomaly_score() / 100.0;
                // Determine which score bucket this falls into
                let score_bucket =
                    ((line_score * SCORE_BUCKETS as f64).floor() as usize).min(SCORE_BUCKETS - 1);
                anomaly_distributions[bucket_idx].buckets[score_bucket] += 1;
            }
        }

        (buckets, anomaly_distributions)
    }

    fn timestamp_to_bucket(
        ts: DateTime<Local>,
        start_time: DateTime<Local>,
        bucket_size: Duration,
    ) -> usize {
        let elapsed = ts - start_time;
        ((elapsed.as_seconds_f64() / bucket_size.as_secs_f64()) as usize).min(NUM_BUCKETS - 1)
    }
}

impl Default for HistogramWorker {
    fn default() -> Self {
        Self::new()
    }
}
