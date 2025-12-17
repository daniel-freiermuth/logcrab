use egui::{text::LayoutJob, Color32, TextFormat};
use fancy_regex::Regex;

/// A filter pattern with its associated color for highlighting
#[derive(Debug, Clone)]
pub struct FilterHighlight {
    pub regex: Regex,
    pub color: Color32,
}

impl FilterHighlight {
    /// Highlight matches from all filters in the text with alpha blending for overlaps
    pub fn highlight_text_with_filters(
        text: &str,
        base_color: Color32,
        all_filter_highlights: &[Self],
        dark_mode: bool,
    ) -> egui::text::LayoutJob {
        let mut job = LayoutJob::default();

        if text.is_empty() {
            return job;
        }

        // Collect all matches from all filters
        let mut matches: Vec<(usize, usize, Color32)> = Vec::new();

        for highlight in all_filter_highlights.iter().rev() {
            for mat in highlight.regex.find_iter(text).flatten() {
                matches.push((mat.start(), mat.end(), highlight.color));
            }
        }

        if matches.is_empty() {
            // No matches, return plain text
            job.append(
                text,
                0.0,
                TextFormat {
                    color: base_color,
                    ..Default::default()
                },
            );
            return job;
        }

        // Create a character-level color map for blending overlapping highlights
        let mut char_colors: Vec<Option<Color32>> = vec![None; text.len()];

        for (start, end, color) in matches {
            let range_end = end.min(text.len());
            for char_color in &mut char_colors[start..range_end] {
                *char_color = Some(match *char_color {
                    None => color,
                    Some(existing) => Self::blend_colors(existing, color),
                });
            }
        }

        // Build the job by merging adjacent characters with the same color
        let mut current_start = 0;
        let mut current_color = char_colors[0];

        for i in 1..text.len() {
            let next_color = char_colors[i];

            if next_color != current_color {
                // Color changed, append the current segment
                if let Some(bg_color) = current_color {
                    let text_color = Self::choose_text_color(bg_color, dark_mode);
                    job.append(
                        &text[current_start..i],
                        0.0,
                        TextFormat {
                            color: text_color,
                            background: bg_color,
                            ..Default::default()
                        },
                    );
                } else {
                    job.append(
                        &text[current_start..i],
                        0.0,
                        TextFormat {
                            color: base_color,
                            ..Default::default()
                        },
                    );
                }

                current_start = i;
                current_color = next_color;
            }
        }

        // Append the final segment
        if current_start < text.len() {
            if let Some(bg_color) = current_color {
                let text_color = Self::choose_text_color(bg_color, dark_mode);
                job.append(
                    &text[current_start..],
                    0.0,
                    TextFormat {
                        color: text_color,
                        background: bg_color,
                        ..Default::default()
                    },
                );
            } else {
                job.append(
                    &text[current_start..],
                    0.0,
                    TextFormat {
                        color: base_color,
                        ..Default::default()
                    },
                );
            }
        }

        job
    }

    /// Choose black or white text color based on background brightness
    /// Uses relative luminance calculation from WCAG guidelines
    fn choose_text_color(background: Color32, dark_mode: bool) -> Color32 {
        // For semi-transparent backgrounds, blend with the base background to get effective color
        let alpha = f32::from(background.a()) / 255.0;

        // Base background: black for dark mode, white for light mode
        let (base_r, base_g, base_b) = if dark_mode {
            (0.0, 0.0, 0.0)
        } else {
            (1.0, 1.0, 1.0)
        };

        // Blend highlight color with base background
        let effective_r = (f32::from(background.r()) / 255.0) * alpha + base_r * (1.0 - alpha);
        let effective_g = (f32::from(background.g()) / 255.0) * alpha + base_g * (1.0 - alpha);
        let effective_b = (f32::from(background.b()) / 255.0) * alpha + base_b * (1.0 - alpha);

        // Linearize (gamma correction) for proper luminance calculation
        let linearize = |c_norm: f32| -> f32 {
            if c_norm <= 0.03928 {
                c_norm / 12.92
            } else {
                ((c_norm + 0.055) / 1.055).powf(2.4)
            }
        };

        let r_linear = linearize(effective_r);
        let g_linear = linearize(effective_g);
        let b_linear = linearize(effective_b);

        // Calculate relative luminance: L = 0.2126 * R + 0.7152 * G + 0.0722 * B
        let luminance = 0.0722f32.mul_add(b_linear, 0.2126f32.mul_add(r_linear, 0.7152 * g_linear));

        // Use black text on bright backgrounds, white text on dark backgrounds
        // Threshold of 0.5 works well in practice
        if luminance > 0.5 {
            Color32::BLACK
        } else {
            Color32::WHITE
        }
    }

    /// Blend two colors with alpha compositing (Porter-Duff "over" operator)
    fn blend_colors(bottom: Color32, top: Color32) -> Color32 {
        // Convert to float for blending
        let bottom_a = f32::from(bottom.a()) / 255.0;
        let top_a = f32::from(top.a()) / 255.0;

        // Alpha compositing: out_a = top_a + bottom_a * (1 - top_a)
        let out_a = top_a + bottom_a * (1.0 - top_a);

        if out_a == 0.0 {
            return Color32::TRANSPARENT;
        }

        // For each color channel: out_c = (top_c * top_a + bottom_c * bottom_a * (1 - top_a)) / out_a
        let blend_channel = |top_c: u8, bottom_c: u8| -> u8 {
            let top_cf = f32::from(top_c) / 255.0;
            let bottom_cf = f32::from(bottom_c) / 255.0;

            let out_cf = (top_cf * top_a + bottom_cf * bottom_a * (1.0 - top_a)) / out_a;
            (out_cf * 255.0).round() as u8
        };

        Color32::from_rgba_premultiplied(
            blend_channel(top.r(), bottom.r()),
            blend_channel(top.g(), bottom.g()),
            blend_channel(top.b(), bottom.b()),
            (out_a * 255.0).round() as u8,
        )
    }
}
