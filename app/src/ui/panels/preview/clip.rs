use crate::state::context_types::{PreviewEditTarget, SelectionTarget};
use library::model::frame::entity::{FrameGroupKind, FrameItem};
use library::model::frame::frame::FrameInfo;
use library::model::frame::transform::Transform;
use library::model::project::{NodeContainer, Project};
use library::model::Node;
use library::rendering::renderer::Affine2D;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewSpatialKind {
    Content,
    ShapeTransform,
    ImageTransform,
}

#[derive(Clone)]
pub struct PreviewSpatialLayer {
    pub node: Node,
    pub kind: PreviewSpatialKind,
    /// Direct transform owned by this Node, excluding every outer layer.
    pub transform: Transform,
    /// Affine stack outside this layer. Pointer deltas are mapped through its
    /// inverse before writing this Node's direct Project properties.
    pub parent_transform: Affine2D,
}

impl PreviewSpatialLayer {
    fn is_editable(&self) -> bool {
        ["position", "rotation", "scale", "anchor"]
            .into_iter()
            .all(|key| self.node.properties().get(key).is_some())
    }
}

/// One interactive visual that actually reached the rendered Composition.
///
/// This is an ephemeral UI projection of `FrameInfo`, not a second project
/// model. Its identity and ordering come from authoritative frame evaluation;
/// wrapper Nodes such as Style, Effect, and Merge never replace the authored
/// geometry Node or its optional spatial Transform owner.
pub struct PreviewClip {
    /// Generator that owns the rendered content (text, path, media, shader).
    pub content_node: Node,
    /// Ordered outer-to-inner spatial provenance. Image Transform groups are
    /// explicit layers; the innermost entry is the Shape/content placement.
    pub spatial_layers: Vec<PreviewSpatialLayer>,
    /// Nearest authoritative Timeline/Inspector owner for a Preview hit.
    pub owner_target: SelectionTarget,
    /// Final evaluated visual transform (normalized scale/opacity). This may
    /// include downstream Shape Effector contributions and is used for hit
    /// testing/drawing only.
    pub transform: Transform,
    /// Every evaluated group/layer transform composed with the content.
    pub world_transform: Affine2D,
    pub content_bounds: Option<(f32, f32, f32, f32)>,
    /// Stable render-branch identity. Project selection remains the source
    /// Node ID, while this path distinguishes fan-out of that Node through
    /// multiple Merge/Reference branches.
    pub instance_path: Vec<Uuid>,
}

impl PreviewClip {
    pub fn content_id(&self) -> Uuid {
        self.content_node.id
    }

    pub fn spatial_id(&self) -> Option<Uuid> {
        self.editable_spatial_id()
    }

    /// A stale/malformed spatial owner is never made draggable. Every Preview
    /// spatial gesture writes this complete native property contract.
    pub fn editable_spatial_id(&self) -> Option<Uuid> {
        self.spatial_layers
            .iter()
            .find(|layer| layer.is_editable())
            .map(|layer| layer.node.id)
    }

    pub fn matches_node_id(&self, node_id: Uuid) -> bool {
        self.content_id() == node_id
            || self
                .spatial_layers
                .iter()
                .any(|layer| layer.node.id == node_id)
    }

    pub fn spatial_layer(&self, node_id: Uuid) -> Option<&PreviewSpatialLayer> {
        self.spatial_layers
            .iter()
            .find(|layer| layer.node.id == node_id && layer.is_editable())
    }

    pub fn edit_target(&self) -> PreviewEditTarget {
        PreviewEditTarget {
            owner: self.owner_target,
            content_node_id: self.content_id(),
            spatial_node_id: self.editable_spatial_id(),
            instance_path: self.instance_path.clone(),
        }
    }
}

pub fn from_evaluated_frame(project: &Project, frame: &FrameInfo) -> Vec<PreviewClip> {
    let mut visuals = Vec::new();
    let mut path = Vec::new();
    let mut image_layers = Vec::new();
    for item in &frame.items {
        collect_visuals(
            project,
            item,
            Affine2D::IDENTITY,
            &mut path,
            &mut image_layers,
            &mut visuals,
        );
    }
    visuals
}

/// Resolve one rendered instance for either its content or spatial Node.
///
/// An explicit branch path wins. If selection came from another panel (and
/// therefore has no Preview branch), renderer order makes the last matching
/// instance the top-most deterministic fallback.
pub fn visual_for_selection<'a>(
    visuals: &'a [PreviewClip],
    node_id: Uuid,
    instance_path: Option<&[Uuid]>,
) -> Option<&'a PreviewClip> {
    instance_path
        .and_then(|path| {
            visuals
                .iter()
                .find(|visual| visual.matches_node_id(node_id) && visual.instance_path == path)
        })
        .or_else(|| {
            visuals
                .iter()
                .rev()
                .find(|visual| visual.matches_node_id(node_id))
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnerEditTargetResolution {
    Resolved(PreviewEditTarget),
    Ambiguous { candidate_node_ids: Vec<Uuid> },
    Unavailable,
}

/// Resolve the canonical facade edit behind a Timeline/Inspector owner.
///
/// A common outer Image Transform wins, followed by a common innermost
/// Shape/content transform. Multiple independent candidates are deliberately
/// ambiguous: Timeline-only editing must never mutate an arbitrary front-most
/// Node. A direct Preview hit may still choose one exact branch explicitly.
pub fn resolve_owner_edit_target(
    visuals: &[PreviewClip],
    owner: SelectionTarget,
) -> OwnerEditTargetResolution {
    let owned = visuals
        .iter()
        .filter(|visual| visual.owner_target == owner)
        .collect::<Vec<_>>();
    if owned.is_empty() {
        return OwnerEditTargetResolution::Unavailable;
    }

    for kind in [
        PreviewSpatialKind::ImageTransform,
        PreviewSpatialKind::ShapeTransform,
        PreviewSpatialKind::Content,
    ] {
        let candidates = owned
            .iter()
            .filter_map(|visual| {
                visual
                    .spatial_layers
                    .iter()
                    .find(|layer| layer.kind == kind && layer.is_editable())
                    .map(|layer| layer.node.id)
            })
            .collect::<Vec<_>>();
        let Some(first) = candidates.first().copied() else {
            continue;
        };
        if candidates.len() == owned.len() && candidates.iter().all(|candidate| *candidate == first)
        {
            let Some(visual) = owned
                .iter()
                .rev()
                .find(|visual| visual.spatial_layer(first).is_some())
            else {
                return OwnerEditTargetResolution::Unavailable;
            };
            return OwnerEditTargetResolution::Resolved(PreviewEditTarget {
                owner,
                content_node_id: visual.content_id(),
                spatial_node_id: Some(first),
                instance_path: visual.instance_path.clone(),
            });
        }
    }

    let mut candidate_node_ids = owned
        .iter()
        .filter_map(|visual| visual.editable_spatial_id())
        .collect::<Vec<_>>();
    candidate_node_ids.sort_unstable();
    candidate_node_ids.dedup();
    if candidate_node_ids.is_empty() {
        OwnerEditTargetResolution::Unavailable
    } else if owned.len() == 1 {
        OwnerEditTargetResolution::Resolved(owned[0].edit_target())
    } else {
        OwnerEditTargetResolution::Ambiguous { candidate_node_ids }
    }
}

fn collect_visuals(
    project: &Project,
    item: &FrameItem,
    parent_transform: Affine2D,
    path: &mut Vec<Uuid>,
    image_layers: &mut Vec<PreviewSpatialLayer>,
    visuals: &mut Vec<PreviewClip>,
) {
    match item {
        FrameItem::Object(object) => {
            let Some(content_node) = project.get_node(object.source_node_id) else {
                // A render result can arrive immediately after a Project
                // replacement. Never make stale frame identity interactive.
                return;
            };
            let shape_spatial_node = object
                .spatial_transform_node_id
                .and_then(|node_id| project.get_node(node_id))
                .cloned();
            let transform = object.content.transform().clone();
            path.push(object.source_node_id);
            let distinct_spatial_id = object
                .spatial_transform_node_id
                .filter(|node_id| *node_id != object.source_node_id);
            if let Some(node_id) = distinct_spatial_id {
                path.push(node_id);
            }
            let mut spatial_layers = image_layers.clone();
            if let Some(node) = shape_spatial_node {
                spatial_layers.push(PreviewSpatialLayer {
                    kind: if node.id == object.source_node_id {
                        PreviewSpatialKind::Content
                    } else {
                        PreviewSpatialKind::ShapeTransform
                    },
                    node,
                    transform: object.spatial_transform.as_ref().clone(),
                    parent_transform,
                });
            }
            let editable_layer = spatial_layers.iter().find(|layer| layer.is_editable());
            let owner_node_id = editable_layer
                .map(|layer| layer.node.id)
                .unwrap_or(object.source_node_id);
            visuals.push(PreviewClip {
                content_node: content_node.clone(),
                spatial_layers,
                owner_target: selection_owner_for_node(project, owner_node_id)
                    .unwrap_or(SelectionTarget::Node(owner_node_id)),
                world_transform: parent_transform.compose(Affine2D::from(&transform)),
                transform,
                content_bounds: object.content_bounds.map(|bounds| bounds.as_tuple()),
                instance_path: path.clone(),
            });
            if distinct_spatial_id.is_some() {
                path.pop();
            }
            path.pop();
        }
        FrameItem::Group(group) => {
            path.push(group.source_id);
            let mut pushed_image_layer = false;
            if group.kind == FrameGroupKind::ImageTransform {
                if let Some(node) = project.get_node(group.source_id).cloned() {
                    image_layers.push(PreviewSpatialLayer {
                        node,
                        kind: PreviewSpatialKind::ImageTransform,
                        transform: group.transform.clone(),
                        parent_transform,
                    });
                    pushed_image_layer = true;
                }
            }
            let transform = parent_transform.compose(Affine2D::from(&group.transform));
            for child in &group.items {
                collect_visuals(project, child, transform, path, image_layers, visuals);
            }
            if pushed_image_layer {
                image_layers.pop();
            }
            path.pop();
        }
    }
}

fn selection_owner_for_node(project: &Project, node_id: Uuid) -> Option<SelectionTarget> {
    match project.find_node_container(node_id)? {
        NodeContainer::Clip(id) => Some(SelectionTarget::Clip(id)),
        NodeContainer::Track(id) => Some(SelectionTarget::Track(id)),
        NodeContainer::Composition(id) => Some(SelectionTarget::Composition(id)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        from_evaluated_frame, resolve_owner_edit_target, visual_for_selection,
        OwnerEditTargetResolution, PreviewSpatialKind,
    };
    use crate::state::context_types::SelectionTarget;
    use crate::test_support::generator_node;
    use library::editor::project_service::GeneratorNodeRequest;
    use library::model::frame::color::Color;
    use library::model::frame::entity::{
        FrameBounds, FrameContent, FrameGroup, FrameGroupKind, FrameItem, FrameObject,
    };
    use library::model::frame::frame::FrameInfo;
    use library::model::frame::transform::{Position, Transform};
    use library::model::project::{Composition, NodeContainer};
    use library::model::BlendMode;
    use library::model::Clip;
    use ordered_float::OrderedFloat;
    use uuid::Uuid;

    fn frame_object(node_id: Uuid) -> FrameObject {
        FrameObject {
            source_node_id: node_id,
            spatial_transform_node_id: Some(node_id),
            spatial_transform: Box::new(Transform {
                position: Position { x: 5.0, y: 7.0 },
                ..Transform::default()
            }),
            content_bounds: Some(FrameBounds::new(1.0, 2.0, 30.0, 40.0)),
            content: FrameContent::SkSL {
                shader: "half4 main(float2 p) { return half4(1); }".to_string(),
                resolution: (30.0, 40.0),
                effects: Vec::new(),
                transform: Transform {
                    position: Position { x: 5.0, y: 7.0 },
                    ..Transform::default()
                },
            },
        }
    }

    fn object(node_id: Uuid) -> FrameItem {
        FrameItem::Object(frame_object(node_id))
    }

    fn frame_group(source_id: Uuid, kind: FrameGroupKind, items: Vec<FrameItem>) -> FrameGroup {
        FrameGroup {
            source_id,
            kind,
            width: 1920,
            height: 1080,
            background_color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
            transform: Transform::default(),
            blend_mode: BlendMode::Normal,
            effect_time: OrderedFloat(0.0),
            effects: Vec::new(),
            items,
        }
    }

    fn group(source_id: Uuid, kind: FrameGroupKind, items: Vec<FrameItem>) -> FrameItem {
        FrameItem::Group(frame_group(source_id, kind, items))
    }

    #[test]
    fn frame_object_identity_survives_style_effect_and_merge_wrappers_in_render_order() {
        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();
        let mut project = library::model::project::Project::new("preview");
        let mut first = generator_node(
            "first",
            GeneratorNodeRequest::SkSL {
                shader: "half4 main(float2 p) { return half4(1); }".to_string(),
            },
        );
        first.id = first_id;
        project.add_node(first);
        let mut second = generator_node(
            "second",
            GeneratorNodeRequest::SkSL {
                shader: "half4 main(float2 p) { return half4(1); }".to_string(),
            },
        );
        second.id = second_id;
        project.add_node(second);
        let frame = FrameInfo {
            width: 1920,
            height: 1080,
            background_color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items: vec![group(
                Uuid::new_v4(),
                FrameGroupKind::Merge,
                vec![
                    group(
                        Uuid::new_v4(),
                        FrameGroupKind::Effect,
                        vec![group(
                            Uuid::new_v4(),
                            FrameGroupKind::Node,
                            vec![object(first_id)],
                        )],
                    ),
                    object(second_id),
                ],
            )],
        };

        let visuals = from_evaluated_frame(&project, &frame);
        assert_eq!(
            visuals
                .iter()
                .map(|visual| visual.content_id())
                .collect::<Vec<_>>(),
            vec![first_id, second_id]
        );
    }

    #[test]
    fn shape_visual_keeps_content_and_spatial_transform_identity_separate() {
        let content_id = Uuid::new_v4();
        let spatial_id = Uuid::new_v4();
        let mut project = library::model::project::Project::new("dual identity");
        let mut content = generator_node(
            "text content",
            GeneratorNodeRequest::Text {
                text: "Dual".to_string(),
                font: "Arial".to_string(),
            },
        );
        content.id = content_id;
        let mut spatial = library::plugin::PluginManager::default()
            .create_shape_transform_operation_node()
            .unwrap();
        spatial.id = spatial_id;
        project.add_node(content);
        project.add_node(spatial);
        let frame = FrameInfo {
            width: 1920,
            height: 1080,
            background_color: Color::black(),
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items: vec![FrameItem::Object(FrameObject {
                source_node_id: content_id,
                spatial_transform_node_id: Some(spatial_id),
                spatial_transform: Box::new(Transform {
                    position: Position { x: 12.0, y: 18.0 },
                    ..Transform::default()
                }),
                content_bounds: Some(FrameBounds::new(0.0, 0.0, 60.0, 30.0)),
                content: FrameContent::Text {
                    text: "Dual".to_string(),
                    font: "Arial".to_string(),
                    size: 24.0,
                    styles: Vec::new(),
                    effects: Vec::new(),
                    ensemble: None,
                    transform: Transform {
                        position: Position { x: 12.0, y: 18.0 },
                        ..Transform::default()
                    },
                },
            })],
        };

        let visuals = from_evaluated_frame(&project, &frame);
        let [visual] = visuals.as_slice() else {
            panic!("dual-identity frame must project one visual")
        };
        assert_eq!(visual.content_id(), content_id);
        assert_eq!(visual.spatial_id(), Some(spatial_id));
        assert_eq!(visual.editable_spatial_id(), Some(spatial_id));
        assert!(visual.matches_node_id(content_id));
        assert!(visual.matches_node_id(spatial_id));
        assert!(visual.instance_path.ends_with(&[content_id, spatial_id]));
        assert_eq!(
            visual_for_selection(&visuals, content_id, None)
                .unwrap()
                .instance_path,
            visual.instance_path
        );
        assert_eq!(
            visual_for_selection(&visuals, spatial_id, None)
                .unwrap()
                .instance_path,
            visual.instance_path
        );
        assert!(visual.content_node.properties().get("position").is_none());
    }

    #[test]
    fn untransformed_shape_visual_has_no_spatial_edit_owner() {
        let content_id = Uuid::new_v4();
        let mut project = library::model::project::Project::new("untransformed Shape");
        let mut content = generator_node(
            "shape content",
            GeneratorNodeRequest::Shape {
                path: "M0 0 H10 V10 H0 Z".to_string(),
            },
        );
        content.id = content_id;
        project.add_node(content);
        let frame = FrameInfo {
            width: 100,
            height: 100,
            background_color: Color::black(),
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items: vec![FrameItem::Object(FrameObject {
                source_node_id: content_id,
                spatial_transform_node_id: None,
                spatial_transform: Box::new(Transform::default()),
                content_bounds: Some(FrameBounds::new(0.0, 0.0, 10.0, 10.0)),
                content: FrameContent::Shape {
                    path: "M0 0 H10 V10 H0 Z".to_string(),
                    styles: Vec::new(),
                    path_effects: Vec::new(),
                    effects: Vec::new(),
                    ensemble: None,
                    transform: Transform::default(),
                },
            })],
        };

        let visuals = from_evaluated_frame(&project, &frame);
        assert_eq!(visuals[0].content_id(), content_id);
        assert_eq!(visuals[0].spatial_id(), None);
        assert_eq!(visuals[0].editable_spatial_id(), None);
    }

    #[test]
    fn merge_fan_out_keeps_node_selection_and_topmost_branch_identity_separate() {
        let source_id = Uuid::new_v4();
        let bottom_connection_id = Uuid::new_v4();
        let top_connection_id = Uuid::new_v4();
        let mut project = library::model::project::Project::new("preview fan-out");
        let mut source = generator_node(
            "source",
            GeneratorNodeRequest::SkSL {
                shader: "half4 main(float2 p) { return half4(1); }".to_string(),
            },
        );
        source.id = source_id;
        project.add_node(source);
        let mut top_group = frame_group(
            top_connection_id,
            FrameGroupKind::ConnectedImage,
            vec![object(source_id)],
        );
        top_group.transform.position.x = 100.0;
        let top_branch = FrameItem::Group(top_group);
        let frame = FrameInfo {
            width: 1920,
            height: 1080,
            background_color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items: vec![group(
                Uuid::new_v4(),
                FrameGroupKind::Merge,
                vec![
                    group(
                        bottom_connection_id,
                        FrameGroupKind::ConnectedImage,
                        vec![object(source_id)],
                    ),
                    top_branch,
                ],
            )],
        };

        let visuals = from_evaluated_frame(&project, &frame);
        assert_eq!(visuals.len(), 2);
        assert_eq!(visuals[0].content_id(), source_id);
        assert_eq!(visuals[1].content_id(), source_id);
        assert_ne!(visuals[0].instance_path, visuals[1].instance_path);
        assert!(visuals[0].instance_path.contains(&bottom_connection_id));
        assert!(visuals[1].instance_path.contains(&top_connection_id));

        let topmost = visual_for_selection(&visuals, source_id, None).unwrap();
        assert_eq!(topmost.instance_path, visuals[1].instance_path);
        assert_eq!(topmost.world_transform.translate_x, 105.0);
        let explicit_bottom = visual_for_selection(
            &visuals,
            source_id,
            Some(visuals[0].instance_path.as_slice()),
        )
        .unwrap();
        assert_eq!(explicit_bottom.instance_path, visuals[0].instance_path);
        assert_eq!(explicit_bottom.world_transform.translate_x, 5.0);
    }

    #[test]
    fn direct_composition_and_track_nodes_project_as_visuals() {
        let composition_node_id = Uuid::new_v4();
        let track_node_id = Uuid::new_v4();
        let mut project = library::model::project::Project::new("direct nodes");
        let (composition, track) = Composition::new("main", 1920, 1080, 30.0, 1.0);
        let composition_id = composition.id;
        let track_id = track.id;
        project.add_track(track);
        project.add_composition(composition);
        for (id, name) in [
            (composition_node_id, "composition node"),
            (track_node_id, "track node"),
        ] {
            let mut node = generator_node(
                name,
                GeneratorNodeRequest::SkSL {
                    shader: "half4 main(float2 p) { return half4(1); }".to_string(),
                },
            );
            node.id = id;
            project.add_node(node);
        }
        project
            .attach_node_to_container(
                NodeContainer::Composition(composition_id),
                composition_node_id,
            )
            .unwrap();
        project
            .attach_node_to_container(NodeContainer::Track(track_id), track_node_id)
            .unwrap();
        let frame = FrameInfo {
            width: 1920,
            height: 1080,
            background_color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items: vec![
                object(composition_node_id),
                group(track_id, FrameGroupKind::Track, vec![object(track_node_id)]),
            ],
        };

        let visuals = from_evaluated_frame(&project, &frame);
        assert_eq!(visuals.len(), 2);
        assert_eq!(visuals[0].content_id(), composition_node_id);
        assert_eq!(visuals[1].content_id(), track_node_id);
        assert_eq!(
            visuals[0].owner_target,
            SelectionTarget::Composition(composition_id)
        );
        assert_eq!(visuals[1].owner_target, SelectionTarget::Track(track_id));

        let empty = FrameInfo {
            items: Vec::new(),
            ..frame
        };
        assert!(from_evaluated_frame(&project, &empty).is_empty());
    }

    #[test]
    fn downstream_effector_transform_does_not_replace_source_edit_baseline() {
        let source_id = Uuid::new_v4();
        let spatial_id = Uuid::new_v4();
        let mut project = library::model::project::Project::new("effector baseline");
        let mut source = generator_node(
            "shape",
            GeneratorNodeRequest::Shape {
                path: "M 0 0 H 100 V 100 Z".to_string(),
            },
        );
        source.id = source_id;
        project.add_node(source);
        let mut spatial = library::plugin::PluginManager::default()
            .create_shape_transform_operation_node()
            .unwrap();
        spatial.id = spatial_id;
        project.add_node(spatial);
        let mut object = frame_object(source_id);
        object.spatial_transform_node_id = Some(spatial_id);
        object.content = FrameContent::Shape {
            path: "M 0 0 H 100 V 100 Z".to_string(),
            styles: Vec::new(),
            path_effects: Vec::new(),
            effects: Vec::new(),
            ensemble: None,
            transform: Transform {
                position: Position { x: 13.0, y: 10.0 },
                ..Transform::default()
            },
        };
        let frame = FrameInfo {
            width: 1920,
            height: 1080,
            background_color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items: vec![FrameItem::Object(object)],
        };

        let visuals = from_evaluated_frame(&project, &frame);
        let visual = &visuals[0];
        assert_eq!(visual.content_id(), source_id);
        assert_eq!(visual.editable_spatial_id(), Some(spatial_id));
        assert_eq!(
            (
                visual
                    .spatial_layer(spatial_id)
                    .unwrap()
                    .transform
                    .position
                    .x,
                visual
                    .spatial_layer(spatial_id)
                    .unwrap()
                    .transform
                    .position
                    .y
            ),
            (5.0, 7.0)
        );
        assert_eq!(
            (visual.transform.position.x, visual.transform.position.y),
            (13.0, 10.0)
        );
        assert_eq!(
            (
                visual.world_transform.translate_x,
                visual.world_transform.translate_y
            ),
            (13.0, 10.0)
        );
    }

    #[test]
    fn nested_image_transform_layers_preserve_shape_identity_and_clip_facade() {
        let mut project = library::model::project::Project::new("image transform preview");
        let (composition, track) = Composition::new("main", 1920, 1080, 30.0, 3.0);
        let track_id = track.id;
        project.add_track(track);
        project.add_composition(composition);
        let clip = Clip::new("visual", 0.0, 3.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let mut content = generator_node(
            "shape",
            GeneratorNodeRequest::Shape {
                path: "M0 0 H20 V10 H0 Z".to_string(),
            },
        );
        let content_id = content.id;
        let plugins = library::plugin::PluginManager::default();
        let mut shape_transform = plugins.create_shape_transform_operation_node().unwrap();
        let shape_transform_id = shape_transform.id;
        let mut inner_image = plugins.create_image_transform_operation_node().unwrap();
        let inner_image_id = inner_image.id;
        let mut outer_image = plugins.create_image_transform_operation_node().unwrap();
        let outer_image_id = outer_image.id;
        for node in [
            &mut content,
            &mut shape_transform,
            &mut inner_image,
            &mut outer_image,
        ] {
            project.add_node(node.clone());
            project
                .attach_node_to_container(NodeContainer::Clip(clip_id), node.id)
                .unwrap();
        }

        let mut rendered_object = frame_object(content_id);
        rendered_object.spatial_transform_node_id = Some(shape_transform_id);
        rendered_object.spatial_transform = Box::new(Transform {
            position: Position { x: 40.0, y: 25.0 },
            ..Transform::default()
        });
        rendered_object.content = FrameContent::Shape {
            path: "M0 0 H20 V10 H0 Z".to_string(),
            styles: Vec::new(),
            path_effects: Vec::new(),
            effects: Vec::new(),
            ensemble: None,
            transform: rendered_object.spatial_transform.as_ref().clone(),
        };
        let mut inner = frame_group(
            inner_image_id,
            FrameGroupKind::ImageTransform,
            vec![FrameItem::Object(rendered_object)],
        );
        inner.transform.position.x = 12.0;
        let mut outer = frame_group(
            outer_image_id,
            FrameGroupKind::ImageTransform,
            vec![FrameItem::Group(inner)],
        );
        outer.transform.rotation = 15.0;
        let frame = FrameInfo {
            width: 1920,
            height: 1080,
            background_color: Color::black(),
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items: vec![FrameItem::Group(outer)],
        };

        let visuals = from_evaluated_frame(&project, &frame);
        let [visual] = visuals.as_slice() else {
            panic!("nested Image Transform frame must project one visual")
        };
        assert_eq!(visual.owner_target, SelectionTarget::Clip(clip_id));
        assert_eq!(visual.content_id(), content_id);
        assert_eq!(visual.editable_spatial_id(), Some(outer_image_id));
        assert_eq!(
            visual
                .spatial_layers
                .iter()
                .map(|layer| (layer.node.id, layer.kind))
                .collect::<Vec<_>>(),
            vec![
                (outer_image_id, PreviewSpatialKind::ImageTransform),
                (inner_image_id, PreviewSpatialKind::ImageTransform),
                (shape_transform_id, PreviewSpatialKind::ShapeTransform),
            ]
        );
        assert!(visual.matches_node_id(shape_transform_id));
        assert!(visual.matches_node_id(inner_image_id));
        assert_eq!(
            visual
                .spatial_layer(inner_image_id)
                .unwrap()
                .parent_transform,
            library::rendering::renderer::Affine2D::from(&visual.spatial_layers[0].transform)
        );
        assert!(matches!(
            resolve_owner_edit_target(&visuals, SelectionTarget::Clip(clip_id)),
            OwnerEditTargetResolution::Resolved(target)
                if target.spatial_node_id == Some(outer_image_id)
                    && target.content_node_id == content_id
        ));
    }

    #[test]
    fn owner_facade_rejects_multiple_independent_spatial_candidates() {
        let mut project = library::model::project::Project::new("ambiguous facade");
        let (composition, track) = Composition::new("main", 100, 100, 30.0, 1.0);
        let track_id = track.id;
        project.add_track(track);
        project.add_composition(composition);
        let clip = Clip::new("ambiguous", 0.0, 1.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let mut ids = Vec::new();
        for name in ["first", "second"] {
            let node = generator_node(
                name,
                GeneratorNodeRequest::SkSL {
                    shader: "half4 main(float2 p) { return half4(1); }".to_string(),
                },
            );
            ids.push(node.id);
            project.add_node(node.clone());
            project
                .attach_node_to_container(NodeContainer::Clip(clip_id), node.id)
                .unwrap();
        }
        let frame = FrameInfo {
            width: 100,
            height: 100,
            background_color: Color::black(),
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items: ids.iter().copied().map(object).collect(),
        };
        let visuals = from_evaluated_frame(&project, &frame);
        match resolve_owner_edit_target(&visuals, SelectionTarget::Clip(clip_id)) {
            OwnerEditTargetResolution::Ambiguous { candidate_node_ids } => {
                assert_eq!(candidate_node_ids.len(), 2);
                assert!(ids.iter().all(|id| candidate_node_ids.contains(id)));
            }
            other => panic!("independent Clip transforms must be ambiguous: {other:?}"),
        }
    }
}
