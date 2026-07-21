use super::test_fixture::fixture;
use super::*;
use crate::test_support::generator_node;
use library::animation::EasingFunction;
use library::editor::project_service::GeneratorNodeRequest;
use library::model::frame::color::Color;
use library::model::frame::draw_type::DrawStyle;
use library::model::frame::entity::StyleConfig;
use library::model::project::{
    ProjectConnection, FMOD_X_INPUT_PORT, FRAME_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT,
    MERGE_IMAGES_PORT, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
};
use library::model::property::{Keyframe, Property, PropertyMap};
use library::model::Composition;
use library::plugin::{
    FrameEvaluationContext, OperationDescriptor, OperationDescriptorError, Plugin, StylePlugin,
};

fn style_graph_factory() -> library::editor::project_service::ProjectManager {
    library::editor::project_service::ProjectManager::new(
        Arc::new(RwLock::new(Project::new("detached graph factory"))),
        Arc::new(PluginManager::default()),
    )
}

fn run_wire_interaction_frames(
    project: &Project,
    edge: &RenderedEdge,
    rendered_ports: &Arc<Mutex<HashMap<RenderedPortKey, egui::Rect>>>,
    state: &mut NodeEditorState,
    frames: Vec<Vec<egui::Event>>,
) -> Vec<QueuedNodeEdit> {
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 420.0));
    let mut queued = Vec::new();
    for (frame, events) in frames.into_iter().enumerate() {
        drop(context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(frame as f64 / 60.0),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    queued.extend(wire_interactions(
                        ui,
                        state,
                        WireInteractionFrame {
                            project,
                            edges: std::slice::from_ref(edge),
                            rendered_ports,
                            canvas_clip: screen,
                            graph_item_rects: &[],
                            to_global: egui::emath::TSTransform::IDENTITY,
                        },
                    ));
                });
            },
        ));
    }
    queued
}

fn plugin_operation_component(node: &Node) -> Option<&str> {
    match node.content() {
        NodeContent::PluginOperation(operation) => Some(&operation.component_id),
        _ => None,
    }
}

fn assert_detached_graph_has_clean_ltr_layout(project: &Project, graph: &NodeGraphBundle) {
    let mut measurement_project = project.clone();
    for node in &graph.nodes {
        measurement_project.add_node(node.clone());
    }
    let rects = graph
        .nodes
        .iter()
        .map(|node| {
            (
                node.id,
                egui::Rect::from_min_size(
                    egui::pos2(node.ui_position[0], node.ui_position[1]),
                    estimated_node_size(&measurement_project, node.id),
                ),
            )
        })
        .collect::<HashMap<_, _>>();

    for (index, left) in graph.nodes.iter().enumerate() {
        for right in &graph.nodes[index + 1..] {
            assert!(
                !padded_intersection(rects[&left.id], rects[&right.id]),
                "detached Nodes overlap or have less than {AUTO_LAYOUT_NODE_PADDING}px gap: {} {:?} / {} {:?}",
                left.id,
                rects[&left.id],
                right.id,
                rects[&right.id],
            );
        }
    }

    let ids = graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
    let edges = graph
        .connections
        .iter()
        .filter_map(|connection| {
            let (PortOwner::Node(from), PortOwner::Node(to)) =
                (connection.from.owner, connection.to.owner)
            else {
                return None;
            };
            Some((from, to))
        })
        .collect::<Vec<_>>();
    let ranks = rank_nodes_by_scc(&ids, &edges);
    for (from, to) in edges {
        if ranks[&from] == ranks[&to] {
            continue;
        }
        assert!(
            rects[&from].right() + AUTO_LAYOUT_NODE_PADDING <= rects[&to].left(),
            "detached edge is not left-to-right: {from} {:?} -> {to} {:?}",
            rects[&from],
            rects[&to],
        );
    }
}

fn queued_property_edit(
    owner: PortOwner,
    key: &str,
    time: f64,
    value: PropertyValue,
    finished: bool,
) -> QueuedNodeEdit {
    QueuedNodeEdit::Continuous {
        pending: NodeEditorPendingEdit {
            owner,
            key: key.to_string(),
        },
        edit: Some(NodeEdit::SetProperty {
            owner,
            key: key.to_string(),
            time,
            value,
        }),
        finished,
    }
}

fn queued_finish(owner: PortOwner, key: &str) -> QueuedNodeEdit {
    QueuedNodeEdit::Continuous {
        pending: NodeEditorPendingEdit {
            owner,
            key: key.to_string(),
        },
        edit: None,
        finished: true,
    }
}

fn assert_single_gesture_undo_redo(
    history: &mut HistoryManager,
    initial: &Project,
    edited: &Project,
) {
    assert_eq!(history.undo_depth(), 2);
    assert_eq!(history.undo(edited), Some(initial.clone()));
    assert_eq!(history.redo(initial), Some(edited.clone()));
}

#[derive(Clone, Copy)]
struct HierarchyFixtureIds {
    composition: Uuid,
    track: Uuid,
    empty_track: Uuid,
    clip: Uuid,
    sibling_clip: Uuid,
    empty_clip: Uuid,
    solid: Uuid,
    merge: Uuid,
    text: Uuid,
    text_fill: Uuid,
    shape: Uuid,
    shape_fill: Uuid,
    composition_node: Uuid,
}

fn adversarial_hierarchy_fixture() -> (Project, HierarchyFixtureIds) {
    let (mut project, composition, track, clip, solid, merge) = fixture();
    let overlapping_position = [420.0, 300.0];
    project.get_node_mut(solid).unwrap().ui_position = overlapping_position;
    project.get_node_mut(merge).unwrap().ui_position = overlapping_position;

    let sibling_clip = Uuid::from_u128(0x7_001);
    let mut clip_b = library::model::Clip::new("Sibling Clip", 0.0, 5.0);
    clip_b.id = sibling_clip;
    clip_b.ui_position = [260.0, 260.0];
    clip_b.ui_size = [360.0, 220.0];
    project.add_clip(clip_b);
    project.attach_clip_to_track(track, sibling_clip).unwrap();

    let text = Uuid::from_u128(0x7_002);
    let mut text_node = generator_node(
        "Tall Text",
        GeneratorNodeRequest::Text {
            text: "A deliberately tall text Node".to_string(),
            font: "Arial".to_string(),
        },
    );
    text_node.id = text;
    text_node.ui_position = overlapping_position;
    project.add_node(text_node);
    project
        .attach_node_to_container(NodeContainer::Clip(sibling_clip), text)
        .unwrap();
    let text_fill = Uuid::from_u128(0x7_007);
    let mut text_fill_node = PluginManager::default()
        .create_style_operation_node("fill")
        .unwrap();
    text_fill_node.id = text_fill;
    text_fill_node.ui_position = overlapping_position;
    project.add_node(text_fill_node);
    project
        .attach_node_to_container(NodeContainer::Clip(sibling_clip), text_fill)
        .unwrap();
    project.get_clip_mut(sibling_clip).unwrap().output_node_id = Some(text_fill);

    let empty_clip = Uuid::from_u128(0x7_003);
    let mut collapsed_clip = library::model::Clip::new("Collapsed Empty", 0.0, 5.0);
    collapsed_clip.id = empty_clip;
    collapsed_clip.ui_collapsed = true;
    collapsed_clip.ui_position = [260.0, 260.0];
    project.add_clip(collapsed_clip);
    project.attach_clip_to_track(track, empty_clip).unwrap();

    let shape = Uuid::from_u128(0x7_004);
    let mut shape_node = generator_node(
        "Track Shape",
        GeneratorNodeRequest::Shape {
            path: "M 0 0 H 100 V 100 Z".to_string(),
        },
    );
    shape_node.id = shape;
    shape_node.ui_position = overlapping_position;
    project.add_node(shape_node);
    project
        .attach_node_to_container(NodeContainer::Track(track), shape)
        .unwrap();
    let shape_fill = Uuid::from_u128(0x7_008);
    let mut shape_fill_node = PluginManager::default()
        .create_style_operation_node("fill")
        .unwrap();
    shape_fill_node.id = shape_fill;
    shape_fill_node.ui_position = overlapping_position;
    project.add_node(shape_fill_node);
    project
        .attach_node_to_container(NodeContainer::Track(track), shape_fill)
        .unwrap();

    let composition_node = Uuid::from_u128(0x7_005);
    let mut root_merge = Node::new_merge("Composition Merge");
    root_merge.id = composition_node;
    root_merge.ui_position = overlapping_position;
    project.add_node(root_merge);
    project
        .attach_node_to_container(NodeContainer::Composition(composition), composition_node)
        .unwrap();

    let empty_track = Uuid::from_u128(0x7_006);
    let mut collapsed_track = library::model::Track::new("Collapsed Empty Track");
    collapsed_track.id = empty_track;
    collapsed_track.ui_collapsed = true;
    collapsed_track.ui_position = [110.0, 140.0];
    assert!(
        project.add_track(collapsed_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project
        .attach_track_to_composition(composition, empty_track)
        .unwrap();

    for (source, fill) in [(text, text_fill), (shape, shape_fill)] {
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(source), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(fill), SHAPE_INPUT_PORT),
            )
            .unwrap();
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(fill), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge), MERGE_IMAGES_PORT),
            )
            .unwrap();
    }
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(merge), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(composition_node), MERGE_IMAGES_PORT),
        )
        .unwrap();

    (
        project,
        HierarchyFixtureIds {
            composition,
            track,
            empty_track,
            clip,
            sibling_clip,
            empty_clip,
            solid,
            merge,
            text,
            text_fill,
            shape,
            shape_fill,
            composition_node,
        },
    )
}

fn render_test_graph_with_context_menu_exclusions(
    project: &Project,
    composition_id: Uuid,
) -> (
    HashMap<String, egui::Rect>,
    Vec<egui::Rect>,
    egui::emath::TSTransform,
    Vec<ContainerVisual>,
) {
    let context = egui::Context::default();
    let (mut snarl, containers) = build_snarl(project, composition_id);
    let mut final_context_menu_exclusion_rects = Vec::new();
    let mut final_to_global = egui::emath::TSTransform::default();
    reset_test_rects();

    for frame in 0..5 {
        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1800.0, 1200.0),
            )),
            time: Some(frame as f64 / 60.0),
            ..Default::default()
        };
        drop(context.run(raw_input, |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let mut edits = Vec::new();
                let mut navigation = None;
                let mut wire_context_request = None;
                let mut context_menu_exclusion_rects = Vec::new();
                let mut to_global = egui::emath::TSTransform::default();
                let mut canvas_clip = ui.clip_rect();
                let rendered_ports = Arc::new(Mutex::new(HashMap::new()));
                let mut merge_layer_reorder = None;
                let mut viewer = ProjectNodeViewer {
                    project,
                    plugin_manager: None,
                    containers: &containers,
                    edits: &mut edits,
                    pending_navigation: &mut navigation,
                    selected_node_ids: &[],
                    selected_container_owners: &[],
                    current_time: 0.0,
                    context_menu_exclusion_rects: &mut context_menu_exclusion_rects,
                    wire_context_request: &mut wire_context_request,
                    suppress_wire_connect: false,
                    locked_canvas_transform: None,
                    previous_canvas_transform: None,
                    to_global: &mut to_global,
                    canvas_clip: &mut canvas_clip,
                    rendered_ports: Arc::clone(&rendered_ports),
                    merge_layer_reorder: &mut merge_layer_reorder,
                    rendered_node_rects: Arc::new(Mutex::new(HashMap::new())),
                    surface_capture: Arc::new(Mutex::new(SurfaceCapture::default())),
                };
                let style = SnarlStyle {
                    collapsible: Some(false),
                    min_scale: Some(0.05),
                    max_scale: Some(1.0),
                    ..Default::default()
                };
                snarl.show(
                    &mut viewer,
                    &style,
                    egui::Id::new(("hierarchical-layout-test", composition_id)),
                    ui,
                );
                drop(viewer);
                final_context_menu_exclusion_rects = context_menu_exclusion_rects;
                final_to_global = to_global;
                for container in &containers {
                    register_container_chrome(
                        container,
                        to_global,
                        canvas_clip,
                        project,
                        0.0,
                        false,
                    );
                }
                register_rendered_edges(project, &rendered_ports, canvas_clip, None);
            });
        }));
    }
    (
        test_rects(),
        final_context_menu_exclusion_rects,
        final_to_global,
        containers,
    )
}

fn render_test_graph(project: &Project, composition_id: Uuid) -> HashMap<String, egui::Rect> {
    render_test_graph_with_context_menu_exclusions(project, composition_id).0
}

mod authoring;
mod canvas;
mod catalog;
mod creation;
mod graph_state;
mod layout;
mod reparent_targets;
mod reparent_transactions;
mod selection;
mod timeline_semantics;
mod wire_editing;
