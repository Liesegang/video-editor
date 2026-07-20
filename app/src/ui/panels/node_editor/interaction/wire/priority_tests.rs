use super::{wire_interactions, WireInteractionFrame};
use crate::action::HistoryManager;
use crate::state::context_types::NodeEditorState;
use crate::ui::panels::node_editor::{
    apply_queued_node_edits, NodeEdit, QueuedNodeEdit, RenderedEdge, RenderedEdgeKind,
    RenderedPortKey,
};
use eframe::egui;
use library::model::project::{
    NodeContainer, PortAddress, PortDataType, PortDirection, PortOwner, IMAGE_OUTPUT_PORT,
    MERGE_IMAGES_PORT,
};
use library::model::{Composition, Node, Project};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

struct CompetingDragResult {
    queued: Vec<QueuedNodeEdit>,
    header_delta: egui::Vec2,
    wire_gesture_observed: bool,
    state: NodeEditorState,
}

fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn run_competing_header_frames(
    project: &Project,
    edges: &[RenderedEdge],
    rendered_ports: &Arc<Mutex<HashMap<RenderedPortKey, egui::Rect>>>,
    header_rect: egui::Rect,
    frames: Vec<Vec<egui::Event>>,
) -> CompetingDragResult {
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 420.0));
    let mut state = NodeEditorState::default();
    let mut queued = Vec::new();
    let mut header_delta = egui::Vec2::ZERO;
    let mut wire_gesture_observed = false;
    for (frame_number, events) in frames.into_iter().enumerate() {
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(frame_number as f64 / 60.0),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    // Snarl's broad container-frame interaction is rendered
                    // before the custom foreground wire interaction.
                    let header = ui.interact(
                        header_rect,
                        egui::Id::new("competing_container_header"),
                        egui::Sense::click_and_drag(),
                    );
                    if header.dragged_by(egui::PointerButton::Primary) {
                        header_delta += header.drag_delta();
                    }
                    queued.extend(wire_interactions(
                        ui,
                        &mut state,
                        WireInteractionFrame {
                            project,
                            edges,
                            rendered_ports,
                            canvas_clip: screen,
                            graph_item_rects: &[header_rect],
                            to_global: egui::emath::TSTransform::IDENTITY,
                        },
                    ));
                    wire_gesture_observed |= state.wire_gesture.is_some();
                });
            },
        ));
    }
    CompetingDragResult {
        queued,
        header_delta,
        wire_gesture_observed,
        state,
    }
}

fn track_output_fixture() -> (Project, PortAddress, PortAddress) {
    let mut project = Project::new("container output pointer priority");
    let (composition, track) = Composition::new("Main", 640, 360, 24.0, 2.0);
    let composition_id = composition.id;
    let track_id = track.id;
    project.add_track(track);
    project.add_composition(composition);
    let merge = Node::new_merge("Composition Merge");
    let merge_id = merge.id;
    project.add_node(merge);
    project
        .attach_node_to_container(NodeContainer::Composition(composition_id), merge_id)
        .expect("attach target Merge");
    (
        project,
        PortAddress::new(PortOwner::Track(track_id), IMAGE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
    )
}

#[test]
fn padded_container_output_press_claims_wire_before_header_and_is_one_undo_step() {
    let (mut project, from, to) = track_output_fixture();
    let initial = project.clone();
    let source_rect =
        egui::Rect::from_center_size(egui::pos2(400.0, 130.0), egui::Vec2::splat(13.0));
    let target_rect =
        egui::Rect::from_center_size(egui::pos2(560.0, 240.0), egui::Vec2::splat(13.0));
    // This point is in the published ±5pt wire/drop target and in the
    // container header, but deliberately outside Snarl's 13pt socket.
    let press = egui::pos2(source_rect.left() - 3.0, source_rect.center().y);
    let target = target_rect.center();
    let header_rect = egui::Rect::from_min_max(
        egui::pos2(100.0, 100.0),
        egui::pos2(source_rect.left() - 1.0, 150.0),
    );
    assert!(header_rect.contains(press));
    assert!(!source_rect.contains(press));
    assert!(source_rect.expand(5.0).contains(press));

    let rendered_ports = Arc::new(Mutex::new(HashMap::from([
        (
            RenderedPortKey {
                address: from.clone(),
                direction: PortDirection::Output,
                connection_id: None,
            },
            source_rect,
        ),
        (
            RenderedPortKey {
                address: to.clone(),
                direction: PortDirection::Input,
                connection_id: None,
            },
            target_rect,
        ),
    ])));
    let result = run_competing_header_frames(
        &project,
        &[],
        &rendered_ports,
        header_rect,
        vec![
            vec![egui::Event::PointerMoved(press)],
            vec![pointer_button(press, true)],
            vec![egui::Event::PointerMoved(target)],
            vec![pointer_button(target, false)],
        ],
    );

    assert_eq!(result.header_delta, egui::Vec2::ZERO);
    assert!(result.state.normal_connect_gesture.is_none());
    assert!(result.state.container_resize.is_none());
    assert!(matches!(
        result.queued.as_slice(),
        [QueuedNodeEdit::Atomic(NodeEdit::Connect {
            from: queued_from,
            to: queued_to,
        })] if *queued_from == from && *queued_to == to
    ));

    let mut history = HistoryManager::new();
    history.push_project_state(initial.clone());
    let mut state = result.state;
    assert!(apply_queued_node_edits(
        &mut project,
        result.queued,
        &mut history,
        &mut state,
    ));
    let edited = project.clone();
    assert_eq!(history.undo_depth(), 2);
    assert_eq!(history.undo(&edited), Some(initial.clone()));
    assert_eq!(history.redo(&initial), Some(edited));
}

#[test]
fn header_drag_outside_output_hit_remains_available() {
    let (project, from, _) = track_output_fixture();
    let source_rect =
        egui::Rect::from_center_size(egui::pos2(400.0, 130.0), egui::Vec2::splat(13.0));
    let rendered_ports = Arc::new(Mutex::new(HashMap::from([(
        RenderedPortKey {
            address: from,
            direction: PortDirection::Output,
            connection_id: None,
        },
        source_rect,
    )])));
    let start = egui::pos2(180.0, 130.0);
    let end = egui::pos2(260.0, 170.0);
    let header_rect = egui::Rect::from_min_max(egui::pos2(100.0, 100.0), egui::pos2(390.0, 190.0));
    let result = run_competing_header_frames(
        &project,
        &[],
        &rendered_ports,
        header_rect,
        vec![
            vec![egui::Event::PointerMoved(start)],
            vec![pointer_button(start, true)],
            vec![egui::Event::PointerMoved(end)],
            vec![pointer_button(end, false)],
        ],
    );

    assert!(result.header_delta.length() > 1.0);
    assert!(result.queued.is_empty());
    assert!(result.state.normal_connect_gesture.is_none());
}

#[test]
fn output_binding_endpoint_claims_pointer_before_overlapping_header() {
    let mut project = Project::new("output binding before header");
    let (composition, track) = Composition::new("Main", 640, 360, 24.0, 2.0);
    let track_id = track.id;
    project.add_track(track);
    project.add_composition(composition);
    let output = Node::new_merge("Track Output");
    let output_id = output.id;
    project.add_node(output);
    project
        .attach_node_to_container(NodeContainer::Track(track_id), output_id)
        .expect("attach output Node");
    project
        .set_output_node(NodeContainer::Track(track_id), Some(output_id))
        .expect("bind Track output");
    let initial = project.clone();
    let endpoint = egui::pos2(400.0, 130.0);
    let edge = RenderedEdge {
        kind: RenderedEdgeKind::OutputBinding {
            owner: PortOwner::Track(track_id),
            node_id: output_id,
            data_type: PortDataType::Image,
        },
        start: egui::pos2(180.0, 130.0),
        control_a: egui::pos2(250.0, 130.0),
        control_b: egui::pos2(330.0, 130.0),
        end: endpoint,
    };
    let header_rect = egui::Rect::from_min_max(egui::pos2(260.0, 100.0), egui::pos2(410.0, 155.0));
    assert!(header_rect.contains(endpoint));
    let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
    let dragged = endpoint + egui::vec2(80.0, 70.0);
    let result = run_competing_header_frames(
        &project,
        std::slice::from_ref(&edge),
        &rendered_ports,
        header_rect,
        vec![
            vec![egui::Event::PointerMoved(endpoint)],
            vec![pointer_button(endpoint, true)],
            vec![egui::Event::PointerMoved(dragged)],
            vec![pointer_button(dragged, false)],
        ],
    );

    assert!(result.wire_gesture_observed);
    assert_eq!(result.header_delta, egui::Vec2::ZERO);
    assert!(result.queued.is_empty());
    assert!(result.state.wire_gesture.is_none());
    assert_eq!(project, initial);
}
