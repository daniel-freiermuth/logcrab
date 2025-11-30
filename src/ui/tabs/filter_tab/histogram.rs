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

use crate::parser::line::LogLine;
use egui::{Color32, Ui};

/// Event emitted when histogram is clicked
#[derive(Debug, Clone)]
pub struct HistogramClickEvent {
    pub line_index: usize,
}

/// Reusable timeline histogram component
pub struct Histogram;

impl Histogram {
    /// Render the timeline histogram
    ///
    /// Returns Some(event) if the histogram was clicked
    pub fn render(
        ui: &mut Ui,
        lines: &[LogLine],
        filtered_indices: &[usize],
        selected_line_index: usize,
    ) -> Option<HistogramClickEvent> {
        if lines.is_empty() || filtered_indices.is_empty() {
            if lines.is_empty() {
                return None;
            }
            ui.label("No logs match the current filter");
            return None;
        }

        // Validate filtered_indices to prevent index out of bounds
        let max_valid_index = lines.len().saturating_sub(1);
        let invalid_indices: Vec<_> = filtered_indices
            .iter()
            .filter(|&&idx| idx >= lines.len())
            .collect();

        if !invalid_indices.is_empty() {
            log::warn!(
                "Found {} invalid indices in filtered_indices (max valid: {}, total lines: {}). First few invalid indices: {:?}",
                invalid_indices.len(),
                max_valid_index,
                lines.len(),
                invalid_indices.iter().take(5).collect::<Vec<_>>()
            );
            // Filter out invalid indices for safety
            let valid_filtered_indices: Vec<_> = filtered_indices
                .iter()
                .filter(|&&idx| idx < lines.len())
                .copied()
                .collect();

            if valid_filtered_indices.is_empty() {
                ui.label("No valid filtered indices available");
                return None;
            }

            // Continue with valid indices
            return Self::render_internal(ui, lines, &valid_filtered_indices, selected_line_index);
        }

        Self::render_internal(ui, lines, filtered_indices, selected_line_index)
    }

    fn render_internal(
        ui: &mut Ui,
        lines: &[LogLine],
        filtered_indices: &[usize],
        selected_line_index: usize,
    ) -> Option<HistogramClickEvent> {
        // Get time range from filtered lines only
        let first_ts = filtered_indices
            .iter()
            .map(|&idx| lines[idx].timestamp)
            .next();
        let last_ts = filtered_indices
            .iter()
            .rev()
            .map(|&idx| lines[idx].timestamp)
            .next();

        if first_ts.is_none() || last_ts.is_none() {
            ui.label("No timestamps available for histogram");
            return None;
        }

        let start_time = first_ts.unwrap();
        let end_time = last_ts.unwrap();
        let time_range = (end_time.timestamp() - start_time.timestamp()).max(1);

        const NUM_BUCKETS: usize = 100;
        let bucket_size = time_range as f64 / NUM_BUCKETS as f64;

        // Count lines per bucket (only filtered lines)
        let mut buckets = vec![0usize; NUM_BUCKETS];
        for &line_idx in filtered_indices {
            let ts = lines[line_idx].timestamp;
            let elapsed = (ts.timestamp() - start_time.timestamp()) as f64;
            let bucket_idx = ((elapsed / bucket_size) as usize).min(NUM_BUCKETS - 1);
            buckets[bucket_idx] += 1;
        }

        let max_count = *buckets.iter().max().unwrap_or(&1);

        // Calculate selected line position if present
        let selected_bucket = {
            let sel_ts = lines[selected_line_index].timestamp;
            let elapsed = (sel_ts.timestamp() - start_time.timestamp()) as f64;
            // Only show indicator if the selected time is within this filter's time range
            if elapsed >= 0.0 && sel_ts.timestamp() <= end_time.timestamp() {
                Some(((elapsed / bucket_size) as usize).min(NUM_BUCKETS - 1))
            } else {
                None
            }
        };

        // Render histogram
        let desired_size = egui::vec2(ui.available_width(), 60.0);
        let (response, painter) = ui.allocate_painter(desired_size, egui::Sense::click());
        let rect = response.rect;

        // Draw background
        painter.rect_filled(rect, 0.0, Color32::from_gray(20));

        let bar_width = rect.width() / NUM_BUCKETS as f32;

        // Draw bars
        for (i, &count) in buckets.iter().enumerate() {
            if count > 0 {
                let x = rect.min.x + i as f32 * bar_width;
                let height = (count as f32 / max_count as f32) * rect.height();
                let y = rect.max.y - height;

                let bar_rect = egui::Rect::from_min_size(
                    egui::pos2(x, y),
                    egui::vec2(bar_width.max(1.0), height),
                );

                // Color based on whether this bucket is selected
                let color = if Some(i) == selected_bucket {
                    Color32::from_rgb(255, 200, 100) // Orange for selected
                } else {
                    Color32::from_rgb(100, 150, 255) // Blue for normal
                };

                painter.rect_filled(bar_rect, 0.0, color);
            }
        }

        // Draw selected line indicator
        if let Some(bucket_idx) = selected_bucket {
            let x = rect.min.x + bucket_idx as f32 * bar_width + bar_width / 2.0;
            painter.vline(x, rect.y_range(), (2.0, Color32::RED));
        }

        // Handle clicks to jump to time
        let mut click_event = None;
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                // Calculate which bucket was clicked based on bar positions
                let relative_x = pos.x - rect.min.x;
                let bucket_idx = ((relative_x / bar_width).floor() as usize).min(NUM_BUCKETS - 1);

                if bucket_idx < NUM_BUCKETS && relative_x >= 0.0 {
                    // Find the time range for this bucket
                    let bucket_start_time =
                        start_time.timestamp() + (bucket_idx as f64 * bucket_size) as i64;
                    let bucket_end_time =
                        start_time.timestamp() + ((bucket_idx + 1) as f64 * bucket_size) as i64;

                    // Calculate the exact time that corresponds to where the user clicked
                    let click_time_in_bucket = bucket_start_time
                        + ((relative_x % bar_width) / bar_width * bucket_size as f32) as i64;

                    // Find the line closest to the exact click position
                    let mut closest_idx = None;
                    let mut min_diff = i64::MAX;

                    for &line_idx in filtered_indices {
                        let ts = lines[line_idx].timestamp;
                        let ts_value = ts.timestamp();
                        // Only consider lines that are actually in this bucket
                        if ts_value >= bucket_start_time && ts_value < bucket_end_time {
                            let diff = (ts_value - click_time_in_bucket).abs();
                            if diff < min_diff {
                                min_diff = diff;
                                closest_idx = Some(line_idx);
                            }
                        }
                    }

                    // If no line found in the exact bucket, fall back to bucket center
                    if closest_idx.is_none() {
                        let bucket_center_time =
                            bucket_start_time + (bucket_end_time - bucket_start_time) / 2;
                        for &line_idx in filtered_indices {
                            let ts = lines[line_idx].timestamp;
                            let diff = (ts.timestamp() - bucket_center_time).abs();
                            if diff < min_diff {
                                min_diff = diff;
                                closest_idx = Some(line_idx);
                            }
                        }
                    }

                    if let Some(idx) = closest_idx {
                        if idx < lines.len() {
                            click_event = Some(HistogramClickEvent { line_index: idx });
                        }
                    }
                }
            }
        }

        // Show time range below
        ui.horizontal(|ui| {
            ui.label(format!(
                "Timeline: {} → {}",
                start_time.format("%H:%M:%S"),
                end_time.format("%H:%M:%S")
            ));
            let sel_ts = lines[selected_line_index].timestamp;
            ui.separator();
            ui.colored_label(
                Color32::YELLOW,
                format!("Selected: {}", sel_ts.format("%H:%M:%S%.3f")),
            );
        });

        click_event
    }
}
