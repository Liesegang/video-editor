#[cfg(test)]
mod tests {
    use super::{
        from_evaluated_frame, resolve_owner_edit_target, topmost_visual_for_node,
        visual_for_exact_instance, visual_for_selection, OwnerEditTargetResolution,
        PreviewSpatialKind,
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

    fn attach_nodes_to_composition(
        project: &mut library::model::project::Project,
        node_ids: impl IntoIterator<Item = Uuid>,
    ) -> Uuid {
        let (composition, track) = Composition::new("test", 1920, 1080, 30.0, 3.0);
        let composition_id = composition.id;
        project.add_track(track);
        project.add_composition(composition);
        for node_id in node_ids {
            project
                .attach_node_to_container(NodeContainer::Composition(composition_id), node_id)
                .unwrap();
        }
        composition_id
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
        attach_nodes_to_composition(&mut project, [first_id, second_id]);
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
        attach_nodes_to_composition(&mut project, [content_id, spatial_id]);
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
        attach_nodes_to_composition(&mut project, [content_id]);
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
        attach_nodes_to_composition(&mut project, [source_id]);
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

        let topmost = topmost_visual_for_node(&visuals, source_id).unwrap();
        assert_eq!(topmost.instance_path, visuals[1].instance_path);
        assert_eq!(topmost.world_transform.translate_x, 105.0);
        let explicit_bottom =
            visual_for_exact_instance(&visuals, source_id, visuals[0].instance_path.as_slice())
                .unwrap();
        assert_eq!(explicit_bottom.instance_path, visuals[0].instance_path);
        assert_eq!(explicit_bottom.world_transform.translate_x, 5.0);

        let mut stale_bottom_path = visuals[0].instance_path.clone();
        let branch_index = stale_bottom_path
            .iter()
            .position(|id| *id == bottom_connection_id)
            .unwrap();
        stale_bottom_path[branch_index] = Uuid::new_v4();
        assert!(visual_for_exact_instance(&visuals, source_id, &stale_bottom_path).is_none());
        assert!(visual_for_selection(&visuals, source_id, Some(&stale_bottom_path)).is_none());
        assert_eq!(
            topmost_visual_for_node(&visuals, source_id)
                .unwrap()
                .instance_path,
            visuals[1].instance_path
        );
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
        attach_nodes_to_composition(&mut project, [source_id, spatial_id]);
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
    fn detached_spatial_wrapper_inherits_unique_content_owner() {
        let mut project = library::model::project::Project::new("detached wrapper");
        let (composition, track) = Composition::new("main", 100, 100, 30.0, 1.0);
        let track_id = track.id;
        project.add_track(track);
        project.add_composition(composition);
        let clip = Clip::new("owned content", 0.0, 1.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let content = generator_node(
            "content",
            GeneratorNodeRequest::SkSL {
                shader: "half4 main(float2 p) { return half4(1); }".to_string(),
            },
        );
        let content_id = content.id;
        project.add_node(content);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), content_id)
            .unwrap();
        let wrapper = library::plugin::PluginManager::default()
            .create_image_transform_operation_node()
            .unwrap();
        let wrapper_id = wrapper.id;
        project.add_node(wrapper);
        assert!(project.find_node_container(wrapper_id).is_none());

        let frame = FrameInfo {
            width: 100,
            height: 100,
            background_color: Color::black(),
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items: vec![group(
                wrapper_id,
                FrameGroupKind::ImageTransform,
                vec![object(content_id)],
            )],
        };

        let visuals = from_evaluated_frame(&project, &frame);
        let [visual] = visuals.as_slice() else {
            panic!("detached wrapper with owned content must remain interactive")
        };
        assert_eq!(visual.owner_target, SelectionTarget::Clip(clip_id));
        assert_eq!(visual.editable_spatial_id(), Some(wrapper_id));
    }

    #[test]
    fn duplicate_spatial_containment_fails_closed() {
        let mut project = library::model::project::Project::new("duplicate containment");
        let (composition, track) = Composition::new("main", 100, 100, 30.0, 1.0);
        let track_id = track.id;
        project.add_track(track);
        project.add_composition(composition);
        let clip = Clip::new("content", 0.0, 1.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let content = generator_node(
            "content",
            GeneratorNodeRequest::SkSL {
                shader: "half4 main(float2 p) { return half4(1); }".to_string(),
            },
        );
        let content_id = content.id;
        let wrapper = library::plugin::PluginManager::default()
            .create_image_transform_operation_node()
            .unwrap();
        let wrapper_id = wrapper.id;
        project.add_node(content);
        project.add_node(wrapper);
        for node_id in [content_id, wrapper_id] {
            project
                .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
                .unwrap();
        }
        // Simulate a corrupt/stale pre-v1 Project that bypassed graph APIs.
        project
            .get_track_mut(track_id)
            .unwrap()
            .node_ids
            .push(wrapper_id);

        let frame = FrameInfo {
            width: 100,
            height: 100,
            background_color: Color::black(),
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items: vec![group(
                wrapper_id,
                FrameGroupKind::ImageTransform,
                vec![object(content_id)],
            )],
        };

        assert!(from_evaluated_frame(&project, &frame).is_empty());
    }

    #[test]
    fn owner_facade_resolves_common_inner_image_transform() {
        let mut project = library::model::project::Project::new("common inner transform");
        let (composition, track) = Composition::new("main", 100, 100, 30.0, 1.0);
        let track_id = track.id;
        project.add_track(track);
        project.add_composition(composition);
        let clip = Clip::new("two branches", 0.0, 1.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let plugins = library::plugin::PluginManager::default();
        let first = generator_node(
            "first",
            GeneratorNodeRequest::SkSL {
                shader: "half4 main(float2 p) { return half4(1); }".to_string(),
            },
        );
        let first_id = first.id;
        let second = generator_node(
            "second",
            GeneratorNodeRequest::SkSL {
                shader: "half4 main(float2 p) { return half4(1); }".to_string(),
            },
        );
        let second_id = second.id;
        let common = plugins.create_image_transform_operation_node().unwrap();
        let common_id = common.id;
        let first_outer = plugins.create_image_transform_operation_node().unwrap();
        let first_outer_id = first_outer.id;
        let second_outer = plugins.create_image_transform_operation_node().unwrap();
        let second_outer_id = second_outer.id;
        for node in [first, second, common, first_outer, second_outer] {
            let node_id = node.id;
            project.add_node(node);
            project
                .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
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
            items: vec![
                group(
                    first_outer_id,
                    FrameGroupKind::ImageTransform,
                    vec![group(
                        common_id,
                        FrameGroupKind::ImageTransform,
                        vec![object(first_id)],
                    )],
                ),
                group(
                    second_outer_id,
                    FrameGroupKind::ImageTransform,
                    vec![group(
                        common_id,
                        FrameGroupKind::ImageTransform,
                        vec![object(second_id)],
                    )],
                ),
            ],
        };

        let visuals = from_evaluated_frame(&project, &frame);
        assert_eq!(visuals.len(), 2);
        assert_eq!(visuals[0].editable_spatial_id(), Some(first_outer_id));
        assert_eq!(visuals[1].editable_spatial_id(), Some(second_outer_id));
        assert!(matches!(
            resolve_owner_edit_target(&visuals, SelectionTarget::Clip(clip_id)),
            OwnerEditTargetResolution::Resolved(target)
                if target.spatial_node_id == Some(common_id)
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
