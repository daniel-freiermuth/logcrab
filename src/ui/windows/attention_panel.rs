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

use crate::anomaly::sidecar_client::ExplainResult;
use crate::core::log_store::{LogStore, StoreID};
use egui::{Color32, RichText, Ui};

/// Maximum number of attention entries shown in the panel.
const MAX_ENTRIES: usize = 30;

/// Render the floating attention-weights panel.
///
/// - `open`       — toggled to `false` when the user closes the window.
/// - `target`     — the `StoreID` of the line the user right-clicked.
/// - `result`     — the latest `ExplainResult` from the sidecar, if any.
/// - `is_pending` — `true` while a request is in flight.
pub fn render_attention_panel(
    ctx: &egui::Context,
    open: &mut bool,
    store: &LogStore,
    target: Option<StoreID>,
    result: Option<&ExplainResult>,
    is_pending: bool,
) {
    egui::Window::new("Attention Weights")
        .collapsible(false)
        .resizable(true)
        .default_width(640.0)
        .default_height(480.0)
        .open(open)
        .show(ctx, |ui| {
            render_content(ui, store, target, result, is_pending);
        });
}

fn render_content(
    ui: &mut Ui,
    store: &LogStore,
    target: Option<StoreID>,
    result: Option<&ExplainResult>,
    is_pending: bool,
) {
    // ── Target line preview ───────────────────────────────────────────────────
    if let Some(t) = target {
        if let Some(line) = store.get_by_id(&t) {
            let preview: String = line.message.chars().take(100).collect();
            ui.label(
                RichText::new(format!("Line {}: {preview}", line.line_number))
                    .strong()
                    .monospace(),
            );
        }
    } else {
        ui.label("Right-click a scored line → Show Attention");
        return;
    }

    ui.separator();

    // ── Pending state ─────────────────────────────────────────────────────────
    if is_pending {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Computing attention…");
        });
        return;
    }

    // ── No result yet (panel just opened, before first click) ─────────────────
    let Some(result) = result else {
        ui.label("Right-click a scored line → Show Attention");
        return;
    };

    // ── Target not in corpus ──────────────────────────────────────────────────
    if !result.target_in_corpus {
        ui.label("⚠ This line was filtered out by the model's corpus filter — no attention available.");
        return;
    }

    // ── Score info ────────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        if let Some(score) = result.target_score {
            ui.label(format!("Loss: {score:.4}"));
        }
        if result.target_is_unk {
            ui.label(
                RichText::new(if result.target_is_rare { "RARE" } else { "UNK" })
                    .color(Color32::from_rgb(180, 120, 60)),
            );
        }
    });

    if result.attention.is_empty() {
        ui.label("No attention entries returned.");
        return;
    }

    ui.separator();
    ui.label(
        RichText::new(format!(
            "Context lines by attention (top {})",
            result.attention.len().min(MAX_ENTRIES)
        ))
        .strong(),
    );
    ui.add_space(4.0);

    // ── Attention table ───────────────────────────────────────────────────────
    let source_id = target.map(|t| t.source_id()).unwrap_or(0);

    // Entries are already sorted by weight descending from the sidecar.
    let max_weight = result
        .attention
        .first()
        .map_or(1.0_f32, |e| e.weight)
        .max(f32::EPSILON);

    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("attention_grid")
            .striped(true)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                // Header
                ui.label(RichText::new("Line").strong());
                ui.label(RichText::new("Weight").strong());
                ui.label(RichText::new("Message").strong());
                ui.end_row();

                for entry in result.attention.iter().take(MAX_ENTRIES) {
                    let sid = StoreID::make(source_id, entry.line_number);
                    let msg = store
                        .get_by_id(&sid)
                        .map(|l| l.message.chars().take(80).collect::<String>())
                        .unwrap_or_else(|| format!("<line {}>", entry.line_number));

                    ui.label(entry.line_number.to_string());

                    // Attention bar
                    let normalized = entry.weight / max_weight;
                    let fill = weight_color(normalized);
                    ui.add(
                        egui::ProgressBar::new(normalized)
                            .desired_width(120.0)
                            .fill(fill),
                    );

                    ui.add(egui::Label::new(RichText::new(msg).monospace()).wrap());
                    ui.end_row();
                }
            });
    });
}

/// Map a normalized attention weight (0 → 1) to a color: gray → orange → red.
fn weight_color(normalized: f32) -> Color32 {
    let t = normalized.clamp(0.0, 1.0);
    if t < 0.5 {
        // Gray → orange
        let u = t * 2.0;
        let r = (u.mul_add(255.0 - 128.0, 128.0)) as u8;
        let g = (u.mul_add(165.0 - 128.0, 128.0)) as u8;
        let b = ((1.0 - u) * 128.0) as u8;
        Color32::from_rgb(r, g, b)
    } else {
        // Orange → red
        let u = (t - 0.5) * 2.0;
        let r = 255;
        let g = ((1.0 - u) * 165.0) as u8;
        let b = 0;
        Color32::from_rgb(r, g, b)
    }
}
