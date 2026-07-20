#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::generator_node;
    use library::editor::project_service::GeneratorNodeRequest;

    fn empty_preview_frame(time: f64) -> library::model::frame::frame::FrameInfo {
        library::model::frame::frame::FrameInfo {
            width: 1920,
            height: 1080,
            background_color: library::model::frame::color::Color::black(),
            color_profile: "sRGB".to_string(),
            render_scale: ordered_float::OrderedFloat(1.0),
            now_time: ordered_float::OrderedFloat(time),
            region: None,
            items: Vec::new(),
        }
    }

    #[test]
    fn stale_preview_worker_results_never_replace_the_current_request() {
        let current = empty_preview_frame(3.0);
        let stale = empty_preview_frame(2.0);

        assert!(preview_result_is_current(false, Some(&current), &current));
        assert!(!preview_result_is_current(false, Some(&current), &stale));
        assert!(!preview_result_is_current(false, None, &current));
        assert!(!preview_result_is_current(true, Some(&current), &current));
        assert!(preview_render_wait_requires_repaint(false, true, false));
        assert!(!preview_render_wait_requires_repaint(false, true, true));
        assert!(!preview_render_wait_requires_repaint(false, false, false));
        assert!(!preview_render_wait_requires_repaint(true, true, false));
    }

    #[test]
    fn shared_gl_texture_validation_rejects_invalid_ffi_inputs() {
        assert_eq!(
            ValidGlPreviewTexture::new(7, 1920, 1080).map(|texture| (
                texture.id.get(),
                texture.width,
                texture.height
            )),
            Some((7, 1920, 1080))
        );
        assert!(ValidGlPreviewTexture::new(0, 1920, 1080).is_none());
        assert!(ValidGlPreviewTexture::new(7, 0, 1080).is_none());
        assert!(ValidGlPreviewTexture::new(7, 1920, 0).is_none());
        assert!(ValidGlPreviewTexture::new(7, u32::MAX, 1080).is_none());
        assert!(ValidGlPreviewTexture::new(7, 1920, u32::MAX).is_none());
    }

    #[test]
    fn preview_visual_qa_rect_uses_evaluated_world_and_camera_coordinates() {
        use library::model::frame::transform::Transform;
        use library::rendering::renderer::Affine2D;

        let node = generator_node(
            "QA visual",
            GeneratorNodeRequest::SkSL {
                shader: "half4 main(float2 p) { return half4(1); }".to_string(),
            },
        );
        let visual = clip::PreviewClip {
            content_node: node.clone(),
            spatial_layers: vec![clip::PreviewSpatialLayer {
                node: node.clone(),
                kind: clip::PreviewSpatialKind::Content,
                transform: Transform::default(),
                parent_transform: Affine2D::IDENTITY,
            }],
            owner_target: SelectionTarget::Node(node.id),
            transform: Transform::default(),
            world_transform: Affine2D::IDENTITY,
            content_bounds: Some((1.0, 2.0, 30.0, 40.0)),
            instance_path: vec![node.id],
        };

        let rect = preview_visual_screen_rect(&visual, &|position| {
            egui::pos2(10.0 + position.x * 2.0, 20.0 + position.y * 2.0)
        })
        .expect("positive content bounds publish a QA rectangle");
        assert_eq!(rect.min, egui::pos2(12.0, 24.0));
        assert_eq!(rect.max, egui::pos2(72.0, 104.0));
    }

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.001,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn fit_uses_offset_viewport_rect_and_centers_canvas() {
        let viewport =
            egui::Rect::from_min_size(egui::pos2(137.0, 83.0), egui::vec2(1000.0, 700.0));
        let canvas_size = egui::vec2(1920.0, 1080.0);
        let fitted = fit_canvas_to_viewport(viewport, canvas_size).unwrap();
        let screen_canvas =
            preview_content_rect(viewport, fitted.pan, fitted.zoom, canvas_size).unwrap();

        assert_near(screen_canvas.center().x, viewport.center().x);
        assert_near(screen_canvas.center().y, viewport.center().y);
        assert!(screen_canvas.left() >= viewport.left());
        assert!(screen_canvas.right() <= viewport.right());
        assert!(screen_canvas.top() >= viewport.top());
        assert!(screen_canvas.bottom() <= viewport.bottom());
    }

    #[test]
    fn preview_content_rect_rejects_invalid_camera_geometry() {
        let viewport = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
        let canvas = egui::vec2(640.0, 360.0);

        assert!(preview_content_rect(viewport, egui::Vec2::ZERO, 1.0, canvas).is_some());
        assert!(preview_content_rect(viewport, egui::Vec2::ZERO, 0.0, canvas).is_none());
        assert!(preview_content_rect(viewport, egui::Vec2::NAN, 1.0, canvas).is_none());
        assert!(preview_content_rect(viewport, egui::Vec2::ZERO, 1.0, egui::Vec2::ZERO).is_none());
    }

    #[test]
    fn fit_is_stable_in_logical_points_across_high_dpi_scales() {
        let viewport = egui::Rect::from_min_size(egui::pos2(20.0, 30.0), egui::vec2(800.0, 600.0));
        let canvas_size = egui::vec2(1920.0, 1080.0);
        let fitted = fit_canvas_to_viewport(viewport, canvas_size).unwrap();

        for pixels_per_point in [1.0_f32, 1.5, 2.0, 3.0] {
            let physical_viewport_center = viewport.center().to_vec2() * pixels_per_point;
            let logical_canvas_center =
                viewport.min.to_vec2() + fitted.pan + canvas_size * fitted.zoom * 0.5;
            let physical_canvas_center = logical_canvas_center * pixels_per_point;
            assert_near(physical_canvas_center.x, physical_viewport_center.x);
            assert_near(physical_canvas_center.y, physical_viewport_center.y);
        }
    }

    #[test]
    fn initial_and_requested_fit_center_content_without_touching_project() {
        let composition_id = uuid::Uuid::new_v4();
        let composition = Some((composition_id, 1920, 1080));
        let viewport = egui::Rect::from_min_size(egui::pos2(91.0, 57.0), egui::vec2(900.0, 620.0));
        let canvas_size = egui::vec2(1920.0, 1080.0);
        let mut runtime = PreviewViewportRuntimeState::default();
        let mut pan = egui::vec2(-400.0, 900.0);
        let mut zoom = 7.0;

        assert!(update_preview_fit(
            &mut runtime,
            &mut pan,
            &mut zoom,
            composition,
            viewport,
        ));
        let initially_fitted = preview_content_rect(viewport, pan, zoom, canvas_size).unwrap();
        assert_near(initially_fitted.center().x, viewport.center().x);
        assert_near(initially_fitted.center().y, viewport.center().y);

        // A user-authored camera survives ordinary frames and resizes once
        // auto-fit has been disabled by an interaction.
        runtime.auto_fit = false;
        pan = egui::vec2(13.0, 29.0);
        zoom = 1.75;
        let resized = viewport.expand(80.0);
        assert!(!update_preview_fit(
            &mut runtime,
            &mut pan,
            &mut zoom,
            composition,
            resized,
        ));
        assert_eq!(pan, egui::vec2(13.0, 29.0));
        assert_eq!(zoom, 1.75);

        runtime.request_fit();
        assert!(update_preview_fit(
            &mut runtime,
            &mut pan,
            &mut zoom,
            composition,
            resized,
        ));
        let reset_fit = preview_content_rect(resized, pan, zoom, canvas_size).unwrap();
        assert_near(reset_fit.center().x, resized.center().x);
        assert_near(reset_fit.center().y, resized.center().y);
    }

    fn run_preview_interaction_frame(
        context: &egui::Context,
        editor_context: &mut EditorContext,
        _project: &Arc<RwLock<Project>>,
        pending_actions: &mut Vec<PreviewAction>,
        frame: usize,
        events: Vec<egui::Event>,
    ) -> PreviewGestureDecision {
        let mut decision = PreviewGestureDecision::default();
        let _output = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                time: Some(frame as f64 / 60.0),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let viewport = ui.available_rect_before_wrap().shrink(24.0);
                    let pan_requested = ui.input(|input| input.key_down(egui::Key::Space));
                    let input = ui.input(|input| PreviewGestureInput {
                        primary_pressed: input.pointer.button_pressed(egui::PointerButton::Primary),
                        primary_down: input.pointer.button_down(egui::PointerButton::Primary),
                        primary_released: input
                            .pointer
                            .button_released(egui::PointerButton::Primary),
                        primary_dragging: input.pointer.is_decidedly_dragging(),
                        press_started_in_viewport: input
                            .pointer
                            .press_origin()
                            .is_some_and(|position| viewport.contains(position)),
                        pan_requested,
                    });
                    decision = arbitrate_primary_gesture(
                        &mut editor_context.interaction.preview_viewport.primary_gesture,
                        input,
                    );

                    let pointer_delta = ui.input(|input| input.pointer.delta());
                    let (mut viewport_changed, response) = {
                        let mut viewport_state = PreviewViewportState {
                            pan: &mut editor_context.view.pan,
                            zoom: &mut editor_context.view.zoom,
                        };
                        let controller_id = ui.make_persistent_id("preview-space-pan-real-events");
                        let mut controller = ViewportController::new(ui, controller_id, None);
                        controller.interact_with_rect(
                            viewport,
                            &mut viewport_state,
                            &mut editor_context.interaction.handled_hand_tool_drag,
                        )
                    };
                    viewport_changed |= apply_owned_primary_pan(
                        decision.pan_owned,
                        input.primary_pressed,
                        input.primary_down,
                        input.primary_released,
                        pointer_delta,
                        &mut editor_context.view.pan,
                        &mut editor_context.interaction.handled_hand_tool_drag,
                    );
                    if viewport_changed {
                        editor_context.interaction.preview_viewport.auto_fit = false;
                    }

                    let mut interactions = interaction::PreviewInteractions::new(
                        ui,
                        editor_context,
                        &[],
                        |position| position,
                        |position| position,
                    );
                    interactions.handle(&response, viewport, decision.pan_owned, pending_actions);
                    drop(interactions);

                    if decision.finish_after_frame {
                        editor_context.interaction.preview_viewport.primary_gesture =
                            PreviewPrimaryGesture::Idle;
                    }
                });
            },
        );
        decision
    }

    fn run_transformed_visual_frame(
        context: &egui::Context,
        editor_context: &mut EditorContext,
        visual: &clip::PreviewClip,
        pending_actions: &mut Vec<PreviewAction>,
        frame: usize,
        events: Vec<egui::Event>,
    ) {
        let _output = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                time: Some(frame as f64 / 60.0),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    let viewport = ui.available_rect_before_wrap();
                    let response = ui.interact(
                        viewport,
                        ui.make_persistent_id("preview-transformed-visual"),
                        egui::Sense::click_and_drag(),
                    );
                    let visuals = std::slice::from_ref(visual);
                    let mut interactions = interaction::PreviewInteractions::new(
                        ui,
                        editor_context,
                        visuals,
                        |position| position,
                        |position| position,
                    );
                    interactions.handle(&response, viewport, false, pending_actions);
                    drop(interactions);
                    gizmo::draw_gizmo(ui, editor_context, visuals, |position| position, true);
                });
            },
        );
    }

    fn raw_pointer_drag(
        context: &egui::Context,
        editor_context: &mut EditorContext,
        visual: &clip::PreviewClip,
        pending_actions: &mut Vec<PreviewAction>,
        start: egui::Pos2,
        threshold: egui::Pos2,
        end: egui::Pos2,
    ) {
        run_transformed_visual_frame(
            context,
            editor_context,
            visual,
            pending_actions,
            0,
            Vec::new(),
        );
        run_transformed_visual_frame(
            context,
            editor_context,
            visual,
            pending_actions,
            1,
            vec![egui::Event::PointerMoved(start)],
        );
        run_transformed_visual_frame(
            context,
            editor_context,
            visual,
            pending_actions,
            2,
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        run_transformed_visual_frame(
            context,
            editor_context,
            visual,
            pending_actions,
            3,
            vec![egui::Event::PointerMoved(threshold)],
        );
        run_transformed_visual_frame(
            context,
            editor_context,
            visual,
            pending_actions,
            4,
            vec![egui::Event::PointerMoved(end)],
        );
        run_transformed_visual_frame(
            context,
            editor_context,
            visual,
            pending_actions,
            5,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }

    #[test]
    fn dual_identity_preview_routes_spatial_drag_to_transform_and_text_tool_to_generator() {
        use library::model::frame::transform::{Position, Transform};
        use library::plugin::PluginManager;
        use library::rendering::renderer::Affine2D;

        let mut content_node = generator_node(
            "Text content",
            GeneratorNodeRequest::Text {
                text: "Editable text".to_string(),
                font: "Arial".to_string(),
            },
        );
        let content_id = uuid::Uuid::new_v4();
        content_node.id = content_id;
        let mut spatial_node = PluginManager::default()
            .create_shape_transform_operation_node()
            .expect("native Transform factory must be available");
        let spatial_id = uuid::Uuid::new_v4();
        spatial_node.id = spatial_id;
        let spatial_transform = Transform {
            position: Position { x: 240.0, y: 160.0 },
            ..Transform::default()
        };
        let visual = clip::PreviewClip {
            content_node,
            spatial_layers: vec![clip::PreviewSpatialLayer {
                node: spatial_node,
                kind: clip::PreviewSpatialKind::ShapeTransform,
                transform: spatial_transform.clone(),
                parent_transform: Affine2D::IDENTITY,
            }],
            owner_target: SelectionTarget::Clip(content_id),
            transform: spatial_transform.clone(),
            world_transform: Affine2D::from(&spatial_transform),
            content_bounds: Some((-40.0, -20.0, 80.0, 40.0)),
            instance_path: vec![content_id, spatial_id],
        };
        let center = egui::pos2(240.0, 160.0);

        // Preview behaves like a conventional NLE: its primary selection is
        // the Clip facade while the view-local edit target routes the drag to
        // the explicit spatial Transform behind that facade.
        let context = egui::Context::default();
        let mut editor_context = EditorContext::new(uuid::Uuid::new_v4());
        editor_context.view.zoom = 1.0;
        editor_context.select_target(SelectionTarget::Node(content_id));
        let mut actions = Vec::new();
        raw_pointer_drag(
            &context,
            &mut editor_context,
            &visual,
            &mut actions,
            center,
            center + egui::vec2(8.0, 0.0),
            center + egui::vec2(20.0, 10.0),
        );
        assert_eq!(
            editor_context.selection.primary(),
            Some(SelectionTarget::Clip(content_id))
        );
        assert_eq!(
            editor_context
                .interaction
                .preview_edit_target
                .as_ref()
                .and_then(|target| target.spatial_node_id),
            Some(spatial_id)
        );
        assert!(actions.iter().any(|action| matches!(
            action,
            PreviewAction::UpdateProperty {
                node_id,
                prop_name,
                ..
            } if *node_id == spatial_id && prop_name == "position"
        )));
        assert!(!actions.iter().any(|action| matches!(
            action,
            PreviewAction::UpdateProperty { node_id, .. } if *node_id == content_id
        )));

        // Text editing deliberately resolves the same rendered instance back
        // to the content generator rather than the spatial edit owner.
        let context = egui::Context::default();
        let mut editor_context = EditorContext::new(uuid::Uuid::new_v4());
        editor_context.view.active_tool = PreviewTool::Text;
        run_transformed_visual_frame(
            &context,
            &mut editor_context,
            &visual,
            &mut Vec::new(),
            0,
            vec![egui::Event::PointerMoved(center)],
        );
        run_transformed_visual_frame(
            &context,
            &mut editor_context,
            &visual,
            &mut Vec::new(),
            1,
            vec![egui::Event::PointerButton {
                pos: center,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        run_transformed_visual_frame(
            &context,
            &mut editor_context,
            &visual,
            &mut Vec::new(),
            2,
            vec![egui::Event::PointerButton {
                pos: center,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(
            editor_context.selection.primary(),
            Some(SelectionTarget::Clip(content_id))
        );
        assert_eq!(
            editor_context.interaction.editing_text_entity_id,
            Some(content_id)
        );
        assert_eq!(editor_context.interaction.text_edit_buffer, "Editable text");
    }

    #[test]
    fn real_space_primary_drag_pans_without_content_selection_or_drag_updates() {
        use crate::model::ui_types::GizmoHandle;
        use crate::model::vector::VectorEditorState;
        use crate::state::context_types::{BodyDragState, GizmoState};
        use library::model::vector::HandleType;
        use std::collections::HashMap;

        let context = egui::Context::default();
        let composition_id = uuid::Uuid::new_v4();
        let selected_id = uuid::Uuid::new_v4();
        let project = Arc::new(RwLock::new(Project::new("preview gesture")));
        let project_before = project.read().unwrap().clone();
        let mut editor_context = EditorContext::new(composition_id);
        editor_context.select_target(SelectionTarget::Node(selected_id));
        let mut pending_actions = Vec::new();
        let hover = egui::pos2(40.0, 40.0);

        // Warm egui's widget memory before the real key/pointer sequence.
        assert!(
            !run_preview_interaction_frame(
                &context,
                &mut editor_context,
                &project,
                &mut pending_actions,
                0,
                vec![egui::Event::PointerMoved(hover)],
            )
            .pan_owned
        );

        editor_context.interaction.is_moving_selected_entity = true;
        editor_context.interaction.preview_selection_drag_start = Some(egui::pos2(4.0, 5.0));
        editor_context.interaction.body_drag_state = Some(BodyDragState {
            start_mouse_pos: egui::pos2(4.0, 5.0),
            original_positions: HashMap::from([(selected_id, [12.0, 34.0])]),
        });
        editor_context.interaction.gizmo_state = Some(GizmoState {
            start_mouse_pos: egui::pos2(4.0, 5.0),
            active_handle: GizmoHandle::Rotation,
            original_position: [12.0, 34.0],
            original_scale_x: 100.0,
            original_scale_y: 100.0,
            original_rotation: 0.0,
            original_visual_position: [12.0, 34.0],
            original_visual_scale_x: 100.0,
            original_visual_scale_y: 100.0,
            original_visual_rotation: 0.0,
            original_anchor_x: 0.0,
            original_anchor_y: 0.0,
            original_width: 100.0,
            original_height: 100.0,
        });
        editor_context.interaction.vector_editor_state = Some(VectorEditorState {
            selected_handle: Some((0, HandleType::Vertex)),
            ..Default::default()
        });

        let pan_before = editor_context.view.pan;
        let zoom_before = editor_context.view.zoom;
        let start = egui::pos2(180.0, 160.0);
        let midpoint = egui::pos2(255.0, 202.5);
        let end = egui::pos2(330.0, 245.0);
        let key_event = |pressed| egui::Event::Key {
            key: egui::Key::Space,
            physical_key: Some(egui::Key::Space),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };

        let pressed = run_preview_interaction_frame(
            &context,
            &mut editor_context,
            &project,
            &mut pending_actions,
            1,
            vec![
                key_event(true),
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(pressed.pan_owned);
        assert_eq!(editor_context.view.pan, pan_before);

        let dragged = run_preview_interaction_frame(
            &context,
            &mut editor_context,
            &project,
            &mut pending_actions,
            2,
            vec![egui::Event::PointerMoved(midpoint)],
        );
        assert!(dragged.pan_owned);
        assert_near(
            editor_context.view.pan.x,
            pan_before.x + midpoint.x - start.x,
        );
        assert_near(
            editor_context.view.pan.y,
            pan_before.y + midpoint.y - start.y,
        );

        let modifier_released = run_preview_interaction_frame(
            &context,
            &mut editor_context,
            &project,
            &mut pending_actions,
            3,
            vec![key_event(false)],
        );
        assert!(modifier_released.pan_owned);
        assert!(!modifier_released.finish_after_frame);

        let released = run_preview_interaction_frame(
            &context,
            &mut editor_context,
            &project,
            &mut pending_actions,
            4,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(released.pan_owned);
        assert!(released.finish_after_frame);
        assert_near(editor_context.view.pan.x, pan_before.x + end.x - start.x);
        assert_near(editor_context.view.pan.y, pan_before.y + end.y - start.y);
        assert_eq!(editor_context.view.zoom, zoom_before);
        assert!(!editor_context.interaction.preview_viewport.auto_fit);
        assert_eq!(
            editor_context.interaction.preview_viewport.primary_gesture,
            PreviewPrimaryGesture::Idle
        );

        assert_eq!(
            editor_context.selection.targets(),
            &[SelectionTarget::Node(selected_id)]
        );
        assert_eq!(
            editor_context.selection.primary(),
            Some(SelectionTarget::Node(selected_id))
        );
        assert!(!editor_context.interaction.is_moving_selected_entity);
        assert!(editor_context.interaction.body_drag_state.is_none());
        assert!(editor_context
            .interaction
            .preview_selection_drag_start
            .is_none());
        assert!(editor_context.interaction.gizmo_state.is_none());
        assert!(editor_context
            .interaction
            .vector_editor_state
            .as_ref()
            .is_some_and(|state| state.selected_handle.is_none()));
        assert!(pending_actions.is_empty());
        assert_eq!(*project.read().unwrap(), project_before);
        assert_eq!(editor_context.view.zoom, zoom_before);
        assert!(!editor_context.interaction.preview_viewport.auto_fit);
    }

    #[test]
    fn raw_input_parent_transform_drags_edit_source_space_without_baking_downstream_transform() {
        use library::cache::CacheManager;
        use library::model::frame::transform::{Position, Scale, Transform};
        use library::model::property::{Property, PropertyValue, Vec2 as PropertyVec2};
        use library::model::Node;
        use library::plugin::PluginManager;
        use library::rendering::renderer::Affine2D;
        use ordered_float::OrderedFloat;

        fn source_node() -> Node {
            let mut source = generator_node(
                "Transformed source",
                GeneratorNodeRequest::SkSL {
                    shader: "half4 main(float2 p) { return half4(1); }".to_string(),
                },
            );
            source
                .set_property(
                    "position".to_string(),
                    Property::constant(PropertyValue::Vec2(PropertyVec2 {
                        x: OrderedFloat(7.0),
                        y: OrderedFloat(11.0),
                    })),
                )
                .expect("SkSL factory initializes position");
            source
                .set_property(
                    "scale".to_string(),
                    Property::constant(PropertyValue::Vec2(PropertyVec2 {
                        x: OrderedFloat(100.0),
                        y: OrderedFloat(100.0),
                    })),
                )
                .expect("SkSL factory initializes scale");
            source
                .set_property(
                    "rotation".to_string(),
                    Property::constant(PropertyValue::Number(OrderedFloat(0.0))),
                )
                .expect("SkSL factory initializes rotation");
            source
        }

        fn transformed_visual(source: &Node) -> clip::PreviewClip {
            let source_transform = Transform {
                position: Position { x: 7.0, y: 11.0 },
                ..Transform::default()
            };
            // This is the final value after a downstream Transform Effector.
            // It deliberately differs from the directly editable source.
            let transform = Transform {
                position: Position { x: 20.0, y: 30.0 },
                scale: Scale { x: 2.0, y: 1.0 },
                ..Transform::default()
            };
            let parent_transform = Affine2D::from(&Transform {
                position: Position { x: 300.0, y: 150.0 },
                scale: Scale { x: 2.0, y: 0.5 },
                rotation: 90.0,
                ..Transform::default()
            });
            clip::PreviewClip {
                content_node: source.clone(),
                spatial_layers: vec![clip::PreviewSpatialLayer {
                    node: source.clone(),
                    kind: clip::PreviewSpatialKind::Content,
                    transform: source_transform,
                    parent_transform,
                }],
                owner_target: SelectionTarget::Clip(source.id),
                world_transform: parent_transform.compose(Affine2D::from(&transform)),
                transform,
                content_bounds: Some((-20.0, -20.0, 40.0, 40.0)),
                instance_path: vec![source.id],
            }
        }

        fn apply_actions(source: Node, actions: Vec<PreviewAction>) -> Node {
            let source_id = source.id;
            let mut model = Project::new("transformed preview edit");
            model.add_node(source);
            let project = Arc::new(RwLock::new(model));
            let service = EditorService::new(
                Arc::clone(&project),
                Arc::new(PluginManager::default()),
                Arc::new(CacheManager::new()),
            )
            .unwrap();
            let mut history = HistoryManager::new();
            history.push_project_state(project.read().unwrap().clone());
            apply_preview_actions(actions, &service, &project, &mut history);
            let edited = project.read().unwrap().get_node(source_id).unwrap().clone();
            edited
        }

        fn vector_property(node: &Node, key: &str) -> (f64, f64) {
            let Some(PropertyValue::Vec2(value)) =
                node.properties().get(key).and_then(Property::value)
            else {
                panic!("{key} must remain a Vec2 property")
            };
            (value.x.into_inner(), value.y.into_inner())
        }

        // Body drag: the final screen delta (3, 8) maps through the inverse
        // parent matrix to source-local (4, -6). It must be added to the
        // source position (7, 11), not the downstream position (20, 30).
        let source = source_node();
        let visual = transformed_visual(&source);
        let context = egui::Context::default();
        let mut editor_context = EditorContext::new(uuid::Uuid::new_v4());
        editor_context.view.zoom = 1.0;
        editor_context.select_target(visual.owner_target);
        editor_context.interaction.preview_edit_target = Some(visual.edit_target());
        let (center_x, center_y) = visual.world_transform.map_point(0.0, 0.0);
        let center = egui::pos2(center_x as f32, center_y as f32);
        let mut actions = Vec::new();
        raw_pointer_drag(
            &context,
            &mut editor_context,
            &visual,
            &mut actions,
            center,
            center + egui::vec2(0.0, 8.0),
            center + egui::vec2(3.0, 16.0),
        );
        assert!(actions.iter().any(|action| matches!(
            action,
            PreviewAction::UpdateProperty { node_id, prop_name, .. }
                if *node_id == source.id && prop_name == "position"
        )));
        assert!(actions
            .iter()
            .any(|action| matches!(action, PreviewAction::CommitHistory)));
        let body_positions = actions
            .iter()
            .filter_map(|action| match action {
                PreviewAction::UpdateProperty {
                    prop_name,
                    value: PropertyValue::Vec2(value),
                    ..
                } if prop_name == "position" => Some((value.x.into_inner(), value.y.into_inner())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(body_positions.last(), Some(&(11.0, 5.0)));
        let edited = apply_actions(source, actions);
        assert_eq!(vector_property(&edited, "position"), (11.0, 5.0));
        assert_eq!(vector_property(&edited, "scale"), (100.0, 100.0));

        // Right-handle drag captures at pointer press, so its complete screen
        // delta (0, 28) maps to +14 on the local X axis. The displayed width
        // is 80 after the downstream 2x scale, so the source scale becomes
        // 117.5% and source position moves by +7. Neither downstream 200% nor
        // its (20, 30) position is baked.
        let source = source_node();
        let visual = transformed_visual(&source);
        let context = egui::Context::default();
        let mut editor_context = EditorContext::new(uuid::Uuid::new_v4());
        editor_context.view.zoom = 1.0;
        editor_context.select_target(visual.owner_target);
        editor_context.interaction.preview_edit_target = Some(visual.edit_target());
        let (right_x, right_y) = visual.world_transform.map_point(20.0, 0.0);
        let right_handle = egui::pos2(right_x as f32, right_y as f32);
        let mut actions = Vec::new();
        raw_pointer_drag(
            &context,
            &mut editor_context,
            &visual,
            &mut actions,
            right_handle,
            right_handle + egui::vec2(0.0, 8.0),
            right_handle + egui::vec2(0.0, 28.0),
        );
        assert!(actions.iter().any(|action| matches!(
            action,
            PreviewAction::UpdateProperty { node_id, prop_name, .. }
                if *node_id == source.id && prop_name == "scale"
        )));
        assert!(actions
            .iter()
            .any(|action| matches!(action, PreviewAction::CommitHistory)));
        let edited = apply_actions(source, actions);
        assert_eq!(vector_property(&edited, "position"), (14.0, 11.0));
        assert_eq!(vector_property(&edited, "scale"), (117.5, 100.0));
    }

}
