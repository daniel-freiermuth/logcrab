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

use crate::core::{log_store::StoreID, LogStore};
use crate::ui::tabs::filter_tab::log_table;
use chrono::{DateTime, Datelike, Local};
use egui::{Color32, Ui};

/// Number of horizontal time buckets in the histogram
const NUM_BUCKETS: usize = 100;

/// Number of vertical buckets for anomaly score distribution
const SCORE_BUCKETS: usize = 20;

/// Marker data for showing filter matches in histogram
#[derive(Clone)]
pub struct HistogramMarker {
    pub name: String,
    pub indices: Vec<StoreID>,
    pub color: Color32,
}

/// Distribution of anomaly scores within a histogram bucket
#[derive(Debug, Clone, Copy, Default)]
struct AnomalyDistribution {
    buckets: [usize; SCORE_BUCKETS],
}

/// Event emitted when histogram is clicked
#[derive(Clone)]
pub struct HistogramClickEvent {
    pub line_index: StoreID,
}

/// Cached histogram computation results
#[derive(Clone, Default)]
pub struct HistogramCache {
    /// Cache key: (store_version, indices_len, hide_epoch)
    key: (u64, usize, bool),
    /// Filtered indices after epoch removal (if hide_epoch is true)
    effective_indices: Vec<StoreID>,
    /// Time range of the histogram
    time_range: Option<(DateTime<Local>, DateTime<Local>)>,
    /// Bucket size in seconds
    bucket_size: f64,
    /// Count per time bucket
    buckets: Vec<usize>,
    /// Anomaly score distribution per bucket
    anomaly_buckets: Vec<AnomalyDistribution>,
}

impl HistogramCache {
    /// Check if cache is valid for the given inputs
    fn is_valid(&self, store_version: u64, indices_len: usize, hide_epoch: bool) -> bool {
        self.key == (store_version, indices_len, hide_epoch)
    }
}

/// Calculate which bucket a timestamp belongs to
fn timestamp_to_bucket(
    ts: chrono::DateTime<chrono::Local>,
    start_time: chrono::DateTime<chrono::Local>,
    bucket_size: f64,
) -> usize {
    let elapsed = (ts.timestamp() - start_time.timestamp()) as f64;
    ((elapsed / bucket_size) as usize).min(NUM_BUCKETS - 1)
}

/// Reusable timeline histogram component
pub struct Histogram;

impl Histogram {
    /// Render the timeline histogram
    ///
    /// Returns Some(event) if the histogram was clicked
    pub fn render(
        ui: &mut Ui,
        store: &LogStore,
        filtered_indices: &[StoreID],
        selected_line_index: Option<StoreID>,
        hide_epoch: bool,
        markers: &[HistogramMarker],
        cache: &mut HistogramCache,
    ) -> Option<HistogramClickEvent> {
        profiling::scope!("Histogram::render");

        if store.total_lines() == 0 {
            return None;
        }
        if filtered_indices.is_empty() {
            ui.label("No logs match the current filter");
            return None;
        }

        let store_version = store.version();

        // Check if we need to recompute the cached data
        if !cache.is_valid(store_version, filtered_indices.len(), hide_epoch) {
            profiling::scope!("Histogram::recompute_cache");

            // Filter out January 1st timestamps if requested
            let effective_indices: Vec<StoreID> = if hide_epoch {
                profiling::scope!("Histogram::filter_epoch");
                filtered_indices
                    .iter()
                    .filter_map(|idx| {
                        store.get_by_id(idx).and_then(|line| {
                            let ts = line.timestamp;
                            // Exclude all timestamps that are January 1st (any year)
                            if !(ts.month0() == 0 && ts.day0() == 0) {
                                Some(idx.clone())
                            } else {
                                None
                            }
                        })
                    })
                    .collect()
            } else {
                filtered_indices.to_vec()
            };

            // Calculate time range
            let time_range = Self::calculate_time_range(store, &effective_indices);

            if let Some((start_time, end_time)) = time_range {
                let time_span = (end_time.timestamp() - start_time.timestamp()).max(1);
                let bucket_size = time_span as f64 / NUM_BUCKETS as f64;

                let (buckets, anomaly_buckets) =
                    Self::create_buckets(store, &effective_indices, start_time, bucket_size);

                cache.time_range = Some((start_time, end_time));
                cache.bucket_size = bucket_size;
                cache.buckets = buckets;
                cache.anomaly_buckets = anomaly_buckets;
            } else {
                cache.time_range = None;
                cache.buckets.clear();
                cache.anomaly_buckets.clear();
            }

            cache.effective_indices = effective_indices;
            cache.key = (store_version, filtered_indices.len(), hide_epoch);
        }

        if cache.time_range.is_none() {
            ui.label("No timestamps available for histogram");
            return None;
        };

        // Now render using cached data
        if cache.effective_indices.is_empty() {
            ui.label("No logs match the current filter (all timestamps are January 1st)");
            return None;
        }

        Self::render_cached(ui, store, cache, selected_line_index, markers)
    }

    fn render_cached(
        ui: &mut Ui,
        store: &LogStore,
        cache: &HistogramCache,
        selected_line_index: Option<StoreID>,
        markers: &[HistogramMarker],
    ) -> Option<HistogramClickEvent> {
        let max_count = *cache.buckets.iter().max().unwrap_or(&1);

        let selected_x_fraction = Self::calculate_selected_x_fraction(
            store,
            selected_line_index.clone(),
            cache.time_range.unwrap().0,
            cache.time_range.unwrap().1,
        );

        let dark_mode = ui.visuals().dark_mode;
        let bg_color = ui.visuals().extreme_bg_color;

        let click_event = Self::render_histogram_bars(
            ui,
            cache,
            max_count,
            selected_x_fraction,
            store,
            markers,
            dark_mode,
            bg_color,
        );

        Self::render_timeline_labels(
            ui,
            cache.time_range.unwrap().0,
            cache.time_range.unwrap().1,
            store,
            selected_line_index,
        );

        click_event
    }

    fn calculate_time_range(
        store: &LogStore,
        filtered_indices: &[StoreID],
    ) -> Option<(
        chrono::DateTime<chrono::Local>,
        chrono::DateTime<chrono::Local>,
    )> {
        profiling::scope!("Histogram::calculate_time_range");
        let first_ts = filtered_indices
            .iter()
            .map(|idx| store.get_by_id(idx).unwrap().timestamp)
            .next();
        let last_ts = filtered_indices
            .iter()
            .rev()
            .map(|idx| store.get_by_id(idx).unwrap().timestamp)
            .next();

        match (first_ts, last_ts) {
            (Some(start), Some(end)) => Some((start, end)),
            _ => None,
        }
    }

    fn create_buckets(
        store: &LogStore,
        filtered_indices: &[StoreID],
        start_time: chrono::DateTime<chrono::Local>,
        bucket_size: f64,
    ) -> (Vec<usize>, Vec<AnomalyDistribution>) {
        profiling::scope!("Histogram::create_buckets");
        let mut buckets = vec![0usize; NUM_BUCKETS];
        let mut anomaly_distributions = vec![AnomalyDistribution::default(); NUM_BUCKETS];

        for line_idx in filtered_indices {
            let line = store.get_by_id(line_idx).unwrap();
            let ts = line.timestamp;
            let bucket_idx = timestamp_to_bucket(ts, start_time, bucket_size);
            buckets[bucket_idx] += 1;

            // Use anomaly_score from the line
            let score = line.anomaly_score / 100.0;
            // Determine which score bucket this falls into
            let score_bucket =
                ((score * SCORE_BUCKETS as f64).floor() as usize).min(SCORE_BUCKETS - 1);
            anomaly_distributions[bucket_idx].buckets[score_bucket] += 1;
        }

        (buckets, anomaly_distributions)
    }

    fn calculate_selected_x_fraction(
        store: &LogStore,
        selected_line_index: Option<StoreID>,
        start_time: chrono::DateTime<chrono::Local>,
        end_time: chrono::DateTime<chrono::Local>,
    ) -> Option<f32> {
        let selected_line_index = selected_line_index?;
        let sel_ts = store.get_by_id(&selected_line_index).unwrap().timestamp;
        let total_duration = (end_time.timestamp() - start_time.timestamp()) as f64;

        if total_duration <= 0.0 {
            return None;
        }

        let elapsed = (sel_ts.timestamp_millis() - start_time.timestamp_millis()) as f64;
        let total_millis = (end_time.timestamp_millis() - start_time.timestamp_millis()) as f64;

        if elapsed >= 0.0 && elapsed <= total_millis {
            Some((elapsed / total_millis) as f32)
        } else {
            None
        }
    }

    fn render_histogram_bars(
        ui: &mut Ui,
        cache: &HistogramCache,
        max_count: usize,
        selected_x_fraction: Option<f32>,
        store: &LogStore,
        markers: &[HistogramMarker],
        dark_mode: bool,
        bg_color: Color32,
    ) -> Option<HistogramClickEvent> {
        profiling::scope!("Histogram::draw_bars");
        let desired_size = egui::vec2(ui.available_width(), 60.0);
        let (response, painter) = ui.allocate_painter(desired_size, egui::Sense::click_and_drag());
        let rect = response.rect;

        painter.rect_filled(rect, 0.0, bg_color);

        let bar_width = rect.width() / NUM_BUCKETS as f32;

        Self::draw_bars(
            &painter,
            rect,
            cache.buckets.as_slice(),
            cache.anomaly_buckets.as_slice(),
            max_count,
            bar_width,
            dark_mode,
        );
        Self::draw_markers(
            &painter,
            rect,
            store,
            cache.time_range.unwrap().0,
            cache.bucket_size,
            markers,
        );
        Self::draw_selected_indicator(&painter, rect, selected_x_fraction);

        // Handle hover tooltip for markers
        Self::handle_marker_hover(
            ui,
            &response,
            rect,
            store,
            cache.time_range.unwrap().0,
            cache.bucket_size,
            markers,
        );
        Self::handle_click(
            &response,
            rect,
            bar_width,
            store,
            &cache.effective_indices,
            cache.time_range.unwrap().0,
            cache.bucket_size,
        )
    }

    fn draw_bars(
        painter: &egui::Painter,
        rect: egui::Rect,
        buckets: &[usize],
        anomaly_buckets: &[AnomalyDistribution],
        max_count: usize,
        bar_width: f32,
        dark_mode: bool,
    ) {
        for (i, &count) in buckets.iter().enumerate() {
            if count > 0 {
                let x = rect.min.x + i as f32 * bar_width;
                let total_height = (count as f32 / max_count as f32) * rect.height();

                // Draw gradient based on anomaly distribution
                let dist = &anomaly_buckets[i];
                let total: usize = dist.buckets.iter().sum();

                if total > 0 {
                    // Use gradient to visualize the anomaly intensity
                    Self::draw_gradient_bar(
                        painter,
                        x,
                        rect.max.y,
                        bar_width,
                        total_height,
                        dist,
                        total as f32,
                        dark_mode,
                    );
                } else {
                    // No anomaly data, use default blue
                    let y = rect.max.y - total_height;
                    let bar_rect = egui::Rect::from_min_size(
                        egui::pos2(x, y),
                        egui::vec2(bar_width.max(1.0), total_height),
                    );
                    painter.rect_filled(bar_rect, 0.0, Color32::from_rgb(100, 150, 255));
                }
            }
        }
    }

    /// Draw a bar with a vertical gradient based on anomaly distribution
    /// Each score bucket gets a segment with height proportional to its count
    fn draw_gradient_bar(
        painter: &egui::Painter,
        x: f32,
        bottom_y: f32,
        bar_width: f32,
        total_height: f32,
        dist: &AnomalyDistribution,
        total: f32,
        dark_mode: bool,
    ) {
        let mut current_y = bottom_y;

        // Draw each score bucket from bottom (low scores) to top (high scores)
        for bucket_idx in 0..SCORE_BUCKETS {
            let count = dist.buckets[bucket_idx];
            if count == 0 {
                continue;
            }

            let segment_height = (count as f32 / total) * total_height;

            let score = ((bucket_idx as f32 + 1.0) / SCORE_BUCKETS as f32) * 100.0;

            let color = log_table::score_to_color(score as f64, dark_mode);

            let y = current_y - segment_height;
            let segment_rect = egui::Rect::from_min_size(
                egui::pos2(x, y),
                egui::vec2(bar_width.max(1.0), segment_height.max(5.0)),
            );
            painter.rect_filled(segment_rect, 0.0, color);

            current_y = y;
        }
    }

    fn draw_markers(
        painter: &egui::Painter,
        rect: egui::Rect,
        store: &LogStore,
        start_time: chrono::DateTime<chrono::Local>,
        bucket_size: f64,
        markers: &[HistogramMarker],
    ) {
        profiling::scope!("Histogram::draw_markers");
        let total_width = rect.width();
        let total_time = NUM_BUCKETS as f64 * bucket_size;

        for marker in markers {
            for line_idx in &marker.indices {
                let ts = store.get_by_id(line_idx).unwrap().timestamp;
                let elapsed = (ts.timestamp() - start_time.timestamp()) as f64;

                let x = rect.min.x + (elapsed / total_time * total_width as f64) as f32;

                painter.vline(x, rect.y_range(), (1.0, marker.color));
            }
        }
    }

    fn handle_marker_hover(
        ui: &mut Ui,
        response: &egui::Response,
        rect: egui::Rect,
        store: &LogStore,
        start_time: chrono::DateTime<chrono::Local>,
        bucket_size: f64,
        markers: &[HistogramMarker],
    ) {
        let Some(hover_pos) = response.hover_pos() else {
            return;
        };

        let total_width = rect.width();
        let total_time = NUM_BUCKETS as f64 * bucket_size;
        let hover_threshold = 3.0; // pixels

        struct MarkerMatch<'a> {
            marker: &'a HistogramMarker,
            distance: f32,
            x_pos: f32,
        }

        let mut closest_match: Option<MarkerMatch> = None;

        for marker in markers {
            for line_idx in &marker.indices {
                let ts = store.get_by_id(line_idx).unwrap().timestamp;
                let elapsed = (ts.timestamp() - start_time.timestamp()) as f64;
                let x = rect.min.x + (elapsed / total_time * total_width as f64) as f32;

                let distance = (hover_pos.x - x).abs();
                if distance < hover_threshold
                    && closest_match.as_ref().is_none_or(|m| distance < m.distance)
                {
                    closest_match = Some(MarkerMatch {
                        marker,
                        distance,
                        x_pos: x,
                    });
                }
            }
        }

        if let Some(closest) = closest_match {
            // Show tooltip near the marker line (just above the histogram)
            let tooltip_pos = egui::pos2(closest.x_pos, rect.min.y - 5.0);
            egui::Tooltip::always_open(
                ui.ctx().clone(),
                response.layer_id,
                egui::Id::new("histogram_marker_tooltip"),
                tooltip_pos,
            )
            .show(|ui| {
                ui.colored_label(closest.marker.color, &closest.marker.name);
            });
        }
    }

    fn draw_selected_indicator(
        painter: &egui::Painter,
        rect: egui::Rect,
        selected_x_fraction: Option<f32>,
    ) {
        if let Some(fraction) = selected_x_fraction {
            let x = rect.min.x + fraction * rect.width();
            painter.vline(x, rect.y_range(), (2.0, Color32::RED));
        }
    }

    fn handle_click(
        response: &egui::Response,
        rect: egui::Rect,
        bar_width: f32,
        store: &LogStore,
        filtered_indices: &[StoreID],
        start_time: chrono::DateTime<chrono::Local>,
        bucket_size: f64,
    ) -> Option<HistogramClickEvent> {
        // Handle both click and drag - timeline acts like a scrubber
        if !response.clicked() && !response.dragged() {
            return None;
        }

        let pos = response.interact_pointer_pos()?;
        let relative_x = pos.x - rect.min.x;
        if relative_x < 0.0 {
            return None;
        }

        let bucket_idx = ((relative_x / bar_width).floor() as usize).min(NUM_BUCKETS - 1);

        let bucket_start_time = start_time.timestamp() + (bucket_idx as f64 * bucket_size) as i64;
        let click_time_in_bucket =
            bucket_start_time + ((relative_x % bar_width) / bar_width * bucket_size as f32) as i64;

        let closest_idx = Self::find_closest_line_in_bucket(
            store,
            filtered_indices,
            start_time,
            bucket_size,
            bucket_idx,
            click_time_in_bucket,
        );

        closest_idx.map(|line_index| HistogramClickEvent { line_index })
    }

    fn find_closest_line_in_bucket(
        store: &LogStore,
        filtered_indices: &[StoreID],
        start_time: chrono::DateTime<chrono::Local>,
        bucket_size: f64,
        clicked_bucket: usize,
        click_time_in_bucket: i64,
    ) -> Option<StoreID> {
        let mut closest_idx = None;
        let mut min_diff = i64::MAX;

        for line_idx in filtered_indices {
            let ts = store.get_by_id(line_idx).unwrap().timestamp;
            let line_bucket = timestamp_to_bucket(ts, start_time, bucket_size);

            if line_bucket == clicked_bucket {
                let diff = (ts.timestamp() - click_time_in_bucket).abs();
                if diff < min_diff {
                    min_diff = diff;
                    closest_idx = Some(line_idx.clone());
                }
            }
        }

        // Fallback: find closest line overall if bucket was empty
        if closest_idx.is_none() {
            let bucket_center_time =
                start_time.timestamp() + ((clicked_bucket as f64 + 0.5) * bucket_size) as i64;
            for line_idx in filtered_indices {
                let ts = store.get_by_id(line_idx).unwrap().timestamp;
                let diff = (ts.timestamp() - bucket_center_time).abs();
                if diff < min_diff {
                    min_diff = diff;
                    closest_idx = Some(line_idx.clone());
                }
            }
        }

        closest_idx
    }

    fn render_timeline_labels(
        ui: &mut Ui,
        start_time: chrono::DateTime<chrono::Local>,
        end_time: chrono::DateTime<chrono::Local>,
        store: &LogStore,
        selected_line_index: Option<StoreID>,
    ) {
        profiling::scope!("Histogram::render_timeline_labels");
        let dark_mode = ui.visuals().dark_mode;
        let selected_color = if dark_mode {
            Color32::YELLOW
        } else {
            Color32::from_rgb(180, 120, 0) // Dark golden/orange for light mode
        };

        ui.horizontal(|ui| {
            ui.label(format!(
                "Timeline: {} → {}",
                start_time.format("%H:%M:%S"),
                end_time.format("%H:%M:%S")
            ));
            if let Some(selected_line_index) = selected_line_index {
                let sel_ts = store.get_by_id(&selected_line_index).unwrap().timestamp;
                ui.separator();
                ui.colored_label(
                    selected_color,
                    format!("Selected: {}", sel_ts.format("%H:%M:%S%.3f")),
                );
            }
        });
    }
}
