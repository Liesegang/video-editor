use super::*;

use std::cmp::Ordering;
use std::collections::HashMap;

use library::model::ListContent;
use library::model::project::connection::{LIST_ITEM_OUTPUT_PORT, LIST_ITEMS_INPUT_PORT};
use library::model::project::{AUDIO_OUTPUT_PORT, MERGE_SOUNDS_PORT};

impl Fixture {
    fn add_sound_merge(
        &mut self,
        owner: NodeContainer,
        id: u128,
        name: &str,
        position: [f32; 2],
        size: [f32; 2],
    ) -> Uuid {
        self.add_authored_node(owner, id, Node::new_sound_merge(name), position, size)
    }

    fn add_list_merge(
        &mut self,
        owner: NodeContainer,
        id: u128,
        name: &str,
        position: [f32; 2],
        size: [f32; 2],
    ) -> Uuid {
        self.add_authored_node(
            owner,
            id,
            Node::new_list(name, ListContent::Make),
            position,
            size,
        )
    }
}

fn upstream_layout(fixture: &Fixture, owner: NodeContainer, target: Uuid) -> DirectionalLayoutPlan {
    plan_directional_layout(
        &fixture.project,
        &fixture.request(
            owner,
            target,
            TestSelection::ALL,
            BranchDirection::Upstream,
            LayoutAxis::Horizontal,
            DirectionalLayoutMode::Layout,
        ),
    )
    .unwrap()
}

fn assert_top_to_bottom(fixture: &Fixture, plan: &DirectionalLayoutPlan, expected: &[Uuid]) {
    let actual = {
        let mut nodes = expected.to_vec();
        nodes.sort_by(|left, right| {
            planned_geometry(fixture, plan, *left).position[1]
                .total_cmp(&planned_geometry(fixture, plan, *right).position[1])
        });
        nodes
    };
    assert_eq!(actual, expected);
}

#[test]
fn image_merge_sources_follow_front_to_back_visual_rows() {
    let mut fixture = Fixture::new();
    let owner = NodeContainer::Composition(fixture.composition_id);
    // Authored Y is deliberately the reverse of the Merge row order. The
    // physical variadic order remains authoritative after block packing.
    let back = fixture.add_node(owner, 101, "Back", [0.0, 0.0], [100.0, 80.0]);
    let middle = fixture.add_node(owner, 102, "Middle", [0.0, 100.0], [100.0, 80.0]);
    let front = fixture.add_node(owner, 103, "Front", [0.0, 200.0], [100.0, 80.0]);
    let merge = fixture.add_node(owner, 104, "Image Merge", [400.0, 100.0], [180.0, 100.0]);
    fixture.connect(back, merge, 0);
    fixture.connect(middle, merge, 1);
    fixture.connect(front, merge, 2);

    let plan = upstream_layout(&fixture, owner, merge);

    assert_top_to_bottom(&fixture, &plan, &[front, middle, back]);
}

#[test]
fn sound_merge_sources_follow_canonical_top_to_bottom_rows() {
    let mut fixture = Fixture::new();
    let owner = NodeContainer::Composition(fixture.composition_id);
    let first = fixture.add_node(owner, 111, "First", [0.0, 100.0], [100.0, 80.0]);
    let second = fixture.add_node(owner, 112, "Second", [0.0, 100.0], [100.0, 80.0]);
    let third = fixture.add_node(owner, 113, "Third", [0.0, 100.0], [100.0, 80.0]);
    let merge = fixture.add_sound_merge(owner, 114, "Sound Merge", [400.0, 100.0], [180.0, 100.0]);
    fixture.connect_ports(first, AUDIO_OUTPUT_PORT, merge, MERGE_SOUNDS_PORT, 0);
    fixture.connect_ports(second, AUDIO_OUTPUT_PORT, merge, MERGE_SOUNDS_PORT, 1);
    fixture.connect_ports(third, AUDIO_OUTPUT_PORT, merge, MERGE_SOUNDS_PORT, 2);

    let plan = upstream_layout(&fixture, owner, merge);

    assert_top_to_bottom(&fixture, &plan, &[first, second, third]);
}

#[test]
fn make_list_sources_follow_canonical_top_to_bottom_rows() {
    let mut fixture = Fixture::new();
    let owner = NodeContainer::Composition(fixture.composition_id);
    let first = fixture.add_node(owner, 121, "First", [0.0, 100.0], [100.0, 80.0]);
    let second = fixture.add_node(owner, 122, "Second", [0.0, 100.0], [100.0, 80.0]);
    let third = fixture.add_node(owner, 123, "Third", [0.0, 100.0], [100.0, 80.0]);
    let make = fixture.add_list_merge(owner, 124, "Make List", [400.0, 100.0], [180.0, 100.0]);
    fixture.connect_ports(first, LIST_ITEM_OUTPUT_PORT, make, LIST_ITEMS_INPUT_PORT, 0);
    fixture.connect_ports(
        second,
        LIST_ITEM_OUTPUT_PORT,
        make,
        LIST_ITEMS_INPUT_PORT,
        1,
    );
    fixture.connect_ports(third, LIST_ITEM_OUTPUT_PORT, make, LIST_ITEMS_INPUT_PORT, 2);

    let plan = upstream_layout(&fixture, owner, make);

    assert_top_to_bottom(&fixture, &plan, &[first, second, third]);
}

#[test]
fn merge_row_order_is_scoped_to_the_shared_target_port() {
    let mut fixture = Fixture::new();
    let owner = NodeContainer::Composition(fixture.composition_id);
    let back = fixture.add_node(owner, 131, "Back", [0.0, 100.0], [100.0, 80.0]);
    let front = fixture.add_node(owner, 132, "Front", [0.0, 100.0], [100.0, 80.0]);
    let shared = fixture.add_node(owner, 133, "Shared", [400.0, 100.0], [180.0, 100.0]);
    let unrelated = fixture.add_node(owner, 134, "Unrelated", [700.0, 100.0], [180.0, 100.0]);
    fixture.connect(back, shared, 0);
    fixture.connect(front, shared, 1);
    fixture.connect(back, unrelated, 99);

    let plan = upstream_layout(&fixture, owner, shared);

    assert_top_to_bottom(&fixture, &plan, &[front, back]);
}

#[test]
fn ordinary_same_rank_nodes_keep_geometry_order_instead_of_wire_order() {
    let mut fixture = Fixture::new();
    let owner = NodeContainer::Composition(fixture.composition_id);
    let lower = fixture.add_node(owner, 141, "Lower", [0.0, 300.0], [100.0, 80.0]);
    let upper = fixture.add_node(owner, 142, "Upper", [0.0, 20.0], [100.0, 80.0]);
    let target = fixture.add_node(owner, 143, "Ordinary", [400.0, 100.0], [180.0, 100.0]);
    fixture.connect_ports(lower, "ordinary", target, "ordinary", 99);
    fixture.connect_ports(upper, "ordinary", target, "ordinary", 0);

    let plan = upstream_layout(&fixture, owner, target);

    assert_top_to_bottom(&fixture, &plan, &[upper, lower]);
}

#[test]
fn variadic_pair_order_is_precomputed_before_hot_sort_comparisons() {
    let mut fixture = Fixture::new();
    let owner = NodeContainer::Composition(fixture.composition_id);
    let first = fixture.add_node(owner, 151, "First", [0.0, 0.0], [100.0, 80.0]);
    let second = fixture.add_node(owner, 152, "Second", [0.0, 0.0], [100.0, 80.0]);
    let third = fixture.add_node(owner, 153, "Third", [0.0, 0.0], [100.0, 80.0]);
    let merge = fixture.add_node(owner, 154, "Merge", [400.0, 0.0], [180.0, 100.0]);
    fixture.connect(first, merge, 0);
    fixture.connect(second, merge, 1);
    fixture.connect(third, merge, 2);
    let mut previous = third;
    for offset in 0..64_u128 {
        let next = fixture.add_node(
            owner,
            1_000 + offset,
            &format!("Ordinary {offset}"),
            [600.0 + offset as f32, 0.0],
            [100.0, 80.0],
        );
        fixture.connect_ports(previous, "ordinary", next, "ordinary", offset as i64);
        previous = next;
    }
    let node_ids = composition_node_ids(&fixture.project, fixture.composition_id);
    let edges = super::super::graph::actual_node_edges(&fixture.project, &node_ids);
    let order = super::super::graph::SemanticNodeOrder::new(
        &fixture.project,
        &fixture.geometry,
        &node_ids,
        &edges,
    );

    assert_eq!(order.constraint_count(), 2);
    for _ in 0..10_000 {
        assert_eq!(order.compare(third, first), Ordering::Less);
        assert_eq!(order.compare(first, third), Ordering::Greater);
    }
}

fn cyclic_variadic_constraint_fixture(
    id_offset: u128,
    reverse_insert: bool,
) -> (Fixture, Uuid, [Uuid; 3]) {
    let mut fixture = Fixture::new();
    let owner = NodeContainer::Composition(fixture.composition_id);
    let mut specs = [
        (161 + id_offset, "A"),
        (162 + id_offset, "B"),
        (163 + id_offset, "C"),
    ];
    if reverse_insert {
        specs.reverse();
    }
    let mut by_name = HashMap::new();
    for (id, name) in specs {
        let node = fixture.add_node(owner, id, name, [0.0, 100.0], [100.0, 80.0]);
        by_name.insert(name, node);
    }
    let a = by_name["A"];
    let b = by_name["B"];
    let c = by_name["C"];
    let merge_x = fixture.add_node(owner, 171 + id_offset, "X", [500.0, 0.0], [180.0, 100.0]);
    let merge_y = fixture.add_node(owner, 172 + id_offset, "Y", [500.0, 200.0], [180.0, 100.0]);
    let merge_z = fixture.add_node(owner, 173 + id_offset, "Z", [500.0, 400.0], [180.0, 100.0]);
    fixture.connect(a, merge_x, 1);
    fixture.connect(b, merge_x, 0);
    fixture.connect(b, merge_y, 1);
    fixture.connect(c, merge_y, 0);
    fixture.connect(c, merge_z, 1);
    fixture.connect(a, merge_z, 0);
    let target = fixture.add_node(
        owner,
        174 + id_offset,
        "Ordinary target",
        [800.0, 100.0],
        [180.0, 100.0],
    );
    for source in [a, b, c] {
        fixture.connect_ports(source, "ordinary", target, "ordinary", 0);
    }
    (fixture, target, [a, b, c])
}

#[test]
fn cyclic_multi_target_constraints_collapse_to_a_stable_total_order() {
    let (first_fixture, first_target, first_nodes) = cyclic_variadic_constraint_fixture(0, false);
    let first_owner = NodeContainer::Composition(first_fixture.composition_id);
    let first_plan = upstream_layout(&first_fixture, first_owner, first_target);
    assert_top_to_bottom(&first_fixture, &first_plan, &first_nodes);

    let (second_fixture, second_target, second_nodes) =
        cyclic_variadic_constraint_fixture(10_000, true);
    let second_owner = NodeContainer::Composition(second_fixture.composition_id);
    let second_plan = upstream_layout(&second_fixture, second_owner, second_target);

    assert_eq!(
        named_positions(&first_fixture, &first_plan, &first_nodes),
        named_positions(&second_fixture, &second_plan, &second_nodes),
        "SCC fallback must not depend on insertion order or otherwise-distinct UUIDs",
    );
}

fn constrained_with_unrelated_fixture(
    id_offset: u128,
    reverse_insert: bool,
) -> (Fixture, Uuid, [Uuid; 3]) {
    let mut fixture = Fixture::new();
    let owner = NodeContainer::Composition(fixture.composition_id);
    let mut specs = [
        (181 + id_offset, "A", [0.0, 100.0]),
        (182 + id_offset, "B", [0.0, 0.0]),
        (183 + id_offset, "C", [0.0, 50.0]),
    ];
    if reverse_insert {
        specs.reverse();
    }
    let mut by_name = HashMap::new();
    for (id, name, position) in specs {
        let node = fixture.add_node(owner, id, name, position, [100.0, 40.0]);
        by_name.insert(name, node);
    }
    let a = by_name["A"];
    let b = by_name["B"];
    let c = by_name["C"];
    let constrained_merge = fixture.add_node(
        owner,
        184 + id_offset,
        "Constrained merge",
        [500.0, 0.0],
        [180.0, 100.0],
    );
    fixture.connect(a, constrained_merge, 1);
    fixture.connect(b, constrained_merge, 0);
    let target = fixture.add_node(
        owner,
        185 + id_offset,
        "Ordinary target",
        [800.0, 100.0],
        [180.0, 100.0],
    );
    for source in [a, b, c] {
        fixture.connect_ports(source, "ordinary", target, "ordinary", 0);
    }
    (fixture, target, [c, a, b])
}

#[test]
fn constrained_and_unrelated_nodes_share_one_transitive_global_order() {
    let (first_fixture, first_target, expected_first) =
        constrained_with_unrelated_fixture(0, false);
    let first_plan = upstream_layout(
        &first_fixture,
        NodeContainer::Composition(first_fixture.composition_id),
        first_target,
    );
    assert_top_to_bottom(&first_fixture, &first_plan, &expected_first);

    let (second_fixture, second_target, expected_second) =
        constrained_with_unrelated_fixture(20_000, true);
    let second_plan = upstream_layout(
        &second_fixture,
        NodeContainer::Composition(second_fixture.composition_id),
        second_target,
    );
    assert_eq!(
        named_positions(&first_fixture, &first_plan, &expected_first),
        named_positions(&second_fixture, &second_plan, &expected_second),
    );
}

#[test]
fn edge_sort_is_an_unconditional_transitive_total_order() {
    let (fixture, _, _) = cyclic_variadic_constraint_fixture(30_000, true);
    let node_ids = composition_node_ids(&fixture.project, fixture.composition_id);
    let edges = super::super::graph::actual_node_edges(&fixture.project, &node_ids);
    for left in &edges {
        for middle in &edges {
            for right in &edges {
                let left_middle =
                    super::super::graph::semantic_edge_cmp(&fixture.project, left, middle);
                let middle_right =
                    super::super::graph::semantic_edge_cmp(&fixture.project, middle, right);
                if left_middle != Ordering::Greater && middle_right != Ordering::Greater {
                    assert_ne!(
                        super::super::graph::semantic_edge_cmp(&fixture.project, left, right),
                        Ordering::Greater,
                    );
                }
            }
        }
    }
    let mut reversed = edges.clone();
    reversed.reverse();
    reversed.sort_by(|left, right| {
        super::super::graph::semantic_edge_cmp(&fixture.project, left, right)
    });
    assert_eq!(reversed, edges);
}
