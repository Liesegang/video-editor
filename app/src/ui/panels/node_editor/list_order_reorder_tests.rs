use super::merge_reorder_tests::{pointer_button, render_merge_frame};
use super::*;
use library::model::project::connection::LIST_ITEMS_INPUT_PORT;
use library::model::project::{NUMBER_RESULT_OUTPUT_PORT, PortAddress, ProjectConnection};
use library::model::{Composition, ListContent};

struct ListUiFixture {
    project: Project,
    composition_id: Uuid,
    make_id: Uuid,
    source_ids: [Uuid; 2],
    connection_ids: [Uuid; 3],
}

fn list_ui_fixture() -> ListUiFixture {
    let mut project = Project::new("List ordered input UI");
    let (mut composition, mut track) = Composition::new("Main", 640, 360, 30.0, 4.0);
    composition.ui_position = [10.0, 20.0];
    composition.ui_size = [1500.0, 1000.0];
    track.ui_position = [90.0, 120.0];
    track.ui_size = [1250.0, 760.0];
    let composition_id = composition.id;
    let track_id = track.id;
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();
    let mut clip = Clip::new("Values", 0.0, 4.0);
    clip.ui_position = [180.0, 220.0];
    clip.ui_size = [980.0, 560.0];
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();

    let mut source_ids = [Uuid::nil(); 2];
    for (index, name) in ["First Value", "Second Value"].into_iter().enumerate() {
        let mut node = Node::new_add(name);
        node.ui_position = [310.0, 300.0 + index as f32 * 140.0];
        source_ids[index] = node.id;
        project.add_node(node);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), source_ids[index])
            .unwrap();
    }
    let mut make = Node::new_list("Make List", ListContent::Make);
    make.ui_position = [700.0, 340.0];
    let make_id = make.id;
    project.add_node(make);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), make_id)
        .unwrap();
    let target = PortAddress::new(PortOwner::Node(make_id), LIST_ITEMS_INPUT_PORT);
    let mut connection_ids = [Uuid::nil(); 3];
    for (slot, source_id) in [source_ids[0], source_ids[1], source_ids[0]]
        .into_iter()
        .enumerate()
    {
        connection_ids[slot] = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(source_id), NUMBER_RESULT_OUTPUT_PORT),
                target.clone(),
            )
            .unwrap();
    }
    assert!(project.validate_connections().is_empty());
    ListUiFixture {
        project,
        composition_id,
        make_id,
        source_ids,
        connection_ids,
    }
}

fn canonical_connections(project: &Project, make_id: Uuid) -> Vec<ProjectConnection> {
    let target = PortAddress::new(PortOwner::Node(make_id), LIST_ITEMS_INPUT_PORT);
    let mut connections = project
        .connections
        .iter()
        .filter(|connection| connection.to == target)
        .cloned()
        .collect::<Vec<_>>();
    connections.sort_by_key(|connection| (connection.order, connection.id));
    connections
}

fn raw_drag(
    project: &Project,
    composition_id: Uuid,
    state: &mut NodeEditorState,
    make_id: Uuid,
    connection_id: Uuid,
    target_connection_id: Uuid,
) -> Vec<QueuedNodeEdit> {
    let context = egui::Context::default();
    let mut frame = 0;
    for _ in 0..7 {
        let rendered =
            render_merge_frame(&context, project, composition_id, state, frame, Vec::new());
        assert_eq!(rendered.layout_edit_count, 0);
        frame += 1;
    }
    let start = test_rect(&format!(
        "node_editor.merge_layer.drag_handle:{make_id}:{connection_id}"
    ))
    .unwrap()
    .center();
    let destination = test_rect(&format!(
        "node_editor.merge_layer:{make_id}:{target_connection_id}"
    ))
    .unwrap()
    .center();
    let mut edits = Vec::new();
    for events in [
        vec![egui::Event::PointerMoved(start)],
        vec![pointer_button(start, true)],
        vec![egui::Event::PointerMoved(destination)],
        vec![pointer_button(destination, false)],
    ] {
        edits.extend(
            render_merge_frame(&context, project, composition_id, state, frame, events).edits,
        );
        frame += 1;
    }
    edits
}

#[test]
fn make_list_projects_duplicate_connections_as_distinct_ordered_rows_with_qa() {
    let fixture = list_ui_fixture();
    let rows = merge_layer_rows(&fixture.project, fixture.make_id);
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows.iter().map(|row| row.connection_id).collect::<Vec<_>>(),
        fixture.connection_ids
    );
    assert!(rows.iter().enumerate().all(|(index, row)| {
        row.kind == NativeVariadicMergeKind::List
            && row.canonical_index == index
            && row.visual_index == index
            && !row.authored_blend_available
    }));
    assert_eq!(rows[0].source, rows[2].source);
    assert_ne!(rows[0].connection_id, rows[2].connection_id);
    assert!(matches!(
        merge_input_slots(&fixture.project, fixture.make_id)
            .last()
            .map(|slot| &slot.role),
        Some(MergeInputSlotRole::Vacant(NativeVariadicMergeKind::List))
    ));

    let context = egui::Context::default();
    let mut state = NodeEditorState::default();
    for frame in 0..7 {
        render_merge_frame(
            &context,
            &fixture.project,
            fixture.composition_id,
            &mut state,
            frame,
            Vec::new(),
        );
    }
    for (index, connection_id) in fixture.connection_ids.into_iter().enumerate() {
        let id = format!(
            "node_editor.merge_layer:{}:{connection_id}",
            fixture.make_id
        );
        let metadata = test_metadata(&id).expect("ordered List row QA metadata");
        assert_eq!(metadata["ordered_input"], true);
        assert_eq!(metadata["input_kind"], "list");
        assert_eq!(metadata["canonical_index"], index);
        assert_eq!(metadata["visual_index"], index);
        assert_eq!(metadata["connection_id"], connection_id.to_string());
        assert!(test_rect(&id).is_some());
    }
}

#[test]
fn real_pointer_list_row_drag_reorders_by_connection_identity_and_roundtrips() {
    let mut fixture = list_ui_fixture();
    let mut state = NodeEditorState::default();
    let edits = raw_drag(
        &fixture.project,
        fixture.composition_id,
        &mut state,
        fixture.make_id,
        fixture.connection_ids[0],
        fixture.connection_ids[2],
    );
    assert!(matches!(
        edits.as_slice(),
        [QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
            connection_id,
            new_order: 2,
        })] if *connection_id == fixture.connection_ids[0]
    ));
    assert!(apply_edit(
        &mut fixture.project,
        match edits.into_iter().next().unwrap() {
            QueuedNodeEdit::Atomic(edit) => edit,
            QueuedNodeEdit::Continuous { .. } => panic!("row drag must be atomic"),
        }
    ));
    assert_eq!(
        canonical_connections(&fixture.project, fixture.make_id)
            .iter()
            .map(|connection| connection.id)
            .collect::<Vec<_>>(),
        vec![
            fixture.connection_ids[1],
            fixture.connection_ids[2],
            fixture.connection_ids[0],
        ]
    );
    assert!(
        canonical_connections(&fixture.project, fixture.make_id)
            .iter()
            .enumerate()
            .all(|(index, connection)| connection.order == index as i64)
    );

    let restored: Project =
        serde_json::from_str(&serde_json::to_string(&fixture.project).unwrap()).unwrap();
    assert_eq!(
        canonical_connections(&restored, fixture.make_id)
            .iter()
            .map(|connection| (connection.id, connection.order))
            .collect::<Vec<_>>(),
        canonical_connections(&fixture.project, fixture.make_id)
            .iter()
            .map(|connection| (connection.id, connection.order))
            .collect::<Vec<_>>()
    );
    assert!(restored.validate_connections().is_empty());
}

#[test]
fn vacant_list_pin_can_author_a_second_slot_from_the_same_source() {
    let mut fixture = list_ui_fixture();
    fixture
        .project
        .disconnect_connections([fixture.connection_ids[1], fixture.connection_ids[2]]);
    let (snarl, _) = build_snarl(&fixture.project, fixture.composition_id);
    let source_snarl_id = snarl
        .nodes_ids_data()
        .find_map(|(id, node)| (node.value == GraphItem::Node(fixture.source_ids[0])).then_some(id))
        .unwrap();
    let make_snarl_id = snarl
        .nodes_ids_data()
        .find_map(|(id, node)| (node.value == GraphItem::Node(fixture.make_id)).then_some(id))
        .unwrap();
    let vacant_index = merge_input_slots(&fixture.project, fixture.make_id).len() - 1;
    let edit = edit_for_wire(
        &fixture.project,
        &snarl,
        source_snarl_id,
        0,
        make_snarl_id,
        vacant_index,
        true,
    )
    .expect("vacant physical List row must produce a connect edit");
    assert!(matches!(edit, NodeEdit::ConnectAtIndex { .. }));
    assert!(apply_edit(&mut fixture.project, edit));
    let connections = canonical_connections(&fixture.project, fixture.make_id);
    assert_eq!(connections.len(), 2);
    assert_eq!(connections[0].from, connections[1].from);
    assert_ne!(connections[0].id, connections[1].id);
    assert_eq!([connections[0].order, connections[1].order], [0, 1]);
}
