/// Render the anomaly score explanation window
pub fn render_anomaly_explanation(ctx: &egui::Context, open: &mut bool) {
    egui::Window::new("Anomaly Score Calculation")
        .collapsible(false)
        .resizable(true)
        .default_width(700.0)
        .open(open)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("How Anomaly Scores are Calculated");
                ui.add_space(10.0);

                ui.label("LogCrab uses a multi-component scoring system to identify interesting, unusual, or problematic log lines. Each line receives a score from 0-100, where higher scores indicate higher anomaly.");
                ui.add_space(15.0);

                ui.heading("Scoring Components:");
                ui.add_space(10.0);

                // Rarity Scorer
                ui.label(egui::RichText::new("1. Rarity Scorer (Weight: 3.0)").strong().color(egui::Color32::from_rgb(100, 200, 255)));
                ui.indent("rarity", |ui| {
                    ui.label("• Scores based on template rarity (inverse frequency)");
                    ui.label("• Never-seen-before messages score 1.0 (maximum)");
                    ui.label("• Score = √(1 - frequency) where frequency = count/total");
                    ui.label("• Rare messages get higher scores than common ones");
                    ui.label("• Example: A unique error gets 1.0, while a repeated 'INFO: started' gets ~0.1");
                });
                ui.add_space(10.0);

                // Keyword Scorer
                ui.label(egui::RichText::new("2. Keyword Scorer (Weight: 2.5)").strong().color(egui::Color32::from_rgb(100, 200, 255)));
                ui.indent("keyword", |ui| {
                    ui.label("• Detects important keywords indicating issues");
                    ui.label("• ERROR/EXCEPTION/FATAL/CRASH/PANIC → score 1.0");
                    ui.label("• FAIL/FAILED/TIMEOUT/DENIED → score 0.8");
                    ui.label("• WARN/WARNING/ALERT → score 0.6");
                    ui.label("• ISSUE/PROBLEM/UNABLE/INVALID → score 0.4");
                    ui.label("• Case-insensitive pattern matching");
                });
                ui.add_space(10.0);

                // Temporal Scorer
                ui.label(egui::RichText::new("3. Temporal Scorer (Weight: 2.0)").strong().color(egui::Color32::from_rgb(100, 200, 255)));
                ui.indent("temporal", |ui| {
                    ui.label("• Analyzes time-based patterns with a 30-second window");
                    ui.label("• Recency component: Long gaps since last occurrence → higher score");
                    ui.label("  - Never seen in tracking: +0.7");
                    ui.label("  - Gap > 30 seconds: +0.5");
                    ui.label("  - Gap < 30 seconds: scaled 0.0-0.3 based on gap length");
                    ui.label("• Burst detection: High activity bursts → +0.3");
                    ui.label("  - Triggered when >100 events and >10 events/second");
                });
                ui.add_space(10.0);

                // Entropy Scorer
                ui.label(egui::RichText::new("4. Entropy Scorer (Weight: 1.5)").strong().color(egui::Color32::from_rgb(100, 200, 255)));
                ui.indent("entropy", |ui| {
                    ui.label("• Measures information content using Shannon entropy");
                    ui.label("• Entropy = -Σ(p × log₂(p)) where p = character frequency");
                    ui.label("• Tracks running average of entropy and message length");
                    ui.label("• Score based on deviation from average:");
                    ui.label("  - entropy_deviation = |entropy - avg_entropy| / avg_entropy");
                    ui.label("  - length_deviation = |length - avg_length| / avg_length");
                    ui.label("  - final_score = (entropy_deviation + length_deviation) / 2");
                    ui.label("• Unusual messages (very short/long or random) score higher");
                });
                ui.add_space(15.0);

                ui.separator();
                ui.add_space(10.0);

                ui.heading("Final Score Calculation:");
                ui.add_space(10.0);

                ui.label("1. Each scorer produces a raw score (0.0 - 1.0)");
                ui.label("2. Raw scores are weighted and summed:");
                ui.indent("formula", |ui| {
                    ui.label("raw_score = (rarity × 3.0) + (keyword × 2.5) + (temporal × 2.0) + (entropy × 1.5)");
                });
                ui.label("3. All raw scores are normalized to 0-100 range:");
                ui.indent("normalize", |ui| {
                    ui.label("normalized = ((score - min_score) / (max_score - min_score)) × 100");
                });
                ui.add_space(10.0);

                ui.separator();
                ui.add_space(10.0);

                ui.heading("Color Coding:");
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("■").color(egui::Color32::from_rgb(255, 50, 50)));
                    ui.label("Red (80-100): High anomaly - crashes, errors, rare events");
                });
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("■").color(egui::Color32::from_rgb(255, 140, 0)));
                    ui.label("Orange (60-79): Medium anomaly - warnings, failures");
                });
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("■").color(egui::Color32::from_rgb(255, 200, 200)));
                    ui.label("Pink (30-59): Low anomaly - slightly unusual patterns");
                });
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("■").color(egui::Color32::WHITE));
                    ui.label("White (0-29): Normal - common, expected log lines");
                });

                ui.add_space(15.0);
                ui.separator();
                ui.add_space(10.0);

                ui.label(egui::RichText::new("Note:").strong());
                ui.label("Scores are calculated during file loading in a single pass. The scorer learns patterns as it processes lines sequentially, so later lines benefit from more context.");
            });
        });
}
