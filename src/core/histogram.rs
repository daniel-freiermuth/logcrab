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

//! Histogram data types and computation logic.

use crate::core::log_store::StoreID;
use crate::core::LogStore;
use chrono::{DateTime, Datelike, Local};

/// Number of horizontal time buckets in the histogram
pub const NUM_BUCKETS: usize = 100;

/// Number of vertical buckets for anomaly score distribution
pub const SCORE_BUCKETS: usize = 20;

/// Distribution of anomaly scores within a histogram bucket
#[derive(Debug, Clone, Copy, Default)]
pub struct AnomalyDistribution {
    pub buckets: [usize; SCORE_BUCKETS],
}

/// Cache key for histogram data
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HistogramKey {
    pub store_version: u64,
    pub hide_epoch: bool,
    pub search_str: String,
    pub case_sensitive: bool,
}

/// Computed histogram data
#[derive(Clone)]
pub struct HistogramData {
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

impl HistogramData {
    /// Compute histogram data from filtered indices
    pub fn compute(
        store: &LogStore,
        filtered_indices: &[StoreID],
        hide_epoch: bool,
    ) -> Self {
        profiling::scope!("HistogramData::compute");

        // Filter out January 1st timestamps if requested
        let effective_indices: Vec<StoreID> = if hide_epoch {
            profiling::scope!("filter_epoch");
            filtered_indices
                .iter()
                .filter_map(|idx| {
                    store.get_by_id(idx).and_then(|line| {
                        let ts = line.timestamp;
                        if ts.month0() == 0 && ts.day0() == 0 {
                            None
                        } else {
                            Some(*idx)
                        }
                    })
                })
                .collect()
        } else {
            filtered_indices.to_vec()
        };

        let time_range = Self::calculate_time_range(store, &effective_indices);

        let (bucket_size, buckets, anomaly_buckets) =
            if let Some((start_time, end_time)) = time_range {
                let time_span = (end_time - start_time).as_seconds_f64();
                let bucket_size = time_span / NUM_BUCKETS as f64;
                let (buckets, anomaly_buckets) =
                    Self::create_buckets(store, &effective_indices, start_time, bucket_size);
                (bucket_size, buckets, anomaly_buckets)
            } else {
                (0.0, Vec::new(), Vec::new())
            };

        Self {
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
        profiling::scope!("calculate_time_range");
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
        profiling::scope!("create_buckets");
        let mut buckets = vec![0usize; NUM_BUCKETS];
        let mut anomaly_distributions = vec![AnomalyDistribution::default(); NUM_BUCKETS];

        for line_idx in filtered_indices {
            if let Some(line) = store.get_by_id(line_idx) {
                let ts = line.timestamp;
                let bucket_idx = Self::timestamp_to_bucket(ts, start_time, bucket_size);
                buckets[bucket_idx] += 1;

                let score = line.anomaly_score / 100.0;
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
