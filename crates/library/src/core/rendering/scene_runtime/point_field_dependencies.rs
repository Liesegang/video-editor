//! Dependency ownership for independently consumable Point render outputs.

use crate::model::point::{PointInstruction, PointRenderProgram};

/// Marks instructions which contribute only to Sprite Value selection.
///
/// Point programs historically fail atomically when any ordinary instruction
/// is invalid. Sprite Random mode must not consume or be hidden by a dormant
/// Value-selector expression, so only that selector-exclusive dependency
/// closure is exempt. Color, geometry, and authored attribute stores remain
/// atomic roots; shared dependencies therefore retain the established policy.
pub(super) fn selector_exclusive(program: &PointRenderProgram) -> Vec<bool> {
    let Some(selector) = program.sprite_selection_register else {
        return vec![false; program.instructions.len()];
    };
    let dependencies = dependencies(program);
    let mut selector_reached = vec![false; dependencies.len()];
    visit(usize::from(selector), &dependencies, &mut selector_reached);

    let mut ordinary_reached = vec![false; dependencies.len()];
    visit(
        usize::from(program.color_register),
        &dependencies,
        &mut ordinary_reached,
    );
    if let Some(register) = program.position_register {
        visit(usize::from(register), &dependencies, &mut ordinary_reached);
    }
    if let Some(register) = program.size_register {
        visit(usize::from(register), &dependencies, &mut ordinary_reached);
    }
    for (index, instruction) in program.instructions.iter().enumerate() {
        if matches!(instruction, PointInstruction::StoreAttribute { .. }) {
            visit(index, &dependencies, &mut ordinary_reached);
        }
    }
    selector_reached
        .into_iter()
        .zip(ordinary_reached)
        .map(|(selector, ordinary)| selector && !ordinary)
        .collect()
}

fn dependencies(program: &PointRenderProgram) -> Vec<Vec<usize>> {
    let mut stores = vec![None; program.schema.attributes().len()];
    program
        .instructions
        .iter()
        .enumerate()
        .map(|(index, instruction)| match instruction {
            PointInstruction::Constant { .. }
            | PointInstruction::Age
            | PointInstruction::NormalizedAge
            | PointInstruction::Position
            | PointInstruction::Size
            | PointInstruction::Random { .. } => Vec::new(),
            PointInstruction::LoadAttribute { attribute } => {
                stores[usize::from(*attribute)].into_iter().collect()
            }
            PointInstruction::StoreAttribute { attribute, value } => {
                stores[usize::from(*attribute)] = Some(index);
                vec![usize::from(*value)]
            }
            PointInstruction::Binary { left, right, .. }
            | PointInstruction::Compare { left, right, .. } => {
                vec![usize::from(*left), usize::from(*right)]
            }
            PointInstruction::Length { value } => vec![usize::from(*value)],
            PointInstruction::Select {
                condition,
                when_true,
                when_false,
                ..
            } => vec![
                usize::from(*condition),
                usize::from(*when_true),
                usize::from(*when_false),
            ],
            PointInstruction::ColorRamp { factor, .. } => vec![usize::from(*factor)],
        })
        .collect()
}

fn visit(register: usize, dependencies: &[Vec<usize>], reached: &mut [bool]) {
    if register >= reached.len() || reached[register] {
        return;
    }
    reached[register] = true;
    for &dependency in &dependencies[register] {
        visit(dependency, dependencies, reached);
    }
}
