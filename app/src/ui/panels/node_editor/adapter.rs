//! Adapter connecting library types to egui_node_editor traits.

use egui_node_editor::{
    ConnectionView, NodeDisplay, NodeEditorDataSource, NodeEditorMutator, NodeTypeInfo, PinInfo,
};
use library::model::project::project::Project;
use library::model::project::Node;
use library::plugin::PluginManager;
use uuid::Uuid;

/// Read-only data source backed by a Project + PluginManager.
pub struct VideoEditorDataSource<'a> {
    pub project: &'a Project,
    pub plugin_manager: &'a PluginManager,
    pub current_frame: u64,
}

impl NodeEditorDataSource for VideoEditorDataSource<'_> {
    fn get_container_children(&self, container_id: Uuid) -> Vec<Uuid> {
        match self.project.get_node(container_id) {
            Some(Node::Track(track)) => track.child_ids.clone(),
            _ => vec![],
        }
    }

    fn get_container_name(&self, id: Uuid) -> Option<String> {
        match self.project.get_node(id) {
            Some(Node::Track(track)) => Some(track.name.clone()),
            _ => None,
        }
    }

    fn find_parent_container(&self, node_id: Uuid) -> Option<Uuid> {
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
                let kind_label = format!("{}", clip.kind);
                let mut pins = Vec::new();

                // Output: image (except audio)
                if !matches!(clip.kind, library::model::project::TrackClipKind::Audio) {
                    pins.push(PinInfo::output("image_out", "Image"));
                }

                // Input: property definitions for this clip kind
                let defs = library::model::project::TrackClip::get_definitions_for_kind(&clip.kind);
                for def in &defs {
                    // Skip file_path — not editable via node connections
                    if def.name() == "file_path" {
                        continue;
                    }
                    pins.push(PinInfo::input(def.name(), def.label()));
                }

                // Special connection pins
                pins.push(PinInfo::input("style_in", "Style"));
                pins.push(PinInfo::input("effector_in", "Effector"));
                pins.push(PinInfo::input("decorator_in", "Decorator"));

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
pub struct ReadOnlyMutator<'a> {
    pub project_service: &'a library::EditorService,
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
pub struct VideoEditorMutator<'a> {
    pub project_service: &'a mut library::EditorService,
}

impl NodeEditorMutator for VideoEditorMutator<'_> {
    fn add_node(&mut self, container_id: Uuid, type_id: &str) -> Result<Uuid, String> {
        self.project_service
            .add_graph_node(container_id, type_id)
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
        let from = library::model::project::PinId::new(from_node, from_pin);
        let to = library::model::project::PinId::new(to_node, to_pin);
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
