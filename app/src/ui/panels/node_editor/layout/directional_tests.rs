use super::*;

use library::model::project::{
    PortAddress, PortOwner, ProjectConnection, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT,
};
use library::model::{Clip, Composition, Node, NodeContainer, Project};

#[path = "directional_variadic_tests.rs"]
mod variadic;

const H_GAP: f32 = 30.0;
const V_GAP: f32 = 20.0;

#[derive(Debug)]
struct Fixture {
    project: Project,
    composition_id: Uuid,
    track_id: Uuid,
    geometry: BTreeMap<Uuid, NodeLayoutGeometry>,
}

#[derive(Clone, Copy, Debug)]
struct TestSelection<'a> {
    selected: &'a [Uuid],
    fixed: &'a [Uuid],
}

impl TestSelection<'static> {
    const ALL: Self = Self {
        selected: &[],
        fixed: &[],
    };
}

impl Fixture {
    fn new() -> Self {
        let mut project = Project::new("directional layout");
        let (composition, track) = Composition::new("Main", 1920, 1080, 30.0, 10.0);
        let composition_id = composition.id;
        let track_id = track.id;
        project.add_track(track).unwrap();
        project.add_composition(composition).unwrap();
        Self {
            project,
            composition_id,
            track_id,
            geometry: BTreeMap::new(),
        }
    }

    fn add_node(
        &mut self,
        owner: NodeContainer,
        id: u128,
        name: &str,
        position: [f32; 2],
        size: [f32; 2],
    ) -> Uuid {
        self.add_authored_node(owner, id, Node::new_merge(name), position, size)
    }

    fn add_authored_node(
        &mut self,
        owner: NodeContainer,
        id: u128,
        mut node: Node,
        position: [f32; 2],
        size: [f32; 2],
    ) -> Uuid {
        let id = Uuid::from_u128(id);
        node.id = id;
        node.ui_position = position;
        self.project.add_node(node);
        self.project.attach_node_to_container(owner, id).unwrap();
        self.geometry
            .insert(id, NodeLayoutGeometry { position, size });
        id
    }

    fn connect(&mut self, from: Uuid, to: Uuid, order: i64) {
        self.connect_ports(from, IMAGE_OUTPUT_PORT, to, MERGE_IMAGES_PORT, order);
    }

    fn connect_ports(&mut self, from: Uuid, from_port: &str, to: Uuid, to_port: &str, order: i64) {
        let mut connection = ProjectConnection::new(
            PortAddress::new(PortOwner::Node(from), from_port),
            PortAddress::new(PortOwner::Node(to), to_port),
            order,
        );
        connection.id =
            Uuid::from_u128(100_000 + self.project.connections.len().try_into().unwrap_or(0));
        self.project.connections.push(connection);
    }

    fn request<'a>(
        &'a self,
        owner: NodeContainer,
        anchor: Uuid,
        selection: TestSelection<'a>,
        direction: BranchDirection,
        axis: LayoutAxis,
        mode: DirectionalLayoutMode,
    ) -> DirectionalLayoutRequest<'a> {
        DirectionalLayoutRequest {
            composition_id: self.composition_id,
            direct_owner: owner,
            anchor_node_id: anchor,
            frozen_selected_node_ids: selection.selected,
            fixed_node_ids: selection.fixed,
            direction,
            axis,
            mode,
            node_geometry: &self.geometry,
            horizontal_gap: H_GAP,
            vertical_gap: V_GAP,
        }
    }
}

fn planned_geometry(
    fixture: &Fixture,
    plan: &DirectionalLayoutPlan,
    node_id: Uuid,
) -> NodeLayoutGeometry {
    let geometry = fixture.geometry[&node_id];
    geometry.with_position(
        plan.node_positions
            .get(&node_id)
            .copied()
            .unwrap_or(geometry.position),
    )
}

fn assert_ltr(fixture: &Fixture, plan: &DirectionalLayoutPlan, from: Uuid, to: Uuid) {
    let from = planned_geometry(fixture, plan, from);
    let to = planned_geometry(fixture, plan, to);
    assert!(
        from.right() + H_GAP <= to.position[0] + POSITION_EPSILON,
        "{from:?} must flow left-to-right into {to:?}",
    );
}

#[test]
fn diamond_layout_uses_authored_topology_and_visual_node_sizes() {
    let mut fixture = Fixture::new();
    let owner = NodeContainer::Composition(fixture.composition_id);
    let source = fixture.add_node(owner, 1, "Source", [100.0, 100.0], [120.0, 80.0]);
    let upper = fixture.add_node(owner, 2, "Upper", [110.0, 100.0], [180.0, 70.0]);
    let lower = fixture.add_node(owner, 3, "Lower", [110.0, 100.0], [90.0, 150.0]);
    let merge = fixture.add_node(owner, 4, "Merge", [120.0, 100.0], [240.0, 100.0]);
    fixture.connect(source, upper, 0);
    fixture.connect(source, lower, 0);
    fixture.connect(upper, merge, 1);
    fixture.connect(lower, merge, 0);
    let before = fixture.project.clone();

    let plan = plan_directional_layout(
        &fixture.project,
        &fixture.request(
            owner,
            source,
            TestSelection::ALL,
            BranchDirection::Downstream,
            LayoutAxis::Horizontal,
            DirectionalLayoutMode::Layout,
        ),
    )
    .unwrap();

    assert_eq!(fixture.project, before, "planning must be read-only");
    assert!(!plan.node_positions.contains_key(&source));
    assert_eq!(
        plan.diagnostics
            .eligible_node_ids
            .iter()
            .copied()
            .collect::<HashSet<_>>(),
        HashSet::from([upper, lower, merge]),
    );
    assert_ltr(&fixture, &plan, source, upper);
    assert_ltr(&fixture, &plan, source, lower);
    assert_ltr(&fixture, &plan, upper, merge);
    assert_ltr(&fixture, &plan, lower, merge);
    let upper = planned_geometry(&fixture, &plan, upper);
    let lower = planned_geometry(&fixture, &plan, lower);
    assert!(
        upper.bottom() + V_GAP <= lower.position[1] || lower.bottom() + V_GAP <= upper.position[1],
        "same-rank Nodes must be packed by their rendered heights",
    );
}

#[test]
fn selection_is_intersected_after_reachability_and_unrelated_nodes_stay_fixed() {
    let mut fixture = Fixture::new();
    let owner = NodeContainer::Composition(fixture.composition_id);
    let source = fixture.add_node(owner, 10, "Source", [0.0, 0.0], [100.0, 80.0]);
    let bridge = fixture.add_node(owner, 11, "Bridge", [180.0, 0.0], [100.0, 80.0]);
    let selected = fixture.add_node(owner, 12, "Selected", [20.0, 0.0], [100.0, 80.0]);
    let unrelated = fixture.add_node(owner, 13, "Unrelated", [310.0, 0.0], [140.0, 130.0]);
    fixture.connect(source, bridge, 0);
    fixture.connect(bridge, selected, 0);

    let plan = plan_directional_layout(
        &fixture.project,
        &fixture.request(
            owner,
            source,
            TestSelection {
                selected: &[selected],
                fixed: &[],
            },
            BranchDirection::Downstream,
            LayoutAxis::Horizontal,
            DirectionalLayoutMode::Layout,
        ),
    )
    .unwrap();

    assert_eq!(plan.diagnostics.eligible_node_ids, vec![selected]);
    assert!(plan.diagnostics.reachable_node_ids.contains(&bridge));
    assert!(plan.diagnostics.reachable_node_ids.contains(&selected));
    assert!(!plan.node_positions.contains_key(&bridge));
    assert!(!plan.node_positions.contains_key(&unrelated));
    assert_ltr(&fixture, &plan, bridge, selected);
    let selected_rect = planned_geometry(&fixture, &plan, selected);
    let unrelated_rect = fixture.geometry[&unrelated];
    assert!(selected_rect.position[1] >= unrelated_rect.bottom() + V_GAP);
}

#[test]
fn explicit_fixed_and_cross_owner_nodes_are_reported_and_never_moved() {
    let mut fixture = Fixture::new();
    let composition_owner = NodeContainer::Composition(fixture.composition_id);
    let track_owner = NodeContainer::Track(fixture.track_id);
    let source = fixture.add_node(composition_owner, 20, "Source", [0.0, 0.0], [100.0, 80.0]);
    let fixed = fixture.add_node(composition_owner, 21, "Fixed", [200.0, 0.0], [100.0, 80.0]);
    let foreign = fixture.add_node(track_owner, 22, "Foreign", [400.0, 0.0], [100.0, 80.0]);
    let reentered = fixture.add_node(
        composition_owner,
        23,
        "Re-entered",
        [600.0, 0.0],
        [100.0, 80.0],
    );
    fixture.connect(source, fixed, 0);
    fixture.connect(fixed, foreign, 0);
    fixture.connect(foreign, reentered, 0);

    let plan = plan_directional_layout(
        &fixture.project,
        &fixture.request(
            composition_owner,
            source,
            TestSelection {
                selected: &[],
                fixed: &[fixed],
            },
            BranchDirection::Downstream,
            LayoutAxis::Horizontal,
            DirectionalLayoutMode::Layout,
        ),
    )
    .unwrap();

    assert!(plan.node_positions.is_empty());
    assert!(plan.diagnostics.blocked_nodes.contains(&blocked_node(
        fixed,
        DirectionalLayoutBlockedReason::ExplicitlyFixed,
    )));
    assert!(plan.diagnostics.blocked_nodes.contains(&blocked_node(
        foreign,
        DirectionalLayoutBlockedReason::CrossesDirectOwner,
    )));
    assert!(plan.diagnostics.blocked_nodes.contains(&blocked_node(
        reentered,
        DirectionalLayoutBlockedReason::CrossesDirectOwner,
    )));
}

#[test]
fn container_output_does_not_create_a_hidden_structural_helper_edge() {
    let mut fixture = Fixture::new();
    let mut clip = Clip::new("Clip", 0.0, 5.0);
    let clip_id = clip.id;
    let clip_owner = NodeContainer::Clip(clip_id);
    fixture.project.add_clip(clip.clone());
    fixture
        .project
        .attach_clip_to_track(fixture.track_id, clip_id)
        .unwrap();
    let helper = fixture.add_node(clip_owner, 30, "Output helper", [0.0, 0.0], [100.0, 80.0]);
    let target = fixture.add_node(
        NodeContainer::Track(fixture.track_id),
        31,
        "Target",
        [300.0, 0.0],
        [100.0, 80.0],
    );
    clip = fixture.project.get_clip(clip_id).unwrap().clone();
    clip.output_node_id = Some(helper);
    *fixture.project.get_clip_mut(clip_id).unwrap() = clip;
    fixture.project.connections.push(ProjectConnection::new(
        PortAddress::new(PortOwner::Clip(clip_id), IMAGE_OUTPUT_PORT),
        PortAddress::new(PortOwner::Node(target), MERGE_IMAGES_PORT),
        0,
    ));

    assert!(
        !super::graph::actual_edge_pairs(&fixture.project, fixture.composition_id)
            .contains(&(helper, target))
    );
    let plan = plan_directional_layout(
        &fixture.project,
        &fixture.request(
            clip_owner,
            helper,
            TestSelection::ALL,
            BranchDirection::Downstream,
            LayoutAxis::Horizontal,
            DirectionalLayoutMode::Layout,
        ),
    )
    .unwrap();
    assert!(plan.diagnostics.reachable_node_ids.is_empty());
    assert!(plan.node_positions.is_empty());
}

#[test]
fn layout_keeps_direct_nodes_clear_of_immediate_child_containers(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = Fixture::new();
    let composition_owner = NodeContainer::Composition(fixture.composition_id);
    let track_owner = NodeContainer::Track(fixture.track_id);
    {
        let track = fixture
            .project
            .get_track_mut(fixture.track_id)
            .ok_or("fixture Track is missing")?;
        track.ui_position = [220.0, 40.0];
        track.ui_size = [400.0, 300.0];
    }
    let composition_source = fixture.add_node(
        composition_owner,
        32,
        "Composition source",
        [80.0, 80.0],
        [100.0, 80.0],
    );
    let composition_sink = fixture.add_node(
        composition_owner,
        33,
        "Composition sink",
        [80.0, 220.0],
        [100.0, 80.0],
    );
    fixture.connect(composition_source, composition_sink, 0);
    let composition_plan = plan_directional_layout(
        &fixture.project,
        &fixture.request(
            composition_owner,
            composition_source,
            TestSelection::ALL,
            BranchDirection::Downstream,
            LayoutAxis::Horizontal,
            DirectionalLayoutMode::Layout,
        ),
    )?;
    let track_rect = immediate_child_rects(
        &fixture.project,
        &AutoLayoutPlan::default(),
        composition_owner,
    )
    .into_iter()
    .next()
    .ok_or("Composition child Track rectangle is missing")?;
    let composition_sink_rect = planned_geometry(&fixture, &composition_plan, composition_sink);
    let composition_sink_rect = egui::Rect::from_min_size(
        egui::pos2(
            composition_sink_rect.position[0],
            composition_sink_rect.position[1],
        ),
        egui::vec2(composition_sink_rect.size[0], composition_sink_rect.size[1]),
    );
    assert!(!composition_sink_rect.intersects(track_rect));
    assert!(composition_sink_rect.top() >= track_rect.bottom() + V_GAP);

    let mut clip = Clip::new("Clip obstacle", 0.0, 5.0);
    clip.ui_position = [420.0, 450.0];
    clip.ui_size = [360.0, 260.0];
    let clip_id = clip.id;
    fixture.project.add_clip(clip);
    fixture
        .project
        .attach_clip_to_track(fixture.track_id, clip_id)?;
    let track_source = fixture.add_node(
        track_owner,
        34,
        "Track source",
        [280.0, 500.0],
        [100.0, 80.0],
    );
    let track_sink = fixture.add_node(track_owner, 35, "Track sink", [280.0, 620.0], [100.0, 80.0]);
    fixture.connect(track_source, track_sink, 0);
    let track_plan = plan_directional_layout(
        &fixture.project,
        &fixture.request(
            track_owner,
            track_source,
            TestSelection::ALL,
            BranchDirection::Downstream,
            LayoutAxis::Horizontal,
            DirectionalLayoutMode::Layout,
        ),
    )?;
    let clip_rect =
        immediate_child_rects(&fixture.project, &AutoLayoutPlan::default(), track_owner)
            .into_iter()
            .next()
            .ok_or("Track child Clip rectangle is missing")?;
    let track_sink_rect = planned_geometry(&fixture, &track_plan, track_sink);
    let track_sink_rect = egui::Rect::from_min_size(
        egui::pos2(track_sink_rect.position[0], track_sink_rect.position[1]),
        egui::vec2(track_sink_rect.size[0], track_sink_rect.size[1]),
    );
    assert!(!track_sink_rect.intersects(clip_rect));
    assert!(track_sink_rect.top() >= clip_rect.bottom() + V_GAP);
    Ok(())
}

#[test]
fn cycles_share_an_scc_and_vertical_layout_still_preserves_ltr_flow() {
    let mut fixture = Fixture::new();
    let owner = NodeContainer::Composition(fixture.composition_id);
    let source = fixture.add_node(owner, 40, "Source", [0.0, 0.0], [100.0, 80.0]);
    let cycle_a = fixture.add_node(owner, 41, "Cycle A", [0.0, 0.0], [120.0, 70.0]);
    let cycle_b = fixture.add_node(owner, 42, "Cycle B", [0.0, 0.0], [160.0, 90.0]);
    let sink = fixture.add_node(owner, 43, "Sink", [0.0, 0.0], [80.0, 80.0]);
    fixture.connect(source, cycle_a, 0);
    fixture.connect(cycle_a, cycle_b, 0);
    fixture.connect(cycle_b, cycle_a, 0);
    fixture.connect(cycle_b, sink, 0);
    let request = fixture.request(
        owner,
        source,
        TestSelection::ALL,
        BranchDirection::Downstream,
        LayoutAxis::Vertical,
        DirectionalLayoutMode::Layout,
    );
    let first = plan_directional_layout(&fixture.project, &request).unwrap();
    let second = plan_directional_layout(&fixture.project, &request).unwrap();

    assert_eq!(first, second);
    assert_ltr(&fixture, &first, source, cycle_a);
    assert_ltr(&fixture, &first, cycle_b, sink);
    let cycle_a = planned_geometry(&fixture, &first, cycle_a);
    let cycle_b = planned_geometry(&fixture, &first, cycle_b);
    assert_eq!(cycle_a.position[0], cycle_b.position[0]);
}

fn named_positions(
    fixture: &Fixture,
    plan: &DirectionalLayoutPlan,
    ids: &[Uuid],
) -> BTreeMap<String, [f32; 2]> {
    ids.iter()
        .map(|node_id| {
            (
                fixture.project.get_node(*node_id).unwrap().name.clone(),
                planned_geometry(fixture, plan, *node_id).position,
            )
        })
        .collect()
}

fn semantic_diamond(id_offset: u128, reverse_insert: bool) -> (Fixture, Uuid, Vec<Uuid>) {
    let mut fixture = Fixture::new();
    let owner = NodeContainer::Composition(fixture.composition_id);
    let mut specs = [
        (1, "Source", [0.0, 50.0], [100.0, 80.0]),
        (2, "Upper", [0.0, 10.0], [120.0, 70.0]),
        (3, "Lower", [0.0, 150.0], [120.0, 70.0]),
        (4, "Merge", [0.0, 50.0], [180.0, 100.0]),
    ];
    if reverse_insert {
        specs.reverse();
    }
    let mut by_name = HashMap::new();
    for (id, name, position, size) in &specs {
        by_name.insert(
            *name,
            fixture.add_node(owner, id_offset + *id, name, *position, *size),
        );
    }
    let source = by_name["Source"];
    let upper = by_name["Upper"];
    let lower = by_name["Lower"];
    let merge = by_name["Merge"];
    let mut connections = vec![
        (source, upper, 0),
        (source, lower, 0),
        (upper, merge, 2),
        (lower, merge, 1),
    ];
    if reverse_insert {
        connections.reverse();
    }
    for (from, to, order) in connections {
        fixture.connect(from, to, order);
    }
    (fixture, source, vec![source, upper, lower, merge])
}

#[test]
fn semantic_layout_is_invariant_to_uuid_and_insertion_order() {
    let (first, first_source, first_ids) = semantic_diamond(1_000, false);
    let (second, second_source, second_ids) = semantic_diamond(9_000, true);
    let first_plan = plan_directional_layout(
        &first.project,
        &first.request(
            NodeContainer::Composition(first.composition_id),
            first_source,
            TestSelection::ALL,
            BranchDirection::Downstream,
            LayoutAxis::Horizontal,
            DirectionalLayoutMode::Layout,
        ),
    )
    .unwrap();
    let second_plan = plan_directional_layout(
        &second.project,
        &second.request(
            NodeContainer::Composition(second.composition_id),
            second_source,
            TestSelection::ALL,
            BranchDirection::Downstream,
            LayoutAxis::Horizontal,
            DirectionalLayoutMode::Layout,
        ),
    )
    .unwrap();

    assert_eq!(
        named_positions(&first, &first_plan, &first_ids),
        named_positions(&second, &second_plan, &second_ids),
    );
}

#[test]
fn variable_width_chain_uses_edge_to_edge_gap_not_center_distance() {
    let mut fixture = Fixture::new();
    let owner = NodeContainer::Composition(fixture.composition_id);
    let source = fixture.add_node(owner, 50, "Source", [50.0, 0.0], [130.0, 60.0]);
    let wide = fixture.add_node(owner, 51, "Wide", [0.0, 0.0], [310.0, 100.0]);
    let sink = fixture.add_node(owner, 52, "Sink", [0.0, 0.0], [75.0, 50.0]);
    fixture.connect(source, wide, 0);
    fixture.connect(wide, sink, 0);

    let plan = plan_directional_layout(
        &fixture.project,
        &fixture.request(
            owner,
            source,
            TestSelection::ALL,
            BranchDirection::Downstream,
            LayoutAxis::Horizontal,
            DirectionalLayoutMode::Layout,
        ),
    )
    .unwrap();
    let source = planned_geometry(&fixture, &plan, source);
    let wide = planned_geometry(&fixture, &plan, wide);
    let sink = planned_geometry(&fixture, &plan, sink);
    assert_eq!(wide.position[0], source.right() + H_GAP);
    assert_eq!(sink.position[0], wide.right() + H_GAP);
}

#[test]
fn anchor_stays_fixed_for_align_distribute_and_upstream_distribution() {
    let mut fixture = Fixture::new();
    let owner = NodeContainer::Composition(fixture.composition_id);
    let first = fixture.add_node(owner, 60, "First", [0.0, 300.0], [80.0, 60.0]);
    let middle = fixture.add_node(owner, 61, "Middle", [300.0, 500.0], [160.0, 100.0]);
    let anchor = fixture.add_node(owner, 62, "Anchor", [700.0, 100.0], [120.0, 80.0]);
    fixture.connect(first, middle, 0);
    fixture.connect(middle, anchor, 0);
    let plan = plan_directional_layout(
        &fixture.project,
        &fixture.request(
            owner,
            anchor,
            TestSelection {
                selected: &[anchor, middle, first],
                fixed: &[],
            },
            BranchDirection::Upstream,
            LayoutAxis::Horizontal,
            DirectionalLayoutMode::AlignAndDistribute,
        ),
    )
    .unwrap();

    assert!(!plan.node_positions.contains_key(&anchor));
    let first = planned_geometry(&fixture, &plan, first);
    let middle = planned_geometry(&fixture, &plan, middle);
    let anchor_geometry = fixture.geometry[&anchor];
    assert_eq!(middle.right() + H_GAP, anchor_geometry.position[0]);
    assert_eq!(first.right() + H_GAP, middle.position[0]);
    assert_eq!(first.center()[1], anchor_geometry.center()[1]);
    assert_eq!(middle.center()[1], anchor_geometry.center()[1]);
}

#[test]
fn align_keeps_anchor_and_explicitly_fixed_selection_unchanged() {
    let mut fixture = Fixture::new();
    let owner = NodeContainer::Composition(fixture.composition_id);
    let anchor = fixture.add_node(owner, 65, "Anchor", [100.0, 100.0], [120.0, 80.0]);
    let movable = fixture.add_node(owner, 66, "Movable", [400.0, 500.0], [100.0, 40.0]);
    let fixed = fixture.add_node(owner, 67, "Fixed", [650.0, 700.0], [140.0, 100.0]);
    fixture.connect(anchor, movable, 0);
    fixture.connect(movable, fixed, 0);
    let plan = plan_directional_layout(
        &fixture.project,
        &fixture.request(
            owner,
            anchor,
            TestSelection {
                selected: &[anchor, movable, fixed],
                fixed: &[fixed],
            },
            BranchDirection::Downstream,
            LayoutAxis::Horizontal,
            DirectionalLayoutMode::Align,
        ),
    )
    .unwrap();

    assert!(!plan.node_positions.contains_key(&anchor));
    assert!(!plan.node_positions.contains_key(&fixed));
    assert_eq!(
        planned_geometry(&fixture, &plan, movable).center()[1],
        fixture.geometry[&anchor].center()[1],
    );
    assert!(plan.diagnostics.blocked_nodes.contains(&blocked_node(
        fixed,
        DirectionalLayoutBlockedReason::ExplicitlyFixed,
    )));
}

#[test]
fn vertical_distribution_keeps_visual_gaps_and_horizontal_graph_flow() {
    let mut fixture = Fixture::new();
    let owner = NodeContainer::Composition(fixture.composition_id);
    let source = fixture.add_node(owner, 70, "Source", [0.0, 0.0], [100.0, 100.0]);
    let first = fixture.add_node(owner, 71, "First", [0.0, 0.0], [80.0, 60.0]);
    let second = fixture.add_node(owner, 72, "Second", [0.0, 0.0], [140.0, 120.0]);
    fixture.connect(source, first, 0);
    fixture.connect(first, second, 0);
    let plan = plan_directional_layout(
        &fixture.project,
        &fixture.request(
            owner,
            source,
            TestSelection::ALL,
            BranchDirection::Downstream,
            LayoutAxis::Vertical,
            DirectionalLayoutMode::Distribute,
        ),
    )
    .unwrap();

    let source_rect = fixture.geometry[&source];
    let first_rect = planned_geometry(&fixture, &plan, first);
    let second_rect = planned_geometry(&fixture, &plan, second);
    assert_eq!(first_rect.position[1], source_rect.bottom() + V_GAP);
    assert_eq!(second_rect.position[1], first_rect.bottom() + V_GAP);
    assert_ltr(&fixture, &plan, source, first);
    assert_ltr(&fixture, &plan, first, second);
}
