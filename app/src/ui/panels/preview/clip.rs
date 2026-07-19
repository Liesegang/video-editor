use library::model::Node;
use library::model::frame::entity::{FrameGroupKind, FrameItem};
use library::model::frame::frame::FrameInfo;
use library::model::frame::transform::Transform;
use library::model::project::Project;
use library::rendering::renderer::Affine2D;
use uuid::Uuid;

/// One interactive visual that actually reached the rendered Composition.
///
/// This is an ephemeral UI projection of `FrameInfo`, not a second project
/// model. Its identity and ordering come from authoritative frame evaluation;
/// wrapper Nodes such as Style, Effect, and Merge never replace the authored
/// Node that produced the `FrameObject`.
pub struct PreviewClip {
    pub node: Node,
    pub track_id: Option<Uuid>,
    /// Evaluated transform directly owned by `node`. Preview edits use this
    /// baseline so downstream Effectors are not accidentally baked back into
    /// the generator properties.
    pub source_transform: Transform,
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
    pub fn id(&self) -> Uuid {
        self.node.id
    }
}

pub fn from_evaluated_frame(project: &Project, frame: &FrameInfo) -> Vec<PreviewClip> {
    let mut visuals = Vec::new();
    let mut path = Vec::new();
    for item in &frame.items {
        collect_visuals(
            project,
            item,
            Affine2D::IDENTITY,
            None,
            &mut path,
            &mut visuals,
        );
    }
    visuals
}

/// Resolve one rendered instance for a selected source Node.
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
                .find(|visual| visual.id() == node_id && visual.instance_path == path)
        })
        .or_else(|| visuals.iter().rev().find(|visual| visual.id() == node_id))
}

fn collect_visuals(
    project: &Project,
    item: &FrameItem,
    parent_transform: Affine2D,
    track_id: Option<Uuid>,
    path: &mut Vec<Uuid>,
    visuals: &mut Vec<PreviewClip>,
) {
    match item {
        FrameItem::Object(object) => {
            let Some(node) = project.get_node(object.source_node_id) else {
                // A render result can arrive immediately after a Project
                // replacement. Never make stale frame identity interactive.
                return;
            };
            let transform = object.content.transform().clone();
            path.push(object.source_node_id);
            visuals.push(PreviewClip {
                node: node.clone(),
                track_id: track_id.or_else(|| project.find_parent_track(node.id)),
                source_transform: object.source_transform.as_ref().clone(),
                world_transform: parent_transform.compose(Affine2D::from(&transform)),
                parent_transform,
                transform,
                content_bounds: object.content_bounds.map(|bounds| bounds.as_tuple()),
                instance_path: path.clone(),
            });
            path.pop();
        }
        FrameItem::Group(group) => {
            let track_id = if group.kind == FrameGroupKind::Track {
                Some(group.source_id)
            } else {
                track_id
            };
            let transform = parent_transform.compose(Affine2D::from(&group.transform));
            path.push(group.source_id);
            for child in &group.items {
                collect_visuals(project, child, transform, track_id, path, visuals);
            }
            path.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{from_evaluated_frame, visual_for_selection};
    use library::model::frame::color::Color;
    use library::model::frame::entity::{
        FrameBounds, FrameContent, FrameGroup, FrameGroupKind, FrameItem, FrameObject,
    };
    use library::model::frame::frame::FrameInfo;
    use library::model::frame::transform::{Position, Transform};
    use library::model::{BlendMode, GeneratorContent, Node, NodeContent};
    use ordered_float::OrderedFloat;
    use uuid::Uuid;

    fn object(node_id: Uuid) -> FrameItem {
        FrameItem::Object(FrameObject {
            source_node_id: node_id,
            source_transform: Box::new(Transform {
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
        })
    }

    fn group(source_id: Uuid, kind: FrameGroupKind, items: Vec<FrameItem>) -> FrameItem {
        FrameItem::Group(FrameGroup {
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
        })
    }

    #[test]
    fn frame_object_identity_survives_style_effect_and_merge_wrappers_in_render_order() {
        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();
        let mut project = library::model::project::Project::new("preview");
        let mut first = Node::new("first", NodeContent::Generator(GeneratorContent::SkSL));
        first.id = first_id;
        project.add_node(first);
        let mut second = Node::new("second", NodeContent::Generator(GeneratorContent::SkSL));
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
            visuals.iter().map(|visual| visual.id()).collect::<Vec<_>>(),
            vec![first_id, second_id]
        );
    }

    #[test]
    fn merge_fan_out_keeps_node_selection_and_topmost_branch_identity_separate() {
        let source_id = Uuid::new_v4();
        let bottom_connection_id = Uuid::new_v4();
        let top_connection_id = Uuid::new_v4();
        let mut project = library::model::project::Project::new("preview fan-out");
        let mut source = Node::new("source", NodeContent::Generator(GeneratorContent::SkSL));
        source.id = source_id;
        project.add_node(source);
        let mut top_branch = group(
            top_connection_id,
            FrameGroupKind::ConnectedImage,
            vec![object(source_id)],
        );
        let FrameItem::Group(top_group) = &mut top_branch else {
            unreachable!()
        };
        top_group.transform.position.x = 100.0;
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
        assert_eq!(visuals[0].id(), source_id);
        assert_eq!(visuals[1].id(), source_id);
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
    fn direct_composition_and_track_nodes_project_without_fake_track_ids() {
        let composition_node_id = Uuid::new_v4();
        let track_node_id = Uuid::new_v4();
        let track_id = Uuid::new_v4();
        let mut project = library::model::project::Project::new("direct nodes");
        for (id, name) in [
            (composition_node_id, "composition node"),
            (track_node_id, "track node"),
        ] {
            let mut node = Node::new(name, NodeContent::Generator(GeneratorContent::SkSL));
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
        assert_eq!(visuals[0].id(), composition_node_id);
        assert_eq!(visuals[0].track_id, None);
        assert_eq!(visuals[1].id(), track_node_id);
        assert_eq!(visuals[1].track_id, Some(track_id));

        let empty = FrameInfo {
            items: Vec::new(),
            ..frame
        };
        assert!(from_evaluated_frame(&project, &empty).is_empty());
    }

    #[test]
    fn downstream_effector_transform_does_not_replace_source_edit_baseline() {
        let source_id = Uuid::new_v4();
        let mut project = library::model::project::Project::new("effector baseline");
        let mut source = Node::new("shape", NodeContent::Generator(GeneratorContent::Shape));
        source.id = source_id;
        project.add_node(source);
        let mut item = object(source_id);
        let FrameItem::Object(object) = &mut item else {
            unreachable!()
        };
        object.content.transform_mut().position = Position { x: 13.0, y: 10.0 };
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
            items: vec![item],
        };

        let visuals = from_evaluated_frame(&project, &frame);
        let visual = &visuals[0];
        assert_eq!(
            (
                visual.source_transform.position.x,
                visual.source_transform.position.y
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
