use super::*;

use crate::model::authoring::{Paint, ProjectDocument};
use crate::model::property::{ColorSpaceRef, ColorValue, PatternKind, PatternValue, Vec2};
use ordered_float::OrderedFloat;

fn managed_color(rgba: [f64; 4]) -> ColorValue {
    ColorValue::new(ColorSpaceRef::new("acescg").unwrap(), rgba).unwrap()
}

fn point(x: f64, y: f64) -> Vec2 {
    Vec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    }
}

#[test]
fn palette_generic_command_preserves_typed_pattern_and_undo() {
    let service = TimelineEditorService::create_default("pattern palette").unwrap();
    let pattern = PatternValue::new(
        PatternKind::Dots,
        managed_color([1.0, 0.5, 0.0, 1.0]),
        managed_color([0.0, 0.0, 0.0, 0.25]),
        point(24.0, 12.0),
        point(3.0, 4.0),
        30.0,
        0.4,
    )
    .unwrap();
    let (definition_id, changes) = service
        .add_paint_definition("Dots".to_string(), Paint::Pattern(pattern.clone()))
        .unwrap();

    assert_eq!(
        changes.invalidations,
        vec![ProjectInvalidation::ProjectPalette]
    );
    assert_eq!(
        service.snapshot().unwrap().palette.definitions[&definition_id].paint,
        Paint::Pattern(pattern)
    );
    service.undo().unwrap().expect("undo pattern add");
    assert!(service.snapshot().unwrap().palette.definitions.is_empty());
}

#[test]
fn palette_commands_are_atomic_ordered_and_undoable() {
    let service = TimelineEditorService::create_default("palette").unwrap();
    let first_color = managed_color([2.0, -0.25, 0.5, 0.75]);
    let second_color = managed_color([0.1, 0.2, 0.3, 1.0]);

    let (first, first_change) = service
        .add_solid_paint_definition(" Accent ".to_string(), first_color.clone())
        .unwrap();
    assert_eq!(first_change.revision.get(), 1);
    assert_eq!(
        first_change.invalidations,
        vec![ProjectInvalidation::ProjectPalette]
    );
    let (second, second_change) = service
        .add_solid_paint_definition("Shadow".to_string(), second_color)
        .unwrap();
    assert_eq!(second_change.revision.get(), 2);

    let rename_change = service
        .rename_paint_definition(first, "Primary".to_string())
        .unwrap();
    assert_eq!(rename_change.revision.get(), 3);
    assert_eq!(
        rename_change.invalidations,
        vec![ProjectInvalidation::PaintDefinition {
            definition_id: first
        }]
    );
    let reorder_change = service.reorder_paint_definition(second, 0).unwrap();
    assert_eq!(reorder_change.revision.get(), 4);

    let snapshot = service.snapshot().unwrap();
    assert_eq!(snapshot.palette.ungrouped_order, vec![second, first]);
    assert_eq!(snapshot.palette.definitions[&first].name, "Primary");
    assert_eq!(snapshot.palette.solid_color(first), Some(first_color));
    assert_eq!(
        snapshot
            .palette
            .ungrouped_definitions()
            .map(|definition| definition.id)
            .collect::<Vec<_>>(),
        vec![second, first]
    );
    drop(snapshot);

    let undo_reorder = service.undo().unwrap().expect("undo reorder");
    assert_eq!(
        undo_reorder.invalidations,
        vec![ProjectInvalidation::ProjectPalette]
    );
    assert_eq!(
        service.snapshot().unwrap().palette.ungrouped_order,
        vec![first, second]
    );
    let redo_reorder = service.redo().unwrap().expect("redo reorder");
    assert_eq!(
        redo_reorder.invalidations,
        vec![ProjectInvalidation::ProjectPalette]
    );
    assert_eq!(
        service.snapshot().unwrap().palette.ungrouped_order,
        vec![second, first]
    );

    let delete_change = service.delete_paint_definition(first).unwrap();
    assert_eq!(delete_change.revision.get(), 7);
    assert!(
        !service
            .snapshot()
            .unwrap()
            .palette
            .definitions
            .contains_key(&first)
    );
    let undo_delete = service.undo().unwrap().expect("undo delete");
    assert_eq!(
        undo_delete.invalidations,
        vec![ProjectInvalidation::ProjectPalette]
    );
    assert_eq!(
        service.snapshot().unwrap().palette.solid_color(first),
        Some(managed_color([2.0, -0.25, 0.5, 0.75]))
    );
}

#[test]
fn palette_no_op_commands_do_not_dirty_or_create_history() {
    let service = TimelineEditorService::create_default("palette no-op").unwrap();
    let (first, _) = service
        .add_solid_paint_definition("First".to_string(), managed_color([1.0, 0.0, 0.0, 1.0]))
        .unwrap();
    let (_second, _) = service
        .add_solid_paint_definition("Second".to_string(), managed_color([0.0, 1.0, 0.0, 1.0]))
        .unwrap();
    let revision = service.revision().unwrap();

    let rename = service
        .rename_paint_definition(first, " First ".to_string())
        .unwrap();
    assert_eq!(rename.revision, revision);
    assert!(rename.invalidations.is_empty());
    let reorder = service.reorder_paint_definition(first, 0).unwrap();
    assert_eq!(reorder.revision, revision);
    assert!(reorder.invalidations.is_empty());
    assert_eq!(service.revision().unwrap(), revision);

    let undo = service.undo().unwrap().expect("undo second add");
    assert_eq!(
        undo.invalidations,
        vec![ProjectInvalidation::ProjectPalette]
    );
    assert_eq!(service.snapshot().unwrap().palette.definitions.len(), 1);
}

#[test]
fn paint_definition_history_keeps_its_local_invalidation() {
    let service = TimelineEditorService::create_default("palette rename history").unwrap();
    let (definition_id, _) = service
        .add_solid_paint_definition("Before".to_string(), managed_color([0.0, 0.0, 1.0, 1.0]))
        .unwrap();
    let expected = vec![ProjectInvalidation::PaintDefinition { definition_id }];
    service
        .rename_paint_definition(definition_id, "After".to_string())
        .unwrap();

    let undo = service.undo().unwrap().expect("undo rename");
    assert_eq!(undo.invalidations, expected);
    assert_eq!(
        service.snapshot().unwrap().palette.definitions[&definition_id].name,
        "Before"
    );
    let redo = service.redo().unwrap().expect("redo rename");
    assert_eq!(redo.invalidations, expected);
    assert_eq!(
        service.snapshot().unwrap().palette.definitions[&definition_id].name,
        "After"
    );
}

#[test]
fn palette_round_trip_preserves_identity_order_and_managed_color() {
    let service = TimelineEditorService::create_default("palette persistence").unwrap();
    let color = managed_color([-1.0, 3.0, 0.125, 0.4]);
    let (definition_id, _) = service
        .add_solid_paint_definition("HDR Accent".to_string(), color.clone())
        .unwrap();

    let json = service.document().unwrap().to_json().unwrap();
    let restored = ProjectDocument::from_json(&json).unwrap();
    assert_eq!(
        restored.project.palette.ungrouped_order,
        vec![definition_id]
    );
    assert_eq!(
        restored.project.palette.solid_color(definition_id),
        Some(color)
    );
    assert!(matches!(
        restored.project.palette.definitions[&definition_id].paint,
        Paint::Solid(_)
    ));

    let mut without_palette: serde_json::Value = serde_json::from_str(&json).unwrap();
    without_palette["project"]
        .as_object_mut()
        .unwrap()
        .remove("palette");
    assert!(
        ProjectDocument::from_json(&serde_json::to_string(&without_palette).unwrap()).is_err(),
        "the pre-v1 format must not add a compatibility default for Palette"
    );

    let mut unknown_paint_field: serde_json::Value = serde_json::from_str(&json).unwrap();
    unknown_paint_field["project"]["palette"]["definitions"]
        .as_object_mut()
        .unwrap()
        .get_mut(&definition_id.to_string())
        .unwrap()["paint"]
        .as_object_mut()
        .unwrap()
        .insert(
            "unsupported_future_field".to_string(),
            serde_json::json!(true),
        );
    assert!(
        ProjectDocument::from_json(&serde_json::to_string(&unknown_paint_field).unwrap()).is_err(),
        "unknown Paint fields must be rejected instead of silently discarded"
    );
}

#[test]
fn palette_commands_reject_invalid_targets_without_a_revision() {
    let service = TimelineEditorService::create_default("palette errors").unwrap();
    let missing = crate::model::authoring::PaintDefinitionId::new();
    let revision = service.revision().unwrap();

    let rename_error = service
        .rename_paint_definition(missing, "Name".to_string())
        .unwrap_err()
        .to_string();
    assert!(rename_error.contains("Missing Paint Definition"));
    assert_eq!(service.revision().unwrap(), revision);

    let empty_name_error = service
        .add_solid_paint_definition("  ".to_string(), managed_color([0.0, 0.0, 0.0, 1.0]))
        .unwrap_err()
        .to_string();
    assert!(empty_name_error.contains("Paint Definition name must not be empty"));
    assert_eq!(service.revision().unwrap(), revision);

    let delete_error = service
        .delete_paint_definition(missing)
        .unwrap_err()
        .to_string();
    assert!(delete_error.contains("Missing Paint Definition"));
    assert_eq!(service.revision().unwrap(), revision);
}
