use super::utils::time_mapper_for_owner;
use super::PropertyComponent;
use crate::action::HistoryManager;
use crate::state::context::EditorContext;
use crate::utils::lock::read_or_recover;
use library::animation::EasingFunction;
use library::model::project::Project;
use library::model::property::{KeyframeId, KeyframeUpdate, PropertyValue};
use library::model::Node;
use library::{EditorService, KeyframeBatchUpdate, PropertyOwner};
use ordered_float::OrderedFloat;
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

fn merge_component(
    current: Option<PropertyValue>,
    value: f64,
    component: Option<PropertyComponent>,
) -> PropertyValue {
    if let Some(PropertyValue::Vec2(old)) = current {
        match component {
            Some(PropertyComponent::X) => PropertyValue::Vec2(library::model::property::Vec2 {
                x: OrderedFloat(value),
                y: old.y,
            }),
            Some(PropertyComponent::Y) => PropertyValue::Vec2(library::model::property::Vec2 {
                x: old.x,
                y: OrderedFloat(value),
            }),
            _ => PropertyValue::Number(OrderedFloat(value)),
        }
    } else {
        PropertyValue::Number(OrderedFloat(value))
    }
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
        if matches!(component, Some(PropertyComponent::X | PropertyComponent::Y))
            && !matches!(current, PropertyValue::Vec2(_))
        {
            return Err(format!(
                "Graph component {:?} does not address a Vec2 keyframe",
                component
            ));
        }
        let value = merge_component(Some(current), movement.value, component);
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
            let prepared = project.read().ok().and_then(|project| {
                let composition = project.get_composition(comp_id)?;
                let node = project.get_node(entity_id)?;
                let source_time = time_mapper_for_owner(&project, PropertyOwner::Node(entity_id))
                    .to_source_time(time);
                let current = node.properties().get(&property_key).and_then(|property| {
                    project_service
                        .evaluate_property_value(
                            property,
                            node.properties(),
                            source_time,
                            composition.fps,
                            (composition.width, composition.height),
                        )
                        .ok()
                });
                Some((
                    PropertyOwner::Node(entity_id),
                    source_time,
                    merge_component(current, value, component),
                ))
            });
            if let Some((owner, source_time, value)) = prepared {
                if project_service
                    .add_keyframe(owner, &property_key, source_time, value, None)
                    .is_ok()
                {
                    push_history(project, history_manager);
                }
            }
        }
        Action::SetEasing(name, keyframe_id, easing) => {
            let Some((property_key, _)) = parse_property_name(&name) else {
                log::error!("Graph Editor rejected invalid scoped property name {name:?}");
                return;
            };
            let owner = project.read().ok().and_then(|project| {
                project
                    .get_node(entity_id)
                    .map(|_| PropertyOwner::Node(entity_id))
            });
            if let Some(owner) = owner {
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
        }
        Action::Remove(name, keyframe_id) => {
            let Some((property_key, _)) = parse_property_name(&name) else {
                log::error!("Graph Editor rejected invalid scoped property name {name:?}");
                return;
            };
            let owner = project.read().ok().and_then(|project| {
                project
                    .get_node(entity_id)
                    .map(|_| PropertyOwner::Node(entity_id))
            });
            if let Some(owner) = owner {
                if project_service
                    .remove_keyframe_by_id(owner, &property_key, keyframe_id)
                    .is_ok()
                {
                    push_history(project, history_manager);
                }
            }
        }
        Action::EditKeyframe(name, keyframe_id) => {
            let Some((property_key, component)) = parse_property_name(&name) else {
                log::error!("Graph Editor rejected invalid scoped property name {name:?}");
                return;
            };
            let keyframe = project.read().ok().and_then(|project| {
                let node = project.get_node(entity_id)?;
                let property = node.properties().get(&property_key)?;
                if property.evaluator != "keyframe" {
                    return None;
                }
                property
                    .keyframe_by_id(keyframe_id)
                    .map(|keyframe| (keyframe, PropertyOwner::Node(entity_id)))
            });
            if let Some((keyframe, owner)) = keyframe {
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
                    _ => crate::state::context_types::KeyframeValueComponent::Scalar,
                };
                editor_context.keyframe_dialog.time =
                    project
                        .read()
                        .ok()
                        .map_or(keyframe.time.into_inner(), |project| {
                            time_mapper_for_owner(&project, PropertyOwner::Node(entity_id))
                                .to_global_time(keyframe.time.into_inner())
                        });
                editor_context.keyframe_dialog.value = match component {
                    Some(PropertyComponent::X) => keyframe
                        .value
                        .get_as::<library::model::property::Vec2>()
                        .map_or(0.0, |value| value.x.into_inner()),
                    Some(PropertyComponent::Y) => keyframe
                        .value
                        .get_as::<library::model::property::Vec2>()
                        .map_or(0.0, |value| value.y.into_inner()),
                    _ => keyframe.value.get_as::<f64>().unwrap_or(0.0),
                };
                editor_context.keyframe_dialog.easing = keyframe.easing;
                editor_context.keyframe_dialog.begin_transaction();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::context_types::{GraphKeyframeDragOrigin, GraphKeyframeDragState};
    use crate::test_support::generator_node;
    use library::cache::CacheManager;
    use library::editor::project_service::GeneratorNodeRequest;
    use library::model::frame::color::Color;
    use library::model::property::{Keyframe, Property, Vec2};
    use library::model::{Clip, Composition};
    use library::plugin::PluginManager;

    fn number(value: f64) -> PropertyValue {
        PropertyValue::Number(OrderedFloat(value))
    }

    fn keyframed_number(time: f64, value: f64) -> (Property, KeyframeId) {
        let keyframe = Keyframe::new(time, number(value), EasingFunction::Linear);
        let id = keyframe.id;
        (Property::keyframe(vec![keyframe]), id)
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
        let mut node = generator_node(
            "graph target",
            GeneratorNodeRequest::Solid {
                color: Color::default(),
            },
        );
        let node_id = node.id;
        node.set_property("opacity".to_string(), direct_property)
            .expect("solid factory initializes opacity");
        node.set_property(
            "position".to_string(),
            Property::keyframe(vec![position_keyframe]),
        )
        .expect("solid factory initializes position");
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
        let anchor_name = graph_property_name("opacity", PropertyComponent::Scalar);
        context.graph_editor.keyframe_drag = Some(GraphKeyframeDragState {
            entity_id: node_id,
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
            property_value(&read, node_id, "opacity", direct_id),
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
            entity_id: node_id,
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
    fn graph_selection_and_drag_state_do_not_leak_between_nodes() {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let keyframe_id = KeyframeId::new();
        let mut state = crate::state::context_types::GraphEditorState::default();
        assert!(state.begin_entity(first));
        state
            .selected_keyframes
            .insert(("node:amount".to_string(), keyframe_id));
        state.keyframe_drag = Some(GraphKeyframeDragState {
            entity_id: first,
            anchor: ("node:amount".to_string(), keyframe_id),
            origins: Vec::new(),
            changed: true,
        });

        assert!(!state.begin_entity(first));
        assert_eq!(state.selected_keyframes.len(), 1);
        assert!(state.keyframe_drag.is_some());
        assert!(state.begin_entity(second));
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
            entity_id,
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
