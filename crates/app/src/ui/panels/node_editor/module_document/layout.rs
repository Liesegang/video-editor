//! Deterministic, project-independent layout for a single Module graph.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use library::editor::ModuleNodePresentationUpdate;
use library::model::authoring::ModuleDefinition;
use uuid::Uuid;

use crate::command::CommandId;

const COLUMN_GAP: f32 = 120.0;
const ROW_GAP: f32 = 72.0;

pub(super) fn module_layout_updates(
    definition: &ModuleDefinition,
    command: CommandId,
    selected_nodes: &HashSet<Uuid>,
) -> Vec<ModuleNodePresentationUpdate> {
    let targets = layout_targets(definition, command, selected_nodes);
    if targets.is_empty() {
        return Vec::new();
    }

    let ranks = dependency_ranks(definition, &targets);
    let anchor = layout_anchor(definition, &targets);
    let mut columns = BTreeMap::<usize, Vec<Uuid>>::new();
    for node_id in &targets {
        columns.entry(ranks[node_id]).or_default().push(*node_id);
    }
    for nodes in columns.values_mut() {
        nodes.sort_unstable();
    }

    let mut x = anchor[0];
    let mut updates = Vec::with_capacity(targets.len());
    for nodes in columns.values() {
        let mut y = anchor[1];
        let mut column_width = 0.0_f32;
        for node_id in nodes {
            let Some(node) = definition.graph.nodes.get(node_id) else {
                continue;
            };
            let size = sanitized_size(node.ui_size);
            column_width = column_width.max(size[0]);
            let position = [x, y];
            if node.ui_position != position || node.ui_size != size {
                updates.push(ModuleNodePresentationUpdate {
                    node_id: *node_id,
                    position,
                    size,
                    collapsed: node.ui_collapsed,
                });
            }
            y += size[1] + ROW_GAP;
        }
        x += column_width + COLUMN_GAP;
    }
    updates
}

fn layout_targets(
    definition: &ModuleDefinition,
    command: CommandId,
    selected_nodes: &HashSet<Uuid>,
) -> BTreeSet<Uuid> {
    let selected = || {
        selected_nodes
            .iter()
            .filter(|node_id| definition.graph.nodes.contains_key(node_id))
            .copied()
            .collect::<BTreeSet<_>>()
    };
    match command {
        CommandId::NodeEditorCleanLayout if !selected_nodes.is_empty() => selected(),
        CommandId::NodeEditorCleanLayoutSelection => selected(),
        CommandId::NodeEditorCleanLayout
        | CommandId::NodeEditorCleanLayoutContainer
        | CommandId::NodeEditorCleanLayoutAll => definition.graph.nodes.keys().copied().collect(),
        _ => BTreeSet::new(),
    }
}

fn dependency_ranks(
    definition: &ModuleDefinition,
    targets: &BTreeSet<Uuid>,
) -> HashMap<Uuid, usize> {
    let mut indegree = targets
        .iter()
        .copied()
        .map(|node_id| (node_id, 0_usize))
        .collect::<HashMap<_, _>>();
    let mut outgoing = HashMap::<Uuid, Vec<Uuid>>::new();
    for connection in &definition.graph.connections {
        if targets.contains(&connection.from.node_id) && targets.contains(&connection.to.node_id) {
            *indegree.entry(connection.to.node_id).or_default() += 1;
            outgoing
                .entry(connection.from.node_id)
                .or_default()
                .push(connection.to.node_id);
        }
    }
    for targets in outgoing.values_mut() {
        targets.sort_unstable();
    }

    let mut ready = indegree
        .iter()
        .filter_map(|(node_id, count)| (*count == 0).then_some(*node_id))
        .collect::<BTreeSet<_>>();
    let mut ranks = targets
        .iter()
        .copied()
        .map(|node_id| (node_id, 0_usize))
        .collect::<HashMap<_, _>>();
    while let Some(node_id) = ready.pop_first() {
        let next_rank = ranks[&node_id].saturating_add(1);
        for target in outgoing.get(&node_id).into_iter().flatten() {
            let rank = ranks.entry(*target).or_default();
            *rank = (*rank).max(next_rank);
            let Some(count) = indegree.get_mut(target) else {
                continue;
            };
            *count = count.saturating_sub(1);
            if *count == 0 {
                ready.insert(*target);
            }
        }
    }
    ranks
}

fn layout_anchor(definition: &ModuleDefinition, targets: &BTreeSet<Uuid>) -> [f32; 2] {
    let mut x = f32::INFINITY;
    let mut y = f32::INFINITY;
    for node_id in targets {
        let Some(node) = definition.graph.nodes.get(node_id) else {
            continue;
        };
        if node.ui_position[0].is_finite() {
            x = x.min(node.ui_position[0]);
        }
        if node.ui_position[1].is_finite() {
            y = y.min(node.ui_position[1]);
        }
    }
    [
        if x.is_finite() { x } else { 0.0 },
        if y.is_finite() { y } else { 0.0 },
    ]
}

fn sanitized_size(size: [f32; 2]) -> [f32; 2] {
    [
        if size[0].is_finite() && size[0] > 0.0 {
            size[0]
        } else {
            240.0
        },
        if size[1].is_finite() && size[1] > 0.0 {
            size[1]
        } else {
            160.0
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::authoring::{
        ModuleConnection, ModuleConnectionId, ModuleDefinitionId, ModuleDefinitionSharing,
        ModuleGraph, ModuleInterface, ModulePortAddress,
    };
    use library::model::Node;

    fn definition_with_chain() -> (ModuleDefinition, [Uuid; 3]) {
        let mut first = Node::new_merge("First");
        let mut second = Node::new_merge("Second");
        let mut third = Node::new_merge("Third");
        first.ui_position = [40.0, 80.0];
        second.ui_position = [40.0, 400.0];
        third.ui_position = [40.0, 720.0];
        let ids = [first.id, second.id, third.id];
        let connection = |from, to| ModuleConnection {
            id: ModuleConnectionId::new(),
            from: ModulePortAddress {
                node_id: from,
                port: "image".to_string(),
            },
            to: ModulePortAddress {
                node_id: to,
                port: "image".to_string(),
            },
            order: 0,
            blend_mode: library::model::BlendMode::Normal,
        };
        (
            ModuleDefinition {
                id: ModuleDefinitionId::new(),
                name: "Layout fixture".to_string(),
                sharing: ModuleDefinitionSharing::SharedLocal,
                graph: ModuleGraph {
                    nodes: HashMap::from([(ids[0], first), (ids[1], second), (ids[2], third)]),
                    connections: vec![connection(ids[0], ids[1]), connection(ids[1], ids[2])],
                },
                interface: ModuleInterface::default(),
                topology_revision: 1,
                interface_version: 1,
            },
            ids,
        )
    }

    fn effective_position(
        definition: &ModuleDefinition,
        updates: &[ModuleNodePresentationUpdate],
        node_id: Uuid,
    ) -> [f32; 2] {
        updates
            .iter()
            .find(|update| update.node_id == node_id)
            .map_or(definition.graph.nodes[&node_id].ui_position, |update| {
                update.position
            })
    }

    #[test]
    fn full_layout_places_dependencies_in_successive_columns() {
        let (definition, ids) = definition_with_chain();
        let updates = module_layout_updates(
            &definition,
            CommandId::NodeEditorCleanLayoutAll,
            &HashSet::new(),
        );
        let first = effective_position(&definition, &updates, ids[0]);
        let second = effective_position(&definition, &updates, ids[1]);
        let third = effective_position(&definition, &updates, ids[2]);
        assert!(first[0] < second[0]);
        assert!(second[0] < third[0]);
    }

    #[test]
    fn selection_layout_does_not_emit_updates_for_sibling_nodes() {
        let (definition, ids) = definition_with_chain();
        let updates = module_layout_updates(
            &definition,
            CommandId::NodeEditorCleanLayoutSelection,
            &HashSet::from([ids[1], ids[2]]),
        );
        assert!(updates.iter().all(|update| update.node_id != ids[0]));
        let second = effective_position(&definition, &updates, ids[1]);
        let third = effective_position(&definition, &updates, ids[2]);
        assert!(second[0] < third[0]);
    }

    #[test]
    fn explicit_selection_command_without_a_selection_is_a_noop() {
        let (definition, _) = definition_with_chain();
        assert!(module_layout_updates(
            &definition,
            CommandId::NodeEditorCleanLayoutSelection,
            &HashSet::new(),
        )
        .is_empty());
    }
}
