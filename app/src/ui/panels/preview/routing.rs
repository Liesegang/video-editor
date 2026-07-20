use crate::state::context_types::{PreviewEditTarget, SelectionTarget};
use library::model::project::Project;
use uuid::Uuid;

use super::clip::{
    resolve_owner_edit_target, visual_for_exact_instance, OwnerEditTargetResolution, PreviewClip,
};

/// Reconcile the primary selection with one exact rendered branch.
///
/// Timeline/Inspector owners use the canonical facade resolver. A Node
/// selection stays a Node selection and can expose a Preview gizmo only when
/// that exact spatial Node reaches one rendered branch. Fan-out is ambiguous
/// without an explicit Preview hit and therefore fails closed.
pub(super) fn resolve_primary_edit_target(
    visuals: &[PreviewClip],
    primary: SelectionTarget,
) -> OwnerEditTargetResolution {
    match primary {
        SelectionTarget::Node(node_id) => resolve_node_edit_target(visuals, node_id),
        SelectionTarget::Clip(_) | SelectionTarget::Track(_) | SelectionTarget::Composition(_) => {
            resolve_owner_edit_target(visuals, primary)
        }
    }
}

fn resolve_node_edit_target(visuals: &[PreviewClip], node_id: Uuid) -> OwnerEditTargetResolution {
    let mut matches = visuals
        .iter()
        .filter(|visual| visual.spatial_layer(node_id).is_some());
    let Some(visual) = matches.next() else {
        return OwnerEditTargetResolution::Unavailable;
    };
    if matches.next().is_some() {
        return OwnerEditTargetResolution::Ambiguous {
            candidate_node_ids: vec![node_id],
        };
    }

    OwnerEditTargetResolution::Resolved(PreviewEditTarget {
        owner: SelectionTarget::Node(node_id),
        content_node_id: visual.content_id(),
        spatial_node_id: Some(node_id),
        instance_path: visual.instance_path.clone(),
    })
}

pub(super) fn exact_visual_for_edit_target<'a>(
    visuals: &'a [PreviewClip],
    target: &PreviewEditTarget,
) -> Option<&'a PreviewClip> {
    let lookup_id = target.spatial_node_id.unwrap_or(target.content_node_id);
    let visual = visual_for_exact_instance(visuals, lookup_id, &target.instance_path)?;
    edit_target_matches_visual(target, visual).then_some(visual)
}

pub(super) fn edit_target_matches_visual(target: &PreviewEditTarget, visual: &PreviewClip) -> bool {
    if target.content_node_id != visual.content_id()
        || target
            .spatial_node_id
            .is_some_and(|node_id| visual.spatial_layer(node_id).is_none())
    {
        return false;
    }

    match target.owner {
        SelectionTarget::Node(node_id) => {
            target.spatial_node_id == Some(node_id) && visual.spatial_layer(node_id).is_some()
        }
        SelectionTarget::Clip(_) | SelectionTarget::Track(_) | SelectionTarget::Composition(_) => {
            target.owner == visual.owner_target
        }
    }
}

/// Validate a queued Preview write against the current evaluated branch.
///
/// This is intentionally stricter than a UUID existence check. Reparenting,
/// deletion followed by UUID reuse, a stale instance path, or an incomplete
/// Transform contract all make the write a no-op.
pub(super) fn can_update_property(
    project: &Project,
    visuals: &[PreviewClip],
    target: &PreviewEditTarget,
    node_id: Uuid,
    property_name: &str,
) -> bool {
    let Some(visual) = exact_visual_for_edit_target(visuals, target) else {
        return false;
    };
    let projected_node = if node_id == visual.content_id() {
        Some(&visual.content_node)
    } else {
        visual
            .spatial_layers
            .iter()
            .find(|layer| layer.node.id == node_id)
            .map(|layer| &layer.node)
    };
    let Some(node) = projected_node.filter(|node| {
        project.get_node(node_id) == Some(*node) && node.properties().get(property_name).is_some()
    }) else {
        return false;
    };
    if super::clip::visual_owner_target(project, visual.editable_spatial_id(), visual.content_id())
        != Some(visual.owner_target)
    {
        return false;
    }

    if !matches!(property_name, "position" | "rotation" | "scale" | "anchor") {
        return true;
    }
    target.spatial_node_id == Some(node.id)
        && visual.spatial_layer(node.id).is_some()
        && ["position", "rotation", "scale", "anchor"]
            .into_iter()
            .all(|key| node.properties().get(key).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::generator_node;
    use library::editor::project_service::GeneratorNodeRequest;
    use library::model::frame::transform::Transform;
    use library::plugin::PluginManager;
    use library::rendering::renderer::Affine2D;

    fn visual(
        owner: SelectionTarget,
        content_id: Uuid,
        spatial_id: Uuid,
        branch_id: Uuid,
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
            spatial_layers: vec![super::super::clip::PreviewSpatialLayer {
                node: spatial,
                kind: super::super::clip::PreviewSpatialKind::ShapeTransform,
                transform: Transform::default(),
                parent_transform: Affine2D::IDENTITY,
            }],
            owner_target: owner,
            transform: Transform::default(),
            world_transform: Affine2D::IDENTITY,
            content_bounds: Some((0.0, 0.0, 100.0, 50.0)),
            instance_path: vec![branch_id, content_id, spatial_id],
        }
    }

    #[test]
    fn unique_transform_node_selection_keeps_node_as_primary_route() {
        let clip_id = Uuid::new_v4();
        let content_id = Uuid::new_v4();
        let spatial_id = Uuid::new_v4();
        let visuals = vec![visual(
            SelectionTarget::Clip(clip_id),
            content_id,
            spatial_id,
            Uuid::new_v4(),
        )];

        let OwnerEditTargetResolution::Resolved(target) =
            resolve_primary_edit_target(&visuals, SelectionTarget::Node(spatial_id))
        else {
            panic!("one rendered Transform branch must resolve");
        };
        assert_eq!(target.owner, SelectionTarget::Node(spatial_id));
        assert_eq!(target.spatial_node_id, Some(spatial_id));
        assert!(exact_visual_for_edit_target(&visuals, &target).is_some());
    }

    #[test]
    fn transform_node_fan_out_is_ambiguous_without_a_preview_hit() {
        let clip_id = Uuid::new_v4();
        let spatial_id = Uuid::new_v4();
        let visuals = vec![
            visual(
                SelectionTarget::Clip(clip_id),
                Uuid::new_v4(),
                spatial_id,
                Uuid::new_v4(),
            ),
            visual(
                SelectionTarget::Clip(clip_id),
                Uuid::new_v4(),
                spatial_id,
                Uuid::new_v4(),
            ),
        ];

        assert_eq!(
            resolve_primary_edit_target(&visuals, SelectionTarget::Node(spatial_id)),
            OwnerEditTargetResolution::Ambiguous {
                candidate_node_ids: vec![spatial_id]
            }
        );
    }

    #[test]
    fn exact_target_rejects_stale_path_and_wrong_facade() {
        let clip_id = Uuid::new_v4();
        let content_id = Uuid::new_v4();
        let spatial_id = Uuid::new_v4();
        let visuals = vec![visual(
            SelectionTarget::Clip(clip_id),
            content_id,
            spatial_id,
            Uuid::new_v4(),
        )];
        let mut target = visuals[0].edit_target();
        assert!(exact_visual_for_edit_target(&visuals, &target).is_some());

        target.instance_path[0] = Uuid::new_v4();
        assert!(exact_visual_for_edit_target(&visuals, &target).is_none());
        target.instance_path = visuals[0].instance_path.clone();
        target.owner = SelectionTarget::Clip(Uuid::new_v4());
        assert!(exact_visual_for_edit_target(&visuals, &target).is_none());
    }

    #[test]
    fn queued_write_requires_exact_branch_property_and_transform_contract() {
        let clip_id = Uuid::new_v4();
        let content_id = Uuid::new_v4();
        let spatial_id = Uuid::new_v4();
        let mut visuals = vec![visual(
            SelectionTarget::Clip(clip_id),
            content_id,
            spatial_id,
            Uuid::new_v4(),
        )];
        let target = visuals[0].edit_target();
        let mut project = Project::new("current Preview write");
        project.add_node(visuals[0].content_node.clone());
        project.add_node(visuals[0].spatial_layers[0].node.clone());
        let mut owner = library::model::Clip::new("owner", 0.0, 1.0);
        owner.id = clip_id;
        owner.node_ids = vec![content_id, spatial_id];
        project.add_clip(owner);

        assert!(can_update_property(
            &project, &visuals, &target, spatial_id, "position"
        ));
        assert!(!can_update_property(
            &project, &visuals, &target, content_id, "position"
        ));
        assert!(!can_update_property(
            &project, &visuals, &target, spatial_id, "missing"
        ));

        let original_spatial = project
            .get_node(spatial_id)
            .expect("current spatial Node")
            .clone();
        let mut reused_uuid = original_spatial.clone();
        reused_uuid.name = "different Node reusing UUID".to_string();
        project.nodes.insert(spatial_id, reused_uuid);
        assert!(!can_update_property(
            &project, &visuals, &target, spatial_id, "position"
        ));
        project.nodes.insert(spatial_id, original_spatial);

        let mut other_owner = library::model::Clip::new("other owner", 0.0, 1.0);
        let other_owner_id = other_owner.id;
        other_owner.node_ids.push(spatial_id);
        project.add_clip(other_owner);
        assert!(!can_update_property(
            &project, &visuals, &target, spatial_id, "position"
        ));
        project.clips.remove(&other_owner_id);

        let layer = visuals[0]
            .spatial_layers
            .first_mut()
            .expect("spatial layer");
        let mut persisted = serde_json::to_value(&layer.node).expect("serialize Transform");
        persisted
            .get_mut("properties")
            .and_then(serde_json::Value::as_object_mut)
            .expect("persisted property map")
            .remove("anchor");
        layer.node = serde_json::from_value(persisted).expect("load incomplete pre-v1 Node");
        project.nodes.insert(spatial_id, layer.node.clone());
        assert!(!can_update_property(
            &project, &visuals, &target, spatial_id, "position"
        ));
    }
}
