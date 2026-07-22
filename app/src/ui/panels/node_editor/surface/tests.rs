use std::cell::RefCell;

use super::*;
use library::model::{Composition, Node};

type SurfaceState =
    node_editor_ui::InteractionState<Uuid, SurfacePortId, NodeEditorEditableWire, ProjectPortOwner>;

fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn key_a(pressed: bool) -> egui::Event {
    egui::Event::Key {
        key: egui::Key::A,
        physical_key: Some(egui::Key::A),
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

fn run_pointer_frame(
    context: &egui::Context,
    projection: &SurfaceProjection<'_>,
    state: &mut SurfaceState,
    options: node_editor_ui::InteractionOptions,
    events: Vec<egui::Event>,
) -> Vec<SurfaceOutput> {
    let outputs = RefCell::new(Vec::new());
    drop(context.run(
        egui::RawInput {
            screen_rect: Some(projection.viewport),
            events,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(context, |ui| {
                    outputs
                        .borrow_mut()
                        .extend(node_editor_ui::Editor::interact(
                            ui,
                            &projection.frame(),
                            state,
                            options,
                            false,
                        ));
                });
        },
    ));
    outputs.into_inner()
}

#[test]
fn selection_intent_maps_opaque_groups_without_registry_probe_order() {
    let node = Uuid::from_u128(1);
    let clip = Uuid::from_u128(2);
    let outputs = [SurfaceOutput::Select {
        items: vec![
            ItemId::Node(node),
            ItemId::Group(ProjectPortOwner::Clip(clip)),
        ],
        primary: Some(ItemId::Group(ProjectPortOwner::Clip(clip))),
    }];

    assert_eq!(
        selection_change(&outputs),
        Some(SurfaceSelectionChange {
            targets: vec![SelectionTarget::Node(node), SelectionTarget::Clip(clip)],
            primary: Some(SelectionTarget::Clip(clip)),
        })
    );
}

#[test]
fn production_surface_arms_layout_swipe_from_visual_header_padding() {
    let mut project = Project::new("visual header padding");
    let node = Node::new_merge("Padded header");
    let node_id = node.id;
    project.add_node(node);
    let node_rect = egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(220.0, 120.0));
    let visual_header = egui::Rect::from_min_size(node_rect.min, egui::vec2(220.0, 38.0));
    let content_header = visual_header.shrink2(egui::vec2(9.0, 7.0));
    let padding_point = egui::pos2(content_header.center().x, visual_header.top() + 1.0);
    let mut capture = SurfaceCapture::default();
    capture.record_selectable(SelectionTarget::Node(node_id));
    capture.record_node_header(node_id, visual_header);
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let projection = SurfaceProjection::from_project(
        &project,
        &[],
        &HashMap::from([(node_id, node_rect)]),
        &HashMap::new(),
        &capture,
        &[],
        &[],
        None,
        None,
        viewport,
        egui::emath::TSTransform::IDENTITY,
    );
    assert!(visual_header.contains(padding_point));
    assert!(!content_header.contains(padding_point));

    let context = egui::Context::default();
    let mut state = SurfaceState::default();
    let outputs = run_pointer_frame(
        &context,
        &projection,
        &mut state,
        node_editor_ui::InteractionOptions::ALL,
        vec![
            key_a(true),
            egui::Event::PointerMoved(padding_point),
            pointer_button(padding_point, true),
        ],
    );

    assert!(outputs.iter().any(|output| matches!(
        output,
        node_editor_ui::EditorOutput::LayoutSwipe(intent)
            if intent.phase == node_editor_ui::LayoutSwipePhase::Start
                && intent.anchor == node_id
    )));
    assert!(state.is_layout_swipe_active());
}

#[test]
fn wire_only_selection_does_not_clear_project_item_selection() {
    let wire = NodeEditorEditableWire::ProjectConnection {
        connection_id: Uuid::from_u128(3),
    };
    let outputs = [SurfaceOutput::Select {
        items: vec![ItemId::Wire(wire)],
        primary: Some(ItemId::Wire(wire)),
    }];

    assert_eq!(selection_change(&outputs), None);
}

#[test]
fn production_projection_uses_snarl_order_for_overlapping_nodes_and_ports() {
    let mut project = Project::new("ordered production surface");
    let lower = Node::new_merge("Lower");
    let lower_id = lower.id;
    let upper = Node::new_merge("Upper");
    let upper_id = upper.id;
    project.add_node(lower);
    project.add_node(upper);

    let overlap = egui::Rect::from_min_size(egui::pos2(80.0, 90.0), egui::vec2(180.0, 120.0));
    // Deliberately oppose map insertion and production Snarl draw order.
    let node_rects = HashMap::from([(upper_id, overlap), (lower_id, overlap)]);
    let lower_port = RenderedPortKey {
        address: PortAddress::new(ProjectPortOwner::Node(lower_id), "image"),
        direction: ProjectPortDirection::Output,
        connection_id: None,
    };
    let upper_port = RenderedPortKey {
        address: PortAddress::new(ProjectPortOwner::Node(upper_id), "image"),
        direction: ProjectPortDirection::Output,
        connection_id: None,
    };
    let port_rect = egui::Rect::from_center_size(overlap.center(), egui::Vec2::splat(12.0));
    let port_rects = HashMap::from([
        (upper_port.clone(), port_rect),
        (lower_port.clone(), port_rect),
    ]);
    let mut capture = SurfaceCapture::default();
    capture.record_node_header(lower_id, overlap);
    capture.record_node_header(upper_id, overlap);
    capture.record_selectable(SelectionTarget::Node(lower_id));
    capture.record_selectable(SelectionTarget::Node(upper_id));
    capture.record_port(lower_port);
    capture.record_port(upper_port);
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let projection = SurfaceProjection::from_project(
        &project,
        &[],
        &node_rects,
        &port_rects,
        &capture,
        &[],
        &[],
        None,
        None,
        viewport,
        egui::emath::TSTransform::IDENTITY,
    );

    assert_eq!(
        projection
            .nodes
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        [lower_id, upper_id]
    );
    assert_eq!(
        projection
            .ports
            .iter()
            .map(|port| port.id.port.owner)
            .collect::<Vec<_>>(),
        [
            ProjectPortOwner::Node(lower_id),
            ProjectPortOwner::Node(upper_id)
        ]
    );

    let context = egui::Context::default();
    let mut state = SurfaceState::default();
    let click = overlap.center() + egui::vec2(30.0, 0.0);
    let outputs = run_pointer_frame(
        &context,
        &projection,
        &mut state,
        node_editor_ui::InteractionOptions::SELECTION,
        vec![
            egui::Event::PointerMoved(click),
            pointer_button(click, true),
        ],
    );
    assert_eq!(
        selection_change(&outputs),
        Some(SurfaceSelectionChange {
            targets: vec![SelectionTarget::Node(upper_id)],
            primary: Some(SelectionTarget::Node(upper_id)),
        })
    );
}

#[test]
fn production_adapter_handles_zoomed_marquee_blank_and_wire_deselect() {
    let mut project = Project::new("zoomed production interactions");
    let first = Node::new_merge("First");
    let first_id = first.id;
    let second = Node::new_merge("Second");
    let second_id = second.id;
    project.add_node(first);
    project.add_node(second);
    let first_rect = egui::Rect::from_min_size(egui::pos2(50.0, 60.0), egui::vec2(100.0, 80.0));
    let second_rect = egui::Rect::from_min_size(egui::pos2(190.0, 70.0), egui::vec2(100.0, 80.0));
    let node_rects = HashMap::from([(second_id, second_rect), (first_id, first_rect)]);
    let mut capture = SurfaceCapture::default();
    for (id, rect) in [(first_id, first_rect), (second_id, second_rect)] {
        capture.record_selectable(SelectionTarget::Node(id));
        capture.record_node_header(id, rect);
    }
    let transform = egui::emath::TSTransform::new(egui::vec2(120.0, 75.0), 1.7);
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
    let projection = SurfaceProjection::from_project(
        &project,
        &[],
        &node_rects,
        &HashMap::new(),
        &capture,
        &[],
        &[],
        None,
        Some(Uuid::from_u128(900)),
        viewport,
        transform,
    );
    let context = egui::Context::default();
    let mut state = SurfaceState::default();

    let start = transform * egui::pos2(40.0, 50.0);
    let end = transform * egui::pos2(310.0, 165.0);
    assert!(run_pointer_frame(
        &context,
        &projection,
        &mut state,
        node_editor_ui::InteractionOptions::SELECTION,
        vec![
            egui::Event::PointerMoved(start),
            pointer_button(start, true),
        ],
    )
    .is_empty());
    let marquee = run_pointer_frame(
        &context,
        &projection,
        &mut state,
        node_editor_ui::InteractionOptions::SELECTION,
        vec![egui::Event::PointerMoved(end), pointer_button(end, false)],
    );
    assert_eq!(
        selection_change(&marquee),
        Some(SurfaceSelectionChange {
            targets: vec![
                SelectionTarget::Node(first_id),
                SelectionTarget::Node(second_id),
            ],
            primary: Some(SelectionTarget::Node(second_id)),
        })
    );
    assert!(deselects_wire(&marquee));

    let blank = transform * egui::pos2(340.0, 200.0);
    let _ = run_pointer_frame(
        &context,
        &projection,
        &mut state,
        node_editor_ui::InteractionOptions::SELECTION,
        vec![
            egui::Event::PointerMoved(blank),
            pointer_button(blank, true),
        ],
    );
    let blank_outputs = run_pointer_frame(
        &context,
        &projection,
        &mut state,
        node_editor_ui::InteractionOptions::SELECTION,
        vec![pointer_button(blank, false)],
    );
    assert_eq!(
        selection_change(&blank_outputs),
        Some(SurfaceSelectionChange {
            targets: Vec::new(),
            primary: None,
        })
    );
    assert!(deselects_wire(&blank_outputs));
}

#[test]
fn overview_keeps_node_group_click_and_blank_deselect_without_marquee() {
    let mut project = Project::new("overview production interactions");
    let (mut composition, track) = Composition::new("Overview Group", 320, 180, 24.0, 2.0);
    composition.ui_position = [2_500.0, 100.0];
    composition.ui_size = [600.0, 400.0];
    let composition_id = composition.id;
    assert!(project.add_track(track).is_ok());
    assert!(project.add_composition(composition).is_ok());
    let containers = [ContainerVisual {
        owner: ProjectPortOwner::Composition(composition_id),
        kind: super::super::ContainerKind::Composition,
        position: [2_500.0, 100.0],
        size: [600.0, 400.0],
        collapsed: false,
    }];
    let node = Node::new_merge("Overview Node");
    let node_id = node.id;
    project.add_node(node);
    let node_rect = egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(240.0, 160.0));
    let node_rects = HashMap::from([(node_id, node_rect)]);
    let mut capture = SurfaceCapture::default();
    capture.record_selectable(SelectionTarget::Composition(composition_id));
    capture.record_selectable(SelectionTarget::Node(node_id));
    capture.record_node_header(node_id, node_rect);
    let transform = egui::emath::TSTransform::new(egui::vec2(300.0, 220.0), 0.1);
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
    let projection = SurfaceProjection::from_project(
        &project,
        &containers,
        &node_rects,
        &HashMap::new(),
        &capture,
        &[],
        &[],
        None,
        None,
        viewport,
        transform,
    );
    let context = egui::Context::default();
    let mut state = SurfaceState::default();

    let node_click = transform * node_rect.center();
    let selected = run_pointer_frame(
        &context,
        &projection,
        &mut state,
        node_editor_ui::InteractionOptions::OVERVIEW_SELECTION,
        vec![
            egui::Event::PointerMoved(node_click),
            pointer_button(node_click, true),
        ],
    );
    assert_eq!(
        selection_change(&selected),
        Some(SurfaceSelectionChange {
            targets: vec![SelectionTarget::Node(node_id)],
            primary: Some(SelectionTarget::Node(node_id)),
        })
    );
    let _ = run_pointer_frame(
        &context,
        &projection,
        &mut state,
        node_editor_ui::InteractionOptions::OVERVIEW_SELECTION,
        vec![pointer_button(node_click, false)],
    );

    let container_rect = containers[0].rect();
    let group_header = egui::Rect::from_min_size(
        container_rect.min,
        egui::vec2(container_rect.width(), CONTAINER_HEADER_HEIGHT),
    );
    let group_click = transform * group_header.center();
    let selected_group = run_pointer_frame(
        &context,
        &projection,
        &mut state,
        node_editor_ui::InteractionOptions::OVERVIEW_SELECTION,
        vec![
            egui::Event::PointerMoved(group_click),
            pointer_button(group_click, true),
        ],
    );
    assert_eq!(
        selection_change(&selected_group),
        Some(SurfaceSelectionChange {
            targets: vec![SelectionTarget::Composition(composition_id)],
            primary: Some(SelectionTarget::Composition(composition_id)),
        })
    );

    let blank = egui::pos2(850.0, 550.0);
    let cleared = run_pointer_frame(
        &context,
        &projection,
        &mut state,
        node_editor_ui::InteractionOptions::OVERVIEW_SELECTION,
        vec![
            pointer_button(group_click, false),
            egui::Event::PointerMoved(blank),
            pointer_button(blank, true),
        ],
    );
    assert_eq!(
        selection_change(&cleared),
        Some(SurfaceSelectionChange {
            targets: Vec::new(),
            primary: None,
        })
    );
    assert!(!state.is_marquee_active());
}
