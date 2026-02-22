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

use crate::core::histogram_worker::{
    AnomalyDistribution, HistogramCacheKey, HistogramData, HistogramRequest, HistogramResult,
    HistogramWorkerHandle, NUM_BUCKETS, SCORE_BUCKETS,
};
use crate::core::{log_store::StoreID, LogStore};
use crate::ui::tabs::filter_tab::filter_state::FilterState;
use crate::ui::tabs::filter_tab::log_table;
use chrono::{DateTime, Local, TimeDelta};
use egui::{Color32, Pos2, Ui};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::Duration;

/// Convert a floating-point seconds value to `TimeDelta`, handling negative values.
/// `TimeDelta` doesn't have `from_f64()`, and Duration panics on negatives, so we need this helper.
fn timedelta_from_secs_f64(secs: f64) -> TimeDelta {
    if secs >= 0.0 {
        TimeDelta::from_std(Duration::from_secs_f64(secs)).expect("duration in range")
    } else {
        -TimeDelta::from_std(Duration::from_secs_f64(-secs)).expect("duration in range")
    }
}

/// Convert a floating-point seconds value to `TimeDelta`, handling negative values.
/// `TimeDelta` doesn't have `from_f32()`, and Duration panics on negatives, so we need this helper.
fn timedelta_from_secs_f32(secs: f32) -> TimeDelta {
    if secs >= 0.0 {
        TimeDelta::from_std(Duration::from_secs_f32(secs)).expect("duration in range")
    } else {
        -TimeDelta::from_std(Duration::from_secs_f32(-secs)).expect("duration in range")
    }
}

/// Minimum fraction of view width required for drag-to-zoom selection
const MIN_DRAG_ZOOM_FRACTION: f32 = 0.005;

/// Zoom state for the histogram timeline
#[derive(Clone, Default)]
pub struct HistogramZoomState {
    /// Visible time range (None = show full range)
    pub visible_range: Option<(DateTime<Local>, DateTime<Local>)>,
    /// Drag start position for shift-drag zoom selection (in screen coordinates)
    pub drag_start: Option<Pos2>,
    /// Current drag end position (for drawing selection box)
    pub drag_end: Option<Pos2>,
}

impl HistogramZoomState {
    /// Reset zoom to show full timeline
    pub const fn reset(&mut self) {
        self.visible_range = None;
        self.drag_start = None;
        self.drag_end = None;
    }

    /// Check if currently zoomed in
    pub const fn is_zoomed(&self) -> bool {
        self.visible_range.is_some()
    }

    /// Set the visible time range (zoom in)
    pub fn set_visible_range(&mut self, start: DateTime<Local>, end: DateTime<Local>) {
        // Ensure start < end
        if start < end {
            self.visible_range = Some((start, end));
        }
    }
}

/// Marker data for showing filter matches in histogram
#[derive(Clone)]
pub struct HistogramMarker {
    pub name: String,
    pub indices: Arc<Vec<StoreID>>,
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
    /// Cached histogram data
    data: Option<HistogramData>,
    /// Zoom state for the timeline
    pub zoom: HistogramZoomState,
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
            zoom: HistogramZoomState::default(),
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
        cache_key: &HistogramCacheKey,
        zoom_range: Option<(DateTime<Local>, DateTime<Local>)>,
    ) {
        let request = HistogramRequest {
            filter_id: self.filter_id,
            store: Arc::clone(store),
            filtered_indices: filtered_indices.to_vec(),
            zoom_range,
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

        // Use the search parameters that the current filtered_indices were computed for,
        // not the current search_text (which may have changed while filter is pending)
        let (indices_text, indices_exclude, indices_case, indices_version) =
            filter_state.search.indices_computed_for();
        let search_str = indices_text.to_string();
        let exclude_str = indices_exclude.to_string();

        let cache = &mut filter_state.histogram_cache;

        // Get zoom range for cache key (convert to ms for stable comparison)
        let zoom_range = cache.zoom.visible_range;
        let zoom_range_ms = zoom_range.map(|(s, e)| (s.timestamp_millis(), e.timestamp_millis()));

        let cache_key = HistogramCacheKey {
            store_version: indices_version,
            search_str,
            exclude_str,
            case_sensitive: indices_case,
            zoom_range_ms,
        };

        // Poll for any completed results
        cache.poll_results();

        if !cache.is_valid(&cache_key) {
            if cache.is_pending(&cache_key) {
                ui.ctx().request_repaint(); // Keep polling
            } else {
                // Request new computation
                cache.request_computation(worker, store, filtered_indices, &cache_key, zoom_range);
            }
        }

        if let Some(data) = cache.data.clone() {
            // Cache is valid, render it
            Self::render_cached(
                ui,
                store,
                &data,
                filtered_indices,
                selected_line_index,
                markers,
                &mut cache.zoom,
            )
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
        filtered_indices: &[StoreID],
        selected_line_index: Option<StoreID>,
        markers: &[HistogramMarker],
        zoom: &mut HistogramZoomState,
    ) -> Option<HistogramClickEvent> {
        // The data already contains buckets computed for the current view range
        // (either full range or zoomed range, as computed by the worker)
        let view_start = data.start_time;
        let view_end = data.end_time;

        let max_count = *data.buckets.iter().max().unwrap_or(&1);
        let selected_x_fraction =
            Self::calculate_selected_x_fraction(store, selected_line_index, view_start, view_end);

        let dark_mode = ui.visuals().dark_mode;
        let bg_color = ui.visuals().extreme_bg_color;

        let click_event = Self::render_histogram_bars(
            ui,
            data,
            filtered_indices,
            &data.buckets,
            &data.anomaly_buckets,
            max_count,
            selected_x_fraction,
            store,
            markers,
            dark_mode,
            bg_color,
            zoom,
            view_start,
            view_end,
        );

        Self::render_timeline_labels(
            ui,
            view_start,
            view_end,
            data.full_start,
            data.full_end,
            store,
            selected_line_index,
            zoom.is_zoomed(),
        );

        click_event
    }

    fn calculate_selected_x_fraction(
        store: &LogStore,
        selected_line_index: Option<StoreID>,
        start_time: chrono::DateTime<chrono::Local>,
        end_time: chrono::DateTime<chrono::Local>,
    ) -> Option<f64> {
        let selected_line_index = selected_line_index?;
        let line = store.get_by_id(&selected_line_index)?;
        let sel_ts = store.get_adjusted_timestamp(&selected_line_index, &line);

        let elapsed = (sel_ts - start_time).as_seconds_f64();
        let total = (end_time - start_time).as_seconds_f64();

        if elapsed >= 0.0 && elapsed <= total {
            Some(elapsed / total)
        } else {
            None
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_histogram_bars(
        ui: &mut Ui,
        data: &HistogramData,
        filtered_indices: &[StoreID],
        visible_buckets: &[usize],
        visible_anomaly_buckets: &[AnomalyDistribution],
        max_count: usize,
        selected_x_fraction: Option<f64>,
        store: &LogStore,
        markers: &[HistogramMarker],
        dark_mode: bool,
        bg_color: Color32,
        zoom: &mut HistogramZoomState,
        view_start: DateTime<Local>,
        view_end: DateTime<Local>,
    ) -> Option<HistogramClickEvent> {
        profiling::scope!("Histogram::draw_bars");
        let desired_size = egui::vec2(ui.available_width(), 60.0);
        let (response, painter) = ui.allocate_painter(desired_size, egui::Sense::click_and_drag());
        let rect = response.rect;

        painter.rect_filled(rect, 0.0, bg_color);

        let num_visible_buckets = visible_buckets.len();
        let bar_width = if num_visible_buckets > 0 {
            rect.width() / num_visible_buckets as f32
        } else {
            rect.width() / NUM_BUCKETS as f32
        };

        Self::draw_bars(
            &painter,
            rect,
            visible_buckets,
            visible_anomaly_buckets,
            max_count,
            bar_width,
            dark_mode,
        );

        // Calculate view bucket size for markers
        let view_duration = view_end - view_start;
        let view_bucket_size = Duration::from_secs_f64(
            view_duration.num_milliseconds() as f64 / 1000.0 / num_visible_buckets.max(1) as f64,
        );

        Self::draw_markers(
            &painter,
            rect,
            store,
            view_start,
            view_bucket_size,
            markers,
            num_visible_buckets,
        );
        Self::draw_selected_indicator(&painter, rect, selected_x_fraction);

        // Handle zoom interactions
        let click_event = Self::handle_zoom_interactions(
            ui, &response, &painter, rect, zoom, data, view_start, view_end,
        );

        // If zoom handled the interaction, don't process as click
        if click_event.is_some() {
            return click_event;
        }

        // Handle hover tooltip for markers
        Self::handle_marker_hover(
            ui,
            &response,
            rect,
            store,
            view_start,
            view_bucket_size,
            markers,
            num_visible_buckets,
        );

        Self::handle_click(
            ui,
            &response,
            rect,
            store,
            filtered_indices,
            view_start,
            view_bucket_size,
            num_visible_buckets,
        )
    }

    /// Handle zoom interactions: scroll wheel, shift+drag, double-click
    #[allow(clippy::too_many_arguments)]
    fn handle_zoom_interactions(
        ui: &Ui,
        response: &egui::Response,
        painter: &egui::Painter,
        rect: egui::Rect,
        zoom: &mut HistogramZoomState,
        data: &HistogramData,
        view_start: DateTime<Local>,
        view_end: DateTime<Local>,
    ) -> Option<HistogramClickEvent> {
        let modifiers = ui.input(|i| i.modifiers);

        // Double-click to reset zoom
        if response.double_clicked() {
            zoom.reset();
            return None;
        }

        // Scroll wheel zoom (centered on cursor)
        if response.hovered() {
            let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
            if scroll_delta.abs() > 0.0 {
                if let Some(hover_pos) = response.hover_pos() {
                    Self::handle_scroll_zoom(
                        zoom,
                        scroll_delta,
                        hover_pos,
                        rect,
                        data,
                        view_start,
                        view_end,
                    );
                }
            }
        }

        // Shift+drag for range selection zoom
        if modifiers.shift {
            if response.drag_started() {
                if let Some(pos) = response.interact_pointer_pos() {
                    zoom.drag_start = Some(pos);
                    zoom.drag_end = Some(pos);
                }
            } else if response.dragged() {
                if let Some(pos) = response.interact_pointer_pos() {
                    zoom.drag_end = Some(pos);
                }
            } else if response.drag_stopped() {
                // Complete the selection and zoom
                if let (Some(start), Some(end)) = (zoom.drag_start, zoom.drag_end) {
                    Self::complete_drag_zoom(zoom, start, end, rect, data, view_start, view_end);
                }
                zoom.drag_start = None;
                zoom.drag_end = None;
            }

            // Draw selection rectangle while dragging
            if let (Some(start), Some(end)) = (zoom.drag_start, zoom.drag_end) {
                // Clamp x positions to rect bounds for accurate fraction calculation
                let start_x = start.x.clamp(rect.min.x, rect.max.x);
                let end_x = end.x.clamp(rect.min.x, rect.max.x);

                let start_fraction = (start_x - rect.min.x) / rect.width();
                let end_fraction = (end_x - rect.min.x) / rect.width();
                let selection_fraction = (end_fraction - start_fraction).abs();
                let is_too_small = selection_fraction < MIN_DRAG_ZOOM_FRACTION;

                // Use red tint when selection is too small, blue when valid
                let (fill_color, stroke_color) = if is_too_small {
                    (
                        Color32::from_rgba_unmultiplied(255, 100, 100, 80),
                        Color32::from_rgb(255, 100, 100),
                    )
                } else {
                    (
                        Color32::from_rgba_unmultiplied(100, 150, 255, 80),
                        Color32::from_rgb(100, 150, 255),
                    )
                };

                let selection_rect = egui::Rect::from_two_pos(
                    egui::pos2(start_x.min(end_x), rect.min.y),
                    egui::pos2(start_x.max(end_x), rect.max.y),
                );
                painter.rect(
                    selection_rect,
                    0.0,
                    fill_color,
                    egui::Stroke::new(1.0, stroke_color),
                    egui::StrokeKind::Inside,
                );
            }

            // Return early when shift is held - don't process as regular click
            return None;
        }

        // Show zoom hint on hover (only when not already zooming)
        if response.hovered() && zoom.drag_start.is_none() {
            response.clone().on_hover_text_at_pointer(
                "Scroll to zoom • Shift+drag to select range • Double-click to reset",
            );
        }

        None
    }

    /// Handle scroll wheel zoom centered on cursor position
    fn handle_scroll_zoom(
        zoom: &mut HistogramZoomState,
        scroll_delta: f32,
        hover_pos: Pos2,
        rect: egui::Rect,
        data: &HistogramData,
        view_start: DateTime<Local>,
        view_end: DateTime<Local>,
    ) {
        let zoom_factor: f64 = if scroll_delta > 0.0 { 0.8 } else { 1.25 };
        let full_duration = data.full_end - data.full_start;

        // Calculate cursor position as fraction of view
        let cursor_fraction = ((hover_pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);

        let view_duration = view_end - view_start;
        let new_duration = timedelta_from_secs_f64(view_duration.as_seconds_f64() * zoom_factor)
            // Don't zoom in past 10 milliseconds or zoom out past full range
            .clamp(TimeDelta::milliseconds(10), full_duration);

        // New start and end, keeping cursor at same relative position
        let new_start = view_start
            + timedelta_from_secs_f32(
                cursor_fraction * (view_duration - new_duration).as_seconds_f32(),
            );
        let new_end = new_start + new_duration;

        // Clamp to full data range
        let new_start = new_start.max(data.full_start);
        let new_end = new_end.min(data.full_end);

        zoom.set_visible_range(new_start, new_end);
    }

    /// Complete a shift+drag zoom selection
    fn complete_drag_zoom(
        zoom: &mut HistogramZoomState,
        start_pos: Pos2,
        end_pos: Pos2,
        rect: egui::Rect,
        data: &HistogramData,
        view_start: DateTime<Local>,
        view_end: DateTime<Local>,
    ) {
        let start_fraction = ((start_pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
        let end_fraction = ((end_pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);

        let (start_fraction, end_fraction) = if start_fraction < end_fraction {
            (start_fraction, end_fraction)
        } else {
            (end_fraction, start_fraction)
        };

        // Minimum selection required for zoom
        if (end_fraction - start_fraction) < MIN_DRAG_ZOOM_FRACTION {
            return;
        }

        let view_duration = view_end - view_start;

        let new_start =
            view_start + Duration::from_secs_f32(start_fraction * view_duration.as_seconds_f32());
        let new_end =
            view_start + Duration::from_secs_f32(end_fraction * view_duration.as_seconds_f32());

        // Clamp to full data range
        let new_start = new_start.max(data.full_start);
        let new_end = new_end.min(data.full_end);

        zoom.set_visible_range(new_start, new_end);
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
                let x = (i as f32).mul_add(bar_width, rect.min.x);
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

            let color = log_table::score_to_color(f64::from(score), dark_mode);

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
        view_start: chrono::DateTime<chrono::Local>,
        view_bucket_size: Duration,
        markers: &[HistogramMarker],
        num_visible_buckets: usize,
    ) {
        profiling::scope!("Histogram::draw_markers");
        let total_width = rect.width();
        let total_time = num_visible_buckets as u32 * view_bucket_size;

        for marker in markers {
            for line_idx in marker.indices.iter() {
                let Some(line) = store.get_by_id(line_idx) else {
                    continue;
                };
                let ts = store.get_adjusted_timestamp(line_idx, &line);
                let elapsed = ts - view_start;

                // Skip markers outside visible range
                if elapsed.num_milliseconds() < 0
                    || elapsed.num_milliseconds() > total_time.as_millis() as i64
                {
                    continue;
                }

                let x = rect.min.x
                    + (elapsed.as_seconds_f64() / total_time.as_secs_f64() * f64::from(total_width))
                        as f32;

                painter.vline(x, rect.y_range(), (1.0, marker.color));
            }
        }
    }

    fn handle_marker_hover(
        ui: &Ui,
        response: &egui::Response,
        rect: egui::Rect,
        store: &LogStore,
        view_start: chrono::DateTime<chrono::Local>,
        view_bucket_size: Duration,
        markers: &[HistogramMarker],
        num_visible_buckets: usize,
    ) {
        struct MarkerMatch<'a> {
            marker: &'a HistogramMarker,
            distance: f32,
            x_pos: f32,
        }

        let Some(hover_pos) = response.hover_pos() else {
            return;
        };

        let total_width = rect.width();
        let total_time = num_visible_buckets as u32 * view_bucket_size;
        let hover_threshold = 3.0; // pixels

        let mut closest_match: Option<MarkerMatch> = None;

        for marker in markers {
            for line_idx in marker.indices.iter() {
                let Some(line) = store.get_by_id(line_idx) else {
                    continue;
                };
                let ts = store.get_adjusted_timestamp(line_idx, &line);
                let elapsed = ts - view_start;

                // Skip markers outside visible range
                if elapsed.num_milliseconds() < 0
                    || elapsed.num_milliseconds() > total_time.as_millis() as i64
                {
                    continue;
                }

                let x = rect.min.x
                    + (elapsed.as_seconds_f64() / total_time.as_secs_f64() * f64::from(total_width))
                        as f32;

                let distance = (hover_pos.x - x).abs();
                if distance < hover_threshold
                    && !marker.name.is_empty()
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
                ui.set_min_width(50.0);
                ui.colored_label(closest.marker.color, &closest.marker.name);
            });
        }
    }

    fn draw_selected_indicator(
        painter: &egui::Painter,
        rect: egui::Rect,
        selected_x_fraction: Option<f64>,
    ) {
        if let Some(fraction) = selected_x_fraction {
            let x = (fraction as f32).mul_add(rect.width(), rect.min.x);
            painter.vline(x, rect.y_range(), (2.0, Color32::RED));
        }
    }

    fn handle_click(
        ui: &Ui,
        response: &egui::Response,
        rect: egui::Rect,
        store: &LogStore,
        filtered_indices: &[StoreID],
        view_start: chrono::DateTime<chrono::Local>,
        view_bucket_size: Duration,
        num_visible_buckets: usize,
    ) -> Option<HistogramClickEvent> {
        profiling::scope!("Histogram::handle_click");

        // Don't process clicks/drags when shift is pressed (that's for zoom selection)
        let modifiers = ui.input(|i| i.modifiers);
        if modifiers.shift {
            return None;
        }

        // Handle both click and drag - timeline acts like a scrubber
        if !response.clicked() && !response.dragged() {
            return None;
        }

        let pos = response.interact_pointer_pos()?;
        let relative_x = pos.x - rect.min.x;
        if relative_x < 0.0 {
            return None;
        }

        // Calculate click time directly from x position
        // to match how the selected indicator position is calculated
        let total_time = view_bucket_size * (num_visible_buckets as u32);
        let click_fraction = f64::from(relative_x / rect.width());
        let click_time = view_start
            + chrono::Duration::from_std(Duration::from_secs_f64(
                total_time.as_secs_f64() * click_fraction,
            ))
            .expect("histogram click time within representable range");

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
        target_time: DateTime<Local>,
    ) -> Option<StoreID> {
        profiling::scope!("Histogram::find_closest_line_by_time");
        if filtered_indices.is_empty() {
            return None;
        }

        // Binary search to find insertion point
        // If any line lookup fails (stale indices), just return None
        let idx = filtered_indices.partition_point(|line_idx| {
            store.get_by_id(line_idx).is_some_and(|line| {
                store.get_adjusted_timestamp(line_idx, &line) < target_time
            })
        });

        // Compare neighbors around the insertion point to find the closest
        match idx {
            0 => Some(filtered_indices[0]),
            i if i >= filtered_indices.len() => Some(filtered_indices[filtered_indices.len() - 1]),
            i => {
                let before_line = store.get_by_id(&filtered_indices[i - 1])?;
                let after_line = store.get_by_id(&filtered_indices[i])?;
                let before_ts = store.get_adjusted_timestamp(&filtered_indices[i - 1], &before_line);
                let after_ts = store.get_adjusted_timestamp(&filtered_indices[i], &after_line);

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
        view_start: chrono::DateTime<chrono::Local>,
        view_end: chrono::DateTime<chrono::Local>,
        full_start: chrono::DateTime<chrono::Local>,
        full_end: chrono::DateTime<chrono::Local>,
        store: &LogStore,
        selected_line_index: Option<StoreID>,
        is_zoomed: bool,
    ) {
        profiling::scope!("Histogram::render_timeline_labels");
        let dark_mode = ui.visuals().dark_mode;
        let selected_color = if dark_mode {
            Color32::YELLOW
        } else {
            Color32::from_rgb(180, 120, 0) // Dark golden/orange for light mode
        };
        let zoom_color = if dark_mode {
            Color32::from_rgb(100, 180, 255)
        } else {
            Color32::from_rgb(0, 100, 180)
        };

        ui.horizontal(|ui| {
            ui.label(format!(
                "Timeline: {} → {}",
                view_start.format("%H:%M:%S"),
                view_end.format("%H:%M:%S")
            ));

            if is_zoomed {
                ui.separator();

                // Calculate zoom level
                let full_duration_ms =
                    (full_end.timestamp_millis() - full_start.timestamp_millis()) as f64;
                let view_duration_ms =
                    (view_end.timestamp_millis() - view_start.timestamp_millis()) as f64;
                let zoom_level = full_duration_ms / view_duration_ms;

                ui.colored_label(
                    zoom_color,
                    format!("🔍 {zoom_level:.1}x (double-click to reset)"),
                );
            }

            if let Some(selected_line_index) = selected_line_index {
                if let Some(line) = store.get_by_id(&selected_line_index) {
                    let sel_ts = store.get_adjusted_timestamp(&selected_line_index, &line);
                    ui.separator();
                    ui.colored_label(
                        selected_color,
                        format!("Selected: {}", sel_ts.format("%H:%M:%S%.3f")),
                    );
                }
            }
        });
    }
}
