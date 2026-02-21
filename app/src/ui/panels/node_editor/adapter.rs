//! Adapter connecting library types to egui_node_editor traits.

use egui_node_editor::{
    ConnectionView, NodeDisplay, NodeEditorDataSource, NodeEditorMutator, NodeTypeInfo, PinInfo,
};
use library::model::project::node::Node;
use library::model::project::project::Project;
use library::plugin::PluginManager;
use uuid::Uuid;

/// Read-only data source backed by a Project + PluginManager.
pub(super) struct VideoEditorDataSource<'a> {
    pub(super) project: &'a Project,
    pub(super) plugin_manager: &'a PluginManager,
    pub(super) current_frame: u64,
}

impl NodeEditorDataSource for VideoEditorDataSource<'_> {
    fn get_container_children(&self, container_id: Uuid) -> Vec<Uuid> {
        // Check if this is a composition ID
        if let Some(comp) = self
            .project
            .compositions
            .iter()
            .find(|c| c.id == container_id)
        {
            return vec![comp.root_track_id];
        }
        match self.project.get_node(container_id) {
            Some(Node::Track(track)) => track.child_ids.clone(),
            _ => vec![],
        }
    }

    fn get_container_name(&self, id: Uuid) -> Option<String> {
        // Check compositions first
        if let Some(comp) = self.project.compositions.iter().find(|c| c.id == id) {
            return Some(comp.name.clone());
        }
        match self.project.get_node(id) {
            Some(Node::Track(track)) => Some(track.name.clone()),
            _ => None,
        }
    }

    fn find_parent_container(&self, node_id: Uuid) -> Option<Uuid> {
        // Check if node is a root track of a composition
        for comp in &self.project.compositions {
            if comp.root_track_id == node_id {
                return Some(comp.id);
            }
        }
        for (id, node) in self.project.nodes.iter() {
            if let Node::Track(track) = node {
                if track.child_ids.contains(&node_id) {
                    return Some(*id);
                }
            }
        }
        None
    }

    fn get_node_display(&self, id: Uuid) -> Option<NodeDisplay> {
        match self.project.get_node(id)? {
            Node::Graph(graph_node) => {
                let display_name = self
                    .plugin_manager
                    .get_node_type(&graph_node.type_id)
                    .map(|def| def.display_name.clone())
                    .unwrap_or_else(|| graph_node.type_id.clone());

                let pins = if let Some(def) = self.plugin_manager.get_node_type(&graph_node.type_id)
                {
                    def.inputs
                        .iter()
                        .map(|p| PinInfo::input(&p.name, &p.display_name))
                        .chain(
                            def.outputs
                                .iter()
                                .map(|p| PinInfo::output(&p.name, &p.display_name)),
                        )
                        .collect()
                } else {
                    vec![]
                };

                Some(NodeDisplay::Graph {
                    type_id: graph_node.type_id.clone(),
                    display_name,
                    pins,
                })
            }
            Node::Clip(clip) => {
                use library::model::project::clip::TrackClipKind;
                let kind_label = format!("{}", clip.kind);
                let mut pins = Vec::new();

                // Output pin depends on clip kind
                match clip.kind {
                    TrackClipKind::Text | TrackClipKind::Shape => {
                        pins.push(PinInfo::output("shape_out", "Shape"));
                    }
                    TrackClipKind::Audio => {}
                    _ => {
                        pins.push(PinInfo::output("image_out", "Image"));
                    }
                }

                // Input: property definitions for this clip kind
                let defs =
                    library::model::project::clip::TrackClip::get_definitions_for_kind(&clip.kind);
                for def in &defs {
                    // Skip file_path — not editable via node connections
                    if def.name() == "file_path" {
                        continue;
                    }
                    pins.push(PinInfo::input(def.name(), def.label()));
                }

                // Shape output pin (for text/shape ensemble chain)
                if clip.kind == TrackClipKind::Text || clip.kind == TrackClipKind::Shape {
                    pins.push(PinInfo::output("shape_out", "Shape"));
                }

                Some(NodeDisplay::Leaf { kind_label, pins })
            }
            Node::Track(track) => {
                let pins = vec![PinInfo::output("image_out", "Image")];
                Some(NodeDisplay::Container {
                    name: track.name.clone(),
                    child_ids: track.child_ids.clone(),
                    pins,
                })
            }
        }
    }

    fn get_connections(&self) -> Vec<ConnectionView> {
        self.project
            .connections
            .iter()
            .map(|conn| ConnectionView {
                id: conn.id,
                from_node: conn.from.node_id,
                from_pin: conn.from.pin_name.clone(),
                to_node: conn.to.node_id,
                to_pin: conn.to.pin_name.clone(),
            })
            .collect()
    }

    fn get_node_type_id(&self, id: Uuid) -> Option<String> {
        match self.project.get_node(id)? {
            Node::Graph(g) => Some(g.type_id.clone()),
            Node::Clip(c) => Some(format!("clip.{}", c.kind)),
            Node::Track(_) => Some("track".to_string()),
        }
    }

    fn is_node_active(&self, id: Uuid) -> bool {
        match self.project.get_node(id) {
            Some(Node::Clip(clip)) => {
                self.current_frame >= clip.in_frame && self.current_frame <= clip.out_frame
            }
            _ => true,
        }
    }
}

/// Read-only adapter for render phase (only get_available_node_types is used).
pub(super) struct ReadOnlyMutator<'a> {
    pub(super) project_service: &'a library::EditorService,
}

impl NodeEditorMutator for ReadOnlyMutator<'_> {
    fn add_node(&mut self, _: Uuid, _: &str) -> Result<Uuid, String> {
        Err("read-only".into())
    }
    fn remove_node(&mut self, _: Uuid) -> Result<(), String> {
        Err("read-only".into())
    }
    fn add_connection(&mut self, _: Uuid, _: &str, _: Uuid, _: &str) -> Result<(), String> {
        Err("read-only".into())
    }
    fn remove_connection(&mut self, _: Uuid) -> Result<(), String> {
        Err("read-only".into())
    }
    fn get_available_node_types(&self) -> Vec<NodeTypeInfo> {
        let pm = self.project_service.get_plugin_manager();
        pm.get_available_node_types()
            .into_iter()
            .map(|def| NodeTypeInfo {
                type_id: def.type_id,
                display_name: def.display_name,
                category: def.category.to_string(),
            })
            .collect()
    }
}

/// Mutation adapter backed by EditorService.
pub(super) struct VideoEditorMutator<'a> {
    pub(super) project_service: &'a mut library::EditorService,
}

impl NodeEditorMutator for VideoEditorMutator<'_> {
    fn add_node(&mut self, container_id: Uuid, type_id: &str) -> Result<Uuid, String> {
        // Resolve composition ID to root_track_id if needed
        let actual_container = self
            .project_service
            .get_composition(container_id)
            .map(|c| c.root_track_id)
            .unwrap_or(container_id);
        self.project_service
            .add_graph_node(actual_container, type_id)
            .map_err(|e| e.to_string())
    }

    fn remove_node(&mut self, node_id: Uuid) -> Result<(), String> {
        self.project_service
            .remove_graph_node(node_id)
            .map_err(|e| e.to_string())
    }

    fn add_connection(
        &mut self,
        from_node: Uuid,
        from_pin: &str,
        to_node: Uuid,
        to_pin: &str,
    ) -> Result<(), String> {
        let from = library::model::project::connection::PinId::new(from_node, from_pin);
        let to = library::model::project::connection::PinId::new(to_node, to_pin);
        self.project_service
            .add_graph_connection(from, to)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn remove_connection(&mut self, connection_id: Uuid) -> Result<(), String> {
        self.project_service
            .remove_graph_connection(connection_id)
            .map_err(|e| e.to_string())
    }

    fn get_available_node_types(&self) -> Vec<NodeTypeInfo> {
        let pm = self.project_service.get_plugin_manager();
        pm.get_available_node_types()
            .into_iter()
            .map(|def| NodeTypeInfo {
                type_id: def.type_id,
                display_name: def.display_name,
                category: def.category.to_string(),
            })
            .collect()
    }
}
