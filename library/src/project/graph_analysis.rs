//! Graph analysis utilities for the data-flow graph.
//!
//! These functions allow the timeline/inspector to derive clip-centric views
//! from the graph structure, and the rendering pipeline to determine processing order.

use std::collections::{HashMap, HashSet, VecDeque};
use uuid::Uuid;

use super::connection::{Connection, PinId};
use super::node::Node;
use super::project::Project;

/// Get the chain of effect nodes connected to a clip's image_out pin.
///
/// Follows the image flow: clip.image_out → effect1.image_in, effect1.image_out → effect2.image_in, etc.
/// Returns the list of effect GraphNode IDs in order.
pub fn get_effect_chain(project: &Project, clip_id: Uuid) -> Vec<Uuid> {
    let mut chain = Vec::new();
    let mut current_pin = PinId::new(clip_id, "image_out");

    loop {
        // Find a connection from current_pin to some effect's image_in
        let next_connection = project.connections.iter().find(|c| {
            c.from == current_pin
                && c.to.pin_name == "image_in"
                && is_effect_node(project, c.to.node_id)
        });

        match next_connection {
            Some(conn) => {
                let effect_id = conn.to.node_id;
                // Prevent infinite loops
                if chain.contains(&effect_id) {
                    break;
                }
                chain.push(effect_id);
                current_pin = PinId::new(effect_id, "image_out");
            }
            None => break,
        }
    }

    chain
}

/// Get style nodes associated with a clip (connected to its style input pins).
pub fn get_associated_styles(project: &Project, clip_id: Uuid) -> Vec<Uuid> {
    get_associated_nodes_by_category(project, clip_id, "style.")
}

/// Get effector nodes in the shape chain between clip and style node.
///
/// Follows the shape_out → shape_in chain from clip to style node,
/// collecting all effector.* nodes encountered.
pub fn get_associated_effectors(project: &Project, clip_id: Uuid) -> Vec<Uuid> {
    get_shape_chain_nodes_by_prefix(project, clip_id, "effector.")
}

/// Get decorator nodes in the shape chain between clip and style node.
///
/// Follows the shape_out → shape_in chain from clip to style node,
/// collecting all decorator.* nodes encountered.
pub fn get_associated_decorators(project: &Project, clip_id: Uuid) -> Vec<Uuid> {
    get_shape_chain_nodes_by_prefix(project, clip_id, "decorator.")
}

/// Follow the shape chain from clip.shape_out and collect nodes matching a type prefix.
fn get_shape_chain_nodes_by_prefix(
    project: &Project,
    clip_id: Uuid,
    type_prefix: &str,
) -> Vec<Uuid> {
    let mut result = Vec::new();
    let mut current_pin = PinId::new(clip_id, "shape_out");

    loop {
        // Find connection from current shape_out
        let conn = project
            .connections
            .iter()
            .find(|c| c.from == current_pin && c.to.pin_name == "shape_in");

        match conn {
            Some(c) => {
                let next_id = c.to.node_id;
                // Check if it matches the prefix
                let matches = project
                    .get_graph_node(next_id)
                    .map(|g| g.type_id.starts_with(type_prefix))
                    .unwrap_or(false);
                if matches {
                    if result.contains(&next_id) {
                        break; // Cycle guard
                    }
                    result.push(next_id);
                }
                // Continue following the chain (whether it matched or not)
                current_pin = PinId::new(next_id, "shape_out");
            }
            None => break,
        }
    }

    result
}

/// Get nodes of a specific category connected to a given node, following the chain.
///
/// With chaining (e.g., `style_A.style_out → style_B.style_in, style_B.style_out → clip.style_in`),
/// this follows the chain backwards from the clip's input pin and collects all nodes.
/// Returns them in processing order (furthest from clip first, nearest last).
fn get_associated_nodes_by_category(
    project: &Project,
    node_id: Uuid,
    type_prefix: &str,
) -> Vec<Uuid> {
    // Derive pin name from type prefix (e.g., "style." → "style_in")
    let pin_category = type_prefix.trim_end_matches('.');
    let input_pin_name = format!("{}_in", pin_category);

    let mut chain = Vec::new();
    let mut current_pin = PinId::new(node_id, &input_pin_name);

    loop {
        // Find connection feeding into the current input pin
        let conn = project.connections.iter().find(|c| c.to == current_pin);

        match conn {
            Some(c) => {
                let source_id = c.from.node_id;
                // Verify it's the right node type
                let is_correct_type = project
                    .get_graph_node(source_id)
                    .map(|g| g.type_id.starts_with(type_prefix))
                    .unwrap_or(false);

                if !is_correct_type || chain.contains(&source_id) {
                    break;
                }

                chain.push(source_id);
                // Follow the chain: check this node's own input pin
                current_pin = PinId::new(source_id, &input_pin_name);
            }
            None => break,
        }
    }

    // Reverse so the order is from furthest (applied first) to nearest (closest to clip)
    chain.reverse();
    chain
}

/// Validate a connection before adding it.
///
/// Checks:
/// - Both nodes exist
/// - No self-connections
/// - No duplicate connections to the same input pin
/// - No cycles
pub fn validate_connection(project: &Project, conn: &Connection) -> Result<(), String> {
    // Check nodes exist
    if project.get_node(conn.from.node_id).is_none() {
        return Err(format!("Source node {} not found", conn.from.node_id));
    }
    if project.get_node(conn.to.node_id).is_none() {
        return Err(format!("Destination node {} not found", conn.to.node_id));
    }

    // No self-connections
    if conn.from.node_id == conn.to.node_id {
        return Err("Cannot connect a node to itself".to_string());
    }

    // No duplicate connections to same input pin (each input accepts at most one connection)
    if project
        .connections
        .iter()
        .any(|c| c.to == conn.to && c.id != conn.id)
    {
        return Err(format!(
            "Input pin {}.{} already has a connection",
            conn.to.node_id, conn.to.pin_name
        ));
    }

    // Check for cycles: would adding this connection create a cycle?
    if would_create_cycle(project, conn.from.node_id, conn.to.node_id) {
        return Err("Connection would create a cycle".to_string());
    }

    Ok(())
}

/// Check if connecting from_node → to_node would create a cycle.
/// Returns true if to_node can already reach from_node via existing connections.
fn would_create_cycle(project: &Project, from_node: Uuid, to_node: Uuid) -> bool {
    // BFS from to_node: if from_node is reachable, adding from→to creates a cycle.
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(to_node);

    while let Some(current) = queue.pop_front() {
        if current == from_node {
            return true;
        }
        if !visited.insert(current) {
            continue;
        }
        for conn in &project.connections {
            if conn.from.node_id == current {
                queue.push_back(conn.to.node_id);
            }
        }
    }
    false
}

/// Topological sort of nodes within a container (track).
///
/// Returns nodes in dependency order (sources first, sinks last).
/// Returns Err if there's a cycle.
pub fn topological_sort(project: &Project, container_id: Uuid) -> Result<Vec<Uuid>, String> {
    // Collect all child node IDs of the container
    let child_ids: Vec<Uuid> = match project.get_node(container_id) {
        Some(Node::Track(track)) => track.child_ids.clone(),
        _ => {
            return Err(format!(
                "Container {} not found or not a track",
                container_id
            ));
        }
    };

    let child_set: HashSet<Uuid> = child_ids.iter().copied().collect();

    // Build adjacency list for nodes within this container
    let mut in_degree: HashMap<Uuid, usize> = HashMap::new();
    let mut adj: HashMap<Uuid, Vec<Uuid>> = HashMap::new();

    for &id in &child_ids {
        in_degree.insert(id, 0);
        adj.insert(id, Vec::new());
    }

    for conn in &project.connections {
        // Only consider connections between nodes in this container
        if child_set.contains(&conn.from.node_id) && child_set.contains(&conn.to.node_id) {
            adj.get_mut(&conn.from.node_id)
                .unwrap()
                .push(conn.to.node_id);
            *in_degree.get_mut(&conn.to.node_id).unwrap() += 1;
        }
    }

    // Kahn's algorithm
    let mut queue: VecDeque<Uuid> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(id, _)| *id)
        .collect();

    let mut sorted = Vec::new();

    while let Some(node) = queue.pop_front() {
        sorted.push(node);
        if let Some(neighbors) = adj.get(&node) {
            for &neighbor in neighbors {
                let deg = in_degree.get_mut(&neighbor).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    queue.push_back(neighbor);
                }
            }
        }
    }

    if sorted.len() != child_ids.len() {
        return Err("Cycle detected in container graph".to_string());
    }

    Ok(sorted)
}

/// Get the connection feeding into a specific input pin.
pub fn get_input_connection<'a>(project: &'a Project, pin: &PinId) -> Option<&'a Connection> {
    project.get_input_connection(pin)
}

/// Check if a node is an effect node (type_id starts with "effect.")
fn is_effect_node(project: &Project, node_id: Uuid) -> bool {
    project
        .get_graph_node(node_id)
        .map(|g| g.type_id.starts_with("effect."))
        .unwrap_or(false)
}

/// Resolved context for a clip: all associated graph nodes discovered via connections.
#[derive(Debug, Clone)]
pub struct ClipNodeContext {
    /// The `compositing.transform` node (position/rotation/scale/anchor/opacity).
    pub transform_node: Option<Uuid>,
    /// Effect chain in processing order (clip → effect1 → effect2 …).
    pub effect_chain: Vec<Uuid>,
    /// Style chain (fill/stroke nodes, furthest-from-clip first).
    pub style_chain: Vec<Uuid>,
    /// Effector chain.
    pub effector_chain: Vec<Uuid>,
    /// Decorator chain.
    pub decorator_chain: Vec<Uuid>,
}

/// Resolve all graph nodes associated with a clip.
///
/// This is the single source of truth for determining which graph nodes
/// belong to a clip, used by inspector, preview gizmo, and cascade cleanup.
///
/// Handles both data flows:
/// - Text/Shape: `clip.shape_out → fill.shape_in → fill.image_out → transform.image_in`
/// - Video/Image: `clip.image_out → transform.image_in`
pub fn resolve_clip_context(project: &Project, clip_id: Uuid) -> ClipNodeContext {
    // First, try the shape_out path (text/shape clips)
    let (style_chain, transform_from_shape) = get_shape_chain(project, clip_id);
    // If no shape chain, try the direct image_out path (video/image clips)
    let transform_node = transform_from_shape.or_else(|| get_connected_transform(project, clip_id));

    ClipNodeContext {
        transform_node,
        effect_chain: get_effect_chain(project, clip_id),
        style_chain,
        effector_chain: get_associated_effectors(project, clip_id),
        decorator_chain: get_associated_decorators(project, clip_id),
    }
}

/// Follow the shape chain: clip.shape_out → [effector/decorator]* → style → transform.
///
/// Now traverses through effector/decorator nodes in the shape chain
/// before reaching the style node. Returns (style_nodes, transform_node_id).
fn get_shape_chain(project: &Project, clip_id: Uuid) -> (Vec<Uuid>, Option<Uuid>) {
    let mut style_nodes = Vec::new();
    let mut transform_node = None;

    // Follow the shape chain from clip.shape_out until we find a style node
    let mut current_pin = PinId::new(clip_id, "shape_out");
    let mut visited = HashSet::new();

    let style_node_id = loop {
        let conn = project
            .connections
            .iter()
            .find(|c| c.from == current_pin && c.to.pin_name == "shape_in");

        match conn {
            Some(c) => {
                let next_id = c.to.node_id;
                if !visited.insert(next_id) {
                    break None; // Cycle guard
                }
                if let Some(gn) = project.get_graph_node(next_id) {
                    if gn.type_id.starts_with("style.") {
                        break Some(next_id);
                    }
                }
                // Not a style node (effector/decorator) — continue
                current_pin = PinId::new(next_id, "shape_out");
            }
            None => break None,
        }
    };

    if let Some(fill_id) = style_node_id {
        style_nodes.push(fill_id);
        // Follow fill.image_out → transform.image_in
        let fill_image_out = PinId::new(fill_id, "image_out");
        for c2 in &project.connections {
            if c2.from == fill_image_out
                && c2.to.pin_name == "image_in"
                && project
                    .get_graph_node(c2.to.node_id)
                    .map(|g| g.type_id == "compositing.transform")
                    .unwrap_or(false)
            {
                transform_node = Some(c2.to.node_id);
            }
        }
    }

    (style_nodes, transform_node)
}

/// Find the compositing.transform node connected to clip.image_out.
///
/// For video/image clips: `clip.image_out → transform.image_in`
fn get_connected_transform(project: &Project, clip_id: Uuid) -> Option<Uuid> {
    let clip_image_out = PinId::new(clip_id, "image_out");

    project
        .connections
        .iter()
        .find(|c| {
            c.from == clip_image_out
                && c.to.pin_name == "image_in"
                && project
                    .get_graph_node(c.to.node_id)
                    .map(|g| g.type_id == "compositing.transform")
                    .unwrap_or(false)
        })
        .map(|c| c.to.node_id)
}

/// Collect all graph node IDs associated with a clip (for cascade cleanup).
///
/// Returns the union of transform, effect, style, effector, and decorator nodes.
pub fn collect_all_associated_nodes(project: &Project, clip_id: Uuid) -> Vec<Uuid> {
    let ctx = resolve_clip_context(project, clip_id);
    let mut nodes = Vec::new();
    if let Some(t) = ctx.transform_node {
        nodes.push(t);
    }
    nodes.extend(ctx.effect_chain);
    nodes.extend(ctx.style_chain);
    nodes.extend(ctx.effector_chain);
    nodes.extend(ctx.decorator_chain);
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::connection::Connection;
    use crate::project::graph_node::GraphNode;
    use crate::project::node::Node;
    use crate::project::project::Composition;
    use crate::project::property::PropertyMap;
    use crate::project::track::TrackData;

    fn setup_project() -> (Project, Uuid, Uuid) {
        let mut project = Project::new("Test");
        let root_track = TrackData::new("Root");
        let root_id = root_track.id;
        project.add_node(Node::Track(root_track));

        let comp = Composition::new_with_root("comp", 1920, 1080, 30.0, 10.0, root_id);
        let comp_id = comp.id;
        project.add_composition(comp);

        (project, root_id, comp_id)
    }

    #[test]
    fn test_get_effect_chain_empty() {
        let (project, _, _) = setup_project();
        let clip_id = Uuid::new_v4();
        let chain = get_effect_chain(&project, clip_id);
        assert!(chain.is_empty());
    }

    #[test]
    fn test_get_effect_chain_single() {
        let (mut project, root_id, _) = setup_project();

        // Create a clip and an effect node
        let clip = crate::project::clip::TrackClip {
            id: Uuid::new_v4(),
            reference_id: None,
            kind: crate::project::clip::TrackClipKind::Image,
            in_frame: 0,
            out_frame: 100,
            source_begin_frame: 0,
            duration_frame: None,
            fps: 30.0,
            properties: PropertyMap::new(),
        };
        let clip_id = clip.id;

        let effect = GraphNode::new("effect.blur", PropertyMap::new());
        let effect_id = effect.id;

        project.add_node(Node::Clip(clip));
        project.add_node(Node::Graph(effect));
        project.get_track_mut(root_id).unwrap().add_child(clip_id);
        project.get_track_mut(root_id).unwrap().add_child(effect_id);

        // Connect clip.image_out → effect.image_in
        project.add_connection(Connection::new(
            PinId::new(clip_id, "image_out"),
            PinId::new(effect_id, "image_in"),
        ));

        let chain = get_effect_chain(&project, clip_id);
        assert_eq!(chain, vec![effect_id]);
    }

    #[test]
    fn test_get_effect_chain_multiple() {
        let (mut project, root_id, _) = setup_project();

        let clip = crate::project::clip::TrackClip {
            id: Uuid::new_v4(),
            reference_id: None,
            kind: crate::project::clip::TrackClipKind::Image,
            in_frame: 0,
            out_frame: 100,
            source_begin_frame: 0,
            duration_frame: None,
            fps: 30.0,
            properties: PropertyMap::new(),
        };
        let clip_id = clip.id;

        let effect1 = GraphNode::new("effect.blur", PropertyMap::new());
        let effect1_id = effect1.id;
        let effect2 = GraphNode::new("effect.dilate", PropertyMap::new());
        let effect2_id = effect2.id;

        project.add_node(Node::Clip(clip));
        project.add_node(Node::Graph(effect1));
        project.add_node(Node::Graph(effect2));
        project.get_track_mut(root_id).unwrap().add_child(clip_id);
        project
            .get_track_mut(root_id)
            .unwrap()
            .add_child(effect1_id);
        project
            .get_track_mut(root_id)
            .unwrap()
            .add_child(effect2_id);

        // clip → effect1 → effect2
        project.add_connection(Connection::new(
            PinId::new(clip_id, "image_out"),
            PinId::new(effect1_id, "image_in"),
        ));
        project.add_connection(Connection::new(
            PinId::new(effect1_id, "image_out"),
            PinId::new(effect2_id, "image_in"),
        ));

        let chain = get_effect_chain(&project, clip_id);
        assert_eq!(chain, vec![effect1_id, effect2_id]);
    }

    #[test]
    fn test_validate_connection_self_loop() {
        let (mut project, root_id, _) = setup_project();
        let node = GraphNode::new("effect.blur", PropertyMap::new());
        let node_id = node.id;
        project.add_node(Node::Graph(node));
        project.get_track_mut(root_id).unwrap().add_child(node_id);

        let conn = Connection::new(PinId::new(node_id, "out"), PinId::new(node_id, "in"));
        let result = validate_connection(&project, &conn);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("itself"));
    }

    #[test]
    fn test_topological_sort_linear() {
        let (mut project, root_id, _) = setup_project();

        let clip = crate::project::clip::TrackClip {
            id: Uuid::new_v4(),
            reference_id: None,
            kind: crate::project::clip::TrackClipKind::Image,
            in_frame: 0,
            out_frame: 100,
            source_begin_frame: 0,
            duration_frame: None,
            fps: 30.0,
            properties: PropertyMap::new(),
        };
        let clip_id = clip.id;

        let effect = GraphNode::new("effect.blur", PropertyMap::new());
        let effect_id = effect.id;

        project.add_node(Node::Clip(clip));
        project.add_node(Node::Graph(effect));
        project.get_track_mut(root_id).unwrap().add_child(clip_id);
        project.get_track_mut(root_id).unwrap().add_child(effect_id);

        // clip → effect
        project.add_connection(Connection::new(
            PinId::new(clip_id, "image_out"),
            PinId::new(effect_id, "image_in"),
        ));

        let sorted = topological_sort(&project, root_id).unwrap();
        assert_eq!(sorted.len(), 2);
        // clip should come before effect
        let clip_pos = sorted.iter().position(|&id| id == clip_id).unwrap();
        let effect_pos = sorted.iter().position(|&id| id == effect_id).unwrap();
        assert!(clip_pos < effect_pos);
    }

    #[test]
    fn test_cycle_detection() {
        let (mut project, root_id, _) = setup_project();

        let node_a = GraphNode::new("effect.blur", PropertyMap::new());
        let node_a_id = node_a.id;
        let node_b = GraphNode::new("effect.dilate", PropertyMap::new());
        let node_b_id = node_b.id;

        project.add_node(Node::Graph(node_a));
        project.add_node(Node::Graph(node_b));
        project.get_track_mut(root_id).unwrap().add_child(node_a_id);
        project.get_track_mut(root_id).unwrap().add_child(node_b_id);

        // A → B
        project.add_connection(Connection::new(
            PinId::new(node_a_id, "image_out"),
            PinId::new(node_b_id, "image_in"),
        ));

        // Try to add B → A (would create cycle)
        let cyclic_conn = Connection::new(
            PinId::new(node_b_id, "image_out"),
            PinId::new(node_a_id, "image_in"),
        );
        let result = validate_connection(&project, &cyclic_conn);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cycle"));
    }
}
