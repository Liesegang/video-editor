//! Pixel oracles for Tracking over SkParagraph's visual BiDi order.
//!
//! Runtime element identities remain in logical source order. Tracking is a
//! visual spacing operation: within each line its leftmost shaped element is
//! the anchor, and subsequent visual neighbours receive the signed authored
//! amount. Block continues that rank across lines while Line restarts it.

use std::collections::HashMap;

use super::neutral_ensemble_text::{
    alpha_bounds, assert_pixels_exact, fill, neutral_ensemble, render_raster,
};
use super::*;
use crate::core::ensemble::target::EffectorTarget;
use crate::core::ensemble::types::{EffectorConfig, EnsembleData, TransformData};
use crate::model::frame::runtime_shape::RuntimeTextShape;

fn tracking(amount: f32, target: EffectorTarget) -> EnsembleData {
    EnsembleData {
        enabled: true,
        effector_configs: vec![EffectorConfig::Tracking { amount, target }],
        decorator_configs: Vec::new(),
        patches: HashMap::new(),
    }
}

fn visual_rank_patches(
    shape: &RuntimeTextShape,
    amount: f32,
    target: EffectorTarget,
) -> HashMap<usize, TransformData> {
    let mut patches = HashMap::new();
    let mut block_rank = 0_usize;
    for line in &shape.lines {
        let mut indices = line.element_range.clone().collect::<Vec<_>>();
        indices.sort_by(|left, right| {
            shape.elements[*left]
                .bounds
                .left
                .total_cmp(&shape.elements[*right].bounds.left)
                .then_with(|| {
                    shape.elements[*left]
                        .bounds
                        .right
                        .total_cmp(&shape.elements[*right].bounds.right)
                })
                .then_with(|| left.cmp(right))
        });
        for (line_rank, index) in indices.into_iter().enumerate() {
            let element = &shape.elements[index];
            let rank = match target {
                EffectorTarget::Block => block_rank + line_rank,
                EffectorTarget::Line => line_rank,
                EffectorTarget::Char => 0,
                EffectorTarget::Parts => panic!("Tracking Parts is unsupported"),
            };
            patches.insert(
                element.block_element_index,
                TransformData {
                    translate: (amount * rank as f32, 0.0),
                    ..TransformData::identity()
                },
            );
        }
        block_rank += line.element_range.len();
    }
    patches
}

fn expected_visual_tracking(text: &str, amount: f32, target: EffectorTarget) -> EnsembleData {
    let shape = crate::rendering::text_layout::layout_runtime_text_shape(text, "Arial", 42.0);
    EnsembleData {
        patches: visual_rank_patches(&shape, amount, target),
        ..neutral_ensemble()
    }
}

fn manual_rank_patches(
    shape: &RuntimeTextShape,
    ranks: impl Fn(&crate::model::frame::runtime_shape::RuntimeTextElement) -> usize,
    amount: f32,
) -> HashMap<usize, TransformData> {
    shape
        .elements
        .iter()
        .map(|element| {
            (
                element.block_element_index,
                TransformData {
                    translate: (amount * ranks(element) as f32, 0.0),
                    ..TransformData::identity()
                },
            )
        })
        .collect()
}

fn assert_tracking_matches_visual_order(
    text: &str,
    amount: f32,
    target: EffectorTarget,
    context: &str,
) {
    let styles = [fill()];
    let transform = Affine2D::translate(80.0, 48.0);
    let actual = tracking(amount, target);
    let expected = expected_visual_tracking(text, amount, target);
    let actual_pixels = render_raster(text, &styles, Some(&actual), transform);
    let expected_pixels = render_raster(text, &styles, Some(&expected), transform);
    let actual_bounds = alpha_bounds(&actual_pixels);
    let expected_bounds = alpha_bounds(&expected_pixels);
    assert_eq!(
        actual_bounds, expected_bounds,
        "{context}: Tracking did not preserve the visual-order pixel bounds"
    );
    assert_pixels_exact(context, &expected_pixels, &actual_pixels);

    let neutral = render_raster(text, &styles, Some(&neutral_ensemble()), transform);
    let neutral_bounds = alpha_bounds(&neutral);
    let tracked_bounds = actual_bounds;
    assert_eq!(
        (tracked_bounds.1, tracked_bounds.3),
        (neutral_bounds.1, neutral_bounds.3),
        "{context}: horizontal Tracking changed the line's vertical pixel bounds"
    );
}

#[test]
fn hebrew_tracking_uses_visual_left_to_right_spacing_for_both_signs() {
    let text = "\u{5d0}\u{5d1}\u{5d2}\u{5d3}";
    for amount in [14.0, -4.0] {
        assert_tracking_matches_visual_order(
            text,
            amount,
            EffectorTarget::Line,
            &format!("Hebrew Line Tracking {amount}"),
        );
    }
}

#[test]
fn mixed_bidi_tracking_matches_visual_order_for_line_and_block_targets() {
    let text = "AB \u{5d0}\u{5d1} CD\n\u{5d2}\u{5d3} EF";
    for target in [EffectorTarget::Line, EffectorTarget::Block] {
        for amount in [9.0, -3.0] {
            assert_tracking_matches_visual_order(
                text,
                amount,
                target,
                &format!("mixed BiDi {target:?} Tracking {amount}"),
            );
        }
    }
}

#[test]
fn zero_tracking_and_character_tracking_are_pixel_neutral_for_bidi_text() {
    let text = "A \u{5d0}\u{5d1} B";
    let styles = [fill()];
    let transform = Affine2D::translate(80.0, 48.0);
    let neutral = render_raster(text, &styles, Some(&neutral_ensemble()), transform);
    for ensemble in [
        tracking(0.0, EffectorTarget::Line),
        tracking(0.0, EffectorTarget::Block),
        tracking(18.0, EffectorTarget::Char),
    ] {
        let tracked = render_raster(text, &styles, Some(&ensemble), transform);
        assert_pixels_exact("neutral BiDi Tracking", &neutral, &tracked);
    }
}

#[test]
fn tracking_keeps_an_authoritative_ligature_cluster_atomic() {
    let (text, shape) = ["X ffi Y", "X ff Y", "X \u{644}\u{627} Y"]
        .into_iter()
        .map(|text| {
            (
                text,
                crate::rendering::text_layout::layout_runtime_text_shape(text, "Arial", 42.0),
            )
        })
        .find(|(text, shape)| {
            shape
                .elements
                .iter()
                .any(|element| element.source.chars().count() > 1 && text.contains(&element.source))
        })
        .expect("fixture font must expose one multi-grapheme shaped cluster");
    let cluster = shape
        .elements
        .iter()
        .find(|element| element.source.chars().count() > 1)
        .expect("ligature cluster");
    assert!(cluster.utf8_range.len() > 1);

    assert_tracking_matches_visual_order(
        text,
        11.0,
        EffectorTarget::Line,
        "ligature-cluster Tracking",
    );
}

#[test]
fn arabic_cursive_word_is_one_spacing_unit_for_both_signs() {
    let styles = [fill()];
    let transform = Affine2D::translate(80.0, 48.0);
    for text in [
        "\u{633}\u{644}\u{627}\u{645}",
        // Hamza is a non-joining Arabic letter, but remains part of the same
        // cursive word spacing unit rather than creating a synthetic gap.
        "\u{621}\u{633}\u{644}\u{627}\u{645}",
    ] {
        let neutral = render_raster(text, &styles, Some(&neutral_ensemble()), transform);
        for amount in [12.0, -3.0] {
            let tracked = tracking(amount, EffectorTarget::Line);
            let tracked = render_raster(text, &styles, Some(&tracked), transform);
            assert_pixels_exact(
                &format!("one Arabic cursive word {text:?} at Tracking {amount}"),
                &neutral,
                &tracked,
            );
        }
    }
}

#[test]
fn mixed_arabic_tracking_spaces_whole_words_without_breaking_joins() {
    let text = "A \u{633}\u{644}\u{627}\u{645} B";
    let arabic_start = text.find('\u{633}').expect("Arabic word start");
    let arabic_end = arabic_start + "\u{633}\u{644}\u{627}\u{645}".len();
    let shape = crate::rendering::text_layout::layout_runtime_text_shape(text, "Arial", 42.0);
    let styles = [fill()];
    let transform = Affine2D::translate(80.0, 48.0);

    for amount in [12.0, -3.0] {
        // Explicit authoring oracle: A, whitespace, the entire cursive word,
        // whitespace, and B are five visual spacing units. All shaped
        // elements inside the Arabic word share rank 2, preserving joins.
        let patches = manual_rank_patches(
            &shape,
            |element| {
                if element.utf8_range.end <= arabic_start {
                    usize::from(element.source.chars().all(char::is_whitespace))
                } else if element.utf8_range.start >= arabic_end {
                    if element.source.chars().all(char::is_whitespace) {
                        3
                    } else {
                        4
                    }
                } else {
                    2
                }
            },
            amount,
        );
        let expected = EnsembleData {
            patches,
            ..neutral_ensemble()
        };
        let actual = tracking(amount, EffectorTarget::Line);
        let expected_pixels = render_raster(text, &styles, Some(&expected), transform);
        let actual_pixels = render_raster(text, &styles, Some(&actual), transform);
        assert_pixels_exact(
            &format!("mixed Arabic whole-word Tracking {amount}"),
            &expected_pixels,
            &actual_pixels,
        );
    }
}

#[test]
fn zero_width_format_character_does_not_add_a_tracking_gap() {
    let plain_text = "AB";
    let formatted_text = "A\u{200b}B";
    let styles = [fill()];
    let transform = Affine2D::translate(80.0, 48.0);
    let neutral = neutral_ensemble();
    let plain = render_raster(plain_text, &styles, Some(&neutral), transform);
    let formatted = render_raster(formatted_text, &styles, Some(&neutral), transform);
    assert_pixels_exact("zero-width format precondition", &plain, &formatted);

    let tracked = tracking(12.0, EffectorTarget::Line);
    let plain = render_raster(plain_text, &styles, Some(&tracked), transform);
    let formatted = render_raster(formatted_text, &styles, Some(&tracked), transform);
    assert_pixels_exact("zero-width format Tracking", &plain, &formatted);
}

#[test]
fn arabic_indic_digits_remain_independent_visual_spacing_units() {
    for text in ["\u{661}\u{662}\u{663}", "A\u{661}\u{662}B"] {
        for amount in [12.0, -3.0] {
            assert_tracking_matches_visual_order(
                text,
                amount,
                EffectorTarget::Line,
                &format!("Arabic-Indic digits {text:?} at Tracking {amount}"),
            );
        }
    }
}
