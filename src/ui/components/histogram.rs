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

use egui::{Color32, Ui};
use crate::parser::line::LogLine;
use chrono::DateTime;

/// Event emitted when histogram is clicked
#[derive(Debug, Clone)]
pub struct HistogramClickEvent {
    pub line_index: usize,
    pub timestamp: Option<DateTime<chrono::Local>>,
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
        selected_line_index: Option<usize>,
    ) -> Option<HistogramClickEvent> {
        if lines.is_empty() || filtered_indices.is_empty() {
            if lines.is_empty() {
                return None;
            }
            ui.label("No logs match the current filter");
            return None;
        }
        
        // Get time range from filtered lines only
        let first_ts = filtered_indices.iter()
            .find_map(|&idx| lines[idx].timestamp);
        let last_ts = filtered_indices.iter().rev()
            .find_map(|&idx| lines[idx].timestamp);
        
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
            if let Some(ts) = lines[line_idx].timestamp {
                let elapsed = (ts.timestamp() - start_time.timestamp()) as f64;
                let bucket_idx = ((elapsed / bucket_size) as usize).min(NUM_BUCKETS - 1);
                buckets[bucket_idx] += 1;
            }
        }
        
        let max_count = *buckets.iter().max().unwrap_or(&1);
        
        // Calculate selected line position if present
        let selected_bucket = if let Some(sel_idx) = selected_line_index {
            if sel_idx < lines.len() {
                if let Some(sel_ts) = lines[sel_idx].timestamp {
                    let elapsed = (sel_ts.timestamp() - start_time.timestamp()) as f64;
                    // Only show indicator if the selected time is within this filter's time range
                    if elapsed >= 0.0 && sel_ts.timestamp() <= end_time.timestamp() {
                        Some(((elapsed / bucket_size) as usize).min(NUM_BUCKETS - 1))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
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
                let rel_x = (pos.x - rect.min.x) / rect.width();
                let bucket_idx = (rel_x * NUM_BUCKETS as f32) as usize;
                
                if bucket_idx < NUM_BUCKETS {
                    // Find first line in this bucket
                    let target_time = start_time.timestamp() + (bucket_idx as f64 * bucket_size) as i64;
                    
                    // Find closest filtered line to this time
                    let mut closest_idx = None;
                    let mut min_diff = i64::MAX;
                    
                    for &line_idx in filtered_indices {
                        if let Some(ts) = lines[line_idx].timestamp {
                            let diff = (ts.timestamp() - target_time).abs();
                            if diff < min_diff {
                                min_diff = diff;
                                closest_idx = Some(line_idx);
                            }
                        }
                    }
                    
                    if let Some(idx) = closest_idx {
                        click_event = Some(HistogramClickEvent {
                            line_index: idx,
                            timestamp: lines[idx].timestamp,
                        });
                    }
                }
            }
        }
        
        // Show time range below
        ui.horizontal(|ui| {
            ui.label(format!("Timeline: {} → {}", 
                start_time.format("%H:%M:%S"),
                end_time.format("%H:%M:%S")
            ));
            if let Some(sel_idx) = selected_line_index {
                if sel_idx < lines.len() {
                    if let Some(sel_ts) = lines[sel_idx].timestamp {
                        ui.separator();
                        ui.colored_label(Color32::YELLOW, format!("Selected: {}", sel_ts.format("%H:%M:%S%.3f")));
                    }
                }
            }
        });
        
        click_event
    }
}
