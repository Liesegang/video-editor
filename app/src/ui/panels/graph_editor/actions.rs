use super::PropertyComponent;
use crate::action::HistoryManager;
use crate::state::context::EditorContext;
use library::animation::EasingFunction;
use library::model::project::project::Project;
use library::model::project::property::PropertyValue;
use library::EditorService;
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub enum Action {
    Select(String, usize),
    Move(String, usize, f64, f64, Option<PropertyComponent>), // prop_key, index, new_time, new_value, component
    Add(String, f64, f64, Option<PropertyComponent>),         // prop_key, time, value, component
    SetEasing(String, usize, EasingFunction),
    Remove(String, usize),
    EditKeyframe(String, usize),
    None,
}

fn parse_key(key: &str) -> Option<(usize, String)> {
    if key.starts_with("effect:") {
        let parts: Vec<&str> = key.splitn(3, ':').collect();
        if parts.len() == 3 {
            if let Ok(idx) = parts[1].parse::<usize>() {
                return Some((idx, parts[2].to_string()));
            }
        }
    }
    None
}

fn parse_style_key(key: &str) -> Option<(usize, String)> {
    if key.starts_with("style:") {
        let parts: Vec<&str> = key.splitn(3, ':').collect();
        if parts.len() == 3 {
            if let Ok(idx) = parts[1].parse::<usize>() {
                return Some((idx, parts[2].to_string()));
            }
        }
    }
    None
}

/// Helper to calculate source-local time from global time using flat clip lookup
fn global_to_source_time(
    project: &Project,
    comp_id: Uuid,
    entity_id: Uuid,
    global_time: f64,
) -> f64 {
    if let Some(comp) = project.get_composition(comp_id) {
        if let Some(clip) = project.get_clip(entity_id) {
            let in_time = clip.in_frame as f64 / comp.fps;
            let source_start = clip.source_begin_frame as f64 / clip.fps;
            return source_start + (global_time - in_time);
        }
    }
    global_time
}

pub fn process_action(
    action: Action,
    comp_id: Uuid,
    track_id: Uuid,
    entity_id: Uuid,
    project_service: &EditorService,
    project: &Arc<RwLock<Project>>,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
) {
    match action {
        Action::Select(name, idx) => {
            editor_context.interaction.graph_editor.selected_keyframe = Some((name, idx));
        }

        Action::Move(name, idx, new_time, new_val, component) => {
            let base_name = if let Some(c) = component {
                match c {
                    PropertyComponent::X => name.trim_end_matches(".x"),
                    PropertyComponent::Y => name.trim_end_matches(".y"),
                    _ => name.as_str(),
                }
            } else {
                name.as_str()
            };

            if let Some((eff_idx, prop_key)) = parse_key(base_name) {
                // Effect property - use flat lookup
                let mut current_pv = None;
                if let Ok(proj) = project.read() {
                    if let Some(clip) = proj.get_clip(entity_id) {
                        if let Some(effect) = clip.effects.get(eff_idx) {
                            if let Some(prop) = effect.properties.get(&prop_key) {
                                let keyframes = prop.keyframes();
                                let mut sorted_kf = keyframes.clone();
                                sorted_kf.sort_by(|a, b| a.time.cmp(&b.time));
                                if let Some(kf) = sorted_kf.get(idx) {
                                    current_pv = Some(kf.value.clone());
                                }
                            }
                        }
                    }
                }

                let new_pv = if let Some(PropertyValue::Vec2(old_vec)) = current_pv {
                    match component {
                        Some(PropertyComponent::X) => {
                            PropertyValue::Vec2(library::model::project::property::Vec2 {
                                x: OrderedFloat(new_val),
                                y: old_vec.y,
                            })
                        }
                        Some(PropertyComponent::Y) => {
                            PropertyValue::Vec2(library::model::project::property::Vec2 {
                                x: old_vec.x,
                                y: OrderedFloat(new_val),
                            })
                        }
                        _ => PropertyValue::Number(OrderedFloat(new_val)),
                    }
                } else {
                    PropertyValue::Number(OrderedFloat(new_val))
                };

                // Convert time using helper
                let source_time = if let Ok(proj) = project.read() {
                    global_to_source_time(&proj, comp_id, entity_id, new_time)
                } else {
                    new_time
                };

                let _ = project_service.update_target_keyframe_by_index(
                    entity_id,
                    library::model::project::property::PropertyTarget::Effect(eff_idx),
                    &prop_key,
                    idx,
                    Some(source_time),
                    Some(new_pv),
                    None,
                );
            } else if let Some((style_idx, prop_key)) = parse_style_key(base_name) {
                // Style property - use flat lookup
                let mut current_pv = None;
                if let Ok(proj) = project.read() {
                    if let Some(clip) = proj.get_clip(entity_id) {
                        if let Some(style) = clip.styles.get(style_idx) {
                            if let Some(prop) = style.properties.get(&prop_key) {
                                let keyframes = prop.keyframes();
                                let mut sorted_kf = keyframes.clone();
                                sorted_kf.sort_by(|a, b| a.time.cmp(&b.time));
                                if let Some(kf) = sorted_kf.get(idx) {
                                    current_pv = Some(kf.value.clone());
                                }
                            }
                        }
                    }
                }

                let new_pv = if let Some(PropertyValue::Vec2(old_vec)) = current_pv {
                    match component {
                        Some(PropertyComponent::X) => {
                            PropertyValue::Vec2(library::model::project::property::Vec2 {
                                x: OrderedFloat(new_val),
                                y: old_vec.y,
                            })
                        }
                        Some(PropertyComponent::Y) => {
                            PropertyValue::Vec2(library::model::project::property::Vec2 {
                                x: old_vec.x,
                                y: OrderedFloat(new_val),
                            })
                        }
                        _ => PropertyValue::Number(OrderedFloat(new_val)),
                    }
                } else {
                    PropertyValue::Number(OrderedFloat(new_val))
                };

                let source_time = if let Ok(proj) = project.read() {
                    global_to_source_time(&proj, comp_id, entity_id, new_time)
                } else {
                    new_time
                };

                let _ = project_service.update_target_keyframe_by_index(
                    entity_id,
                    library::model::project::property::PropertyTarget::Style(style_idx),
                    &prop_key,
                    idx,
                    Some(source_time),
                    Some(new_pv),
                    None,
                );
            } else {
                // Clip property - use flat lookup
                let mut current_pv = None;
                if let Ok(proj) = project.read() {
                    if let Some(clip) = proj.get_clip(entity_id) {
                        if let Some(prop) = clip.properties.get(base_name) {
                            let keyframes = prop.keyframes();
                            let mut sorted_kf = keyframes.clone();
                            sorted_kf.sort_by(|a, b| a.time.cmp(&b.time));
                            if let Some(kf) = sorted_kf.get(idx) {
                                current_pv = Some(kf.value.clone());
                            }
                        }
                    }
                }

                let new_pv = if let Some(PropertyValue::Vec2(old_vec)) = current_pv {
                    match component {
                        Some(PropertyComponent::X) => {
                            PropertyValue::Vec2(library::model::project::property::Vec2 {
                                x: OrderedFloat(new_val),
                                y: old_vec.y,
                            })
                        }
                        Some(PropertyComponent::Y) => {
                            PropertyValue::Vec2(library::model::project::property::Vec2 {
                                x: old_vec.x,
                                y: OrderedFloat(new_val),
                            })
                        }
                        _ => PropertyValue::Number(OrderedFloat(new_val)),
                    }
                } else {
                    PropertyValue::Number(OrderedFloat(new_val))
                };

                let source_time = if let Ok(proj) = project.read() {
                    global_to_source_time(&proj, comp_id, entity_id, new_time)
                } else {
                    new_time
                };

                let _ = project_service.update_target_keyframe_by_index(
                    entity_id,
                    library::model::project::property::PropertyTarget::Clip,
                    base_name,
                    idx,
                    Some(source_time),
                    Some(new_pv),
                    None,
                );
            }
            if let Ok(proj_read) = project.read() {
                history_manager.push_project_state(proj_read.clone());
            }
        }
        Action::Add(name, time, val, component) => {
            let base_name = if let Some(c) = component {
                match c {
                    PropertyComponent::X => name.trim_end_matches(".x"),
                    PropertyComponent::Y => name.trim_end_matches(".y"),
                    _ => name.as_str(),
                }
            } else {
                name.as_str()
            };

            let mut current_val_at_t = None;
            let mut eval_time = time;

            if let Ok(proj) = project.read() {
                if let Some(comp) = proj.get_composition(comp_id) {
                    if let Some(entity) = proj.get_clip(entity_id) {
                        // Calculate source time from global time
                        eval_time = global_to_source_time(&proj, comp_id, entity_id, time);

                        if let Some((eff_idx, prop_key)) = parse_key(base_name) {
                            if let Some(effect) = entity.effects.get(eff_idx) {
                                if let Some(prop) = effect.properties.get(&prop_key) {
                                    current_val_at_t =
                                        Some(project_service.evaluate_property_value(
                                            prop,
                                            &effect.properties,
                                            eval_time,
                                            comp.fps,
                                        ));
                                }
                            }
                        } else if let Some((style_idx, prop_key)) = parse_style_key(base_name) {
                            if let Some(style) = entity.styles.get(style_idx) {
                                if let Some(prop) = style.properties.get(&prop_key) {
                                    current_val_at_t =
                                        Some(project_service.evaluate_property_value(
                                            prop,
                                            &style.properties,
                                            eval_time,
                                            comp.fps,
                                        ));
                                }
                            }
                        } else {
                            if let Some(prop) = entity.properties.get(base_name) {
                                current_val_at_t = Some(project_service.evaluate_property_value(
                                    prop,
                                    &entity.properties,
                                    eval_time,
                                    comp.fps,
                                ));
                            }
                        }
                    }
                }
            }

            let new_pv = if let Some(PropertyValue::Vec2(old_vec)) = current_val_at_t {
                match component {
                    Some(PropertyComponent::X) => {
                        PropertyValue::Vec2(library::model::project::property::Vec2 {
                            x: OrderedFloat(val),
                            y: old_vec.y,
                        })
                    }
                    Some(PropertyComponent::Y) => {
                        PropertyValue::Vec2(library::model::project::property::Vec2 {
                            x: old_vec.x,
                            y: OrderedFloat(val),
                        })
                    }
                    _ => PropertyValue::Number(OrderedFloat(val)),
                }
            } else {
                PropertyValue::Number(OrderedFloat(val))
            };

            if let Some((eff_idx, prop_key)) = parse_key(base_name) {
                let _ = project_service.add_target_keyframe(
                    entity_id,
                    library::model::project::property::PropertyTarget::Effect(eff_idx),
                    &prop_key,
                    eval_time,
                    new_pv,
                    None,
                );
            } else if let Some((style_idx, prop_key)) = parse_style_key(base_name) {
                let _ = project_service.add_target_keyframe(
                    entity_id,
                    library::model::project::property::PropertyTarget::Style(style_idx),
                    &prop_key,
                    eval_time,
                    new_pv,
                    None,
                );
            } else {
                let _ = project_service.add_target_keyframe(
                    entity_id,
                    library::model::project::property::PropertyTarget::Clip,
                    base_name,
                    eval_time,
                    new_pv,
                    None,
                );
            }
            if let Ok(proj_read) = project.read() {
                history_manager.push_project_state(proj_read.clone());
            }
        }
        Action::SetEasing(name, idx, easing) => {
            let (base_name, _) = if name.ends_with(".x") {
                (name.trim_end_matches(".x"), Some(PropertyComponent::X))
            } else if name.ends_with(".y") {
                (name.trim_end_matches(".y"), Some(PropertyComponent::Y))
            } else {
                (name.as_str(), None)
            };

            if let Some((eff_idx, prop_key)) = parse_key(base_name) {
                let _ = project_service.update_target_keyframe_by_index(
                    entity_id,
                    library::model::project::property::PropertyTarget::Effect(eff_idx),
                    &prop_key,
                    idx,
                    None,
                    None,
                    Some(easing),
                );
            } else if let Some((style_idx, prop_key)) = parse_style_key(base_name) {
                let _ = project_service.update_target_keyframe_by_index(
                    entity_id,
                    library::model::project::property::PropertyTarget::Style(style_idx),
                    &prop_key,
                    idx,
                    None,
                    None,
                    Some(easing),
                );
            } else {
                let _ = project_service.update_target_keyframe_by_index(
                    entity_id,
                    library::model::project::property::PropertyTarget::Clip,
                    base_name,
                    idx,
                    None,
                    None,
                    Some(easing),
                );
            }
            if let Ok(proj_read) = project.read() {
                history_manager.push_project_state(proj_read.clone());
            }
        }
        Action::Remove(name, idx) => {
            let (base_name, _) = if name.ends_with(".x") {
                (name.trim_end_matches(".x"), Some(PropertyComponent::X))
            } else if name.ends_with(".y") {
                (name.trim_end_matches(".y"), Some(PropertyComponent::Y))
            } else {
                (name.as_str(), None)
            };

            if let Some((eff_idx, prop_key)) = parse_key(base_name) {
                let _ = project_service.remove_target_keyframe_by_index(
                    entity_id,
                    library::model::project::property::PropertyTarget::Effect(eff_idx),
                    &prop_key,
                    idx,
                );
            } else if let Some((style_idx, prop_key)) = parse_style_key(base_name) {
                let _ = project_service.remove_target_keyframe_by_index(
                    entity_id,
                    library::model::project::property::PropertyTarget::Style(style_idx),
                    &prop_key,
                    idx,
                );
            } else {
                let _ = project_service.remove_target_keyframe_by_index(
                    entity_id,
                    library::model::project::property::PropertyTarget::Clip,
                    base_name,
                    idx,
                );
            }
            if let Ok(proj_read) = project.read() {
                history_manager.push_project_state(proj_read.clone());
            }
        }
        Action::EditKeyframe(ref name, idx) => {
            let (base_name, _) = if name.ends_with(".x") {
                (name.trim_end_matches(".x"), Some(PropertyComponent::X))
            } else if name.ends_with(".y") {
                (name.trim_end_matches(".y"), Some(PropertyComponent::Y))
            } else {
                (name.as_str(), None)
            };

            if let Ok(proj) = project.read() {
                // Use flat O(1) lookup
                if let Some(clip) = proj.get_clip(entity_id) {
                    // Effect Property
                    if let Some((eff_idx, prop_key)) = parse_key(base_name) {
                        if let Some(effect) = clip.effects.get(eff_idx) {
                            if let Some(prop) = effect.properties.get(&prop_key) {
                                if prop.evaluator == "keyframe" {
                                    let keyframes = prop.keyframes();
                                    let mut sorted_kf = keyframes.clone();
                                    sorted_kf.sort_by(|a, b| a.time.cmp(&b.time));

                                    if let Some(kf) = sorted_kf.get(idx) {
                                        editor_context.keyframe_dialog.is_open = true;
                                        editor_context.keyframe_dialog.track_id = Some(track_id);
                                        editor_context.keyframe_dialog.entity_id = Some(entity_id);
                                        editor_context.keyframe_dialog.property_name = name.clone();
                                        editor_context.keyframe_dialog.keyframe_index = idx;
                                        editor_context.keyframe_dialog.time = kf.time.into_inner();
                                        editor_context.keyframe_dialog.value = match (
                                            name.ends_with(".x"),
                                            name.ends_with(".y"),
                                        ) {
                                            (true, _) => kf
                                                .value
                                                .get_as::<library::model::project::property::Vec2>()
                                                .map_or(0.0, |v| v.x.into_inner()),
                                            (_, true) => kf
                                                .value
                                                .get_as::<library::model::project::property::Vec2>()
                                                .map_or(0.0, |v| v.y.into_inner()),
                                            _ => kf.value.get_as::<f64>().unwrap_or(0.0),
                                        };
                                        editor_context.keyframe_dialog.easing = kf.easing.clone();
                                    }
                                }
                            }
                        }
                    }
                    // Style Property
                    else if let Some((style_idx, prop_key)) = parse_style_key(base_name) {
                        if let Some(style) = clip.styles.get(style_idx) {
                            if let Some(prop) = style.properties.get(&prop_key) {
                                if prop.evaluator == "keyframe" {
                                    let keyframes = prop.keyframes();
                                    let mut sorted_kf = keyframes.clone();
                                    sorted_kf.sort_by(|a, b| a.time.cmp(&b.time));

                                    if let Some(kf) = sorted_kf.get(idx) {
                                        editor_context.keyframe_dialog.is_open = true;
                                        editor_context.keyframe_dialog.track_id = Some(track_id);
                                        editor_context.keyframe_dialog.entity_id = Some(entity_id);
                                        editor_context.keyframe_dialog.property_name = name.clone();
                                        editor_context.keyframe_dialog.keyframe_index = idx;
                                        editor_context.keyframe_dialog.time = kf.time.into_inner();
                                        editor_context.keyframe_dialog.value = match (
                                            name.ends_with(".x"),
                                            name.ends_with(".y"),
                                        ) {
                                            (true, _) => kf
                                                .value
                                                .get_as::<library::model::project::property::Vec2>()
                                                .map_or(0.0, |v| v.x.into_inner()),
                                            (_, true) => kf
                                                .value
                                                .get_as::<library::model::project::property::Vec2>()
                                                .map_or(0.0, |v| v.y.into_inner()),
                                            _ => kf.value.get_as::<f64>().unwrap_or(0.0),
                                        };
                                        editor_context.keyframe_dialog.easing = kf.easing.clone();
                                    }
                                }
                            }
                        }
                    }
                    // Clip Property
                    else if let Some(prop) = clip.properties.get(base_name) {
                        if prop.evaluator == "keyframe" {
                            let keyframes = prop.keyframes();
                            let mut sorted_kf = keyframes.clone();
                            sorted_kf.sort_by(|a, b| a.time.cmp(&b.time));

                            if let Some(kf) = sorted_kf.get(idx) {
                                editor_context.keyframe_dialog.is_open = true;
                                editor_context.keyframe_dialog.track_id = Some(track_id);
                                editor_context.keyframe_dialog.entity_id = Some(entity_id);
                                editor_context.keyframe_dialog.property_name = name.clone();
                                editor_context.keyframe_dialog.keyframe_index = idx;
                                editor_context.keyframe_dialog.time = kf.time.into_inner();
                                editor_context.keyframe_dialog.value =
                                    match (name.ends_with(".x"), name.ends_with(".y")) {
                                        (true, _) => kf
                                            .value
                                            .get_as::<library::model::project::property::Vec2>()
                                            .map_or(0.0, |v| v.x.into_inner()),
                                        (_, true) => kf
                                            .value
                                            .get_as::<library::model::project::property::Vec2>()
                                            .map_or(0.0, |v| v.y.into_inner()),
                                        _ => kf.value.get_as::<f64>().unwrap_or(0.0),
                                    };
                                editor_context.keyframe_dialog.easing = kf.easing.clone();
                            }
                        }
                    }
                }
            }
        }
        Action::None => {}
    }
}
