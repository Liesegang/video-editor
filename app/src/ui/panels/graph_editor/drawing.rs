use crate::state::context::EditorContext;
use crate::state::context_types::{GraphKeyframeDragOrigin, GraphKeyframeDragState};
use egui::{Color32, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, UiKind, Vec2};
use library::model::property::{Property, PropertyMap, PropertyValue};
use library::EditorService;

use super::actions::{Action, KeyframeMove};
use super::utils::{GraphTransform, PropertyComponent, TimeMapper};

pub fn draw_background(
    painter: &Painter,
    transform: &GraphTransform,
    ruler_rect: Rect,
    valid_range: Option<(f64, f64)>,
) {
    let graph_rect = transform.graph_rect;
    painter.rect_filled(graph_rect, 0.0, Color32::from_gray(45));

    if let Some((start_t, end_t)) = valid_range {
        let start_x = transform.to_screen(start_t, 0.0).x;
        let end_x = transform.to_screen(end_t, 0.0).x;

        let highlight_rect = Rect::from_min_max(
            Pos2::new(start_x.max(graph_rect.min.x), graph_rect.min.y),
            Pos2::new(end_x.min(graph_rect.max.x), graph_rect.max.y),
        );

        if highlight_rect.is_positive() {
            painter.rect_filled(highlight_rect, 0.0, Color32::from_gray(25));
        }
    }

    painter.rect_filled(ruler_rect, 0.0, Color32::from_gray(40));
    painter.line_segment(
        [ruler_rect.left_bottom(), ruler_rect.right_bottom()],
        Stroke::new(1.0, Color32::BLACK),
    );
}

pub fn draw_grid(painter: &Painter, transform: &GraphTransform, ruler_rect: Rect) {
    let graph_rect = transform.graph_rect;
    let pixels_per_second = transform.zoom_x;
    let pixels_per_unit = transform.zoom_y;

    // Time Grid & Ruler
    let start_time = (-transform.pan.x / pixels_per_second) as f64;
    let end_time = ((graph_rect.width() - transform.pan.x) / pixels_per_second) as f64;

    // Adaptive step size
    let min_step_px = 50.0;
    let step_time = (min_step_px / pixels_per_second).max(0.01);
    let step_power = step_time.log10().floor();
    let step_base = 10.0f32.powf(step_power);
    let step_time = if step_time / step_base < 2.0 {
        step_base
    } else if step_time / step_base < 5.0 {
        step_base * 2.0
    } else {
        step_base * 5.0
    };

    let start_step = (start_time / step_time as f64).floor() as i64;
    let end_step = (end_time / step_time as f64).ceil() as i64;

    for i in start_step..=end_step {
        let t = i as f64 * step_time as f64;
        let x = graph_rect.min.x + transform.pan.x + (t as f32 * pixels_per_second);

        if x >= graph_rect.min.x && x <= graph_rect.max.x {
            // Main Vertical Line
            painter.line_segment(
                [
                    Pos2::new(x, graph_rect.min.y),
                    Pos2::new(x, graph_rect.max.y),
                ],
                Stroke::new(1.0, Color32::from_gray(40)),
            );

            // Ruler Tick & Label
            painter.line_segment(
                [
                    Pos2::new(x, ruler_rect.max.y),
                    Pos2::new(x, ruler_rect.max.y - 10.0),
                ],
                Stroke::new(1.0, Color32::GRAY),
            );
            painter.text(
                Pos2::new(x + 2.0, ruler_rect.min.y + 2.0),
                egui::Align2::LEFT_TOP,
                format!("{:.2}", t),
                egui::FontId::proportional(10.0),
                Color32::GRAY,
            );
        }
    }

    // Value Grid & Left Axis
    let zero_y = graph_rect.center().y + transform.pan.y;
    let min_val = (zero_y - graph_rect.max.y) / pixels_per_unit;
    let max_val = (zero_y - graph_rect.min.y) / pixels_per_unit;

    let v_step_val = (30.0 / pixels_per_unit).max(0.01);
    let v_step_power = v_step_val.log10().floor();
    let v_step_base = 10.0f32.powf(v_step_power);
    let v_step_val = if v_step_val / v_step_base < 2.0 {
        v_step_base
    } else if v_step_val / v_step_base < 5.0 {
        v_step_base * 2.0
    } else {
        v_step_base * 5.0
    };

    let start_v = (min_val / v_step_val).floor() as i64;
    let end_v = (max_val / v_step_val).ceil() as i64;

    for i in start_v..=end_v {
        let val = i as f32 * v_step_val;
        let y = zero_y - (val * pixels_per_unit);

        if y >= graph_rect.min.y && y <= graph_rect.max.y {
            let color = if i == 0 {
                Color32::from_gray(80)
            } else {
                Color32::from_gray(40)
            };
            painter.line_segment(
                [
                    Pos2::new(graph_rect.min.x, y),
                    Pos2::new(graph_rect.max.x, y),
                ],
                Stroke::new(1.0, color),
            );
            painter.text(
                Pos2::new(graph_rect.min.x + 2.0, y - 2.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{:.2}", val),
                egui::FontId::proportional(10.0),
                Color32::from_gray(150),
            );
        }
    }
}

pub fn draw_playhead(
    painter: &Painter,
    transform: &GraphTransform,
    ruler_rect: Rect,
    current_time: f64,
) {
    let graph_rect = transform.graph_rect;
    let t_cursor = current_time;
    let x_cursor = graph_rect.min.x + transform.pan.x + (t_cursor as f32 * transform.zoom_x);

    if x_cursor >= graph_rect.min.x && x_cursor <= graph_rect.max.x {
        painter.line_segment(
            [
                Pos2::new(x_cursor, graph_rect.min.y),
                Pos2::new(x_cursor, graph_rect.max.y),
            ],
            Stroke::new(2.0, Color32::RED),
        );
    }

    if x_cursor >= ruler_rect.min.x && x_cursor <= ruler_rect.max.x {
        // Triangle head
        let head_size = 6.0;
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(x_cursor, ruler_rect.max.y),
                Pos2::new(x_cursor - head_size, ruler_rect.max.y - head_size),
                Pos2::new(x_cursor + head_size, ruler_rect.max.y - head_size),
            ],
            Color32::RED,
            Stroke::NONE,
        ));
    }
}

fn keyframe_component_value(value: &PropertyValue, component: PropertyComponent) -> Option<f64> {
    match component {
        PropertyComponent::Scalar => value.get_as::<f64>(),
        PropertyComponent::X => match value {
            PropertyValue::Vec2(vector) => Some(vector.x.into_inner()),
            _ => None,
        },
        PropertyComponent::Y => match value {
            PropertyValue::Vec2(vector) => Some(vector.y.into_inner()),
            _ => None,
        },
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "immediate-mode graph rendering requires the frame UI, coordinate transforms, editable model context, and deferred action outputs together"
)]
pub fn draw_properties(
    ui: &mut Ui,
    painter: &Painter,
    response: &Response,
    transform: &GraphTransform,
    time_mapper: &TimeMapper,
    properties: &[(String, &Property, &PropertyMap, PropertyComponent)],
    entity_id: uuid::Uuid,
    editor_context: &mut EditorContext,
    project_service: &EditorService,
    actions: &mut Vec<Action>,
    composition_fps: f64,
    composition_resolution: (u64, u64),
) {
    let graph_rect = transform.graph_rect;
    let pixels_per_second = transform.zoom_x;

    let mut color_cycle = [
        Color32::RED,
        Color32::GREEN,
        Color32::BLUE,
        Color32::YELLOW,
        Color32::CYAN,
        Color32::MAGENTA,
        Color32::ORANGE,
    ]
    .iter()
    .cycle();

    let mut available_drag_origins = Vec::new();
    for (name, property, _, component) in properties {
        if property.evaluator != "keyframe" {
            continue;
        }
        for keyframe in property.keyframes() {
            let value = keyframe_component_value(&keyframe.value, *component).unwrap_or(0.0);
            available_drag_origins.push(GraphKeyframeDragOrigin {
                property_name: name.clone(),
                keyframe_id: keyframe.id,
                global_time: time_mapper.to_global_time(keyframe.time.into_inner()),
                value,
            });
        }
    }

    for (name, property, map, component_ref) in properties {
        let component = *component_ref;
        let color = color_cycle.next().copied().unwrap_or(Color32::WHITE);

        if !editor_context
            .graph_editor
            .visible_properties
            .contains::<String>(name)
        {
            continue;
        }

        match property.evaluator.as_str() {
            "constant" => {
                let maybe_val = match component {
                    PropertyComponent::Scalar => property.value().and_then(|v| v.get_as::<f64>()),
                    PropertyComponent::X => property.value().and_then(|v| {
                        if let PropertyValue::Vec2(vec) = v {
                            Some(vec.x.into_inner())
                        } else {
                            None
                        }
                    }),
                    PropertyComponent::Y => property.value().and_then(|v| {
                        if let PropertyValue::Vec2(vec) = v {
                            Some(vec.y.into_inner())
                        } else {
                            None
                        }
                    }),
                };
                if let Some(val) = maybe_val {
                    let y = transform.to_screen(0.0, val).y;
                    if y >= graph_rect.min.y && y <= graph_rect.max.y {
                        painter.line_segment(
                            [
                                Pos2::new(graph_rect.min.x, y),
                                Pos2::new(graph_rect.max.x, y),
                            ],
                            Stroke::new(2.0, color),
                        );
                        painter.text(
                            Pos2::new(graph_rect.min.x + 40.0, y - 5.0),
                            egui::Align2::LEFT_BOTTOM,
                            format!("{}: {:.2}", name, val),
                            egui::FontId::default(),
                            color,
                        );

                        // Double Click to add keyframe logic
                        if response.double_clicked() {
                            if let Some(pointer_pos) = response.interact_pointer_pos() {
                                if (pointer_pos.y - y).abs() < 5.0
                                    && graph_rect.contains(pointer_pos)
                                {
                                    let (t, _) = transform.screen_to_graph(pointer_pos);
                                    if actions.is_empty() {
                                        actions.push(Action::Add(name.clone(), t.max(0.0), val));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "keyframe" | "expression" => {
                // 1. Draw Curve via Sampling
                let mut path_points = Vec::new();
                let step_px = 2.0f32; // Sample every 2 pixels

                let start_x = graph_rect.min.x;
                let end_x = graph_rect.max.x;
                let steps = ((end_x - start_x) / step_px).ceil() as usize;

                for s in 0..=steps {
                    let x = start_x + s as f32 * step_px;
                    let global_time = (x - graph_rect.min.x - transform.pan.x) / pixels_per_second;

                    let eval_time = time_mapper.to_source_time(global_time as f64);

                    let Ok(value_pv) = project_service.evaluate_property_value(
                        property,
                        map,
                        eval_time,
                        composition_fps,
                        composition_resolution,
                    ) else {
                        continue;
                    };
                    // Match *component here too
                    let val_f64 = match component {
                        PropertyComponent::Scalar => value_pv.get_as::<f64>(),
                        PropertyComponent::X => {
                            if let PropertyValue::Vec2(vec) = value_pv {
                                Some(vec.x.into_inner())
                            } else {
                                None
                            }
                        }
                        PropertyComponent::Y => {
                            if let PropertyValue::Vec2(vec) = value_pv {
                                Some(vec.y.into_inner())
                            } else {
                                None
                            }
                        }
                    };

                    if let Some(val) = val_f64 {
                        let pos = transform.to_screen(global_time as f64, val);
                        path_points.push(pos);
                    }
                }

                if crate::qa::is_enabled() && property.evaluator == "keyframe" {
                    let keyframe_points = property
                        .keyframes()
                        .into_iter()
                        .filter_map(|keyframe| {
                            keyframe_component_value(&keyframe.value, component).map(|value| {
                                transform.to_screen(
                                    time_mapper.to_global_time(keyframe.time.into_inner()),
                                    value,
                                )
                            })
                        })
                        .collect::<Vec<_>>();
                    let curve_hit = path_points
                        .iter()
                        .copied()
                        .filter(|point| {
                            graph_rect.shrink(8.0).contains(*point)
                                && keyframe_points
                                    .iter()
                                    .all(|keyframe| keyframe.distance(*point) >= 24.0)
                        })
                        .min_by(|left, right| {
                            (left.x - graph_rect.center().x)
                                .abs()
                                .total_cmp(&(right.x - graph_rect.center().x).abs())
                        });
                    if let Some(curve_hit) = curve_hit {
                        let (global_time, value) = transform.screen_to_graph(curve_hit);
                        crate::qa::register_component_with_metadata(
                            format!("graph.curve_hit.{name}"),
                            "graph_curve_hit",
                            Rect::from_center_size(curve_hit, Vec2::splat(12.0)),
                            true,
                            Some(serde_json::json!({
                                "property": name,
                                "component": format!("{component:?}"),
                                "entity_id": entity_id,
                                "global_time": global_time,
                                "source_time": time_mapper.to_source_time(global_time),
                                "value": value,
                                "evaluator": property.evaluator,
                            })),
                        );
                    }
                }

                if path_points.len() > 1 {
                    painter.add(egui::Shape::line(path_points, Stroke::new(2.0, color)));
                }

                // 2. Draw Keyframe Dots (Overlay) if it is a keyframe property
                if property.evaluator == "keyframe" {
                    let keyframes = property.keyframes();
                    let mut sorted_kf = keyframes;
                    sorted_kf.sort_by_key(|keyframe| keyframe.time);

                    for (i, kf) in sorted_kf.iter().enumerate() {
                        let t = kf.time.into_inner();
                        let val_f64 = keyframe_component_value(&kf.value, component);
                        let val = val_f64.unwrap_or(0.0);
                        let global_t = time_mapper.to_global_time(t);
                        let kf_pos = transform.to_screen(global_t, val);

                        // Skip if out of view (optimization)
                        if !graph_rect.expand(10.0).contains(kf_pos) {
                            continue;
                        }

                        // Interaction area
                        let point_rect = Rect::from_center_size(kf_pos, Vec2::splat(12.0));
                        let point_id = response.id.with(name).with(kf.id);
                        let point_response =
                            ui.interact(point_rect, point_id, Sense::click_and_drag());

                        let selection = (name.clone(), kf.id);
                        let is_selected = editor_context
                            .graph_editor
                            .selected_keyframes
                            .contains(&selection);

                        crate::qa::register_component_with_metadata(
                            format!("graph.keyframe.{name}:{}", kf.id),
                            "graph_keyframe",
                            point_rect,
                            true,
                            Some(serde_json::json!({
                                "property": name,
                                "component": format!("{component:?}"),
                                "keyframe_id": kf.id.to_string(),
                                "source_time": t,
                                "global_time": global_t,
                                "value": val,
                                "selected": is_selected,
                                "entity_id": entity_id,
                            })),
                        );

                        // Draw Dot
                        let dot_color = if is_selected { Color32::WHITE } else { color };
                        let radius = if is_selected { 6.0 } else { 4.0 };
                        painter.circle_filled(kf_pos, radius, dot_color);

                        let additive_selection = ui.input(|input| {
                            input.modifiers.command || input.modifiers.ctrl || input.modifiers.shift
                        });
                        if point_response.clicked() {
                            if additive_selection {
                                if !editor_context
                                    .graph_editor
                                    .selected_keyframes
                                    .remove(&selection)
                                {
                                    editor_context
                                        .graph_editor
                                        .selected_keyframes
                                        .insert(selection.clone());
                                }
                            } else {
                                editor_context.graph_editor.selected_keyframes.clear();
                                editor_context
                                    .graph_editor
                                    .selected_keyframes
                                    .insert(selection.clone());
                            }
                            if editor_context
                                .graph_editor
                                .selected_keyframes
                                .contains(&selection)
                            {
                                actions.push(Action::Select(name.clone(), kf.id));
                            }
                        }

                        if point_response.drag_started() {
                            if !is_selected {
                                if !additive_selection {
                                    editor_context.graph_editor.selected_keyframes.clear();
                                }
                                editor_context
                                    .graph_editor
                                    .selected_keyframes
                                    .insert(selection.clone());
                            }
                            let origins = available_drag_origins
                                .iter()
                                .filter(|origin| {
                                    editor_context.graph_editor.selected_keyframes.contains(&(
                                        origin.property_name.clone(),
                                        origin.keyframe_id,
                                    ))
                                })
                                .cloned()
                                .collect();
                            editor_context.graph_editor.keyframe_drag =
                                Some(GraphKeyframeDragState {
                                    entity_id,
                                    anchor: selection.clone(),
                                    origins,
                                    changed: false,
                                });
                            actions.push(Action::Select(name.clone(), kf.id));
                        }

                        // Context Menu
                        let name_for_menu = name.clone();
                        point_response.context_menu(|ui| {
                            ui.label(format!("Keyframe {} - {}", i, name_for_menu));
                            ui.separator();
                            let mut chosen_easing = None;
                            let keyframe_id = kf.id.to_string();
                            crate::ui::easing_menus::show_easing_menu(
                                ui,
                                Some(&kf.easing),
                                Some(crate::ui::easing_menus::EasingMenuQaScope::new(
                                    "graph.keyframe_menu.easing",
                                    &keyframe_id,
                                )),
                                |easing| {
                                    chosen_easing = Some(easing);
                                },
                            );

                            if let Some(easing) = chosen_easing {
                                actions.push(Action::SetEasing(
                                    name_for_menu.clone(),
                                    kf.id,
                                    easing,
                                ));
                                ui.close_kind(UiKind::Menu);
                            }

                            ui.separator();
                            let edit = ui.button("Edit Keyframe...");
                            crate::qa::register_component_with_metadata(
                                format!("graph.keyframe_menu.edit:{}", kf.id),
                                "graph_keyframe_menu_item",
                                edit.rect,
                                edit.enabled(),
                                Some(serde_json::json!({
                                    "property": name_for_menu,
                                    "keyframe_id": kf.id,
                                })),
                            );
                            if edit.clicked() {
                                actions.push(Action::EditKeyframe(name_for_menu.clone(), kf.id));
                                ui.close_kind(UiKind::Menu);
                            }

                            ui.separator();
                            let delete = ui
                                .button(egui::RichText::new("Delete Keyframe").color(Color32::RED));
                            crate::qa::register_component_with_metadata(
                                format!("graph.keyframe_menu.delete:{}", kf.id),
                                "graph_keyframe_menu_item",
                                delete.rect,
                                delete.enabled(),
                                Some(serde_json::json!({
                                    "property": name_for_menu,
                                    "keyframe_id": kf.id,
                                })),
                            );
                            if delete.clicked() {
                                actions.push(Action::Remove(name_for_menu.clone(), kf.id));
                                ui.close_kind(UiKind::Menu);
                            }
                        });

                        // Dragging
                        let is_drag_anchor = editor_context
                            .graph_editor
                            .keyframe_drag
                            .as_ref()
                            .is_some_and(|drag| drag.anchor == selection);
                        if is_drag_anchor && point_response.dragged() {
                            // Origins are a gesture-start snapshot, so apply
                            // the gesture's total displacement. `drag_delta`
                            // is only the latest frame's pointer movement.
                            let delta = point_response.total_drag_delta().unwrap_or_default();
                            let time_delta = f64::from(delta.x / transform.zoom_x);
                            let value_delta = f64::from(-delta.y / transform.zoom_y);
                            let moves = editor_context
                                .graph_editor
                                .keyframe_drag
                                .as_ref()
                                .map(|drag| {
                                    drag.origins
                                        .iter()
                                        .map(|origin| KeyframeMove {
                                            property_name: origin.property_name.clone(),
                                            keyframe_id: origin.keyframe_id,
                                            global_time: (origin.global_time + time_delta).max(0.0),
                                            value: origin.value + value_delta,
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            if !moves.is_empty() {
                                actions.push(Action::MoveBatch(moves));
                            }
                        }
                        if is_drag_anchor && point_response.drag_stopped() {
                            actions.push(Action::FinishMove);
                        }
                    }

                    // Add Keyframe (Double Click) logic constraint
                    if response.double_clicked() {
                        if let Some(pointer_pos) = response.interact_pointer_pos() {
                            if graph_rect.contains(pointer_pos) {
                                let (t, _) = transform.screen_to_graph(pointer_pos);

                                // Evaluate at pointer time
                                let Ok(value_pv) = project_service.evaluate_property_value(
                                    property,
                                    map,
                                    time_mapper.to_source_time(t),
                                    composition_fps,
                                    composition_resolution,
                                ) else {
                                    continue;
                                };
                                let val_at_t = match component {
                                    PropertyComponent::Scalar => {
                                        value_pv.get_as::<f64>().unwrap_or(0.0)
                                    }
                                    PropertyComponent::X => {
                                        if let PropertyValue::Vec2(vec) = value_pv {
                                            vec.x.into_inner()
                                        } else {
                                            0.0
                                        }
                                    }
                                    PropertyComponent::Y => {
                                        if let PropertyValue::Vec2(vec) = value_pv {
                                            vec.y.into_inner()
                                        } else {
                                            0.0
                                        }
                                    }
                                };
                                let curve_pos = transform.to_screen(t, val_at_t);

                                // Distance check
                                if actions.is_empty() && (pointer_pos.y - curve_pos.y).abs() < 10.0
                                {
                                    actions.push(Action::Add(name.clone(), t.max(0.0), val_at_t));
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
