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
use egui_extras::{Column, TableBuilder};

/// Maximum number of attention entries shown in the panel.
const MAX_ENTRIES: usize = 30;

/// Render the floating attention-weights panel.
///
/// - `open`       — toggled to `false` when the user closes the window.
/// - `target`     — the `StoreID` of the line the user right-clicked.
/// - `result`     — the latest `ExplainResult` from the sidecar, if any.
/// - `is_pending`     — `true` while a request is in flight.
/// - `session_error`  — set when the WebSocket closed while pending.
pub fn render_attention_panel(
    ctx: &egui::Context,
    open: &mut bool,
    store: &LogStore,
    target: Option<StoreID>,
    result: Option<&ExplainResult>,
    is_pending: bool,
    session_error: Option<&str>,
) {
    egui::Window::new("Attention Weights")
        .collapsible(false)
        .resizable(true)
        .default_width(640.0)
        .default_height(480.0)
        .open(open)
        .show(ctx, |ui| {
            render_content(ui, store, target, result, is_pending, session_error);
        });
}

fn render_content(
    ui: &mut Ui,
    store: &LogStore,
    target: Option<StoreID>,
    result: Option<&ExplainResult>,
    is_pending: bool,
    session_error: Option<&str>,
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
    // ── Session error (WebSocket closed while pending) ────────────────────
    if let Some(err) = session_error {
        ui.label(
            RichText::new(format!("⚠ {err}"))
                .color(Color32::from_rgb(200, 80, 60)),
        );
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

    // ── Top predicted templates ───────────────────────────────────────────────
    if !result.top_templates.is_empty() {
        ui.separator();
        ui.label(RichText::new("Top predicted templates").strong());
        ui.add_space(2.0);

        let max_prob = result
            .top_templates
            .first()
            .map_or(1.0_f32, |e| e.probability)
            .max(f32::EPSILON);

        ui.push_id("templates_table", |ui| { TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(130.0).at_least(80.0))
            .column(Column::remainder().at_least(100.0))
            .header(20.0, |mut header| {
                header.col(|ui| { ui.label(RichText::new("Prob").strong()); });
                header.col(|ui| { ui.label(RichText::new("Template").strong()); });
            })
            .body(|mut body| {
                for entry in &result.top_templates {
                    body.row(18.0, |mut row| {
                        row.col(|ui| {
                            let normalized = entry.probability / max_prob;
                            ui.add(
                                egui::ProgressBar::new(normalized)
                                    .desired_width(ui.available_width())
                                    .text(format!("{:.1}%", entry.probability * 100.0)),
                            );
                        });
                        row.col(|ui| {
                            ui.add(
                                egui::Label::new(RichText::new(entry.template.clone()).monospace())
                                    .wrap_mode(egui::TextWrapMode::Truncate),
                            );
                        });
                    });
                }
            }); });
    }

    if result.attention.is_empty() {
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

    ui.push_id("attention_table", |ui| { TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::auto().at_least(50.0))
        .column(Column::initial(130.0).at_least(80.0))
        .column(Column::remainder().at_least(100.0))
        .header(20.0, |mut header| {
            header.col(|ui| { ui.label(RichText::new("Line").strong()); });
            header.col(|ui| { ui.label(RichText::new("Weight").strong()); });
            header.col(|ui| { ui.label(RichText::new("Message").strong()); });
        })
        .body(|mut body| {
            for entry in result.attention.iter().take(MAX_ENTRIES) {
                body.row(18.0, |mut row| {
                    let sid = StoreID::make(source_id, entry.line_number);
                    let msg = store
                        .get_by_id(&sid)
                        .map(|l| l.message.clone())
                        .unwrap_or_else(|| format!("<line {}>", entry.line_number));

                    row.col(|ui| { ui.label(entry.line_number.to_string()); });

                    // Attention bar
                    row.col(|ui| {
                        let normalized = entry.weight / max_weight;
                        let fill = weight_color(normalized);
                        ui.add(
                            egui::ProgressBar::new(normalized)
                                .desired_width(ui.available_width())
                                .fill(fill),
                        );
                    });

                    row.col(|ui| {
                        ui.add(
                            egui::Label::new(RichText::new(msg).monospace())
                                .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                    });
                });
            }
        }); });
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
