//! Visual spacing sequences derived from the authoritative shaped Paragraph.
//!
//! Source elements remain in logical order for selection, patches, and timed
//! effectors. Tracking alone uses these visual ordinals, so BiDi reordering
//! does not turn a positive amount into overlap. Cursive words are one spacing
//! unit because translating their already-shaped letters independently would
//! break the joins retained by SkParagraph.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use icu_properties::CodePointMapData;
use icu_properties::props::{GeneralCategory, GeneralCategoryGroup, JoiningType, Script};
use unicode_segmentation::UnicodeSegmentation;

use crate::core::ensemble::effectors::SpacingSequence;
use crate::model::frame::runtime_shape::RuntimeTextShape;

use super::shaped_runs::ShapedGlyphRun;

static CURSIVE_SCRIPTS: LazyLock<Vec<Script>> = LazyLock::new(|| {
    let joining_types = CodePointMapData::<JoiningType>::new();
    let scripts = CodePointMapData::<Script>::new();
    let mut result = Vec::new();
    for range in joining_types.iter_ranges().filter(|range| {
        matches!(
            range.value,
            JoiningType::DualJoining
                | JoiningType::LeftJoining
                | JoiningType::RightJoining
                | JoiningType::JoinCausing
        )
    }) {
        for code_point in range.range {
            let Some(character) = char::from_u32(code_point) else {
                continue;
            };
            let script = scripts.get(character);
            if script != Script::Common && script != Script::Inherited {
                result.push(script);
            }
        }
    }
    result.sort_unstable_by_key(|script| script.to_icu4c_value());
    result.dedup();
    result
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScriptClass {
    Cursive(Script),
    Neutral,
    Other,
}

#[derive(Debug)]
struct SpacingUnit {
    members: Vec<usize>,
    visual_left: f32,
    logical_first: usize,
}

/// Assign visual left-to-right ordinals without changing logical element IDs.
pub(super) fn assign_spacing_sequences(text: &mut RuntimeTextShape, runs: &[ShapedGlyphRun]) {
    for element in &mut text.elements {
        element.line_spacing = SpacingSequence::default();
        element.block_spacing = SpacingSequence::default();
    }
    if text.elements.is_empty() {
        return;
    }

    let ink_left = glyph_ink_left(text, runs);
    let eligible = text
        .elements
        .iter()
        .enumerate()
        .map(|(index, element)| element.advance > 0.0 || ink_left[index].is_some())
        .collect::<Vec<_>>();
    let mut unit_root = (0..text.elements.len()).collect::<Vec<_>>();
    assign_cursive_word_roots(text, &mut unit_root);

    let mut line_units = Vec::with_capacity(text.lines.len());
    for line in &text.lines {
        let mut members_by_root = BTreeMap::<usize, Vec<usize>>::new();
        for index in line.element_range.clone() {
            if let Some(root) = unit_root.get(index).copied() {
                members_by_root.entry(root).or_default().push(index);
            }
        }
        let mut units = members_by_root
            .into_values()
            .filter_map(|members| {
                let visible_members = members
                    .iter()
                    .copied()
                    .filter(|index| eligible[*index])
                    .collect::<Vec<_>>();
                if visible_members.is_empty() {
                    return None;
                }
                let visual_left = visible_members
                    .iter()
                    .map(|index| {
                        let element = &text.elements[*index];
                        if element.advance > 0.0 {
                            element.bounds.left
                        } else {
                            ink_left[*index].unwrap_or(element.bounds.left)
                        }
                    })
                    .min_by(f32::total_cmp)
                    .unwrap_or_default();
                Some(SpacingUnit {
                    logical_first: members[0],
                    members,
                    visual_left,
                })
            })
            .collect::<Vec<_>>();
        units.sort_by(|left, right| {
            left.visual_left
                .total_cmp(&right.visual_left)
                .then_with(|| left.logical_first.cmp(&right.logical_first))
        });
        line_units.push(units);
    }

    let block_total = line_units.iter().map(Vec::len).sum::<usize>();
    let mut block_index = 0_usize;
    for units in line_units {
        let line_total = units.len();
        for (line_index, unit) in units.into_iter().enumerate() {
            for element_index in unit.members {
                text.elements[element_index].line_spacing = SpacingSequence {
                    index: line_index,
                    total: line_total,
                };
                text.elements[element_index].block_spacing = SpacingSequence {
                    index: block_index,
                    total: block_total,
                };
            }
            block_index += 1;
        }
    }
}

fn glyph_ink_left(text: &RuntimeTextShape, runs: &[ShapedGlyphRun]) -> Vec<Option<f32>> {
    let mut result = vec![None::<f32>; text.elements.len()];
    for run in runs {
        for ((source_start, position), bounds) in run
            .source_starts
            .iter()
            .take(run.glyphs.len())
            .zip(&run.positions)
            .zip(&run.bounds)
        {
            if bounds.is_empty() {
                continue;
            }
            let Some(index) = text.element_index_at_utf8(*source_start as usize) else {
                continue;
            };
            let left = run.origin.x + position.x + bounds.left;
            result[index] = Some(result[index].map_or(left, |current| current.min(left)));
        }
    }
    result
}

fn assign_cursive_word_roots(text: &RuntimeTextShape, roots: &mut [usize]) {
    for (word_start, word) in text.text.unicode_word_indices() {
        let word_end = word_start + word.len();
        let element_start = text
            .elements
            .partition_point(|element| element.utf8_range.end <= word_start);
        let element_end = text
            .elements
            .partition_point(|element| element.utf8_range.start < word_end);
        let mut segment_start = element_start;
        let mut active_script = None;
        for index in element_start..element_end {
            match element_script_class(&text.elements[index].source) {
                ScriptClass::Cursive(script) => match active_script {
                    Some(current) if current != script => {
                        assign_shared_root(roots, segment_start..index);
                        segment_start = index;
                        active_script = Some(script);
                    }
                    Some(_) => {}
                    None => active_script = Some(script),
                },
                ScriptClass::Neutral => {}
                ScriptClass::Other => {
                    if active_script.is_some() {
                        assign_shared_root(roots, segment_start..index);
                    }
                    active_script = None;
                    segment_start = index + 1;
                }
            }
        }
        if active_script.is_some() {
            assign_shared_root(roots, segment_start..element_end);
        }
    }
}

fn assign_shared_root(roots: &mut [usize], range: std::ops::Range<usize>) {
    if range.is_empty() {
        return;
    }
    for index in range.clone() {
        roots[index] = range.start;
    }
}

fn element_script_class(source: &str) -> ScriptClass {
    let scripts = CodePointMapData::<Script>::new();
    let categories = CodePointMapData::<GeneralCategory>::new();
    let joining_types = CodePointMapData::<JoiningType>::new();
    let mut result = ScriptClass::Neutral;
    for character in source.chars() {
        let script = scripts.get(character);
        let category = categories.get(character);
        if GeneralCategoryGroup::Mark.contains(category)
            || category == GeneralCategory::Format
            || joining_types.get(character) == JoiningType::JoinCausing
        {
            continue;
        }
        if GeneralCategoryGroup::Letter.contains(category) && CURSIVE_SCRIPTS.contains(&script) {
            match result {
                ScriptClass::Cursive(current) if current != script => return ScriptClass::Other,
                ScriptClass::Other => return ScriptClass::Other,
                _ => result = ScriptClass::Cursive(script),
            }
        } else {
            return ScriptClass::Other;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use icu_properties::CodePointMapData;
    use icu_properties::props::Script;

    use crate::core::rendering::text_layout::ShapedTextLayout;

    fn shaped(text: &str) -> ShapedTextLayout {
        ShapedTextLayout::new(text, "Arial", 40.0)
    }

    #[test]
    fn hebrew_and_mixed_text_use_visual_ordinals_without_reordering_source() {
        let layout = shaped("AB אבג 12");
        let elements = &layout.metadata.elements;
        assert!(
            elements
                .windows(2)
                .all(|pair| pair[0].utf8_range.start < pair[1].utf8_range.start)
        );
        let mut visible = elements
            .iter()
            .filter(|element| element.line_spacing.total > 0)
            .collect::<Vec<_>>();
        visible.sort_by(|left, right| left.bounds.left.total_cmp(&right.bounds.left));
        assert!(
            visible
                .iter()
                .enumerate()
                .all(|(index, element)| element.line_spacing.index == index)
        );
        let hebrew = elements
            .iter()
            .filter(|element| {
                element.source.chars().any(|character| {
                    CodePointMapData::<Script>::new().get(character) == Script::Hebrew
                })
            })
            .collect::<Vec<_>>();
        assert!(hebrew.windows(2).all(|pair| {
            pair[0].utf8_range.start < pair[1].utf8_range.start
                && pair[0].line_spacing.index > pair[1].line_spacing.index
        }));
    }

    #[test]
    fn arabic_word_is_one_spacing_unit_but_latin_remains_independent() {
        let layout = shaped("سلام AB");
        let arabic = layout
            .metadata
            .elements
            .iter()
            .filter(|element| element.utf8_range.end <= "سلام".len())
            .collect::<Vec<_>>();
        assert!(!arabic.is_empty());
        assert!(arabic.iter().all(|element| {
            element.line_spacing == arabic[0].line_spacing
                && element.block_spacing == arabic[0].block_spacing
        }));
        assert!(
            layout.metadata.elements.last().unwrap().line_spacing.index
                > arabic[0].line_spacing.index
        );
    }

    #[test]
    fn arabic_script_digits_remain_independent_spacing_units() {
        let layout = shaped("١٢٣");
        let elements = &layout.metadata.elements;
        assert_eq!(elements.len(), 3);
        assert_eq!(elements[0].line_spacing.total, 3);
        assert_eq!(
            elements
                .iter()
                .map(|element| element.line_spacing.index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn digit_splits_cursive_runs_inside_an_arabic_word() {
        let layout = shaped("س1لام");
        let elements = &layout.metadata.elements;
        let digit = elements
            .iter()
            .find(|element| element.source == "1")
            .unwrap();
        let trailing_arabic = elements
            .iter()
            .filter(|element| element.utf8_range.start > digit.utf8_range.start)
            .collect::<Vec<_>>();
        assert!(trailing_arabic.len() > 1);
        assert!(trailing_arabic.iter().all(|element| {
            element.line_spacing == trailing_arabic[0].line_spacing
                && element.line_spacing != digit.line_spacing
        }));
        assert_eq!(digit.line_spacing.total, 3);
    }

    #[test]
    fn arabic_marks_stay_in_the_word_spacing_unit() {
        let layout = shaped("سَلَام");
        let elements = &layout.metadata.elements;
        assert!(elements.len() > 1);
        assert!(elements.iter().all(|element| {
            element.line_spacing == elements[0].line_spacing
                && element.block_spacing == elements[0].block_spacing
        }));
    }

    #[test]
    fn invisible_zero_width_control_does_not_add_a_spacing_unit() {
        let plain = shaped("AB");
        let controlled = shaped("A\u{200b}B");
        assert_eq!(
            controlled
                .metadata
                .elements
                .iter()
                .map(|element| element.line_spacing.total)
                .max(),
            plain
                .metadata
                .elements
                .iter()
                .map(|element| element.line_spacing.total)
                .max()
        );
    }
}
