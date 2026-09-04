use super::{PreviewInteractions, collect_preview_drag_targets, revalidate_preview_drag_target};
use crate::state::context::EditorContext;
use crate::state::context_types::{BodyDragState, SelectionTarget};
use crate::test_support::generator_node;
use crate::ui::panels::preview::clip::{PreviewClip, PreviewSpatialKind, PreviewSpatialLayer};
use library::editor::project_service::GeneratorNodeRequest;
use library::model::frame::transform::{Position, Transform};
use library::plugin::PluginManager;
use library::rendering::renderer::Affine2D;
use std::collections::HashMap;
use uuid::Uuid;

fn visual(
    owner: SelectionTarget,
    content_id: Uuid,
    spatial_id: Uuid,
    branch_id: Uuid,
    parent_transform: Affine2D,
) -> PreviewClip {
    let mut content = generator_node(
        "content",
        GeneratorNodeRequest::SkSL {
            shader: "half4 main(float2 p) { return half4(1); }".to_string(),
        },
    );
    content.id = content_id;
    let mut spatial = PluginManager::default()
        .create_shape_transform_operation_node()
        .expect("native Transform operation");
    spatial.id = spatial_id;
    PreviewClip {
        content_node: content,
        spatial_layers: vec![PreviewSpatialLayer {
            node: spatial,
            kind: PreviewSpatialKind::ShapeTransform,
            transform: Transform {
                position: Position { x: 12.0, y: 34.0 },
                ..Transform::default()
            },
            parent_transform,
        }],
        owner_target: owner,
        transform: Transform::default(),
        world_transform: Affine2D::IDENTITY,
        content_bounds: Some((0.0, 0.0, 100.0, 50.0)),
        instance_path: vec![branch_id, content_id, spatial_id],
    }
}

fn singular_affine() -> Affine2D {
    Affine2D {
        scale_x: 0.0,
        skew_x: 0.0,
        translate_x: 0.0,
        skew_y: 0.0,
        scale_y: 1.0,
        translate_y: 0.0,
    }
}

#[test]
fn multi_drag_keeps_exact_primary_and_only_canonical_secondaries() {
    let primary_owner = SelectionTarget::Clip(Uuid::new_v4());
    let canonical_owner = SelectionTarget::Clip(Uuid::new_v4());
    let ambiguous_owner = SelectionTarget::Clip(Uuid::new_v4());
    let primary = visual(
        primary_owner,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Affine2D::IDENTITY,
    );
    let primary_other_branch = visual(
        primary_owner,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Affine2D::IDENTITY,
    );
    let canonical = visual(
        canonical_owner,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Affine2D::IDENTITY,
    );
    let ambiguous_first = visual(
        ambiguous_owner,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Affine2D::IDENTITY,
    );
    let ambiguous_second = visual(
        ambiguous_owner,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Affine2D::IDENTITY,
    );
    let explicit_primary = primary.edit_target();
    let visuals = vec![
        primary,
        primary_other_branch,
        canonical,
        ambiguous_first,
        ambiguous_second,
    ];

    let targets = collect_preview_drag_targets(
        &visuals,
        &[ambiguous_owner, canonical_owner, primary_owner],
        Some(primary_owner),
        Some(&explicit_primary),
    );

    assert_eq!(targets.len(), 2);
    let primary_target = targets
        .iter()
        .find(|target| target.edit_target.owner == primary_owner)
        .expect("primary exact hit");
    assert_eq!(primary_target.edit_target, explicit_primary);
    assert!(!primary_target.requires_canonical_owner);
    let canonical_target = targets
        .iter()
        .find(|target| target.edit_target.owner == canonical_owner)
        .expect("canonical secondary");
    assert!(canonical_target.requires_canonical_owner);
    assert!(
        targets
            .iter()
            .all(|target| target.edit_target.owner != ambiguous_owner)
    );
}

#[test]
fn exact_drag_route_rejects_stale_path_reparent_and_new_ambiguity() {
    let owner = SelectionTarget::Clip(Uuid::new_v4());
    let content_id = Uuid::new_v4();
    let spatial_id = Uuid::new_v4();
    let branch_id = Uuid::new_v4();
    let original = visual(owner, content_id, spatial_id, branch_id, Affine2D::IDENTITY);
    let mut targets =
        collect_preview_drag_targets(std::slice::from_ref(&original), &[owner], None, None);
    let target = targets.pop().expect("canonical target");
    assert!(revalidate_preview_drag_target(std::slice::from_ref(&original), &target).is_some());

    let stale_branch = visual(
        owner,
        content_id,
        spatial_id,
        Uuid::new_v4(),
        Affine2D::IDENTITY,
    );
    assert!(revalidate_preview_drag_target(&[stale_branch], &target).is_none());

    let reparented = visual(
        SelectionTarget::Track(Uuid::new_v4()),
        content_id,
        spatial_id,
        branch_id,
        Affine2D::IDENTITY,
    );
    assert!(revalidate_preview_drag_target(&[reparented], &target).is_none());

    let independent = visual(
        owner,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Affine2D::IDENTITY,
    );
    assert!(revalidate_preview_drag_target(&[original, independent], &target).is_none());
    assert!(revalidate_preview_drag_target(&[], &target).is_none());
}

#[test]
fn singular_parent_is_pruned_without_update_or_changed_history_state() {
    let owner = SelectionTarget::Clip(Uuid::new_v4());
    let content_id = Uuid::new_v4();
    let spatial_id = Uuid::new_v4();
    let branch_id = Uuid::new_v4();
    let initial = visual(owner, content_id, spatial_id, branch_id, Affine2D::IDENTITY);
    let explicit = initial.edit_target();
    let targets = collect_preview_drag_targets(
        std::slice::from_ref(&initial),
        &[owner],
        Some(owner),
        Some(&explicit),
    );
    assert_eq!(targets.len(), 1);
    let singular = visual(owner, content_id, spatial_id, branch_id, singular_affine());

    let mut editor_context = EditorContext::new(Uuid::new_v4());
    editor_context.replace_selection([owner], Some(owner));
    editor_context.interaction.preview_edit_target = Some(explicit);
    editor_context.interaction.is_moving_selected_entity = true;
    editor_context.interaction.body_drag_state = Some(BodyDragState {
        start_mouse_pos: egui::Pos2::ZERO,
        original_positions: HashMap::new(),
        preview_targets: targets,
        has_changed: false,
    });

    let context = egui::Context::default();
    let mut pending_actions = Vec::new();
    drop(context.run(egui::RawInput::default(), |context| {
        egui::CentralPanel::default().show(context, |ui| {
            let mut interactions = PreviewInteractions::new(
                ui,
                &mut editor_context,
                std::slice::from_ref(&singular),
                |position| position,
                |position| position,
            );
            interactions.handle_drag_move(Some(egui::pos2(20.0, 0.0)), &mut pending_actions);
        });
    }));

    assert!(pending_actions.is_empty());
    let state = editor_context
        .interaction
        .body_drag_state
        .as_ref()
        .expect("gesture state remains until release");
    assert!(state.preview_targets.is_empty());
    assert!(!state.has_changed);
}

#[test]
fn singular_primary_is_not_armed_for_body_drag() {
    let owner = SelectionTarget::Clip(Uuid::new_v4());
    let visual = visual(
        owner,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        singular_affine(),
    );
    assert!(
        collect_preview_drag_targets(
            std::slice::from_ref(&visual),
            &[owner],
            Some(owner),
            Some(&visual.edit_target()),
        )
        .is_empty()
    );
}
