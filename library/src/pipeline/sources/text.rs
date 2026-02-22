//! Text decomposition: convert text into per-character glyph outline paths.
//!
//! This module converts a text string + font into `ShapeData::Grouped`,
//! where each character is a `ShapeGroup` with an SVG path representing
//! the glyph outline. Effector/decorator nodes can then modify each
//! group's transform before rasterization.

use crate::pipeline::output::{FontInfo, LineInfo, ShapeData, ShapeGroup};
use crate::pipeline::processing::ensemble::types::TransformData;

/// Convert a `skia_safe::Path` to an SVG path data string.
///
/// Iterates over the path verbs and builds M/L/Q/C/Z commands.
fn path_to_svg(path: &skia_safe::Path) -> String {
    use skia_safe::PathVerb;
    use std::fmt::Write;
    let mut svg = String::new();
    for rec in path.iter() {
        let pts = rec.points();
        match rec.verb() {
            PathVerb::Move => {
                let _ = write!(svg, "M {:.2} {:.2} ", pts[0].x, pts[0].y);
            }
            PathVerb::Line => {
                let _ = write!(svg, "L {:.2} {:.2} ", pts[1].x, pts[1].y);
            }
            PathVerb::Quad => {
                let _ = write!(
                    svg,
                    "Q {:.2} {:.2} {:.2} {:.2} ",
                    pts[1].x, pts[1].y, pts[2].x, pts[2].y
                );
            }
            PathVerb::Conic => {
                // Approximate conic as quadratic (Skia conics are rarely used in fonts)
                let _ = write!(
                    svg,
                    "Q {:.2} {:.2} {:.2} {:.2} ",
                    pts[1].x, pts[1].y, pts[2].x, pts[2].y
                );
            }
            PathVerb::Cubic => {
                let _ = write!(
                    svg,
                    "C {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} ",
                    pts[1].x, pts[1].y, pts[2].x, pts[2].y, pts[3].x, pts[3].y
                );
            }
            PathVerb::Close => {
                let _ = write!(svg, "Z ");
            }
        }
    }
    svg.trim().to_string()
}

/// Decompose text into per-character glyph outline shapes.
///
/// Returns `ShapeData::Grouped` with one `ShapeGroup` per character.
/// Each group contains the SVG path of the glyph outline, positioned
/// at the character's layout position.
pub fn decompose_text_to_shapes(text: &str, font_name: &str, size: f64) -> ShapeData {
    let font_mgr = skia_safe::FontMgr::default();
    let typeface = font_mgr
        .match_family_style(font_name, skia_safe::FontStyle::default())
        .unwrap_or_else(|| {
            font_mgr
                .legacy_make_typeface(None, skia_safe::FontStyle::default())
                .unwrap()
        });
    let font = skia_safe::Font::from_typeface(typeface, size as f32);

    let (_, metrics) = font.metrics();
    let line_height = -metrics.ascent + metrics.descent + metrics.leading;

    let mut groups = Vec::new();
    let mut lines = Vec::new();
    let mut global_index: usize = 0;

    // Split text by newlines for multi-line support
    let text_lines: Vec<&str> = text.split('\n').collect();

    let mut y_offset = 0.0f32;
    let mut global_min_x = f32::MAX;
    let mut global_min_y = f32::MAX;
    let mut global_max_x = f32::MIN;
    let mut global_max_y = f32::MIN;

    for (line_idx, line_text) in text_lines.iter().enumerate() {
        let line_start = groups.len();
        let mut x_pos = 0.0f32;

        let mut line_min_x = f32::MAX;
        let mut line_min_y = f32::MAX;
        let mut line_max_x = f32::MIN;
        let mut line_max_y = f32::MIN;

        for ch in line_text.chars() {
            if ch.is_whitespace() && ch == ' ' {
                // Space: measure advance but don't add a glyph path
                let (advance, _) = font.measure_str(&ch.to_string(), None);
                let group = ShapeGroup {
                    path: String::new(),
                    source_char: ch.to_string(),
                    index: global_index,
                    line_index: line_idx,
                    base_position: (x_pos, y_offset),
                    bounds: (0.0, 0.0, advance, line_height),
                    transform: TransformData::identity(),
                    decorations: Vec::new(),
                };
                groups.push(group);
                x_pos += advance;
                global_index += 1;
                continue;
            }

            let ch_str = ch.to_string();
            let (advance, _) = font.measure_str(&ch_str, None);

            // Get glyph IDs for this character
            let glyph_ids = font.str_to_glyphs_vec(&ch_str);
            let glyph_path_svg = if let Some(&glyph_id) = glyph_ids.first() {
                // Extract glyph outline as a Skia Path
                if let Some(glyph_path) = font.get_path(glyph_id) {
                    // Offset the glyph path to its layout position
                    let positioned = glyph_path.make_offset((x_pos, y_offset - metrics.ascent));
                    path_to_svg(&positioned)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            // Compute bounds for this glyph
            let glyph_bounds = if !glyph_path_svg.is_empty() {
                if let Some(&glyph_id) = glyph_ids.first() {
                    if let Some(glyph_path) = font.get_path(glyph_id) {
                        let b = glyph_path.bounds();
                        (b.x(), b.y(), b.width(), b.height())
                    } else {
                        (0.0, 0.0, advance, line_height)
                    }
                } else {
                    (0.0, 0.0, advance, line_height)
                }
            } else {
                (0.0, 0.0, advance, line_height)
            };

            // Update line bounds
            let abs_left = x_pos;
            let abs_top = y_offset;
            let abs_right = x_pos + advance;
            let abs_bottom = y_offset + line_height;
            line_min_x = line_min_x.min(abs_left);
            line_min_y = line_min_y.min(abs_top);
            line_max_x = line_max_x.max(abs_right);
            line_max_y = line_max_y.max(abs_bottom);

            let group = ShapeGroup {
                path: glyph_path_svg,
                source_char: ch.to_string(),
                index: global_index,
                line_index: line_idx,
                base_position: (x_pos, y_offset),
                bounds: glyph_bounds,
                transform: TransformData::identity(),
                decorations: Vec::new(),
            };
            groups.push(group);
            x_pos += advance;
            global_index += 1;
        }

        let line_end = groups.len();

        // Clamp line bounds for empty lines
        if line_min_x > line_max_x {
            line_min_x = 0.0;
            line_max_x = 0.0;
            line_min_y = y_offset;
            line_max_y = y_offset + line_height;
        }

        lines.push(LineInfo {
            group_range: line_start..line_end,
            bounds: (
                line_min_x,
                line_min_y,
                line_max_x - line_min_x,
                line_max_y - line_min_y,
            ),
        });

        // Update global bounds
        global_min_x = global_min_x.min(line_min_x);
        global_min_y = global_min_y.min(line_min_y);
        global_max_x = global_max_x.max(line_max_x);
        global_max_y = global_max_y.max(line_max_y);

        y_offset += line_height;
    }

    // Clamp global bounds
    if global_min_x > global_max_x {
        global_min_x = 0.0;
        global_max_x = 0.0;
        global_min_y = 0.0;
        global_max_y = 0.0;
    }

    ShapeData::Grouped {
        groups,
        bounds: (
            global_min_x,
            global_min_y,
            global_max_x - global_min_x,
            global_max_y - global_min_y,
        ),
        lines,
        font_info: FontInfo {
            family: font_name.to_string(),
            size,
        },
    }
}
