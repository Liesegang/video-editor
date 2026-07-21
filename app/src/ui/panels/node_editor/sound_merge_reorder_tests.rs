use super::merge_reorder_tests::{pointer_button, render_merge_frame};
use super::*;
use library::model::project::{
    PortDefinition, PortExposure, PortSide, ProjectConnection, AUDIO_OUTPUT_PORT,
    IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, MERGE_SOUNDS_PORT,
};
use library::model::{Composition, PluginOperationContent};

struct StructuralSoundFixture {
    project: Project,
    composition_id: Uuid,
    track_id: Uuid,
    clip_ids: [Uuid; 2],
    image_merge_id: Uuid,
    sound_merge_id: Uuid,
    custom_connection_ids: [Uuid; 2],
}

fn audio_source_node(name: &str, position: [f32; 2]) -> Node {
    let content = PluginOperationContent {
        category: "test".to_string(),
        component_id: format!("audio-source-{name}"),
        operation: format!("test.audio-source.{name}.v1"),
        declared_ports: vec![PortDefinition::output(
            AUDIO_OUTPUT_PORT,
            "Audio",
            PortDataType::Audio,
            PortSide::Right,
            PortExposure::Graph,
        )],
    };
    let mut serialized = serde_json::to_value(Node::new_sound_merge(name)).unwrap();
    serialized["content"] = serde_json::json!({
        "type": "PluginOperation",
        "data": content,
    });
    let mut node: Node = serde_json::from_value(serialized).unwrap();
    node.ui_position = position;
    node
}

fn structural_sound_fixture() -> StructuralSoundFixture {
    let mut project = Project::new("Sound Merge physical rows");
    let (mut composition, mut track) = Composition::new("Main", 960, 540, 30.0, 4.0);
    composition.ui_position = [10.0, 20.0];
    composition.ui_size = [1650.0, 1080.0];
    track.ui_position = [70.0, 100.0];
    track.ui_size = [1460.0, 900.0];
    let composition_id = composition.id;
    let track_id = track.id;
    let image_merge_id = track.structural_merge_node_id;
    let sound_merge_id = track.structural_sound_merge_node_id;
    project.add_track(track).unwrap();
    project.add_composition(composition).unwrap();

    let mut clip_ids = [Uuid::nil(); 2];
    for (index, name) in ["First Clip", "Second Clip"].into_iter().enumerate() {
        let mut clip = Clip::new(name, 0.0, 4.0);
        clip.ui_position = [160.0, 190.0 + index as f32 * 250.0];
        clip.ui_size = [620.0, 220.0];
        clip_ids[index] = clip.id;
        project.add_clip(clip);
        project
            .attach_clip_to_track(track_id, clip_ids[index])
            .unwrap();
    }

    project.get_node_mut(image_merge_id).unwrap().ui_position = [930.0, 220.0];
    project.get_node_mut(sound_merge_id).unwrap().ui_position = [930.0, 570.0];

    let mut custom_connection_ids = [Uuid::nil(); 2];
    for (index, name) in ["Voice A", "Voice B"].into_iter().enumerate() {
        let node = audio_source_node(name, [560.0, 650.0 + index as f32 * 100.0]);
        let node_id = node.id;
        project.add_node(node);
        project
            .attach_node_to_container(NodeContainer::Track(track_id), node_id)
            .unwrap();
        custom_connection_ids[index] = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(node_id), AUDIO_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(sound_merge_id), MERGE_SOUNDS_PORT),
            )
            .unwrap();
    }
    assert!(project.validate_connections().is_empty());

    StructuralSoundFixture {
        project,
        composition_id,
        track_id,
        clip_ids,
        image_merge_id,
        sound_merge_id,
        custom_connection_ids,
    }
}

fn canonical_connections(
    project: &Project,
    merge_id: Uuid,
    input_port: &str,
) -> Vec<ProjectConnection> {
    let target = PortAddress::new(PortOwner::Node(merge_id), input_port);
    let mut connections = project
        .connections
        .iter()
        .filter(|connection| connection.to == target)
        .cloned()
        .collect::<Vec<_>>();
    connections.sort_by_key(|connection| (connection.order, connection.id));
    connections
}

fn connection_id_for_child(
    project: &Project,
    merge_id: Uuid,
    input_port: &str,
    child: PortOwner,
    output_port: &str,
) -> Uuid {
    canonical_connections(project, merge_id, input_port)
        .into_iter()
        .find(|connection| connection.from == PortAddress::new(child, output_port))
        .map(|connection| connection.id)
        .unwrap()
}

fn raw_drag(
    project: &Project,
    composition_id: Uuid,
    state: &mut NodeEditorState,
    merge_id: Uuid,
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
    let handle_id = format!("node_editor.merge_layer.drag_handle:{merge_id}:{connection_id}");
    let target_id = format!("node_editor.merge_layer:{merge_id}:{target_connection_id}");
    let start = test_rect(&handle_id).unwrap().center();
    let target = test_rect(&target_id).unwrap().center();
    let mut edits = Vec::new();
    for events in [
        vec![egui::Event::PointerMoved(start)],
        vec![pointer_button(start, true)],
        vec![egui::Event::PointerMoved(target)],
        vec![pointer_button(target, false)],
    ] {
        let rendered = render_merge_frame(&context, project, composition_id, state, frame, events);
        assert_eq!(rendered.layout_edit_count, 0);
        edits.extend(rendered.edits);
        frame += 1;
    }
    edits
}

fn assert_identity_preserved(before: &Project, after: &Project) {
    assert_eq!(before.connections.len(), after.connections.len());
    for original in &before.connections {
        let current = after
            .connections
            .iter()
            .find(|connection| connection.id == original.id)
            .unwrap();
        assert_eq!(current.from, original.from);
        assert_eq!(current.to, original.to);
        assert_eq!(current.blend_mode, original.blend_mode);
    }
}

fn apply_one_gesture_with_undo_redo(
    project: &mut Project,
    state: &mut NodeEditorState,
    edits: Vec<QueuedNodeEdit>,
) -> (Project, Project) {
    assert_eq!(edits.len(), 1, "one pointer gesture must queue one edit");
    let before = project.clone();
    let mut history = HistoryManager::new();
    history.push_project_state(before.clone());
    let undo_depth = history.undo_depth();
    assert!(apply_queued_node_edits(project, edits, &mut history, state));
    assert_eq!(history.undo_depth(), undo_depth + 1);
    let edited = project.clone();
    assert_eq!(history.undo(&edited), Some(before.clone()));
    assert_eq!(history.redo(&before), Some(edited.clone()));
    (before, edited)
}

#[test]
fn sound_merge_projects_audio_connections_in_canonical_top_to_bottom_rows_without_blend() {
    let fixture = structural_sound_fixture();
    let rows = merge_layer_rows(&fixture.project, fixture.sound_merge_id);
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|row| {
        row.kind == NativeVariadicMergeKind::Sound
            && row.canonical_index == row.visual_index
            && !row.authored_blend_available
    }));
    assert_eq!(
        rows.iter().map(|row| row.connection_id).collect::<Vec<_>>(),
        canonical_connections(&fixture.project, fixture.sound_merge_id, MERGE_SOUNDS_PORT)
            .into_iter()
            .map(|connection| connection.id)
            .collect::<Vec<_>>()
    );
    assert!(matches!(
        merge_input_slots(&fixture.project, fixture.sound_merge_id)
            .last()
            .map(|slot| &slot.role),
        Some(MergeInputSlotRole::Vacant(NativeVariadicMergeKind::Sound))
    ));
    let vacant = merge_vacant_slot(&fixture.project, fixture.sound_merge_id).unwrap();
    assert_eq!(vacant.structural_prefix_len, fixture.clip_ids.len());
    assert_eq!(vacant.canonical_index, rows.len());
    assert_eq!(vacant.visual_index, rows.len());
    assert_eq!(vacant.insertion_semantics, "end");
    let target = PortAddress::new(PortOwner::Node(fixture.sound_merge_id), MERGE_SOUNDS_PORT);
    let native = native_variadic_merge_target(&fixture.project, &target).unwrap();
    assert_eq!(native.node_id, fixture.sound_merge_id);
    assert_eq!(native.kind, NativeVariadicMergeKind::Sound);

    let context = egui::Context::default();
    let mut state = NodeEditorState::default();
    for frame in 0..6 {
        render_merge_frame(
            &context,
            &fixture.project,
            fixture.composition_id,
            &mut state,
            frame,
            Vec::new(),
        );
    }
    for row in rows {
        assert!(test_rect(&format!(
            "node_editor.merge_layer.blend_select:{}:{}",
            fixture.sound_merge_id, row.connection_id
        ))
        .is_none());
    }
}

#[test]
fn real_pointer_sound_row_drag_reorders_custom_wires_once_and_keeps_structural_prefix() {
    let mut fixture = structural_sound_fixture();
    let mut state = NodeEditorState::default();
    let edits = raw_drag(
        &fixture.project,
        fixture.composition_id,
        &mut state,
        fixture.sound_merge_id,
        fixture.custom_connection_ids[0],
        fixture.custom_connection_ids[1],
    );
    assert!(matches!(
        edits.as_slice(),
        [QueuedNodeEdit::Atomic(NodeEdit::ReorderConnection {
            connection_id,
            new_order: 3,
        })] if *connection_id == fixture.custom_connection_ids[0]
    ));
    let (before, edited) =
        apply_one_gesture_with_undo_redo(&mut fixture.project, &mut state, edits);
    assert_identity_preserved(&before, &edited);
    assert_eq!(
        fixture
            .project
            .get_track(fixture.track_id)
            .unwrap()
            .clip_ids,
        fixture.clip_ids
    );
    let canonical =
        canonical_connections(&fixture.project, fixture.sound_merge_id, MERGE_SOUNDS_PORT);
    assert_eq!(
        canonical[0].from.owner,
        PortOwner::Clip(fixture.clip_ids[0])
    );
    assert_eq!(
        canonical[1].from.owner,
        PortOwner::Clip(fixture.clip_ids[1])
    );
    assert_eq!(
        canonical[2..]
            .iter()
            .map(|connection| connection.id)
            .collect::<Vec<_>>(),
        vec![
            fixture.custom_connection_ids[1],
            fixture.custom_connection_ids[0]
        ]
    );
}

fn assert_structural_pointer_drag_synchronizes_both_merges(kind: NativeVariadicMergeKind) {
    let mut fixture = structural_sound_fixture();
    let (merge_id, input_port, output_port) = match kind {
        NativeVariadicMergeKind::Image => {
            (fixture.image_merge_id, MERGE_IMAGES_PORT, IMAGE_OUTPUT_PORT)
        }
        NativeVariadicMergeKind::Sound => {
            (fixture.sound_merge_id, MERGE_SOUNDS_PORT, AUDIO_OUTPUT_PORT)
        }
    };
    let first_connection = connection_id_for_child(
        &fixture.project,
        merge_id,
        input_port,
        PortOwner::Clip(fixture.clip_ids[0]),
        output_port,
    );
    let second_connection = connection_id_for_child(
        &fixture.project,
        merge_id,
        input_port,
        PortOwner::Clip(fixture.clip_ids[1]),
        output_port,
    );
    let mut state = NodeEditorState::default();
    let edits = raw_drag(
        &fixture.project,
        fixture.composition_id,
        &mut state,
        merge_id,
        first_connection,
        second_connection,
    );
    assert!(matches!(
        edits.as_slice(),
        [QueuedNodeEdit::Atomic(NodeEdit::ReorderStructuralChild {
            container: NodeContainer::Track(track_id),
            child: PortOwner::Clip(clip_id),
            new_index: 1,
        })] if *track_id == fixture.track_id && *clip_id == fixture.clip_ids[0]
    ));
    let (before, edited) =
        apply_one_gesture_with_undo_redo(&mut fixture.project, &mut state, edits);
    assert_identity_preserved(&before, &edited);
    assert_eq!(
        fixture
            .project
            .get_track(fixture.track_id)
            .unwrap()
            .clip_ids,
        [fixture.clip_ids[1], fixture.clip_ids[0]]
    );
    for (merge_id, input_port) in [
        (fixture.image_merge_id, MERGE_IMAGES_PORT),
        (fixture.sound_merge_id, MERGE_SOUNDS_PORT),
    ] {
        let canonical = canonical_connections(&fixture.project, merge_id, input_port);
        assert_eq!(
            canonical[0].from.owner,
            PortOwner::Clip(fixture.clip_ids[1])
        );
        assert_eq!(
            canonical[1].from.owner,
            PortOwner::Clip(fixture.clip_ids[0])
        );
    }
    let sound = canonical_connections(&fixture.project, fixture.sound_merge_id, MERGE_SOUNDS_PORT);
    assert_eq!(
        sound[2..]
            .iter()
            .map(|connection| connection.id)
            .collect::<Vec<_>>(),
        fixture.custom_connection_ids
    );
}

#[test]
fn image_structural_row_drag_reorders_timeline_and_both_typed_merges() {
    assert_structural_pointer_drag_synchronizes_both_merges(NativeVariadicMergeKind::Image);
}

#[test]
fn sound_structural_row_drag_reorders_timeline_and_both_typed_merges() {
    assert_structural_pointer_drag_synchronizes_both_merges(NativeVariadicMergeKind::Sound);
}

#[test]
fn structural_and_custom_sound_rows_cannot_cross_the_mandatory_prefix() {
    let fixture = structural_sound_fixture();
    let structural_connection = connection_id_for_child(
        &fixture.project,
        fixture.sound_merge_id,
        MERGE_SOUNDS_PORT,
        PortOwner::Clip(fixture.clip_ids[0]),
        AUDIO_OUTPUT_PORT,
    );
    for (source, target) in [
        (structural_connection, fixture.custom_connection_ids[0]),
        (fixture.custom_connection_ids[0], structural_connection),
    ] {
        let mut state = NodeEditorState::default();
        let edits = raw_drag(
            &fixture.project,
            fixture.composition_id,
            &mut state,
            fixture.sound_merge_id,
            source,
            target,
        );
        assert!(edits.is_empty());
        assert!(state
            .merge_layer_reorder
            .as_ref()
            .is_some_and(|gesture| gesture.finished && gesture.target_index.is_none()));
    }
}
