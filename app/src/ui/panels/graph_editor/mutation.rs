//! Authoritative mutation routing for transient Graph property-stack rows.

use std::sync::{Arc, RwLock};

use library::animation::EasingFunction;
use library::editor::project_service::{
    SemanticAnimationSupport, SemanticPropertyAccess, SemanticPropertyOwner,
};
use library::model::project::{NodeContainer, Project};
use library::model::property::{
    Keyframe, KeyframeId, KeyframeUpdate, Property, PropertyMap, PropertyValue,
};
use library::{EditorService, PropertyOwner};

use crate::state::context_types::{GraphPropertyAddress, KeyframeValueComponent, SelectionTarget};

use super::projection::container_for_selection;
use super::utils::{
    property_component_value, time_mapper_for_owner, PropertyComponent, TimeMapper,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum GraphMutationRoute {
    Direct(PropertyOwner),
    Semantic(NodeContainer),
}

pub(crate) struct ResolvedGraphProperty {
    pub route: GraphMutationRoute,
    pub property: Property,
    pub property_map: PropertyMap,
    pub time_mapper: TimeMapper,
}

impl ResolvedGraphProperty {
    pub fn keyframe(&self, keyframe_id: KeyframeId) -> Result<Keyframe, String> {
        if self.property.evaluator != "keyframe" {
            return Err("Graph property is not keyframed".to_string());
        }
        self.property
            .keyframe_by_id(keyframe_id)
            .ok_or_else(|| format!("Graph keyframe {keyframe_id} does not exist"))
    }
}

pub(crate) fn property_component(component: KeyframeValueComponent) -> PropertyComponent {
    match component {
        KeyframeValueComponent::Scalar => PropertyComponent::Scalar,
        KeyframeValueComponent::X => PropertyComponent::X,
        KeyframeValueComponent::Y => PropertyComponent::Y,
        KeyframeValueComponent::Z => PropertyComponent::Z,
        KeyframeValueComponent::W => PropertyComponent::W,
    }
}

pub(crate) fn validate_keyframe_component(
    resolved: &ResolvedGraphProperty,
    address: &GraphPropertyAddress,
    keyframe_id: KeyframeId,
) -> Result<Keyframe, String> {
    let keyframe = resolved.keyframe(keyframe_id)?;
    property_component_value(&keyframe.value, property_component(address.component))?;
    Ok(keyframe)
}

pub(crate) fn resolve_graph_property(
    project_service: &EditorService,
    project: &Arc<RwLock<Project>>,
    address: &GraphPropertyAddress,
) -> Result<ResolvedGraphProperty, String> {
    match address.target {
        SelectionTarget::Node(node_id) => resolve_exact_node(project, address, node_id),
        SelectionTarget::Clip(_) | SelectionTarget::Track(_) | SelectionTarget::Composition(_) => {
            resolve_semantic_stack_property(project_service, project, address)
        }
    }
}

fn resolve_exact_node(
    project: &Arc<RwLock<Project>>,
    address: &GraphPropertyAddress,
    node_id: uuid::Uuid,
) -> Result<ResolvedGraphProperty, String> {
    if address.owner != SemanticPropertyOwner::ExactNode(node_id)
        || address.section_id != format!("node:{node_id}")
    {
        return Err("Graph property address does not belong to the selected Node".to_string());
    }
    let project = project.read().map_err(|error| error.to_string())?;
    let node = project
        .get_node(node_id)
        .ok_or_else(|| format!("Graph Node {node_id} does not exist"))?;
    let property = node
        .properties()
        .get(&address.property_key)
        .cloned()
        .ok_or_else(|| {
            format!(
                "Graph property {:?} does not exist on Node {node_id}",
                address.property_key
            )
        })?;
    Ok(ResolvedGraphProperty {
        route: GraphMutationRoute::Direct(PropertyOwner::Node(node_id)),
        property,
        property_map: node.properties().clone(),
        time_mapper: time_mapper_for_owner(&project, PropertyOwner::Node(node_id)),
    })
}

fn resolve_semantic_stack_property(
    project_service: &EditorService,
    project: &Arc<RwLock<Project>>,
    address: &GraphPropertyAddress,
) -> Result<ResolvedGraphProperty, String> {
    let root = container_for_selection(address.target)
        .ok_or_else(|| "Graph property address is not scoped to a container".to_string())?;
    let stack = project_service
        .semantic_container_property_stack(root)
        .map_err(|error| error.to_string())?;
    let mut matches = stack.sections().iter().filter_map(|section| {
        if section.stable_id() != address.section_id || section.owner() != address.owner {
            return None;
        }
        section
            .properties()
            .iter()
            .find(|entry| entry.key() == address.property_key)
            .map(|entry| (section, entry))
    });
    let (section, entry) = matches
        .next()
        .ok_or_else(|| "Graph property is no longer present in its semantic stack".to_string())?;
    if matches.next().is_some() {
        return Err("Graph property address is ambiguous in its semantic stack".to_string());
    }
    match entry.access() {
        SemanticPropertyAccess::Editable => {}
        SemanticPropertyAccess::Wired { .. } => {
            return Err("Graph property is wired and cannot be edited".to_string());
        }
        SemanticPropertyAccess::ReadOnly { reason, .. } => {
            return Err(format!("Graph property is read-only: {reason}"));
        }
    }
    if entry.animation() != SemanticAnimationSupport::Evaluator {
        return Err("Graph property does not support keyframe evaluators".to_string());
    }
    let mut property_map = PropertyMap::new();
    for property in section.properties() {
        property_map.set(property.key().to_string(), property.property().clone());
    }
    let route = route_for_owner(address.owner);
    let project = project.read().map_err(|error| error.to_string())?;
    Ok(ResolvedGraphProperty {
        route,
        property: entry.property().clone(),
        property_map,
        time_mapper: time_mapper_for_route(&project, route),
    })
}

fn route_for_owner(owner: SemanticPropertyOwner) -> GraphMutationRoute {
    match owner {
        SemanticPropertyOwner::DirectClip(id) => {
            GraphMutationRoute::Direct(PropertyOwner::Clip(id))
        }
        SemanticPropertyOwner::ExactNode(id) => GraphMutationRoute::Direct(PropertyOwner::Node(id)),
        SemanticPropertyOwner::SemanticContainer(owner) => GraphMutationRoute::Semantic(owner),
    }
}

fn time_mapper_for_route(project: &Project, route: GraphMutationRoute) -> TimeMapper {
    match route {
        GraphMutationRoute::Direct(owner) => time_mapper_for_owner(project, owner),
        GraphMutationRoute::Semantic(NodeContainer::Clip(id)) => {
            time_mapper_for_owner(project, PropertyOwner::Clip(id))
        }
        GraphMutationRoute::Semantic(NodeContainer::Track(_) | NodeContainer::Composition(_)) => {
            TimeMapper::identity()
        }
    }
}

pub(crate) fn add_keyframe(
    project_service: &EditorService,
    route: GraphMutationRoute,
    property_key: &str,
    time: f64,
    value: PropertyValue,
    easing: Option<EasingFunction>,
) -> Result<(), String> {
    match route {
        GraphMutationRoute::Direct(owner) => project_service
            .add_keyframe(owner, property_key, time, value, easing)
            .map_err(|error| error.to_string()),
        GraphMutationRoute::Semantic(owner) => project_service
            .add_semantic_container_keyframe(owner, property_key, time, value, easing)
            .map(|_| ())
            .map_err(|error| error.to_string()),
    }
}

pub(crate) fn update_keyframe(
    project_service: &EditorService,
    route: GraphMutationRoute,
    property_key: &str,
    keyframe_id: KeyframeId,
    update: KeyframeUpdate,
) -> Result<(), String> {
    match route {
        GraphMutationRoute::Direct(owner) => project_service
            .update_keyframe_by_id(owner, property_key, keyframe_id, update)
            .map_err(|error| error.to_string()),
        GraphMutationRoute::Semantic(owner) => project_service
            .update_semantic_container_keyframe_by_id(owner, property_key, keyframe_id, update)
            .map_err(|error| error.to_string()),
    }
}

pub(crate) fn remove_keyframe(
    project_service: &EditorService,
    route: GraphMutationRoute,
    property_key: &str,
    keyframe_id: KeyframeId,
) -> Result<(), String> {
    match route {
        GraphMutationRoute::Direct(owner) => project_service
            .remove_keyframe_by_id(owner, property_key, keyframe_id)
            .map_err(|error| error.to_string()),
        GraphMutationRoute::Semantic(owner) => project_service
            .remove_semantic_container_keyframe_by_id(owner, property_key, keyframe_id)
            .map_err(|error| error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::HistoryManager;
    use crate::state::context::EditorContext;
    use crate::state::context_types::{
        GraphKeyframeDragState, KeyframeDialogEditControl, KeyframeValueComponent,
    };
    use crate::ui::dialogs::keyframe_dialog::{
        apply_keyframe_dialog_change, flush_keyframe_dialog_transaction,
    };
    use crate::ui::panels::graph_editor::actions::{process_action, Action, KeyframeMove};
    use crate::ui::panels::graph_editor::projection::GraphPropertyProjection;
    use library::cache::CacheManager;
    use library::editor::project_service::{GeneratorNodeRequest, ProjectManager};
    use library::model::frame::color::Color;
    use library::model::project::{
        NodeGraphBundle, PortAddress, PortOwner, NUMBER_RESULT_OUTPUT_PORT,
    };
    use library::model::property::{Property, Vec2};
    use library::model::{Clip, Composition, Node};
    use library::plugin::{property_port_key, PluginManager};
    use ordered_float::OrderedFloat;

    struct Fixture {
        project: Arc<RwLock<Project>>,
        service: EditorService,
        composition_id: uuid::Uuid,
        track_id: uuid::Uuid,
        clip_id: uuid::Uuid,
    }

    fn number(value: f64) -> PropertyValue {
        PropertyValue::Number(OrderedFloat(value))
    }

    fn vec2(x: f64, y: f64) -> PropertyValue {
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
        })
    }

    fn fixture() -> Fixture {
        let plugins = Arc::new(PluginManager::default());
        let factory = ProjectManager::new(
            Arc::new(RwLock::new(Project::new("factory"))),
            Arc::clone(&plugins),
        );
        let source = factory
            .create_generator_node(
                GeneratorNodeRequest::Solid {
                    color: Color {
                        r: 20,
                        g: 40,
                        b: 60,
                        a: 255,
                    },
                },
                160,
                90,
                160,
                90,
            )
            .expect("Solid generator factory succeeds");
        let source_id = source.id;
        let mut project = Project::new("semantic Graph actions");
        let (composition, mut track) = Composition::new("main", 160, 90, 30.0, 12.0);
        let composition_id = composition.id;
        let track_id = track.id;
        track
            .properties
            .set("track_gain".to_string(), Property::constant(number(1.0)));
        project.add_track(track).expect("track insertion succeeds");
        project
            .add_composition(composition)
            .expect("composition insertion succeeds");
        let mut clip = Clip::new("solid", 4.0, 8.0);
        clip.trim_in = OrderedFloat(1.5);
        clip.time_stretch = OrderedFloat(0.5);
        for (key, value) in [
            ("position", vec2(18.0, 12.0)),
            ("rotation", number(7.0)),
            ("scale", vec2(125.0, 80.0)),
            ("anchor", vec2(4.0, 3.0)),
            ("opacity", number(50.0)),
            ("gain", number(2.0)),
        ] {
            clip.properties
                .set(key.to_string(), Property::constant(value));
        }
        let clip_id = clip.id;
        project.add_clip(clip);
        project
            .attach_clip_to_track(track_id, clip_id)
            .expect("clip attachment succeeds");
        project
            .insert_node_graph(
                NodeContainer::Clip(clip_id),
                NodeGraphBundle::new(vec![source], Vec::new(), Some(source_id)),
            )
            .expect("source graph insertion succeeds");
        let project = Arc::new(RwLock::new(project));
        let service =
            EditorService::new(Arc::clone(&project), plugins, Arc::new(CacheManager::new()))
                .expect("EditorService initializes");
        Fixture {
            project,
            service,
            composition_id,
            track_id,
            clip_id,
        }
    }

    fn projection(fixture: &Fixture, target: SelectionTarget) -> GraphPropertyProjection {
        let owner = container_for_selection(target).expect("container target");
        let stack = fixture
            .service
            .semantic_container_property_stack(owner)
            .expect("semantic stack resolves");
        GraphPropertyProjection::semantic(&fixture.project.read().expect("project read"), &stack)
    }

    fn address(
        projection: &GraphPropertyProjection,
        property_key: &str,
        component: KeyframeValueComponent,
    ) -> GraphPropertyAddress {
        projection
            .rows()
            .find(|row| {
                row.property_key == property_key
                    && row
                        .address()
                        .is_some_and(|address| address.component == component)
            })
            .and_then(|row| row.address())
            .expect("typed Graph row exists")
    }

    fn semantic_property(fixture: &Fixture, property_key: &str) -> Property {
        let stack = fixture
            .service
            .semantic_container_property_stack(NodeContainer::Clip(fixture.clip_id))
            .expect("semantic stack resolves");
        stack
            .sections()
            .iter()
            .filter(|section| {
                section.owner()
                    == SemanticPropertyOwner::SemanticContainer(NodeContainer::Clip(
                        fixture.clip_id,
                    ))
            })
            .flat_map(|section| section.properties())
            .find(|entry| entry.key() == property_key)
            .expect("semantic property exists")
            .property()
            .clone()
    }

    #[test]
    fn semantic_clip_vector_actions_use_clip_time_and_one_history_state_per_gesture() {
        let fixture = fixture();
        let projection = projection(&fixture, SelectionTarget::Clip(fixture.clip_id));
        let position_x = address(&projection, "position", KeyframeValueComponent::X);
        let position_y = address(&projection, "position", KeyframeValueComponent::Y);
        let mut context = EditorContext::new(fixture.composition_id);
        let mut history = HistoryManager::new();
        history.push_project_state(fixture.project.read().expect("project read").clone());

        process_action(
            Action::Add(position_x.clone(), 6.25, 99.0),
            fixture.composition_id,
            &fixture.service,
            &fixture.project,
            &mut context,
            &mut history,
        );
        assert_eq!(history.undo_depth(), 2);
        let position = semantic_property(&fixture, "position");
        let keyframe = position
            .keyframes()
            .first()
            .cloned()
            .expect("semantic keyframe was added");
        assert_eq!(keyframe.time.into_inner(), 2.625);
        assert_eq!(keyframe.value, vec2(99.0, 12.0));

        context.graph_editor.keyframe_drag = Some(GraphKeyframeDragState {
            target: SelectionTarget::Clip(fixture.clip_id),
            anchor: (position_x.stable_id.clone(), keyframe.id),
            origins: Vec::new(),
            changed: false,
        });
        process_action(
            Action::MoveBatch(vec![
                KeyframeMove {
                    address: position_x.clone(),
                    keyframe_id: keyframe.id,
                    global_time: 7.0,
                    value: 101.0,
                },
                KeyframeMove {
                    address: position_y,
                    keyframe_id: keyframe.id,
                    global_time: 7.0,
                    value: 55.0,
                },
            ]),
            fixture.composition_id,
            &fixture.service,
            &fixture.project,
            &mut context,
            &mut history,
        );
        assert_eq!(history.undo_depth(), 2, "drag frames stay pending");
        assert!(
            context
                .graph_editor
                .keyframe_drag
                .as_ref()
                .expect("drag remains active")
                .changed
        );
        let moved = semantic_property(&fixture, "position")
            .keyframe_by_id(keyframe.id)
            .expect("semantic keyframe survives move");
        assert_eq!(moved.time.into_inner(), 3.0);
        assert_eq!(moved.value, vec2(101.0, 55.0));

        process_action(
            Action::FinishMove,
            fixture.composition_id,
            &fixture.service,
            &fixture.project,
            &mut context,
            &mut history,
        );
        assert_eq!(history.undo_depth(), 3);
        process_action(
            Action::EditKeyframe(position_x.clone(), keyframe.id),
            fixture.composition_id,
            &fixture.service,
            &fixture.project,
            &mut context,
            &mut history,
        );
        assert_eq!(
            context.keyframe_dialog.graph_address,
            Some(position_x.clone())
        );
        assert_eq!(context.keyframe_dialog.time, 7.0);
        let baseline = context.keyframe_dialog.values();
        context.keyframe_dialog.time = 7.5;
        context.keyframe_dialog.value = 111.0;
        assert!(apply_keyframe_dialog_change(
            &mut context.keyframe_dialog,
            KeyframeDialogEditControl::Value,
            baseline,
            &mut history,
            &fixture.service,
            &fixture.project,
        ));
        assert_eq!(history.undo_depth(), 3, "dialog gesture stays pending");
        assert!(flush_keyframe_dialog_transaction(
            &mut context.keyframe_dialog,
            &mut history,
            &fixture.service,
        ));
        assert_eq!(history.undo_depth(), 4);
        let dialog_edited = semantic_property(&fixture, "position")
            .keyframe_by_id(keyframe.id)
            .expect("semantic dialog update keeps keyframe identity");
        assert_eq!(dialog_edited.time.into_inner(), 3.25);
        assert_eq!(dialog_edited.value, vec2(111.0, 55.0));

        process_action(
            Action::Remove(position_x, keyframe.id),
            fixture.composition_id,
            &fixture.service,
            &fixture.project,
            &mut context,
            &mut history,
        );
        assert_eq!(history.undo_depth(), 5);
        assert!(semantic_property(&fixture, "position")
            .keyframes()
            .is_empty());
    }

    #[test]
    fn direct_clip_exact_node_wired_read_only_and_constant_only_routes_fail_closed() {
        let fixture = fixture();
        let effect_id = fixture
            .service
            .append_semantic_container_effect(NodeContainer::Clip(fixture.clip_id), "blur")
            .expect("Blur effect appends");
        {
            let mut project = fixture.project.write().expect("project write");
            let driver = Node::new_add("sigma driver");
            let driver_id = driver.id;
            project.add_node(driver);
            project
                .attach_node_to_container(NodeContainer::Clip(fixture.clip_id), driver_id)
                .expect("driver attaches");
            project
                .connect_ports(
                    PortAddress::new(PortOwner::Node(driver_id), NUMBER_RESULT_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(effect_id), property_port_key("sigma_y")),
                )
                .expect("property wire connects");
        }
        let clip_projection = projection(&fixture, SelectionTarget::Clip(fixture.clip_id));
        let gain = address(&clip_projection, "gain", KeyframeValueComponent::Scalar);
        let sigma_x = address(&clip_projection, "sigma_x", KeyframeValueComponent::Scalar);
        let sigma_y = address(&clip_projection, "sigma_y", KeyframeValueComponent::Scalar);
        let timing = clip_projection
            .sections
            .iter()
            .find(|section| section.stable_id == "clip:timing")
            .and_then(|section| section.rows.iter().find_map(|row| row.address()))
            .expect("numeric timing address exists");
        let track_projection = projection(&fixture, SelectionTarget::Track(fixture.track_id));
        let track_gain = address(
            &track_projection,
            "track_gain",
            KeyframeValueComponent::Scalar,
        );
        let mut context = EditorContext::new(fixture.composition_id);
        let mut history = HistoryManager::new();
        history.push_project_state(fixture.project.read().expect("project read").clone());

        for editable in [gain, sigma_x] {
            process_action(
                Action::Add(editable, 6.0, 8.0),
                fixture.composition_id,
                &fixture.service,
                &fixture.project,
                &mut context,
                &mut history,
            );
        }
        assert_eq!(history.undo_depth(), 3);
        assert_eq!(
            fixture
                .project
                .read()
                .expect("project read")
                .get_clip(fixture.clip_id)
                .and_then(|clip| clip.properties.get("gain"))
                .and_then(|property| property.keyframes().first().cloned())
                .expect("DirectClip keyframe exists")
                .time
                .into_inner(),
            2.5
        );
        assert_eq!(
            fixture
                .project
                .read()
                .expect("project read")
                .get_node(effect_id)
                .and_then(|node| node.properties().get("sigma_x"))
                .map(|property| property.evaluator.as_str()),
            Some("keyframe")
        );

        let before_rejected = fixture.project.read().expect("project read").clone();
        for rejected in [sigma_y, timing, track_gain] {
            process_action(
                Action::Add(rejected, 6.0, 99.0),
                fixture.composition_id,
                &fixture.service,
                &fixture.project,
                &mut context,
                &mut history,
            );
        }
        assert_eq!(
            *fixture.project.read().expect("project read"),
            before_rejected
        );
        assert_eq!(history.undo_depth(), 3);
    }
}
