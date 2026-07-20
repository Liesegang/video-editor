use crate::model::ui_types::GizmoHandle;
use crate::state::context::EditorContext;
use crate::state::context_types::SelectionTarget;
use crate::ui::panels::preview::{action::PreviewAction, clip::PreviewClip};
use egui::{CursorIcon, Pos2, Rect, Sense, Ui, Vec2};
use library::model::property::{PropertyValue, Vec2 as PropVec2};
use library::rendering::renderer::Affine2D;
use ordered_float::OrderedFloat;

pub fn handle_gizmo_interaction(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    gui_clips: &[PreviewClip],
    pointer_pos: Option<Pos2>,
    to_world: impl Fn(Pos2) -> Pos2,
    pending_actions: &mut Vec<PreviewAction>,
) -> bool {
    let Some(SelectionTarget::Node(selected_id)) = editor_context.selection.primary() else {
        editor_context.interaction.gizmo_state = None;
        return false;
    };
    let Some(state) = editor_context.interaction.gizmo_state.as_ref() else {
        return false;
    };
    let (
        start_mouse_pos,
        active_handle,
        orig_pos,
        orig_sx,
        orig_sy,
        orig_rot,
        visual_pos,
        visual_sx,
        visual_sy,
        visual_rot,
        base_w,
        base_h,
    ) = (
        state.start_mouse_pos,
        state.active_handle,
        state.original_position,
        state.original_scale_x,
        state.original_scale_y,
        state.original_rotation,
        state.original_visual_position,
        state.original_visual_scale_x,
        state.original_visual_scale_y,
        state.original_visual_rotation,
        state.original_width,
        state.original_height,
    );

    if ui.input(|input| input.pointer.any_released()) {
        editor_context.interaction.gizmo_state = None;
        pending_actions.push(PreviewAction::CommitHistory);
        return true;
    }

    let Some(mouse_pos) = pointer_pos else {
        return true;
    };
    let Some(visual) = crate::ui::panels::preview::clip::visual_for_selection(
        gui_clips,
        selected_id,
        editor_context
            .interaction
            .preview_selected_instance_path
            .as_deref(),
    ) else {
        return true;
    };

    let start_world = to_world(start_mouse_pos);
    let current_world = to_world(mouse_pos);
    let world_delta = current_world - start_world;
    let delta = inverse_map_vector(visual.parent_transform, world_delta).unwrap_or(world_delta);
    let modifiers = ui.input(|input| input.modifiers);
    let keep_aspect_ratio = modifiers.shift;
    let center_scale = modifiers.alt;

    let mut new_scale_x = orig_sx;
    let mut new_scale_y = orig_sy;
    let mut new_pos_x = orig_pos[0];
    let mut new_pos_y = orig_pos[1];
    let mut new_rotation = orig_rot;

    if active_handle == GizmoHandle::Rotation {
        let (center_x, center_y) = visual
            .parent_transform
            .map_point(f64::from(visual_pos[0]), f64::from(visual_pos[1]));
        let center = egui::pos2(center_x as f32, center_y as f32);
        let start = start_world - center;
        let current = current_world - center;
        new_rotation =
            orig_rot + (current.y.atan2(current.x) - start.y.atan2(start.x)).to_degrees();
    } else {
        let radians = visual_rot.to_radians();
        let (sin, cos) = radians.sin_cos();
        let dx = delta.x * cos + delta.y * sin;
        let dy = -delta.x * sin + delta.y * cos;
        let current_w = base_w * visual_sx / 100.0;
        let current_h = base_h * visual_sy / 100.0;
        let (sign_x, sign_y) = handle_sign(active_handle);
        let scale_factor = if center_scale { 2.0 } else { 1.0 };
        let raw_width_delta = dx * sign_x * scale_factor;
        let raw_height_delta = dy * sign_y * scale_factor;
        let mut next_w = current_w + raw_width_delta;
        let mut next_h = current_h + raw_height_delta;

        if keep_aspect_ratio {
            let ratio = if current_h.abs() > f32::EPSILON {
                current_w / current_h
            } else {
                1.0
            };
            if sign_x != 0.0 && sign_y != 0.0 {
                if raw_width_delta.abs() > raw_height_delta.abs() {
                    next_h = next_w / ratio;
                } else {
                    next_w = next_h * ratio;
                }
            } else if sign_x != 0.0 {
                next_h = next_w / ratio;
            } else if sign_y != 0.0 {
                next_w = next_h * ratio;
            }
        }

        let final_width_delta = next_w - current_w;
        let final_height_delta = next_h - current_h;
        if current_w.abs() > f32::EPSILON {
            new_scale_x = orig_sx * next_w / current_w;
        }
        if current_h.abs() > f32::EPSILON {
            new_scale_y = orig_sy * next_h / current_h;
        }
        if !center_scale {
            let shift = rotate_vec(
                egui::vec2(
                    sign_x * final_width_delta / 2.0,
                    sign_y * final_height_delta / 2.0,
                ),
                visual_rot,
            );
            new_pos_x += shift.x;
            new_pos_y += shift.y;
        }
    }

    let current_time = editor_context.timeline.current_time as f64;
    pending_actions.push(PreviewAction::UpdateProperty {
        node_id: selected_id,
        prop_name: "scale".to_string(),
        time: current_time,
        value: PropertyValue::Vec2(PropVec2 {
            x: OrderedFloat(new_scale_x as f64),
            y: OrderedFloat(new_scale_y as f64),
        }),
    });
    pending_actions.push(PreviewAction::UpdateProperty {
        node_id: selected_id,
        prop_name: "position".to_string(),
        time: current_time,
        value: PropertyValue::Vec2(PropVec2 {
            x: OrderedFloat(new_pos_x as f64),
            y: OrderedFloat(new_pos_y as f64),
        }),
    });
    pending_actions.push(PreviewAction::UpdateProperty {
        node_id: selected_id,
        prop_name: "rotation".to_string(),
        time: current_time,
        value: PropertyValue::Number(OrderedFloat(new_rotation as f64)),
    });
    true
}

pub fn draw_gizmo(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    gui_clips: &[PreviewClip],
    to_screen: impl Fn(Pos2) -> Pos2,
    interaction_enabled: bool,
) {
    let primary = editor_context.selection.primary();
    for target in editor_context.selection.targets() {
        let SelectionTarget::Node(selected_id) = *target else {
            continue;
        };
        if Some(*target) == primary {
            continue;
        }
        if let Some(visual) =
            crate::ui::panels::preview::clip::visual_for_selection(gui_clips, selected_id, None)
        {
            let _ = draw_clip_box(
                ui,
                visual,
                &to_screen,
                egui::Color32::from_rgb(0, 200, 255).linear_multiply(0.5),
                1.0,
            );
        }
    }

    let Some(SelectionTarget::Node(selected_id)) = primary else {
        return;
    };
    let Some(visual) = crate::ui::panels::preview::clip::visual_for_selection(
        gui_clips,
        selected_id,
        editor_context
            .interaction
            .preview_selected_instance_path
            .as_deref(),
    ) else {
        return;
    };

    let color = egui::Color32::from_rgb(0, 200, 255);
    let Some((corners, rotation, top)) = draw_clip_box(ui, visual, &to_screen, color, 2.0) else {
        return;
    };
    let rotation_distance = 10.0 / editor_context.view.zoom;
    let rotation_pos = top
        + egui::vec2(
            rotation.sin() * rotation_distance,
            -rotation.cos() * rotation_distance,
        );
    ui.painter()
        .line_segment([top, rotation_pos], egui::Stroke::new(2.0, color));
    ui.painter().circle_filled(rotation_pos, 5.0, color);

    let bottom = corners[2].lerp(corners[3], 0.5);
    let left = corners[0].lerp(corners[3], 0.5);
    let right = corners[1].lerp(corners[2], 0.5);
    let handles = [
        (corners[0], GizmoHandle::TopLeft, CursorIcon::ResizeNwSe),
        (corners[1], GizmoHandle::TopRight, CursorIcon::ResizeNeSw),
        (corners[3], GizmoHandle::BottomLeft, CursorIcon::ResizeNeSw),
        (corners[2], GizmoHandle::BottomRight, CursorIcon::ResizeNwSe),
        (top, GizmoHandle::Top, CursorIcon::ResizeVertical),
        (bottom, GizmoHandle::Bottom, CursorIcon::ResizeVertical),
        (left, GizmoHandle::Left, CursorIcon::ResizeHorizontal),
        (right, GizmoHandle::Right, CursorIcon::ResizeHorizontal),
        (rotation_pos, GizmoHandle::Rotation, CursorIcon::Grab),
    ];

    for (position, handle, cursor) in handles {
        ui.painter().circle_filled(position, 5.0, color);
        if !interaction_enabled {
            continue;
        }
        let response = ui.interact(
            Rect::from_center_size(position, Vec2::splat(15.0)),
            ui.id().with(handle),
            Sense::drag(),
        );
        if response.hovered() {
            ui.ctx().set_cursor_icon(cursor);
        }
        if response.drag_started() {
            let Some((_, _, width, height)) = visual.content_bounds else {
                continue;
            };
            editor_context.interaction.gizmo_state =
                Some(crate::state::context_types::GizmoState {
                    start_mouse_pos: response.hover_pos().unwrap_or(position),
                    active_handle: handle,
                    original_position: [
                        visual.source_transform.position.x as f32,
                        visual.source_transform.position.y as f32,
                    ],
                    original_scale_x: visual.source_transform.scale.x as f32 * 100.0,
                    original_scale_y: visual.source_transform.scale.y as f32 * 100.0,
                    original_rotation: visual.source_transform.rotation as f32,
                    original_visual_position: [
                        visual.transform.position.x as f32,
                        visual.transform.position.y as f32,
                    ],
                    original_visual_scale_x: visual.transform.scale.x as f32 * 100.0,
                    original_visual_scale_y: visual.transform.scale.y as f32 * 100.0,
                    original_visual_rotation: visual.transform.rotation as f32,
                    original_anchor_x: visual.source_transform.anchor.x as f32,
                    original_anchor_y: visual.source_transform.anchor.y as f32,
                    original_width: width,
                    original_height: height,
                });
        }
    }
}

fn draw_clip_box(
    ui: &Ui,
    visual: &PreviewClip,
    to_screen: impl Fn(Pos2) -> Pos2,
    color: egui::Color32,
    thickness: f32,
) -> Option<([Pos2; 4], f32, Pos2)> {
    let (x, y, width, height) = visual.content_bounds?;
    let point = |local_x: f32, local_y: f32| {
        let (world_x, world_y) = visual
            .world_transform
            .map_point(f64::from(local_x), f64::from(local_y));
        to_screen(egui::pos2(world_x as f32, world_y as f32))
    };
    let corners = [
        point(x, y),
        point(x + width, y),
        point(x + width, y + height),
        point(x, y + height),
    ];
    let stroke = egui::Stroke::new(thickness, color);
    for index in 0..corners.len() {
        ui.painter().line_segment(
            [corners[index], corners[(index + 1) % corners.len()]],
            stroke,
        );
    }
    let top = corners[0].lerp(corners[1], 0.5);
    let top_edge = corners[1] - corners[0];
    Some((corners, top_edge.y.atan2(top_edge.x), top))
}

fn inverse_map_vector(transform: Affine2D, vector: Vec2) -> Option<Vec2> {
    let determinant = transform.scale_x * transform.scale_y - transform.skew_x * transform.skew_y;
    if determinant.abs() <= f64::EPSILON {
        return None;
    }
    Some(egui::vec2(
        ((transform.scale_y * f64::from(vector.x) - transform.skew_x * f64::from(vector.y))
            / determinant) as f32,
        ((-transform.skew_y * f64::from(vector.x) + transform.scale_x * f64::from(vector.y))
            / determinant) as f32,
    ))
}

fn rotate_vec(vector: Vec2, angle_degrees: f32) -> Vec2 {
    let (sin, cos) = angle_degrees.to_radians().sin_cos();
    egui::vec2(
        vector.x * cos - vector.y * sin,
        vector.x * sin + vector.y * cos,
    )
}

fn handle_sign(handle: GizmoHandle) -> (f32, f32) {
    match handle {
        GizmoHandle::TopLeft => (-1.0, -1.0),
        GizmoHandle::Top => (0.0, -1.0),
        GizmoHandle::TopRight => (1.0, -1.0),
        GizmoHandle::Left => (-1.0, 0.0),
        GizmoHandle::Right => (1.0, 0.0),
        GizmoHandle::BottomLeft => (-1.0, 1.0),
        GizmoHandle::Bottom => (0.0, 1.0),
        GizmoHandle::BottomRight => (1.0, 1.0),
        GizmoHandle::Rotation => (0.0, 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::{handle_gizmo_interaction, PreviewClip};
    use crate::model::ui_types::GizmoHandle;
    use crate::state::context::EditorContext;
    use crate::state::context_types::{GizmoState, SelectionTarget};
    use library::model::frame::transform::Transform;
    use library::model::Node;
    use library::rendering::renderer::Affine2D;
    use uuid::Uuid;

    #[test]
    fn clip_target_with_same_uuid_cannot_drive_node_gizmo() {
        let shared_id = Uuid::new_v4();
        let mut node = Node::new_merge("same UUID visual");
        node.id = shared_id;
        let visual = PreviewClip {
            node,
            source_transform: Transform::default(),
            transform: Transform::default(),
            parent_transform: Affine2D::IDENTITY,
            world_transform: Affine2D::IDENTITY,
            content_bounds: Some((0.0, 0.0, 100.0, 100.0)),
            instance_path: vec![shared_id],
        };
        let mut editor_context = EditorContext::new(Uuid::new_v4());
        editor_context.select_target(SelectionTarget::Clip(shared_id));
        editor_context.interaction.gizmo_state = Some(GizmoState {
            start_mouse_pos: egui::pos2(10.0, 10.0),
            active_handle: GizmoHandle::Right,
            original_position: [0.0, 0.0],
            original_scale_x: 100.0,
            original_scale_y: 100.0,
            original_rotation: 0.0,
            original_visual_position: [0.0, 0.0],
            original_visual_scale_x: 100.0,
            original_visual_scale_y: 100.0,
            original_visual_rotation: 0.0,
            original_anchor_x: 0.0,
            original_anchor_y: 0.0,
            original_width: 100.0,
            original_height: 100.0,
        });

        let context = egui::Context::default();
        let mut pending_actions = Vec::new();
        let mut handled = true;
        drop(context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                handled = handle_gizmo_interaction(
                    ui,
                    &mut editor_context,
                    std::slice::from_ref(&visual),
                    Some(egui::pos2(20.0, 10.0)),
                    |position| position,
                    &mut pending_actions,
                );
            });
        }));

        assert!(!handled);
        assert!(pending_actions.is_empty());
        assert!(editor_context.interaction.gizmo_state.is_none());
        assert_eq!(
            editor_context.selection.primary(),
            Some(SelectionTarget::Clip(shared_id))
        );
    }
}
