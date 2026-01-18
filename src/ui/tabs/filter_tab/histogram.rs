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

use crate::core::histogram_worker::{
    AnomalyDistribution, HistogramCacheKey, HistogramData, HistogramRequest, HistogramResult,
    HistogramWorkerHandle, NUM_BUCKETS, SCORE_BUCKETS,
};
use crate::core::{log_store::StoreID, LogStore};
use crate::ui::tabs::filter_tab::filter_state::FilterState;
use crate::ui::tabs::filter_tab::log_table;
use egui::{Color32, Ui};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

/// Marker data for showing filter matches in histogram
#[derive(Clone)]
pub struct HistogramMarker {
    pub name: String,
    pub indices: Vec<StoreID>,
    pub color: Color32,
}

/// Event emitted when histogram is clicked
#[derive(Clone)]
pub struct HistogramClickEvent {
    pub line_index: StoreID,
}

/// Cached histogram computation results
pub struct HistogramCache {
    /// Filter ID for this cache (used for worker requests)
    filter_id: usize,
    /// Cache key for the current valid data
    key: Option<HistogramCacheKey>,
    /// Pending computation key (what we're waiting for)
    pending_key: Option<HistogramCacheKey>,
    /// Channel to receive computation results
    result_rx: Receiver<HistogramResult>,
    result_tx: mpsc::Sender<HistogramResult>,
    /// Filtered indices after epoch removal (if `hide_epoch` is true)
    data: Option<HistogramData>,
}

impl HistogramCache {
    /// Create a new histogram cache with the given filter ID
    pub fn new(filter_id: usize) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            filter_id,
            result_rx: rx,
            result_tx: tx,
            key: None,
            pending_key: None,
            data: None,
        }
    }

    /// Check if cache is valid for the given inputs
    fn is_valid(&self, key: &HistogramCacheKey) -> bool {
        self.key == Some(key.clone())
    }

    /// Check if we're already waiting for the right computation
    fn is_pending(&self, key: &HistogramCacheKey) -> bool {
        self.pending_key == Some(key.clone())
    }

    /// Poll for completed results and update cache if ready
    fn poll_results(&mut self) {
        match self.result_rx.try_recv() {
            Ok(result) => {
                // Update cache with result
                self.key = Some(result.cache_key);
                self.data = result.data;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                // Worker died or task was cancelled, clear pending state
            }
        }
    }

    /// Request a new histogram computation
    fn request_computation(
        &mut self,
        worker: &HistogramWorkerHandle,
        store: &Arc<LogStore>,
        filtered_indices: &[StoreID],
        hide_epoch: bool,
        cache_key: &HistogramCacheKey,
    ) {
        let request = HistogramRequest {
            filter_id: self.filter_id,
            store: Arc::clone(store),
            filtered_indices: filtered_indices.to_vec(),
            hide_epoch,
            result_tx: self.result_tx.clone(),
            key: cache_key.clone(),
        };

        worker.send_request(request);
        self.pending_key = Some(cache_key.clone());
    }
}

/// Reusable timeline histogram component
pub struct Histogram;

impl Histogram {
    /// Render the timeline histogram
    ///
    /// Returns Some(event) if the histogram was clicked
    pub fn render(
        ui: &mut Ui,
        store: &Arc<LogStore>,
        filtered_indices: &[StoreID],
        selected_line_index: Option<StoreID>,
        hide_epoch: bool,
        markers: &[HistogramMarker],
        filter_state: &mut FilterState,
        worker: &HistogramWorkerHandle,
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
        // Use the search parameters that the current filtered_indices were computed for,
        // not the current search_text (which may have changed while filter is pending)
        let (indices_text, indices_case) = filter_state.search.indices_computed_for();
        let cache_key = HistogramCacheKey {
            store_version,
            hide_epoch,
            search_str: indices_text.to_string(),
            case_sensitive: indices_case,
        };

        let cache = &mut filter_state.histogram_cache;

        // Poll for any completed results
        cache.poll_results();

        if !cache.is_valid(&cache_key) {
            if cache.is_pending(&cache_key) {
                ui.ctx().request_repaint(); // Keep polling
            } else {
                // Request new computation
                cache.request_computation(worker, store, filtered_indices, hide_epoch, &cache_key);
            }
        }

        if let Some(data) = &cache.data {
            // Cache is valid, render it
            Self::render_cached(ui, store, data, selected_line_index, markers)
        } else {
            // No stale data available, show loading
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Computing histogram...");
            });
            ui.ctx().request_repaint(); // Keep polling
            None
        }
    }

    fn render_cached(
        ui: &mut Ui,
        store: &LogStore,
        data: &HistogramData,
        selected_line_index: Option<StoreID>,
        markers: &[HistogramMarker],
    ) -> Option<HistogramClickEvent> {
        let max_count = *data.buckets.iter().max().unwrap_or(&1);
        let selected_x_fraction = Self::calculate_selected_x_fraction(
            store,
            selected_line_index,
            data.start_time,
            data.end_time,
        );

        let dark_mode = ui.visuals().dark_mode;
        let bg_color = ui.visuals().extreme_bg_color;

        let click_event = Self::render_histogram_bars(
            ui,
            data,
            max_count,
            selected_x_fraction,
            store,
            markers,
            dark_mode,
            bg_color,
        );

        Self::render_timeline_labels(
            ui,
            data.start_time,
            data.end_time,
            store,
            selected_line_index,
        );

        click_event
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
        data: &HistogramData,
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
            data.buckets.as_slice(),
            data.anomaly_buckets.as_slice(),
            max_count,
            bar_width,
            dark_mode,
        );
        Self::draw_markers(
            &painter,
            rect,
            store,
            data.start_time,
            data.bucket_size,
            markers,
        );
        Self::draw_selected_indicator(&painter, rect, selected_x_fraction);

        // Handle hover tooltip for markers
        Self::handle_marker_hover(
            ui,
            &response,
            rect,
            store,
            data.start_time,
            data.bucket_size,
            markers,
        );
        Self::handle_click(
            &response,
            rect,
            store,
            &data.effective_indices,
            data.start_time,
            data.bucket_size,
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
        store: &LogStore,
        filtered_indices: &[StoreID],
        start_time: chrono::DateTime<chrono::Local>,
        bucket_size: f64,
    ) -> Option<HistogramClickEvent> {
        profiling::scope!("Histogram::handle_click");
        // Handle both click and drag - timeline acts like a scrubber
        if !response.clicked() && !response.dragged() {
            return None;
        }

        let pos = response.interact_pointer_pos()?;
        let relative_x = pos.x - rect.min.x;
        if relative_x < 0.0 {
            return None;
        }

        // Calculate click time directly from x position - no need for bucket math
        let total_time = NUM_BUCKETS as f64 * bucket_size;
        let click_fraction = (relative_x / rect.width()) as f64;
        let click_time = start_time.timestamp() + (click_fraction * total_time) as i64;

        // Binary search to find the closest line by timestamp
        // Since filtered_indices are sorted by timestamp, we can use binary search
        let closest_idx = Self::find_closest_line_by_time(store, filtered_indices, click_time);

        closest_idx.map(|line_index| HistogramClickEvent { line_index })
    }

    /// Find the line closest to a given timestamp using binary search
    /// Assumes `filtered_indices` are sorted by timestamp
    fn find_closest_line_by_time(
        store: &LogStore,
        filtered_indices: &[StoreID],
        target_time: i64,
    ) -> Option<StoreID> {
        profiling::scope!("Histogram::find_closest_line_by_time");
        if filtered_indices.is_empty() {
            return None;
        }

        // Binary search to find insertion point
        let idx = filtered_indices.partition_point(|line_idx| {
            store
                .get_by_id(line_idx)
                .map(|line| line.timestamp.timestamp() < target_time)
                .expect("Logline not found during histogram search.")
        });

        // Compare neighbors around the insertion point to find the closest
        match idx {
            0 => Some(filtered_indices[0]),
            i if i >= filtered_indices.len() => Some(filtered_indices[filtered_indices.len() - 1]),
            i => {
                let before_ts = store
                    .get_by_id(&filtered_indices[i - 1])
                    .expect("Logline not found during histogram search.")
                    .timestamp
                    .timestamp();
                let after_ts = store
                    .get_by_id(&filtered_indices[i])
                    .expect("Logline not found during histogram search.")
                    .timestamp
                    .timestamp();

                let dist_before = (target_time - before_ts).abs();
                let dist_after = (after_ts - target_time).abs();

                if dist_before <= dist_after {
                    Some(filtered_indices[i - 1])
                } else {
                    Some(filtered_indices[i])
                }
            }
        }
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
