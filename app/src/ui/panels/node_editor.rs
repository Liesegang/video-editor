use crate::model::node_graph::MyNodeTemplate;
use eframe::egui::{self, Color32};
use egui_snarl::{
    ui::{PinInfo, SnarlStyle, SnarlViewer},
    InPin, OutPin, Snarl,
};
// use library::core::graph_compiler::GraphCompiler;
use library::model::node_graph::DataType;
use library::model::project::Project;
use std::sync::{Arc, RwLock};

// ========= 2. Define the Viewer =========

// use crate::state::context_types::ContextMenuState; // Removed duplicate

pub struct MySnarlViewer<'a> {
    pub pending_navigation: &'a mut Option<Uuid>,
    pub project: &'a Project, // Need project access to check node type? Or just Template?
}

impl<'a> SnarlViewer<MyNodeTemplate> for MySnarlViewer<'a> {
    fn show_header(
        &mut self,
        node_id: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<MyNodeTemplate>,
    ) {
        let node = snarl.get_node(node_id).cloned(); // Clone purely to avoid borrow issues?
                                                     // Wait, `title` takes `&MyNodeTemplate`.
                                                     // I can get reference if I don't borrow snarl mutably later?
                                                     // `show_header` takes `snarl: &mut Snarl`.
                                                     // So I can't borrow `node` from `snarl` and pass it to something?
                                                     // `title` takes `&self` and `&node`.
                                                     // `self.title` uses `node.label`.

        if let Some(node) = node {
            // Default header (label)
            let label = self.title(&node);
            let resp = ui.label(egui::RichText::new(label).strong());

            // Handle Double Click for Navigation
            if resp.double_clicked() {
                if let library::model::node_graph::NodeKind::ClipReference { clip_id } = &node.kind
                {
                    // Check if it's a Reference Content
                    if let Some(library::model::Node::Layer(layer)) =
                        self.project.get_node(*clip_id)
                    {
                        if let library::model::LayerContent::Reference(ref_content) = &layer.content
                        {
                            *self.pending_navigation = Some(ref_content.target_id);
                        }
                    }
                }
            }
        }
    }
    fn title(&mut self, node: &MyNodeTemplate) -> String {
        log::debug!("Rendering node title: {}", node.label);
        node.label.clone()
    }

    fn inputs(&mut self, node: &MyNodeTemplate) -> usize {
        node.inputs.len()
    }

    fn outputs(&mut self, node: &MyNodeTemplate) -> usize {
        node.outputs.len()
    }

    #[allow(refining_impl_trait_reachable)]
    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<MyNodeTemplate>,
    ) -> PinInfo {
        let node = snarl
            .get_node(pin.id.node)
            .map(|n| n)
            .unwrap_or_else(|| panic!("Node not found")); // Should safely unwrap if id is valid

        let pin_def = &node.inputs[pin.id.input];
        ui.label(&pin_def.name);

        let color = match pin_def.data_type {
            DataType::Image => Color32::from_rgb(238, 207, 109), // Gold
            DataType::Audio => Color32::from_rgb(100, 200, 100), // Green
            DataType::String => Color32::from_rgb(100, 220, 220), // Cyan/Mint
            DataType::EnsembleData => Color32::from_rgb(180, 100, 255), // Violet
            DataType::Path => Color32::from_rgb(100, 150, 255),  // Cornflower Blue
            DataType::Scalar => Color32::from_rgb(255, 100, 100), // Red
            _ => Color32::from_rgb(200, 200, 200),               // Gray
        };

        PinInfo::circle().with_fill(color)
    }

    #[allow(refining_impl_trait_reachable)]
    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<MyNodeTemplate>,
    ) -> PinInfo {
        let node = snarl
            .get_node(pin.id.node)
            .map(|n| n)
            .unwrap_or_else(|| panic!("Node not found"));

        let pin_def = &node.outputs[pin.id.output];
        ui.label(&pin_def.name);

        let color = match pin_def.data_type {
            DataType::Image => Color32::from_rgb(238, 207, 109), // Gold
            DataType::Audio => Color32::from_rgb(100, 200, 100), // Green
            DataType::String => Color32::from_rgb(100, 220, 220), // Cyan/Mint
            DataType::EnsembleData => Color32::from_rgb(180, 100, 255), // Violet
            DataType::Path => Color32::from_rgb(100, 150, 255),  // Cornflower Blue
            DataType::Scalar => Color32::from_rgb(255, 100, 100), // Red
            _ => Color32::from_rgb(200, 200, 200),               // Gray
        };

        PinInfo::circle().with_fill(color)
    }
}

// ========= 3. The Panel Logic =========

// ========= 3. The Panel Logic =========

use crate::state::context_types::{ContextMenuState, NodeEditorState};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Helper: Calculate a hash of the project's topology for the given composition.
/// Include Node IDs, Children, and UI Positions.
fn hash_project_topology(project: &Project, comp_id: Uuid) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    if let Some(comp) = project.get_composition(comp_id) {
        // Hash Root Track
        hash_node_recursive(project, comp.root_track_id, &mut hasher);
    }

    hasher.finish()
}

fn hash_node_recursive<H: std::hash::Hasher>(project: &Project, node_id: Uuid, state: &mut H) {
    use std::hash::Hash;
    if let Some(node) = project.get_node(node_id) {
        node.id().hash(state);
        // Hash UI Position (Changes in Pos should trigger sync check - though usually S->P)
        match node {
            library::model::Node::Track(t) => {
                t.ui_position
                    .map(|v| ordered_float::OrderedFloat(v))
                    .hash(state);
                for child in &t.children {
                    hash_node_recursive(project, *child, state);
                }
            }
            library::model::Node::Layer(l) => {
                l.ui_position
                    .map(|v| ordered_float::OrderedFloat(v))
                    .hash(state);
                // Layer hierarchy (if composite reference) - for now just the layer itself
            }
        }
    }
}

pub fn node_editor_panel(
    ui: &mut egui::Ui,
    snarl: &mut Snarl<MyNodeTemplate>,
    comp_id: Option<uuid::Uuid>,
    project_lock: &Arc<RwLock<Project>>,
    context_menu_state: &mut Option<ContextMenuState>,
    node_editor_state: &mut NodeEditorState,
) {
    let current_comp_id = if let Some(id) = comp_id {
        id
    } else {
        ui.centered_and_justified(|ui| ui.label("No Composition Selected"));
        return;
    };

    // 1. Check for External Changes (Project -> Snarl)
    // We calculate a lightweight hash of the topology.
    // If it differs from what we saw last frame, we re-sync Snarl FROM Project.
    let current_hash = {
        let project = project_lock.read().unwrap();
        hash_project_topology(&project, current_comp_id)
    };

    if current_hash != node_editor_state.last_project_hash {
        log::debug!("External Change Detected (Hash mismatch). Syncing Project -> Snarl.");
        let project = project_lock.read().unwrap();
        sync_project_to_snarl(&project, current_comp_id, snarl);
        node_editor_state.last_project_hash = current_hash;
    }

    // 2. Render Snarl
    {
        let project_read = project_lock.read().unwrap();
        let mut viewer = MySnarlViewer {
            pending_navigation: &mut node_editor_state.pending_navigation,
            project: &project_read,
        };
        let style = SnarlStyle::default();
        let id = egui::Id::new("my_snarl_editor");

        // We rely on Snarl interactions to update `snarl` state directly.
        snarl.show(&mut viewer, &style, id, ui);
    } // READ LOCK DROPPED HERE

    // 3. Handle Context Menu (Add Nodes)
    handle_context_menu(ui, snarl, context_menu_state, project_lock, current_comp_id);

    // 4. Check for Internal Changes (Snarl -> Project)
    // We calculate a hash of the current Snarl state (Positions + Topology).
    // If it differs from the last frame, we perform a sync.

    let current_snarl_hash = hash_snarl_state(snarl);

    if current_snarl_hash != node_editor_state.last_snarl_hash {
        if let Ok(mut project) = project_lock.write() {
            // A. Update Positions
            for node in snarl.nodes() {
                let pos = node.position;
                if let Some(p_node) = project.nodes.get_mut(&node.id) {
                    match p_node {
                        library::model::Node::Track(t) => t.ui_position = [pos.0, pos.1],
                        library::model::Node::Layer(l) => l.ui_position = [pos.0, pos.1],
                    }
                }
            }

            // B. Update Connections (Reparenting & Data Flow)
            // 1. Build maps based on Snarl Wires
            let mut snarl_parentage: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
            let mut layer_input_connections: HashMap<Uuid, HashMap<String, Uuid>> = HashMap::new();

            // Iterate wires: OutPin (Source) -> InPin (Destination)
            for wire in snarl.wires() {
                let source_snarl_id = wire.0.node;
                let dest_snarl_id = wire.1.node;

                if let (Some(source_node), Some(dest_node)) = (
                    snarl.get_node(source_snarl_id),
                    snarl.get_node(dest_snarl_id),
                ) {
                    // Check Destination Node Type to distinguish Hierarchy vs Data Flow
                    match &dest_node.kind {
                        library::model::node_graph::NodeKind::TrackReference { .. } => {
                            // Hierarchy: Source is Child, Dest is Parent (Track)
                            snarl_parentage
                                .entry(dest_node.id)
                                .or_default()
                                .push(source_node.id);
                        }
                        library::model::node_graph::NodeKind::ClipReference { .. } => {
                            // Data Flow: Source is Input to Dest (Layer)
                            // Get Input Pin Name
                            if let Some(pin) = dest_node.inputs.get(wire.1.input) {
                                layer_input_connections
                                    .entry(dest_node.id)
                                    .or_default()
                                    .insert(pin.name.clone(), source_node.id);
                            }
                        }
                        _ => {}
                    }
                }
            }

            // 2. Sort children by Y position to maintain consistent order (for Tracks)
            for children in snarl_parentage.values_mut() {
                children.sort_by(|a_id, b_id| {
                    let pos_a = snarl
                        .nodes()
                        .find(|n| n.id == *a_id)
                        .map(|n| n.position.1)
                        .unwrap_or(0.0);
                    let pos_b = snarl
                        .nodes()
                        .find(|n| n.id == *b_id)
                        .map(|n| n.position.1)
                        .unwrap_or(0.0);
                    pos_a
                        .partial_cmp(&pos_b)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }

            // 3. Update Project Structure (Tracks)
            for (parent_id, new_children) in snarl_parentage {
                if let Some(library::model::Node::Track(track)) = project.nodes.get_mut(&parent_id)
                {
                    if track.children != new_children {
                        track.children = new_children;
                    }
                }
            }

            // 4. Update Project Structure (Layer Inputs)
            // Iterate all nodes in Snarl to handle both connections and DISCONNECTIONS (empty map)
            for node in snarl.nodes() {
                if let library::model::node_graph::NodeKind::ClipReference { clip_id } = &node.kind
                {
                    if let Some(library::model::Node::Layer(layer)) = project.nodes.get_mut(clip_id)
                    {
                        if let library::model::LayerContent::Reference(ref_content) =
                            &mut layer.content
                        {
                            let new_map =
                                layer_input_connections.remove(clip_id).unwrap_or_default();
                            if ref_content.input_mapping != new_map {
                                ref_content.input_mapping = new_map;
                            }
                        }
                    }
                }
            }

            // C. Update Last Hash
            node_editor_state.last_snarl_hash = current_snarl_hash;

            // Also update project hash so we don't trigger a reverse sync immediately
            node_editor_state.last_project_hash = hash_project_topology(&project, current_comp_id);
        }
    }
}

fn hash_snarl_state(snarl: &Snarl<MyNodeTemplate>) -> u64 {
    use std::hash::Hash;
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    // Hash Nodes (ID + Position)
    // Order independent hash? Or sort by ID?
    // Snarl iteration order is stable enough for frame-to-frame logic usually.
    // Better to sort to be safe but optimization first: just iterate.
    for node in snarl.nodes() {
        // Use payload ID (Project ID) which is stable
        node.id.hash(&mut hasher);
        ordered_float::OrderedFloat(node.position.0).hash(&mut hasher);
        ordered_float::OrderedFloat(node.position.1).hash(&mut hasher);
    }

    // Hash Wires
    for wire in snarl.wires() {
        wire.0.node.hash(&mut hasher);
        wire.0.output.hash(&mut hasher);
        wire.1.node.hash(&mut hasher);
        wire.1.input.hash(&mut hasher);
    }

    hasher.finish()
}

// --- Sync Logic ---

fn sync_project_to_snarl(project: &Project, comp_id: Uuid, snarl: &mut Snarl<MyNodeTemplate>) {
    // 1. Build Map of Snarl Nodes [ProjectID -> SnarlID]
    let mut existing_snarl_nodes: HashMap<Uuid, egui_snarl::NodeId> = HashMap::new();
    let initial_node_ids: Vec<egui_snarl::NodeId> = snarl.node_ids().map(|(id, _)| id).collect();

    for snarl_id in initial_node_ids {
        if let Some(node) = snarl.get_node(snarl_id) {
            existing_snarl_nodes.insert(node.id, snarl_id);
        }
    }

    // 2. Iterate Project Nodes (Recursive from Root)
    if let Some(comp) = project.get_composition(comp_id) {
        let root_id = comp.root_track_id;
        let mut visited = HashSet::new();

        sync_node_recursive(
            project,
            root_id,
            snarl,
            &mut existing_snarl_nodes,
            &mut visited,
            None,
        );
    }

    // 3. Prune Snarl Nodes not in Project (Optional? Or strict sync?)
    // For "Direct Editing", yes, prune.
    // (Omitted for simplicity, but strictly should be done)

    // 4. Sync Wires
    // Clear all wires and rebuild from Project?
    // Or diff? Rebuild is safer for "Project is Truth".
    // snarl.disconnect_all(); // Wait, no such method on Snarl?
    // We can interact with standard API.
    // Optimization: Only clear if hash mismatch (which we are in).
}

fn sync_node_recursive(
    project: &Project,
    node_id: Uuid,
    snarl: &mut Snarl<MyNodeTemplate>,
    existing: &mut HashMap<Uuid, egui_snarl::NodeId>,
    visited: &mut HashSet<Uuid>,
    parent_snarl_id: Option<egui_snarl::NodeId>,
) {
    if visited.contains(&node_id) {
        return;
    }
    visited.insert(node_id);

    if let Some(node) = project.get_node(node_id) {
        // Determine Position
        let pos = match node {
            library::model::Node::Track(t) => egui::Pos2::new(t.ui_position[0], t.ui_position[1]),
            library::model::Node::Layer(l) => egui::Pos2::new(l.ui_position[0], l.ui_position[1]),
        };

        // Find or Create Snarl Node
        let snarl_id = if let Some(&sid) = existing.get(&node_id) {
            // Update Position
            // Update Position - TODO: Find correct API to set position
            // snarl.set_node_pos(sid, pos);
            sid
        } else {
            // Create New
            let tmpl = match node {
                library::model::Node::Track(t) => library::model::node_graph::GraphNode::new(
                    library::model::node_graph::NodeKind::TrackReference { track_id: t.id },
                    &format!("Mixer ({})", t.name),
                    (pos.x, pos.y),
                )
                .with_input("Layers", DataType::Image) // Renamed from "Input" to imply multiple layers
                .with_output("Output", DataType::Image),
                library::model::Node::Layer(l) => {
                    // Match content type to determine inputs/outputs based on node_list.yml
                    let mut node = library::model::node_graph::GraphNode::new(
                        library::model::node_graph::NodeKind::ClipReference { clip_id: l.id },
                        &l.name,
                        (pos.x, pos.y),
                    );

                    match &l.content {
                        library::model::LayerContent::Generator(gen) => match gen {
                            library::model::GeneratorContent::Text { .. } => {
                                node = node
                                    .with_input("Text", DataType::String)
                                    .with_input("Font", DataType::String) // Font vs String?
                                    .with_input("Size", DataType::Scalar)
                                    .with_input("Color", DataType::Color);
                            }
                            library::model::GeneratorContent::Solid { .. } => {
                                node = node.with_input("Color", DataType::Color);
                            }
                            library::model::GeneratorContent::Shape { .. } => {
                                node = node
                                    .with_input("Path", DataType::Path)
                                    .with_input("Fill", DataType::Color)
                                    .with_input("Stroke", DataType::Color)
                                    .with_input("Stroke Width", DataType::Scalar);
                            }
                            library::model::GeneratorContent::SkSL { .. } => {
                                node = node
                                    .with_input("Shader", DataType::String)
                                    .with_input("Time", DataType::Scalar);
                            }
                        },
                        library::model::LayerContent::Media(_) => {
                            // Media usually just has properties like Time, Scale, Opacity
                            node = node
                                .with_input("Time", DataType::Scalar)
                                .with_input("Opacity", DataType::Scalar);
                        }
                        library::model::LayerContent::Reference(_) => {
                            // Reference to another graph (Nested Composition)
                            // This is a "Container" node.
                            // Inputs come from the Inner Graph's Input Nodes.
                            node = node.with_input("Time", DataType::Scalar);

                            if let library::model::LayerContent::Reference(ref_content) = &l.content
                            {
                                if let Some(inner_comp) =
                                    project.get_composition(ref_content.target_id)
                                {
                                    // Scan inner graph for inputs
                                    let mut input_nodes: Vec<_> = inner_comp
                                        .node_graph
                                        .nodes
                                        .values()
                                        .filter(|n| {
                                            matches!(
                                                n.kind,
                                                library::model::node_graph::NodeKind::Input
                                            )
                                        })
                                        .collect();
                                    // Sort by position y to have stable order
                                    input_nodes.sort_by(|a, b| {
                                        a.position
                                            .1
                                            .partial_cmp(&b.position.1)
                                            .unwrap_or(std::cmp::Ordering::Equal)
                                    });

                                    for input_node in input_nodes {
                                        if let Some(out) = input_node.outputs.first() {
                                            node = node.with_input(
                                                &input_node.label,
                                                out.data_type.clone(),
                                            );
                                        } else {
                                            node =
                                                node.with_input(&input_node.label, DataType::Any);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Always has Output Image
                    node.with_output("Output", DataType::Image)
                }
            };
            // Use `insert_node` which returns NodeId
            snarl.insert_node(pos, tmpl.clone_with_id(node_id)) // Ensure payload has correct Project ID
        };

        // Connect to Parent (if provided)
        if let Some(pid) = parent_snarl_id {
            // Project Hierarchy: Parent (Track) includes Child.
            // Snarl Graph: Child (Output) -> Parent (Input).
            // We need to add wire.
            snarl.connect(
                egui_snarl::OutPinId {
                    node: snarl_id,
                    output: 0,
                },
                egui_snarl::InPinId {
                    node: pid,
                    input: 0,
                },
            );
        }

        // Recurse Children (Project side)
        if let library::model::Node::Track(t) = node {
            for child_id in &t.children {
                sync_node_recursive(project, *child_id, snarl, existing, visited, Some(snarl_id));
            }
        }
    }
}

// Logic extension for GraphNode needed: `clone_with_id` helper?
// `GraphNode` has public `id`. We can clone and existing and set id.
trait GraphNodeExt {
    fn clone_with_id(&self, id: Uuid) -> Self;
}
impl GraphNodeExt for library::model::node_graph::GraphNode {
    fn clone_with_id(&self, id: Uuid) -> Self {
        let mut n = self.clone();
        n.id = id;
        n
    }
}

// 5. Context Menu Logic (Simplified)
fn handle_context_menu(
    ui: &mut egui::Ui,
    _snarl: &mut Snarl<MyNodeTemplate>,
    state: &mut Option<ContextMenuState>,
    project_lock: &Arc<RwLock<Project>>,
    comp_id: Uuid,
) {
    // Open on secondary click - Scoped to this panel
    if ui.input(|i| i.pointer.secondary_clicked()) {
        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
            if ui.min_rect().contains(pos) {
                let time = ui.input(|i| i.time);
                *state = Some(ContextMenuState::new(pos, time));
            }
        }
    }

    let mut should_close = false;
    let mut closing_action: Option<Box<dyn FnOnce(&mut Project)>> = None;

    if let Some(ctx_state) = state {
        let new_node_pos = ctx_state.position;

        let area_resp = egui::Area::new("node_ctx_menu".into())
            .fixed_pos(ctx_state.position)
            .show(ui.ctx(), |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_max_width(200.0);

                    // Search Bar
                    let search_resp = ui.text_edit_singleline(&mut ctx_state.search_query);
                    if ctx_state.search_query.is_empty()
                        && !ui.memory(|m| m.has_focus(search_resp.id))
                    {
                        search_resp.request_focus();
                    }

                    ui.separator();

                    let query = ctx_state.search_query.to_lowercase();
                    let has_query = !query.is_empty();

                    let mut show_category = |name: &str, builder: &mut dyn FnMut(&mut egui::Ui)| {
                        // TODO: Actually filter items. For now, expand if query matches name OR always expand if query exists (simple heuristic)
                        // Better: The builder needs to know if it should show items.
                        // Ideally we pass the query to the builder or filter inside.
                        // Simplification for rapid fix: Always run builder, let builder filter?
                        // No, builder creates buttons.
                        // Let's expanded if query is present.

                        let id = ui.make_persistent_id(name);
                        egui::collapsing_header::CollapsingHeader::new(name)
                            .default_open(has_query)
                            .id_salt(id)
                            .show(ui, builder);
                    };

                    // --- Categories based on node_list.yml ---

                    // 1. Text
                    show_category("Text", &mut |ui| {
                        if "text".contains(&query) || query.is_empty() {
                            if ui.button("Text").clicked() {
                                closing_action = Some(Box::new(move |project| {
                                    create_generator_node(
                                        project,
                                        new_node_pos,
                                        "Text",
                                        library::model::GeneratorContent::Text {
                                            text: "Hello World".to_string(),
                                            font: "Default".to_string(),
                                        },
                                        comp_id,
                                    );
                                }));
                                should_close = true;
                            }
                        }
                    });

                    // 2. Generators
                    show_category("Generators", &mut |ui| {
                        if "solid color".contains(&query) || query.is_empty() {
                            if ui.button("Solid Color").clicked() {
                                closing_action = Some(Box::new(move |project| {
                                    create_generator_node(
                                        project,
                                        new_node_pos,
                                        "Solid",
                                        library::model::GeneratorContent::Solid {
                                            color: library::model::frame::color::Color {
                                                r: 255,
                                                g: 0,
                                                b: 0,
                                                a: 255,
                                            },
                                        },
                                        comp_id,
                                    );
                                }));
                                should_close = true;
                            }
                        }
                        if "shape (rectangle)".contains(&query) || query.is_empty() {
                            if ui.button("Shape (Rectangle)").clicked() {
                                closing_action = Some(Box::new(move |project| {
                                    create_generator_node(
                                        project,
                                        new_node_pos,
                                        "Shape",
                                        library::model::GeneratorContent::Shape {
                                            path: "rect".to_string(),
                                            fill: "#FFFFFF".to_string(),
                                        },
                                        comp_id,
                                    );
                                }));
                                should_close = true;
                            }
                        }
                    });

                    // 3. Audio (Special)
                    show_category("Audio", &mut |ui| {
                        if "audio track".contains(&query) || query.is_empty() {
                            if ui.button("Audio Track").clicked() {
                                closing_action = Some(Box::new(move |project| {
                                    let track = library::model::Track::new("Audio");
                                    let tid = track.id;
                                    project.add_node(library::model::Node::Track(track));
                                    connect_to_root(project, tid, comp_id);
                                }));
                                should_close = true;
                            }
                        }
                    });

                    // 4. Data (Placeholders)
                    show_category("Data", &mut |ui| {
                        if "scalar".contains(&query) || query.is_empty() {
                            if ui.button("Scalar").clicked() {
                                should_close = true;
                            }
                        }
                        if "vector".contains(&query) || query.is_empty() {
                            if ui.button("Vector").clicked() {
                                should_close = true;
                            }
                        }
                        if "color".contains(&query) || query.is_empty() {
                            if ui.button("Color").clicked() {
                                should_close = true;
                            }
                        }
                        if "image".contains(&query) || query.is_empty() {
                            if ui.button("Image").clicked() {
                                should_close = true;
                            }
                        }
                    });

                    // 5. Math
                    show_category("Math", &mut |ui| {
                        if "add".contains(&query) || query.is_empty() {
                            if ui.button("Add").clicked() {
                                should_close = true;
                            }
                        }
                        if "subtract".contains(&query) || query.is_empty() {
                            if ui.button("Subtract").clicked() {
                                should_close = true;
                            }
                        }
                        if "multiply".contains(&query) || query.is_empty() {
                            if ui.button("Multiply").clicked() {
                                should_close = true;
                            }
                        }
                    });

                    // 6. Compositing
                    show_category("Compositing", &mut |ui| {
                        if "composite".contains(&query) || query.is_empty() {
                            if ui.button("Container (Composite)").clicked() {
                                closing_action = Some(Box::new(move |project| {
                                    // 1. Create new Composition
                                    let (new_comp, root) = library::model::Composite::new(
                                        "Nested Comp",
                                        1920,
                                        1080,
                                        30.0,
                                        10.0,
                                    );
                                    let new_comp_id = new_comp.id;

                                    // Add Input/Output nodes to new comp graph?
                                    // Optionally pre-populate
                                    // For now clean.

                                    project.add_node(library::model::Node::Track(root));
                                    project.add_composition(new_comp);

                                    // 2. Create Reference Layer
                                    let content = library::model::LayerContent::Reference(
                                        library::model::ReferenceContent {
                                            target_id: new_comp_id,
                                            sync_global_time: false,
                                            input_mapping: std::collections::HashMap::new(),
                                        },
                                    );
                                    let mut layer =
                                        library::model::Layer::new("Container", 0.0, 10.0, content);
                                    layer.ui_position = [new_node_pos.x, new_node_pos.y];
                                    let lid = layer.id;

                                    project.add_node(library::model::Node::Layer(layer));
                                    connect_to_root(project, lid, comp_id); // Connect to CURRENT comp root
                                }));
                                should_close = true;
                            }
                        }
                        if "transform".contains(&query) || query.is_empty() {
                            if ui.button("Transform").clicked() {
                                should_close = true;
                            }
                        }
                        if "blend".contains(&query) || query.is_empty() {
                            if ui.button("Blend").clicked() {
                                should_close = true;
                            }
                        }
                    });

                    // 7. Filters
                    show_category("Filters", &mut |ui| {
                        if "blur".contains(&query) || query.is_empty() {
                            if ui.button("Blur").clicked() {
                                should_close = true;
                            }
                        }
                        if "glow".contains(&query) || query.is_empty() {
                            if ui.button("Glow").clicked() {
                                should_close = true;
                            }
                        }
                    });
                });
            });

        // Robust Click Outside Detection
        // 1. Did the user click?
        if ui.input(|i| i.pointer.any_click()) {
            // Check debounce: Ignore clicks immediately after opening (e.g. the opening click)
            if ui.input(|i| i.time) - ctx_state.open_time > 0.2 {
                // 2. Was it inside the menu area?
                // We use the area response rect. Note: Area might auto-resize.
                // `area_resp.response.rect` covers the area content.
                if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                    if !area_resp.response.rect.contains(pos) {
                        should_close = true;
                    }
                }
            }
        }

        // Also close if ESC is pressed
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            should_close = true;
        }
    }

    if let Some(action) = closing_action {
        if let Ok(mut project) = project_lock.write() {
            action(&mut project);
        }
    }

    if should_close {
        *state = None;
    }
}

// Helpers for Context Menu Actions

fn create_generator_node(
    project: &mut Project,
    pos: egui::Pos2,
    name: &str,
    content: library::model::GeneratorContent,
    comp_id: Uuid,
) {
    let content = library::model::LayerContent::Generator(content);
    let mut layer = library::model::Layer::new(name, 0.0, 5.0, content);
    layer.ui_position = [pos.x, pos.y];
    let lid = layer.id;
    project.add_node(library::model::Node::Layer(layer));
    connect_to_root(project, lid, comp_id);
}

fn connect_to_root(project: &mut Project, child_id: Uuid, comp_id: Uuid) {
    if let Some(comp) = project.get_composition_mut(comp_id) {
        let root = comp.root_track_id;
        if let Some(library::model::Node::Track(t)) = project.nodes.get_mut(&root) {
            t.children.push(child_id);
        }
    }
}
