use library::model::frame::entity::FrameItem;
use library::model::frame::frame::FrameInfo;
use library::model::frame::transform::Transform;
use library::model::project::Project;
use library::model::Node;
use library::rendering::renderer::Affine2D;
use uuid::Uuid;

/// One interactive visual that actually reached the rendered Composition.
///
/// This is an ephemeral UI projection of `FrameInfo`, not a second project
/// model. Its identity and ordering come from authoritative frame evaluation;
/// wrapper Nodes such as Style, Effect, and Merge never replace the authored
/// geometry Node or its optional spatial Transform owner.
pub struct PreviewClip {
    /// Generator that owns the rendered content (text, path, media, shader).
    pub content_node: Node,
    /// Node that owns absolute position/rotation/scale/anchor. This may be the
    /// same generator for raster content, an explicit Transform for Shape
    /// content, or absent for an untransformed Shape value.
    pub spatial_node: Option<Node>,
    /// Evaluated transform directly owned by `spatial_node`. Preview edits use this
    /// baseline so downstream Effectors are not accidentally baked back into
    /// the generator properties.
    pub spatial_transform: Transform,
    /// Final evaluated visual transform (normalized scale/opacity). This may
    /// include downstream Shape Effector contributions and is used for hit
    /// testing/drawing only.
    pub transform: Transform,
    /// All evaluated container/group transforms above `node`.
    pub parent_transform: Affine2D,
    /// `parent_transform` composed with the Node's evaluated transform.
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
        self.spatial_node.as_ref().map(|node| node.id)
    }

    /// A stale/malformed spatial owner is never made draggable. Every Preview
    /// spatial gesture writes this complete native property contract.
    pub fn editable_spatial_id(&self) -> Option<Uuid> {
        self.spatial_node.as_ref().and_then(|node| {
            ["position", "rotation", "scale", "anchor"]
                .into_iter()
                .all(|key| node.properties().get(key).is_some())
                .then_some(node.id)
        })
    }

    pub fn matches_node_id(&self, node_id: Uuid) -> bool {
        self.content_id() == node_id || self.spatial_id() == Some(node_id)
    }
}

pub fn from_evaluated_frame(project: &Project, frame: &FrameInfo) -> Vec<PreviewClip> {
    let mut visuals = Vec::new();
    let mut path = Vec::new();
    for item in &frame.items {
        collect_visuals(project, item, Affine2D::IDENTITY, &mut path, &mut visuals);
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

fn collect_visuals(
    project: &Project,
    item: &FrameItem,
    parent_transform: Affine2D,
    path: &mut Vec<Uuid>,
    visuals: &mut Vec<PreviewClip>,
) {
    match item {
        FrameItem::Object(object) => {
            let Some(content_node) = project.get_node(object.source_node_id) else {
                // A render result can arrive immediately after a Project
                // replacement. Never make stale frame identity interactive.
                return;
            };
            let spatial_node = object
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
            visuals.push(PreviewClip {
                content_node: content_node.clone(),
                spatial_node,
                spatial_transform: object.spatial_transform.as_ref().clone(),
                world_transform: parent_transform.compose(Affine2D::from(&transform)),
                parent_transform,
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
            let transform = parent_transform.compose(Affine2D::from(&group.transform));
            path.push(group.source_id);
            for child in &group.items {
                collect_visuals(project, child, transform, path, visuals);
            }
            path.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{from_evaluated_frame, visual_for_selection};
    use crate::test_support::generator_node;
    use library::editor::project_service::GeneratorNodeRequest;
    use library::model::frame::color::Color;
    use library::model::frame::entity::{
        FrameBounds, FrameContent, FrameGroup, FrameGroupKind, FrameItem, FrameObject,
    };
    use library::model::frame::frame::FrameInfo;
    use library::model::frame::transform::{Position, Transform};
    use library::model::BlendMode;
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
        let track_id = Uuid::new_v4();
        let mut project = library::model::project::Project::new("direct nodes");
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
                visual.spatial_transform.position.x,
                visual.spatial_transform.position.y
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
}
