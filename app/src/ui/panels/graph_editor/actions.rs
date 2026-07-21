use super::utils::{property_component_value, replace_property_component, time_mapper_for_owner};
use super::PropertyComponent;
use crate::action::HistoryManager;
use crate::state::context::EditorContext;
use crate::utils::lock::read_or_recover;
use library::animation::EasingFunction;
use library::model::project::Project;
use library::model::property::{KeyframeId, KeyframeUpdate, PropertyValue};
use library::model::Node;
use library::{EditorService, KeyframeBatchUpdate, PropertyOwner};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub enum Action {
    Select(String, KeyframeId),
    MoveBatch(Vec<KeyframeMove>),
    FinishMove,
    Add(String, f64, f64),
    SetEasing(String, KeyframeId, EasingFunction),
    Remove(String, KeyframeId),
    EditKeyframe(String, KeyframeId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeyframeMove {
    pub property_name: String,
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

fn parse_property_name(name: &str) -> Option<(String, Option<PropertyComponent>)> {
    let (base_name, component) = split_component(name);
    match base_name.split(':').collect::<Vec<_>>().as_slice() {
        ["node", property] if !property.is_empty() => Some(((*property).to_string(), component)),
        _ => None,
    }
}

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

fn current_keyframe_value(
    node: &Node,
    property_key: &str,
    keyframe_id: KeyframeId,
) -> Option<PropertyValue> {
    let property = node.properties().get(property_key)?;
    property
        .keyframe_by_id(keyframe_id)
        .map(|keyframe| keyframe.value)
}

fn validate_keyframe_component(
    project: &Project,
    entity_id: Uuid,
    property_key: &str,
    keyframe_id: KeyframeId,
    component: Option<PropertyComponent>,
) -> Result<PropertyOwner, String> {
    let node = project
        .get_node(entity_id)
        .ok_or_else(|| format!("Graph Node {entity_id} does not exist"))?;
    let property = node.properties().get(property_key).ok_or_else(|| {
        format!("Graph property {property_key:?} does not exist on Node {entity_id}")
    })?;
    if property.evaluator != "keyframe" {
        return Err(format!("Graph property {property_key:?} is not keyframed"));
    }
    let keyframe = property
        .keyframe_by_id(keyframe_id)
        .ok_or_else(|| format!("Graph keyframe {keyframe_id} does not exist"))?;
    property_component_value(
        &keyframe.value,
        component.unwrap_or(PropertyComponent::Scalar),
    )?;
    Ok(PropertyOwner::Node(entity_id))
}

#[derive(Clone)]
struct PreparedMove {
    property_key: String,
    keyframe_id: KeyframeId,
    source_time: f64,
    value: PropertyValue,
}

fn prepare_move_batch(
    project: &Project,
    entity_id: Uuid,
    moves: &[KeyframeMove],
) -> Result<(PropertyOwner, Vec<PreparedMove>), String> {
    let node = project
        .get_node(entity_id)
        .ok_or_else(|| format!("Graph Node {entity_id} does not exist"))?;
    let mapper = time_mapper_for_owner(project, PropertyOwner::Node(entity_id));
    let mut prepared: Vec<PreparedMove> = Vec::new();

    for movement in moves {
        if !movement.global_time.is_finite() || !movement.value.is_finite() {
            return Err(format!(
                "Graph keyframe {} has a non-finite time or value",
                movement.keyframe_id
            ));
        }
        let (property_key, component) =
            parse_property_name(&movement.property_name).ok_or_else(|| {
                format!(
                    "invalid scoped Graph property name {:?}",
                    movement.property_name
                )
            })?;
        let existing_index = prepared.iter().position(|candidate| {
            candidate.property_key == property_key && candidate.keyframe_id == movement.keyframe_id
        });
        let current = existing_index
            .map(|index| prepared[index].value.clone())
            .or_else(|| current_keyframe_value(node, &property_key, movement.keyframe_id))
            .ok_or_else(|| {
                format!(
                    "Graph keyframe {} was not found in property {}",
                    movement.keyframe_id, property_key
                )
            })?;
        let value = replace_property_component(
            &current,
            component.unwrap_or(PropertyComponent::Scalar),
            movement.value,
        )?;
        let source_time = mapper.to_source_time(movement.global_time);
        if let Some(index) = existing_index {
            prepared[index].source_time = source_time;
            prepared[index].value = value;
        } else {
            prepared.push(PreparedMove {
                property_key,
                keyframe_id: movement.keyframe_id,
                source_time,
                value,
            });
        }
    }

    if prepared.is_empty() {
        return Err("Graph move batch is empty".to_string());
    }
    Ok((PropertyOwner::Node(entity_id), prepared))
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
    reason = "graph actions need stable composition/entity identity plus model, UI, and history services for one atomic edit"
)]
pub fn process_action(
    action: Action,
    comp_id: Uuid,
    entity_id: Uuid,
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
            let prepared = project
                .read()
                .map_err(|error| error.to_string())
                .and_then(|project| prepare_move_batch(&project, entity_id, &moves));
            match prepared.and_then(|(owner, prepared)| {
                let updates = prepared
                    .into_iter()
                    .map(|movement| KeyframeBatchUpdate {
                        owner,
                        property_key: movement.property_key,
                        keyframe_id: movement.keyframe_id,
                        update: KeyframeUpdate {
                            time: Some(movement.source_time),
                            value: Some(movement.value),
                            ..Default::default()
                        },
                    })
                    .collect::<Vec<_>>();
                project_service
                    .update_keyframes_batch(&updates)
                    .map_err(|error| error.to_string())
            }) {
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
        Action::Add(name, time, value) => {
            let Some((property_key, component)) = parse_property_name(&name) else {
                log::error!("Graph Editor rejected invalid scoped property name {name:?}");
                return;
            };
            let prepared = project
                .read()
                .map_err(|error| error.to_string())
                .and_then(|project| {
                    let composition = project
                        .get_composition(comp_id)
                        .ok_or_else(|| format!("Graph composition {comp_id} does not exist"))?;
                    let node = project
                        .get_node(entity_id)
                        .ok_or_else(|| format!("Graph Node {entity_id} does not exist"))?;
                    let source_time =
                        time_mapper_for_owner(&project, PropertyOwner::Node(entity_id))
                            .to_source_time(time);
                    let property = node.properties().get(&property_key).ok_or_else(|| {
                        format!(
                            "Graph property {property_key:?} does not exist on Node {entity_id}"
                        )
                    })?;
                    let current = project_service
                        .evaluate_property_value(
                            property,
                            node.properties(),
                            source_time,
                            composition.fps,
                            (composition.width, composition.height),
                        )
                        .map_err(|error| error.to_string())?;
                    let value = replace_property_component(
                        &current,
                        component.unwrap_or(PropertyComponent::Scalar),
                        value,
                    )?;
                    Ok((PropertyOwner::Node(entity_id), source_time, value))
                });
            match prepared {
                Ok((owner, source_time, value)) => {
                    if project_service
                        .add_keyframe(owner, &property_key, source_time, value, None)
                        .is_ok()
                    {
                        push_history(project, history_manager);
                    }
                }
                Err(error) => log::error!("Rejected Graph keyframe add: {error}"),
            }
        }
        Action::SetEasing(name, keyframe_id, easing) => {
            let Some((property_key, component)) = parse_property_name(&name) else {
                log::error!("Graph Editor rejected invalid scoped property name {name:?}");
                return;
            };
            let owner = project
                .read()
                .map_err(|error| error.to_string())
                .and_then(|project| {
                    validate_keyframe_component(
                        &project,
                        entity_id,
                        &property_key,
                        keyframe_id,
                        component,
                    )
                });
            match owner {
                Ok(owner) => {
                    if project_service
                        .update_keyframe_by_id(
                            owner,
                            &property_key,
                            keyframe_id,
                            KeyframeUpdate {
                                easing: Some(easing),
                                ..Default::default()
                            },
                        )
                        .is_ok()
                    {
                        push_history(project, history_manager);
                    }
                }
                Err(error) => log::error!("Rejected Graph easing update: {error}"),
            }
        }
        Action::Remove(name, keyframe_id) => {
            let Some((property_key, component)) = parse_property_name(&name) else {
                log::error!("Graph Editor rejected invalid scoped property name {name:?}");
                return;
            };
            let owner = project
                .read()
                .map_err(|error| error.to_string())
                .and_then(|project| {
                    validate_keyframe_component(
                        &project,
                        entity_id,
                        &property_key,
                        keyframe_id,
                        component,
                    )
                });
            match owner {
                Ok(owner) => {
                    if project_service
                        .remove_keyframe_by_id(owner, &property_key, keyframe_id)
                        .is_ok()
                    {
                        push_history(project, history_manager);
                    }
                }
                Err(error) => log::error!("Rejected Graph keyframe removal: {error}"),
            }
        }
        Action::EditKeyframe(name, keyframe_id) => {
            let Some((property_key, component)) = parse_property_name(&name) else {
                log::error!("Graph Editor rejected invalid scoped property name {name:?}");
                return;
            };
            let prepared = project
                .read()
                .map_err(|error| error.to_string())
                .and_then(|project| {
                    let node = project
                        .get_node(entity_id)
                        .ok_or_else(|| format!("Graph Node {entity_id} does not exist"))?;
                    let property = node.properties().get(&property_key).ok_or_else(|| {
                        format!(
                            "Graph property {property_key:?} does not exist on Node {entity_id}"
                        )
                    })?;
                    if property.evaluator != "keyframe" {
                        return Err(format!("Graph property {property_key:?} is not keyframed"));
                    }
                    let keyframe = property
                        .keyframe_by_id(keyframe_id)
                        .ok_or_else(|| format!("Graph keyframe {keyframe_id} does not exist"))?;
                    let value = property_component_value(
                        &keyframe.value,
                        component.unwrap_or(PropertyComponent::Scalar),
                    )?;
                    let global_time =
                        time_mapper_for_owner(&project, PropertyOwner::Node(entity_id))
                            .to_global_time(keyframe.time.into_inner());
                    Ok((keyframe, PropertyOwner::Node(entity_id), global_time, value))
                });
            match prepared {
                Ok((keyframe, owner, global_time, value)) => {
                    editor_context.keyframe_dialog.is_open = true;
                    editor_context.keyframe_dialog.property_name = name;
                    editor_context.keyframe_dialog.owner = Some(owner);
                    editor_context.keyframe_dialog.property_key = property_key;
                    editor_context.keyframe_dialog.keyframe_id = Some(keyframe_id);
                    editor_context.keyframe_dialog.component = match component {
                        Some(PropertyComponent::X) => {
                            crate::state::context_types::KeyframeValueComponent::X
                        }
                        Some(PropertyComponent::Y) => {
                            crate::state::context_types::KeyframeValueComponent::Y
                        }
                        Some(PropertyComponent::Z) => {
                            crate::state::context_types::KeyframeValueComponent::Z
                        }
                        Some(PropertyComponent::W) => {
                            crate::state::context_types::KeyframeValueComponent::W
                        }
                        _ => crate::state::context_types::KeyframeValueComponent::Scalar,
                    };
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
        GraphKeyframeDragOrigin, GraphKeyframeDragState, SelectionTarget,
    };
    use library::cache::CacheManager;
    use library::model::property::{Keyframe, Property, Vec2, Vec3, Vec4};
    use library::model::{Clip, Composition};
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
                graph_property_name("b", PropertyComponent::W),
                keyframe_id,
                EasingFunction::EaseInQuad,
            ),
            Action::Remove(graph_property_name("b", PropertyComponent::W), keyframe_id),
        ] {
            process_action(
                action,
                composition_id,
                node_id,
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
                graph_property_name("b", PropertyComponent::Z),
                keyframe_id,
                EasingFunction::EaseInQuad,
            ),
            composition_id,
            node_id,
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

        process_action(
            Action::Add(property_name.clone(), 1.0, 8.0),
            composition_id,
            node_id,
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
                property_name: property_name.clone(),
                keyframe_id,
                global_time: 2.0,
                value: 9.0,
            }]),
            composition_id,
            node_id,
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
            node_id,
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
                property_name: graph_property_name("b", PropertyComponent::Scalar),
                keyframe_id,
                global_time: 3.0,
                value: 10.0,
            }]),
            composition_id,
            node_id,
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
            node_id,
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
        context.graph_editor.keyframe_drag = Some(GraphKeyframeDragState {
            target: SelectionTarget::Node(node_id),
            anchor: (anchor_name.clone(), direct_id),
            origins: vec![GraphKeyframeDragOrigin {
                property_name: anchor_name.clone(),
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
                    property_name: anchor_name.clone(),
                    keyframe_id: direct_id,
                    global_time,
                    value: 10.0 + offset,
                },
                KeyframeMove {
                    property_name: graph_property_name("position", PropertyComponent::X),
                    keyframe_id: position_id,
                    global_time,
                    value: 20.0 + offset,
                },
                KeyframeMove {
                    property_name: graph_property_name("position", PropertyComponent::Y),
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
            node_id,
            &service,
            &project,
            &mut context,
            &mut history,
        );
        process_action(
            Action::MoveBatch(movement(2.4, 4.0)),
            composition_id,
            node_id,
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
            node_id,
            &service,
            &project,
            &mut context,
            &mut history,
        );
        assert_eq!(history.undo_depth(), 2);
        process_action(
            Action::FinishMove,
            composition_id,
            node_id,
            &service,
            &project,
            &mut context,
            &mut history,
        );
        assert_eq!(history.undo_depth(), 2, "one gesture must commit once");

        let before_invalid_batch = project.read().unwrap().clone();
        context.graph_editor.keyframe_drag = Some(GraphKeyframeDragState {
            target: SelectionTarget::Node(node_id),
            anchor: (anchor_name.clone(), direct_id),
            origins: Vec::new(),
            changed: false,
        });
        process_action(
            Action::MoveBatch(vec![
                KeyframeMove {
                    property_name: anchor_name,
                    keyframe_id: direct_id,
                    global_time: 3.0,
                    value: 99.0,
                },
                KeyframeMove {
                    property_name: "effect:obsolete:amount".to_string(),
                    keyframe_id: direct_id,
                    global_time: 3.0,
                    value: 99.0,
                },
            ]),
            composition_id,
            node_id,
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
            node_id,
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
