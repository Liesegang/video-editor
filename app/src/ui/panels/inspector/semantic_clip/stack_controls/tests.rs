use std::collections::{BTreeSet, HashMap};
use std::error::Error;
use std::io;
use std::sync::{Arc, RwLock};

use library::cache::CacheManager;
use library::model::project::{
    NodeContainer, PortAddress, PortOwner, Project, ProjectConnection, IMAGE_INPUT_PORT,
    IMAGE_OUTPUT_PORT, NUMBER_RESULT_OUTPUT_PORT,
};
use library::model::property::{Property, PropertyValue};
use library::model::Node;
use library::plugin::{property_port_key, PluginManager, IMAGE_OPACITY_STYLE_COMPONENT_ID};
use library::EditorService;

use super::*;
use crate::state::context::EditorContext;
use crate::state::context_types::SelectionTarget;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

struct Fixture {
    service: EditorService,
    project: Arc<RwLock<Project>>,
    composition_id: Uuid,
    clip_id: Uuid,
    owner: NodeContainer,
}

fn fixture() -> TestResult<Fixture> {
    let project = Arc::new(RwLock::new(Project::new("semantic stack Inspector")));
    let service = EditorService::new(
        Arc::clone(&project),
        Arc::new(PluginManager::default()),
        Arc::new(CacheManager::new()),
    )?;
    let composition_id = service.add_composition("Comp", 1920, 1080, 30.0, 10.0)?;
    let track_id = service.add_track(composition_id, "Track")?;
    let bundle = service.create_shape_clip(0.0, 5.0, 1920, 1080)?;
    let clip_id = service.add_clip_to_track(composition_id, track_id, bundle, None)?;
    Ok(Fixture {
        service,
        project,
        composition_id,
        clip_id,
        owner: NodeContainer::Clip(clip_id),
    })
}

fn snapshot(project: &Arc<RwLock<Project>>) -> Result<Project, io::Error> {
    project
        .read()
        .map(|project| project.clone())
        .map_err(|_| io::Error::other("Project read lock poisoned"))
}

fn connection_map(project: &Project) -> HashMap<Uuid, ProjectConnection> {
    project
        .connections
        .iter()
        .cloned()
        .map(|connection| (connection.id, connection))
        .collect()
}

#[test]
fn descriptor_catalogs_are_typed_hierarchical_and_exclude_image_opacity_as_shape_style(
) -> TestResult {
    let fixture = fixture()?;
    let effects = effect_catalog(&fixture.service, fixture.clip_id);
    assert!(effects.iter().any(|item| {
        matches!(&item.value, StackAction::AppendEffect(id) if id == "blur")
            && item
                .category
                .as_deref()
                .is_some_and(|path| path.starts_with("Effect / "))
    }));
    let styles = style_catalog(&fixture.service, fixture.clip_id, None);
    assert!(styles.iter().any(|item| {
        matches!(&item.value, StackAction::AppendStyle { component_id, after: None } if component_id == "fill")
            && item.category.as_deref() == Some("Style / Shape")
    }));
    assert!(!styles.iter().any(|item| {
        matches!(&item.value, StackAction::AppendStyle { component_id, .. } if component_id == IMAGE_OPACITY_STYLE_COMPONENT_ID)
    }));
    let decorators = decorator_catalog(
        &fixture.service,
        fixture.clip_id,
        DecoratorAnchor::Style(Uuid::new_v4()),
    );
    assert!(decorators.iter().all(|item| {
        item.category.as_deref() == Some("Decorator / Shape")
            && item
                .qa_id
                .as_deref()
                .is_some_and(|id| id.starts_with("inspector.semantic.menu.decorator:"))
    }));
    assert_eq!(
        transform_catalog(&fixture.service, fixture.clip_id).len(),
        1
    );
    Ok(())
}

#[test]
fn anchored_decorator_action_qa_ids_are_unique_extensions_of_their_row_ids() {
    let clip_id = Uuid::from_u128(1);
    let node_id = Uuid::from_u128(2);
    let first_anchor = Uuid::from_u128(3);
    let second_anchor = Uuid::from_u128(4);
    let first_row = qa::stack_item_qa_id("decorator", clip_id, node_id, Some(first_anchor));
    let second_row = qa::stack_item_qa_id("decorator", clip_id, node_id, Some(second_anchor));
    let first_move = qa::stack_action_qa_id(
        "decorator",
        clip_id,
        node_id,
        Some(first_anchor),
        StackQaAction::MoveUp,
    );
    let second_move = qa::stack_action_qa_id(
        "decorator",
        clip_id,
        node_id,
        Some(second_anchor),
        StackQaAction::MoveUp,
    );

    assert_ne!(first_row, second_row);
    assert_ne!(first_move, second_move);
    assert_eq!(first_move, format!("{first_row}.move_up"));
    assert_eq!(second_move, format!("{second_row}.move_up"));
    assert!(first_row.contains(&format!("anchor:{first_anchor}:node:{node_id}")));
}

#[test]
fn reorder_and_remove_qa_metadata_publish_distinct_preservation_contracts() {
    let node_id = Uuid::from_u128(10);
    let anchor_id = Uuid::from_u128(11);
    let reorder = qa::stack_action_qa_metadata(
        "style",
        node_id,
        None,
        StackQaAction::MoveDown,
        "reorder existing Merge wires",
    );
    let remove = qa::stack_action_qa_metadata(
        "decorator",
        node_id,
        Some(anchor_id),
        StackQaAction::Remove,
        "typed semantic remove",
    );
    let reorder_preserves = reorder["preserves"].as_array().expect("reorder preserves");
    let remove_preserves = remove["preserves"].as_array().expect("remove preserves");

    assert_eq!(reorder["action"], "reorder");
    assert!(reorder_preserves.iter().any(|item| item == "node_uuid"));
    assert!(reorder_preserves
        .iter()
        .any(|item| item == "external_property_wires"));
    assert_eq!(reorder["changes"], serde_json::json!(["merge_input_order"]));

    assert_eq!(remove["action"], "remove");
    assert_eq!(remove["style_anchor_id"], anchor_id.to_string());
    for false_claim in [
        "node_uuid",
        "properties",
        "external_property_wires",
        "fanout",
    ] {
        assert!(!remove_preserves.iter().any(|item| item == false_claim));
    }
    assert_eq!(
        remove["changes"],
        serde_json::json!(["target_node", "incident_wires", "semantic_stack_topology"])
    );
}

#[test]
fn effect_actions_append_at_post_merge_tail_and_reorder_only_main_flow_endpoints() -> TestResult {
    let fixture = fixture()?;
    let mut history = HistoryManager::new();
    history.push_project_state(snapshot(&fixture.project)?);
    let mut editor_context = EditorContext::new(fixture.composition_id);
    editor_context.select_target(SelectionTarget::Clip(fixture.clip_id));
    let selected_before = editor_context.selection.primary();
    let initial_depth = history.undo_depth();

    for effect in ["blur", "drop_shadow", "tile"] {
        execute_with_history(
            &fixture.service,
            &mut history,
            fixture.owner,
            StackAction::AppendEffect(effect.to_string()),
        )?;
    }
    assert_eq!(history.undo_depth(), initial_depth + 3);
    let effect_stack = fixture
        .service
        .semantic_container_effect_stack(fixture.owner)?;
    let [blur, shadow, tile] = effect_stack.node_ids() else {
        return Err(io::Error::other("three Effects were not appended").into());
    };
    let (blur, shadow, tile) = (*blur, *shadow, *tile);
    let merge_id = fixture
        .service
        .semantic_container_style_stack(fixture.owner)?
        .merge_node_id()
        .ok_or_else(|| io::Error::other("Shape Clip semantic Merge missing"))?;
    assert!(snapshot(&fixture.project)?
        .connections
        .iter()
        .any(|connection| {
            connection.from == PortAddress::new(PortOwner::Node(merge_id), IMAGE_OUTPUT_PORT)
                && connection.to == PortAddress::new(PortOwner::Node(blur), IMAGE_INPUT_PORT)
        }));

    let (property_wire, fanout_wire) = {
        let mut project = fixture
            .project
            .write()
            .map_err(|_| io::Error::other("Project write lock poisoned"))?;
        project
            .get_node_mut(blur)
            .ok_or_else(|| io::Error::other("Blur missing"))?
            .set_property(
                "sigma_x".to_string(),
                Property::expression("3.0 + time".to_string(), PropertyValue::from(3.0)),
            )
            .map_err(io::Error::other)?;
        let driver = Node::new_add("Blur driver");
        let driver_id = driver.id;
        project.add_node(driver);
        project.attach_node_to_container(fixture.owner, driver_id)?;
        let property_wire = project.connect_ports(
            PortAddress::new(PortOwner::Node(driver_id), NUMBER_RESULT_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(blur), property_port_key("sigma_y")),
        )?;
        let fanout = fixture
            .service
            .get_plugin_manager()
            .create_image_transform_operation_node()?;
        let fanout_id = fanout.id;
        project.add_node(fanout);
        project.attach_node_to_container(fixture.owner, fanout_id)?;
        let fanout_wire = project.connect_ports(
            PortAddress::new(PortOwner::Node(shadow), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(fanout_id), IMAGE_INPUT_PORT),
        )?;
        (property_wire, fanout_wire)
    };
    history.push_project_state(snapshot(&fixture.project)?);
    let before = snapshot(&fixture.project)?;
    let before_connections = connection_map(&before);
    let before_nodes = [blur, shadow, tile]
        .into_iter()
        .map(|id| {
            before
                .get_node(id)
                .cloned()
                .map(|node| (id, node))
                .ok_or_else(|| io::Error::other(format!("Effect {id} missing")))
        })
        .collect::<Result<HashMap<_, _>, _>>()?;
    let history_before_reorder = history.undo_depth();

    execute_with_history(
        &fixture.service,
        &mut history,
        fixture.owner,
        StackAction::ReorderEffects(vec![tile, blur, shadow]),
    )?;
    let after = snapshot(&fixture.project)?;
    let after_connections = connection_map(&after);
    assert_eq!(
        fixture
            .service
            .semantic_container_effect_stack(fixture.owner)?
            .node_ids(),
        &[tile, blur, shadow]
    );
    assert_eq!(history.undo_depth(), history_before_reorder + 1);
    assert_eq!(editor_context.selection.primary(), selected_before);
    assert_eq!(
        before_connections.keys().copied().collect::<BTreeSet<_>>(),
        after_connections.keys().copied().collect::<BTreeSet<_>>(),
        "reorder must not allocate or delete wires"
    );
    for (connection_id, original) in &before_connections {
        let current = after_connections
            .get(connection_id)
            .ok_or_else(|| io::Error::other("connection disappeared"))?;
        assert_eq!(
            (current.order, current.blend_mode),
            (original.order, original.blend_mode),
            "connection metadata changed for {connection_id}"
        );
    }
    for connection_id in [property_wire, fanout_wire] {
        assert_eq!(
            after_connections.get(&connection_id),
            before_connections.get(&connection_id),
            "external Effect wire changed"
        );
    }
    for (node_id, original) in before_nodes {
        assert_eq!(after.get_node(node_id), Some(&original));
    }
    Ok(())
}

#[test]
fn rejected_stack_action_preserves_project_history_and_clip_selection() -> TestResult {
    let fixture = fixture()?;
    let mut history = HistoryManager::new();
    history.push_project_state(snapshot(&fixture.project)?);
    execute_with_history(
        &fixture.service,
        &mut history,
        fixture.owner,
        StackAction::AppendEffect("blur".to_string()),
    )?;
    let blur = fixture
        .service
        .semantic_container_effect_stack(fixture.owner)?
        .node_ids()[0];
    let before = snapshot(&fixture.project)?;
    let history_before = history.undo_depth();
    let mut editor_context = EditorContext::new(fixture.composition_id);
    editor_context.select_target(SelectionTarget::Clip(fixture.clip_id));

    assert!(execute_with_history(
        &fixture.service,
        &mut history,
        fixture.owner,
        StackAction::ReorderEffects(vec![blur, blur]),
    )
    .is_err());
    assert_eq!(snapshot(&fixture.project)?, before);
    assert_eq!(history.undo_depth(), history_before);
    assert_eq!(
        editor_context.selection.primary(),
        Some(SelectionTarget::Clip(fixture.clip_id))
    );
    Ok(())
}
