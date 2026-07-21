use super::mutation::{
    add_keyframe, property_component, remove_keyframe, resolve_graph_property, update_keyframe,
    validate_keyframe_component, GraphMutationRoute,
};
use super::utils::{property_component_value, replace_property_component};
use super::PropertyComponent;
use crate::action::HistoryManager;
use crate::state::context::EditorContext;
use crate::state::context_types::GraphPropertyAddress;
use crate::utils::lock::read_or_recover;
use library::animation::EasingFunction;
use library::model::project::Project;
use library::model::property::{KeyframeId, KeyframeUpdate, PropertyValue};
use library::{EditorService, KeyframeBatchUpdate};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub enum Action {
    Select(String, KeyframeId),
    MoveBatch(Vec<KeyframeMove>),
    FinishMove,
    Add(GraphPropertyAddress, f64, f64),
    SetEasing(GraphPropertyAddress, KeyframeId, EasingFunction),
    Remove(GraphPropertyAddress, KeyframeId),
    EditKeyframe(GraphPropertyAddress, KeyframeId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeyframeMove {
    pub address: GraphPropertyAddress,
    pub keyframe_id: KeyframeId,
    pub global_time: f64,
    pub value: f64,
}

pub fn graph_property_name(property_key: &str, component: PropertyComponent) -> String {
    let suffix = match component {
        PropertyComponent::Scalar => "",
        PropertyComponent::X => ".x",
        PropertyComponent::Y => ".y",
        PropertyComponent::Z => ".z",
        PropertyComponent::W => ".w",
    };
    format!("node:{property_key}{suffix}")
}

#[cfg(test)]
fn parse_property_name(name: &str) -> Option<(String, Option<PropertyComponent>)> {
    let (base_name, component) = split_component(name);
    match base_name.split(':').collect::<Vec<_>>().as_slice() {
        ["node", property] if !property.is_empty() => Some(((*property).to_string(), component)),
        _ => None,
    }
}

#[cfg(test)]
fn split_component(name: &str) -> (&str, Option<PropertyComponent>) {
    if let Some(base) = name.strip_suffix(".x") {
        (base, Some(PropertyComponent::X))
    } else if let Some(base) = name.strip_suffix(".y") {
        (base, Some(PropertyComponent::Y))
    } else if let Some(base) = name.strip_suffix(".z") {
        (base, Some(PropertyComponent::Z))
    } else if let Some(base) = name.strip_suffix(".w") {
        (base, Some(PropertyComponent::W))
    } else {
        (name, None)
    }
}

#[derive(Clone)]
struct PreparedMove {
    route: GraphMutationRoute,
    property_key: String,
    keyframe_id: KeyframeId,
    source_time: f64,
    value: PropertyValue,
}

fn prepare_move_batch(
    project_service: &EditorService,
    project: &Arc<RwLock<Project>>,
    moves: &[KeyframeMove],
) -> Result<Vec<PreparedMove>, String> {
    let mut prepared: Vec<PreparedMove> = Vec::new();
    let target = moves.first().map(|movement| movement.address.target);

    for movement in moves {
        if !movement.global_time.is_finite() || !movement.value.is_finite() {
            return Err(format!(
                "Graph keyframe {} has a non-finite time or value",
                movement.keyframe_id
            ));
        }
        if Some(movement.address.target) != target {
            return Err("Graph move batch spans multiple selected targets".to_string());
        }
        let resolved = resolve_graph_property(project_service, project, &movement.address)?;
        let keyframe =
            validate_keyframe_component(&resolved, &movement.address, movement.keyframe_id)?;
        let existing_index = prepared.iter().position(|candidate| {
            candidate.route == resolved.route
                && candidate.property_key == movement.address.property_key
                && candidate.keyframe_id == movement.keyframe_id
        });
        let current = existing_index
            .map(|index| prepared[index].value.clone())
            .unwrap_or(keyframe.value);
        let value = replace_property_component(
            &current,
            property_component(movement.address.component),
            movement.value,
        )?;
        let source_time = resolved.time_mapper.to_source_time(movement.global_time);
        if let Some(index) = existing_index {
            prepared[index].source_time = source_time;
            prepared[index].value = value;
        } else {
            prepared.push(PreparedMove {
                route: resolved.route,
                property_key: movement.address.property_key.clone(),
                keyframe_id: movement.keyframe_id,
                source_time,
                value,
            });
        }
    }

    if prepared.is_empty() {
        return Err("Graph move batch is empty".to_string());
    }
    if prepared
        .iter()
        .any(|movement| matches!(movement.route, GraphMutationRoute::Semantic(_)))
        && (prepared.len() != 1 || !matches!(prepared[0].route, GraphMutationRoute::Semantic(_)))
    {
        return Err(
            "Graph semantic drags may update only one property keyframe atomically".to_string(),
        );
    }
    Ok(prepared)
}

fn apply_move_batch(
    project_service: &EditorService,
    prepared: Vec<PreparedMove>,
) -> Result<(), String> {
    if let [movement] = prepared.as_slice() {
        if matches!(movement.route, GraphMutationRoute::Semantic(_)) {
            return update_keyframe(
                project_service,
                movement.route,
                &movement.property_key,
                movement.keyframe_id,
                KeyframeUpdate {
                    time: Some(movement.source_time),
                    value: Some(movement.value.clone()),
                    ..Default::default()
                },
            );
        }
    }
    let updates = prepared
        .into_iter()
        .map(|movement| {
            let GraphMutationRoute::Direct(owner) = movement.route else {
                return Err("semantic Graph move escaped the atomicity gate".to_string());
            };
            Ok(KeyframeBatchUpdate {
                owner,
                property_key: movement.property_key,
                keyframe_id: movement.keyframe_id,
                update: KeyframeUpdate {
                    time: Some(movement.source_time),
                    value: Some(movement.value),
                    ..Default::default()
                },
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    project_service
        .update_keyframes_batch(&updates)
        .map_err(|error| error.to_string())
}

fn push_history(project: &Arc<RwLock<Project>>, history_manager: &mut HistoryManager) {
    history_manager.push_project_state(read_or_recover(project.as_ref()).clone());
}

pub fn finish_pending_move(
    editor_context: &mut EditorContext,
    project: &Arc<RwLock<Project>>,
    history_manager: &mut HistoryManager,
) -> bool {
    let changed = editor_context
        .graph_editor
        .keyframe_drag
        .take()
        .is_some_and(|drag| drag.changed);
    if changed {
        push_history(project, history_manager);
    }
    changed
}

#[allow(
    clippy::too_many_arguments,
    reason = "graph actions coordinate composition context, model services, UI state, and history"
)]
pub fn process_action(
    action: Action,
    comp_id: Uuid,
    project_service: &EditorService,
    project: &Arc<RwLock<Project>>,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
) {
    match action {
        Action::Select(name, keyframe_id) => {
            editor_context.interaction.selected_keyframe = Some((name, keyframe_id));
        }
        Action::MoveBatch(moves) => {
            let move_target = moves.first().map(|movement| movement.address.target);
            if editor_context
                .graph_editor
                .keyframe_drag
                .as_ref()
                .is_none_or(|drag| Some(drag.target) != move_target)
            {
                log::error!("Rejected Graph move batch outside its active typed drag target");
                return;
            }
            match prepare_move_batch(project_service, project, &moves)
                .and_then(|prepared| apply_move_batch(project_service, prepared))
            {
                Ok(()) => {
                    if let Some(drag) = &mut editor_context.graph_editor.keyframe_drag {
                        drag.changed = true;
                    }
                }
                Err(error) => log::error!("Rejected atomic Graph move batch: {error}"),
            }
        }
        Action::FinishMove => {
            finish_pending_move(editor_context, project, history_manager);
        }
        Action::Add(address, time, value) => {
            let prepared = (|| {
                if !time.is_finite() || !value.is_finite() {
                    return Err("Graph keyframe has a non-finite time or value".to_string());
                }
                let resolved = resolve_graph_property(project_service, project, &address)?;
                if !matches!(
                    resolved.property.evaluator.as_str(),
                    "constant" | "keyframe"
                ) {
                    return Err("Graph expressions cannot be converted to keyframes".to_string());
                }
                let (fps, resolution) = {
                    let project = project.read().map_err(|error| error.to_string())?;
                    let composition = project
                        .get_composition(comp_id)
                        .ok_or_else(|| format!("Graph composition {comp_id} does not exist"))?;
                    (composition.fps, (composition.width, composition.height))
                };
                let source_time = resolved.time_mapper.to_source_time(time);
                let current = project_service
                    .evaluate_property_value(
                        &resolved.property,
                        &resolved.property_map,
                        source_time,
                        fps,
                        resolution,
                    )
                    .map_err(|error| error.to_string())?;
                let value = replace_property_component(
                    &current,
                    property_component(address.component),
                    value,
                )?;
                Ok((resolved.route, source_time, value))
            })();
            match prepared.and_then(|(route, source_time, value)| {
                add_keyframe(
                    project_service,
                    route,
                    &address.property_key,
                    source_time,
                    value,
                    None,
                )
            }) {
                Ok(()) => push_history(project, history_manager),
                Err(error) => log::error!("Rejected Graph keyframe add: {error}"),
            }
        }
        Action::SetEasing(address, keyframe_id, easing) => {
            let prepared =
                resolve_graph_property(project_service, project, &address).and_then(|resolved| {
                    validate_keyframe_component(&resolved, &address, keyframe_id)?;
                    Ok(resolved.route)
                });
            match prepared.and_then(|route| {
                update_keyframe(
                    project_service,
                    route,
                    &address.property_key,
                    keyframe_id,
                    KeyframeUpdate {
                        easing: Some(easing),
                        ..Default::default()
                    },
                )
            }) {
                Ok(()) => push_history(project, history_manager),
                Err(error) => log::error!("Rejected Graph easing update: {error}"),
            }
        }
        Action::Remove(address, keyframe_id) => {
            let prepared =
                resolve_graph_property(project_service, project, &address).and_then(|resolved| {
                    validate_keyframe_component(&resolved, &address, keyframe_id)?;
                    Ok(resolved.route)
                });
            match prepared.and_then(|route| {
                remove_keyframe(project_service, route, &address.property_key, keyframe_id)
            }) {
                Ok(()) => push_history(project, history_manager),
                Err(error) => log::error!("Rejected Graph keyframe removal: {error}"),
            }
        }
        Action::EditKeyframe(address, keyframe_id) => {
            let prepared =
                resolve_graph_property(project_service, project, &address).and_then(|resolved| {
                    let keyframe = validate_keyframe_component(&resolved, &address, keyframe_id)?;
                    let value = property_component_value(
                        &keyframe.value,
                        property_component(address.component),
                    )?;
                    let global_time = resolved
                        .time_mapper
                        .to_global_time(keyframe.time.into_inner());
                    Ok((keyframe, resolved.route, global_time, value))
                });
            match prepared {
                Ok((keyframe, route, global_time, value)) => {
                    editor_context.keyframe_dialog.is_open = true;
                    editor_context.keyframe_dialog.property_name = address.stable_id.clone();
                    editor_context.keyframe_dialog.owner = match route {
                        GraphMutationRoute::Direct(owner) => Some(owner),
                        GraphMutationRoute::Semantic(_) => None,
                    };
                    editor_context.keyframe_dialog.graph_address = Some(address.clone());
                    editor_context.keyframe_dialog.property_key = address.property_key.clone();
                    editor_context.keyframe_dialog.keyframe_id = Some(keyframe_id);
                    editor_context.keyframe_dialog.component = address.component;
                    editor_context.keyframe_dialog.time = global_time;
                    editor_context.keyframe_dialog.value = value;
                    editor_context.keyframe_dialog.easing = keyframe.easing;
                    editor_context.keyframe_dialog.begin_transaction();
                }
                Err(error) => log::error!("Rejected Graph keyframe dialog: {error}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::context_types::{
        GraphKeyframeDragOrigin, GraphKeyframeDragState, KeyframeValueComponent, SelectionTarget,
    };
    use library::cache::CacheManager;
    use library::editor::project_service::SemanticPropertyOwner;
    use library::model::property::{Keyframe, Property, Vec2, Vec3, Vec4};
    use library::model::{Clip, Composition, Node};
    use library::plugin::PluginManager;
    use ordered_float::OrderedFloat;

    fn number(value: f64) -> PropertyValue {
        PropertyValue::Number(OrderedFloat(value))
    }

    fn vec4(x: f64, y: f64, z: f64, w: f64) -> PropertyValue {
        PropertyValue::Vec4(Vec4 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
            z: OrderedFloat(z),
            w: OrderedFloat(w),
        })
    }

    fn keyframed_number(time: f64, value: f64) -> (Property, KeyframeId) {
        let keyframe = Keyframe::new(time, number(value), EasingFunction::Linear);
        let id = keyframe.id;
        (Property::keyframe(vec![keyframe]), id)
    }

    fn vec3(x: f64, y: f64, z: f64) -> PropertyValue {
        PropertyValue::Vec3(Vec3 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
            z: OrderedFloat(z),
        })
    }

    fn property_value(
        project: &Project,
        node_id: Uuid,
        property_key: &str,
        keyframe_id: KeyframeId,
    ) -> (f64, PropertyValue) {
        let keyframe = project
            .get_node(node_id)
            .unwrap()
            .properties()
            .get(property_key)
            .unwrap()
            .keyframe_by_id(keyframe_id)
            .unwrap();
        (keyframe.time.into_inner(), keyframe.value)
    }

    fn exact_address(
        node_id: Uuid,
        property_key: &str,
        component: PropertyComponent,
    ) -> GraphPropertyAddress {
        let component = match component {
            PropertyComponent::Scalar => KeyframeValueComponent::Scalar,
            PropertyComponent::X => KeyframeValueComponent::X,
            PropertyComponent::Y => KeyframeValueComponent::Y,
            PropertyComponent::Z => KeyframeValueComponent::Z,
            PropertyComponent::W => KeyframeValueComponent::W,
        };
        GraphPropertyAddress {
            target: SelectionTarget::Node(node_id),
            section_id: format!("node:{node_id}"),
            stable_id: graph_property_name(property_key, property_component(component)),
            owner: SemanticPropertyOwner::ExactNode(node_id),
            property_key: property_key.to_string(),
            component,
        }
    }

    #[test]
    fn scoped_names_address_only_the_selected_nodes_direct_properties() {
        let name = graph_property_name("amount", PropertyComponent::X);
        assert_eq!(
            parse_property_name(&name),
            Some(("amount".to_string(), Some(PropertyComponent::X)))
        );
        assert!(parse_property_name("amount").is_none());
        assert!(parse_property_name("effect:obsolete:amount").is_none());
    }

    #[test]
    fn easing_and_remove_reject_wrong_vector_axis_without_history_or_mutation() {
        let keyframe = Keyframe::new(1.0, vec3(1.0, 2.0, 3.0), EasingFunction::Linear);
        let keyframe_id = keyframe.id;
        let mut node = Node::new_add("typed vector action");
        let node_id = node.id;
        assert!(node
            .set_property("b".to_string(), Property::keyframe(vec![keyframe]))
            .is_ok());
        let mut model = Project::new("typed vector action");
        model.add_node(node);
        let project = Arc::new(RwLock::new(model));
        let service = EditorService::new(
            Arc::clone(&project),
            Arc::new(PluginManager::default()),
            Arc::new(CacheManager::new()),
        )
        .expect("test EditorService initializes");
        let composition_id = Uuid::new_v4();
        let mut context = EditorContext::new(composition_id);
        let mut history = HistoryManager::new();
        history.push_project_state(project.read().expect("project read").clone());
        let before = project.read().expect("project read").clone();

        for action in [
            Action::SetEasing(
                exact_address(node_id, "b", PropertyComponent::W),
                keyframe_id,
                EasingFunction::EaseInQuad,
            ),
            Action::Remove(
                exact_address(node_id, "b", PropertyComponent::W),
                keyframe_id,
            ),
        ] {
            process_action(
                action,
                composition_id,
                &service,
                &project,
                &mut context,
                &mut history,
            );
        }
        assert_eq!(*project.read().expect("project read"), before);
        assert_eq!(history.undo_depth(), 1);

        process_action(
            Action::SetEasing(
                exact_address(node_id, "b", PropertyComponent::Z),
                keyframe_id,
                EasingFunction::EaseInQuad,
            ),
            composition_id,
            &service,
            &project,
            &mut context,
            &mut history,
        );
        let after_easing = project.read().expect("project read");
        let updated = after_easing
            .get_node(node_id)
            .and_then(|node| node.properties().get("b"))
            .and_then(|property| property.keyframe_by_id(keyframe_id))
            .expect("keyframe survives easing edit");
        assert_eq!(updated.value, vec3(1.0, 2.0, 3.0));
        assert_eq!(updated.easing, EasingFunction::EaseInQuad);
        drop(after_easing);
        assert_eq!(history.undo_depth(), 2);
    }

    #[test]
    fn exact_node_vec4_w_add_and_move_preserve_xyz_and_reject_scalar_mismatch() {
        let mut node = Node::new_add("Vec4 Graph target");
        let node_id = node.id;
        assert!(node
            .set_property(
                "b".to_string(),
                Property::constant(vec4(1.0, 2.0, 3.0, 4.0)),
            )
            .is_ok());
        let (mut composition, track) = Composition::new("main", 640, 360, 30.0, 10.0);
        let composition_id = composition.id;
        composition.track_ids = vec![track.id];
        let mut model = Project::new("Vec4 Graph actions");
        assert!(model.add_track(track).is_ok());
        model.add_node(node);
        assert!(model.add_composition(composition).is_ok());
        let project = Arc::new(RwLock::new(model));
        let service = EditorService::new(
            Arc::clone(&project),
            Arc::new(PluginManager::default()),
            Arc::new(CacheManager::new()),
        )
        .expect("test EditorService initializes");
        let mut context = EditorContext::new(composition_id);
        let mut history = HistoryManager::new();
        history.push_project_state(project.read().expect("project read").clone());
        let property_name = graph_property_name("b", PropertyComponent::W);
        let address = exact_address(node_id, "b", PropertyComponent::W);

        process_action(
            Action::Add(address.clone(), 1.0, 8.0),
            composition_id,
            &service,
            &project,
            &mut context,
            &mut history,
        );
        assert_eq!(history.undo_depth(), 2);
        let keyframe_id = project
            .read()
            .expect("project read")
            .get_node(node_id)
            .and_then(|node| node.properties().get("b"))
            .and_then(|property| property.keyframes().first().cloned())
            .expect("added Vec4 keyframe exists")
            .id;
        assert_eq!(
            property_value(
                &project.read().expect("project read"),
                node_id,
                "b",
                keyframe_id,
            )
            .1,
            vec4(1.0, 2.0, 3.0, 8.0)
        );

        context.graph_editor.keyframe_drag = Some(GraphKeyframeDragState {
            target: SelectionTarget::Node(node_id),
            anchor: (property_name.clone(), keyframe_id),
            origins: Vec::new(),
            changed: false,
        });
        process_action(
            Action::MoveBatch(vec![KeyframeMove {
                address,
                keyframe_id,
                global_time: 2.0,
                value: 9.0,
            }]),
            composition_id,
            &service,
            &project,
            &mut context,
            &mut history,
        );
        assert_eq!(history.undo_depth(), 2, "drag frame must remain pending");
        assert_eq!(
            property_value(
                &project.read().expect("project read"),
                node_id,
                "b",
                keyframe_id,
            ),
            (2.0, vec4(1.0, 2.0, 3.0, 9.0))
        );
        process_action(
            Action::FinishMove,
            composition_id,
            &service,
            &project,
            &mut context,
            &mut history,
        );
        assert_eq!(history.undo_depth(), 3);

        let before_mismatch = project.read().expect("project read").clone();
        context.graph_editor.keyframe_drag = Some(GraphKeyframeDragState {
            target: SelectionTarget::Node(node_id),
            anchor: (property_name, keyframe_id),
            origins: Vec::new(),
            changed: false,
        });
        process_action(
            Action::MoveBatch(vec![KeyframeMove {
                address: exact_address(node_id, "b", PropertyComponent::Scalar),
                keyframe_id,
                global_time: 3.0,
                value: 10.0,
            }]),
            composition_id,
            &service,
            &project,
            &mut context,
            &mut history,
        );
        assert_eq!(*project.read().expect("project read"), before_mismatch);
        assert!(
            !context
                .graph_editor
                .keyframe_drag
                .as_ref()
                .expect("drag state remains")
                .changed
        );
        process_action(
            Action::FinishMove,
            composition_id,
            &service,
            &project,
            &mut context,
            &mut history,
        );
        assert_eq!(history.undo_depth(), 3);
    }

    #[test]
    fn absolute_direct_property_drag_does_not_overshoot_and_commits_one_history_state() {
        let source_time = 2.0;

        let (direct_property, direct_id) = keyframed_number(source_time, 10.0);
        let position_keyframe = Keyframe::new(
            source_time,
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(20.0),
                y: OrderedFloat(30.0),
            }),
            EasingFunction::Linear,
        );
        let position_id = position_keyframe.id;
        let mut node = PluginManager::default()
            .create_image_transform_operation_node()
            .expect("Image Transform descriptor is valid");
        node.name = "graph target".to_string();
        let node_id = node.id;
        node.set_property("rotation".to_string(), direct_property)
            .expect("Image Transform factory initializes rotation");
        node.set_property(
            "position".to_string(),
            Property::keyframe(vec![position_keyframe]),
        )
        .expect("Image Transform factory initializes position");
        let (mut composition, track) = Composition::new("main", 640, 360, 30.0, 10.0);
        let composition_id = composition.id;
        let track_id = track.id;
        let mut clip = Clip::new("mapped", 1.25, 6.0);
        clip.trim_in = OrderedFloat(0.5);
        clip.time_stretch = OrderedFloat(2.0);
        clip.node_ids = vec![node_id];
        clip.output_node_id = Some(node_id);
        composition.track_ids = vec![track_id];

        let mut model = Project::new("graph drag");
        assert!(
            model.add_track(track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        model.add_clip(clip);
        model.add_node(node);
        assert!(
            model.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );
        let project = Arc::new(RwLock::new(model));
        let service = EditorService::new(
            Arc::clone(&project),
            Arc::new(PluginManager::default()),
            Arc::new(CacheManager::new()),
        )
        .unwrap();
        let mut context = EditorContext::new(composition_id);
        let anchor_name = graph_property_name("rotation", PropertyComponent::Scalar);
        let rotation_address = exact_address(node_id, "rotation", PropertyComponent::Scalar);
        let position_x_address = exact_address(node_id, "position", PropertyComponent::X);
        let position_y_address = exact_address(node_id, "position", PropertyComponent::Y);
        context.graph_editor.keyframe_drag = Some(GraphKeyframeDragState {
            target: SelectionTarget::Node(node_id),
            anchor: (anchor_name.clone(), direct_id),
            origins: vec![GraphKeyframeDragOrigin {
                address: rotation_address.clone(),
                keyframe_id: direct_id,
                global_time: 2.0,
                value: 10.0,
            }],
            changed: false,
        });
        let mut history = HistoryManager::new();
        history.push_project_state(project.read().unwrap().clone());

        let movement = |global_time, offset| {
            vec![
                KeyframeMove {
                    address: rotation_address.clone(),
                    keyframe_id: direct_id,
                    global_time,
                    value: 10.0 + offset,
                },
                KeyframeMove {
                    address: position_x_address.clone(),
                    keyframe_id: position_id,
                    global_time,
                    value: 20.0 + offset,
                },
                KeyframeMove {
                    address: position_y_address.clone(),
                    keyframe_id: position_id,
                    global_time,
                    value: 30.0 + offset,
                },
            ]
        };

        // These are cumulative gesture deltas (2 then 4), not incremental
        // deltas. The second frame must end at origin+4, never origin+2+4.
        process_action(
            Action::MoveBatch(movement(2.2, 2.0)),
            composition_id,
            &service,
            &project,
            &mut context,
            &mut history,
        );
        process_action(
            Action::MoveBatch(movement(2.4, 4.0)),
            composition_id,
            &service,
            &project,
            &mut context,
            &mut history,
        );
        assert_eq!(
            history.undo_depth(),
            1,
            "drag frames must not commit history"
        );

        let read = project.read().unwrap();
        assert_eq!(
            property_value(&read, node_id, "rotation", direct_id),
            (2.8, number(14.0))
        );
        let (_, position) = property_value(&read, node_id, "position", position_id);
        assert_eq!(
            position,
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(24.0),
                y: OrderedFloat(34.0),
            })
        );
        drop(read);

        process_action(
            Action::FinishMove,
            composition_id,
            &service,
            &project,
            &mut context,
            &mut history,
        );
        assert_eq!(history.undo_depth(), 2);
        process_action(
            Action::FinishMove,
            composition_id,
            &service,
            &project,
            &mut context,
            &mut history,
        );
        assert_eq!(history.undo_depth(), 2, "one gesture must commit once");

        let before_invalid_batch = project.read().unwrap().clone();
        context.graph_editor.keyframe_drag = Some(GraphKeyframeDragState {
            target: SelectionTarget::Node(node_id),
            anchor: (anchor_name, direct_id),
            origins: Vec::new(),
            changed: false,
        });
        process_action(
            Action::MoveBatch(vec![
                KeyframeMove {
                    address: rotation_address,
                    keyframe_id: direct_id,
                    global_time: 3.0,
                    value: 99.0,
                },
                KeyframeMove {
                    address: exact_address(node_id, "obsolete", PropertyComponent::Scalar),
                    keyframe_id: direct_id,
                    global_time: 3.0,
                    value: 99.0,
                },
            ]),
            composition_id,
            &service,
            &project,
            &mut context,
            &mut history,
        );
        assert_eq!(*project.read().unwrap(), before_invalid_batch);
        assert!(!context.graph_editor.keyframe_drag.as_ref().unwrap().changed);
        process_action(
            Action::FinishMove,
            composition_id,
            &service,
            &project,
            &mut context,
            &mut history,
        );
        assert_eq!(history.undo_depth(), 2, "rejected batches must not commit");
    }

    #[test]
    fn graph_visibility_selection_and_drag_do_not_leak_between_typed_targets() {
        let first = Uuid::new_v4();
        let keyframe_id = KeyframeId::new();
        let mut state = crate::state::context_types::GraphEditorState::default();
        assert!(state.begin_entity(first));
        state.sync_properties(["node:amount".to_string()]);
        state
            .selected_keyframes
            .insert(("node:amount".to_string(), keyframe_id));
        state.keyframe_drag = Some(GraphKeyframeDragState {
            target: SelectionTarget::Node(first),
            anchor: ("node:amount".to_string(), keyframe_id),
            origins: Vec::new(),
            changed: true,
        });

        assert!(!state.begin_entity(first));
        assert_eq!(state.selected_keyframes.len(), 1);
        assert!(state.keyframe_drag.is_some());
        assert!(state.begin_target(SelectionTarget::Clip(first)));
        assert!(state.visible_properties.is_empty());
        assert!(state.known_properties.is_empty());
        assert!(state.selected_keyframes.is_empty());
        assert!(state.keyframe_drag.is_none());
    }

    #[test]
    fn interrupted_changed_drag_is_finalized_before_graph_owner_switch() {
        let original = Project::new("before drag");
        let project = Arc::new(RwLock::new(original.clone()));
        let composition_id = Uuid::new_v4();
        let entity_id = Uuid::new_v4();
        let keyframe_id = KeyframeId::new();
        let mut context = EditorContext::new(composition_id);
        context.graph_editor.keyframe_drag = Some(GraphKeyframeDragState {
            target: SelectionTarget::Node(entity_id),
            anchor: ("node:amount".to_string(), keyframe_id),
            origins: Vec::new(),
            changed: true,
        });
        project.write().unwrap().name = "after drag".to_string();
        let edited = project.read().unwrap().clone();
        let mut history = HistoryManager::new();
        history.push_project_state(original.clone());

        assert!(finish_pending_move(&mut context, &project, &mut history));
        assert_eq!(history.undo_depth(), 2);
        assert_eq!(history.undo(&edited), Some(original));
        assert!(context.graph_editor.keyframe_drag.is_none());
    }
}
