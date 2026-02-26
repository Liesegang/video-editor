//! Adapter connecting library types to egui_node_editor traits.

use egui_node_editor::{
    ConnectionView, ContainerKind, NodeDisplay, NodeEditorDataSource, NodeEditorMutator,
    NodeTypeInfo, PinDataType, PinEditValue, PinInfo, PinPropertyInfo,
};
use library::plugin::PluginManager;
use library::project::connection::PinDataType as LibPinDataType;
use library::project::node::Node;
use library::project::project::Project;
use uuid::Uuid;

/// Convert library PinDataType to editor PinDataType.
fn convert_pin_data_type(lib_type: &LibPinDataType) -> PinDataType {
    match lib_type {
        LibPinDataType::Image => PinDataType::Image,
        LibPinDataType::Scalar => PinDataType::Scalar,
        LibPinDataType::Integer => PinDataType::Integer,
        LibPinDataType::Boolean => PinDataType::Boolean,
        LibPinDataType::Vec2 => PinDataType::Vec2,
        LibPinDataType::Vec3 => PinDataType::Vec3,
        LibPinDataType::Color => PinDataType::Color,
        LibPinDataType::String => PinDataType::String,
        LibPinDataType::Path => PinDataType::Path,
        LibPinDataType::Enum => PinDataType::Enum,
        LibPinDataType::Style => PinDataType::Style,
        LibPinDataType::Shape => PinDataType::Shape,
        LibPinDataType::List => PinDataType::List,
        LibPinDataType::Audio => PinDataType::Audio,
        LibPinDataType::Any => PinDataType::Any,
        // Map remaining library types to closest match
        _ => PinDataType::Any,
    }
}

/// Read-only data source backed by a Project + PluginManager.
pub(super) struct VideoEditorDataSource<'a> {
    pub(super) project: &'a Project,
    pub(super) plugin_manager: &'a PluginManager,
    pub(super) current_frame: u64,
}

impl NodeEditorDataSource for VideoEditorDataSource<'_> {
    fn get_container_children(&self, container_id: Uuid) -> Vec<Uuid> {
        self.project
            .get_container_child_ids(container_id)
            .cloned()
            .unwrap_or_default()
    }

    fn get_container_name(&self, id: Uuid) -> Option<String> {
        // Check compositions first
        if let Some(comp) = self.project.all_compositions().find(|c| c.id == id) {
            return Some(comp.name.clone());
        }
        match self.project.get_node(id) {
            Some(Node::Track(track)) => Some(track.name.clone()),
            Some(Node::Layer(layer)) => Some(layer.name.clone()),
            _ => None,
        }
    }

    fn find_parent_container(&self, node_id: Uuid) -> Option<Uuid> {
        self.project.find_parent_container(node_id)
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
                        .map(|p| {
                            PinInfo::input(
                                &p.name,
                                &p.display_name,
                                convert_pin_data_type(&p.data_type),
                            )
                        })
                        .chain(def.outputs.iter().map(|p| {
                            PinInfo::output(
                                &p.name,
                                &p.display_name,
                                convert_pin_data_type(&p.data_type),
                            )
                        }))
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
            Node::Source(clip) => {
                use library::project::source::SourceKind;
                let kind_label = format!("{}", clip.kind);
                let mut pins = Vec::new();

                // Output pin depends on clip kind
                match clip.kind {
                    SourceKind::Text | SourceKind::Shape => {
                        pins.push(PinInfo::output("shape_out", "Shape", PinDataType::Shape));
                    }
                    SourceKind::Audio => {}
                    _ => {
                        pins.push(PinInfo::output("image_out", "Image", PinDataType::Image));
                    }
                }

                // Input: property definitions for this clip kind
                let defs =
                    library::project::source::SourceData::get_definitions_for_kind(&clip.kind);
                for def in &defs {
                    // Skip file_path — not editable via node connections
                    if def.name() == "file_path" {
                        continue;
                    }
                    pins.push(PinInfo::input(
                        def.name(),
                        def.label(),
                        convert_pin_data_type(&def.ui_type().pin_data_type()),
                    ));
                }

                Some(NodeDisplay::Leaf { kind_label, pins })
            }
            Node::Track(track) => {
                let pins = vec![
                    PinInfo::input("image_in", "Image", PinDataType::Image),
                    PinInfo::output("image_out", "Image", PinDataType::Image),
                    PinInfo::input("audio_in", "Audio", PinDataType::Audio),
                    PinInfo::output("audio_out", "Audio", PinDataType::Audio),
                ];
                Some(NodeDisplay::Container {
                    kind: ContainerKind::Track,
                    name: track.name.clone(),
                    child_ids: track.child_ids.clone(),
                    pins,
                })
            }
            Node::Layer(layer) => {
                let pins = vec![
                    PinInfo::input("image_in", "Image", PinDataType::Image),
                    PinInfo::output("image_out", "Image", PinDataType::Image),
                    PinInfo::input("audio_in", "Audio", PinDataType::Audio),
                    PinInfo::output("audio_out", "Audio", PinDataType::Audio),
                ];
                Some(NodeDisplay::Container {
                    kind: ContainerKind::Layer,
                    name: layer.name.clone(),
                    child_ids: layer.child_ids.clone(),
                    pins,
                })
            }
            _ => None,
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
            Node::Source(c) => Some(format!("source.{}", c.kind)),
            Node::Track(_) => Some("track".to_string()),
            Node::Layer(_) => Some("layer".to_string()),
            _ => None,
        }
    }

    fn is_node_active(&self, id: Uuid) -> bool {
        match self.project.get_node(id) {
            Some(Node::Layer(layer)) => {
                // Layer timing is derived from its child clips
                layer.child_ids.iter().any(|child_id| {
                    if let Some(Node::Source(clip)) = self.project.get_node(*child_id) {
                        self.current_frame >= clip.in_frame && self.current_frame <= clip.out_frame
                    } else {
                        false
                    }
                })
            }
            // Clips are always active — the layer container handles grayout
            _ => true,
        }
    }

    fn is_pin_connected(&self, node_id: Uuid, pin_name: &str) -> bool {
        self.project
            .connections
            .iter()
            .any(|c| c.to.node_id == node_id && c.to.pin_name == pin_name)
    }

    fn get_pin_value_display(&self, node_id: Uuid, pin_name: &str) -> Option<String> {
        // For graph nodes, show property value
        if let Some(graph_node) = self.project.get_graph_node(node_id) {
            if let Some(prop) = graph_node.properties.get(pin_name) {
                return Some(prop.display_value());
            }
        }
        // For clips, show clip property value
        if let Some(clip) = self.project.get_source(node_id) {
            if let Some(prop) = clip.properties.get(pin_name) {
                return Some(prop.display_value());
            }
        }
        None
    }

    fn get_pin_property(&self, node_id: Uuid, pin_name: &str) -> Option<PinPropertyInfo> {
        use library::project::property::PropertyValue;

        // Try graph node first, then clip
        let prop_value = self
            .project
            .get_graph_node(node_id)
            .and_then(|g| g.properties.get(pin_name))
            .and_then(|p| p.value())
            .or_else(|| {
                self.project
                    .get_source(node_id)
                    .and_then(|c| c.properties.get(pin_name))
                    .and_then(|p| p.value())
            })?;

        let edit_value = match prop_value {
            PropertyValue::Number(n) => PinEditValue::Scalar(n.into_inner()),
            PropertyValue::Integer(n) => PinEditValue::Integer(*n),
            PropertyValue::Boolean(b) => PinEditValue::Boolean(*b),
            PropertyValue::String(s) => PinEditValue::String(s.clone()),
            PropertyValue::Color(c) => PinEditValue::Color([
                c.r as f32 / 255.0,
                c.g as f32 / 255.0,
                c.b as f32 / 255.0,
                c.a as f32 / 255.0,
            ]),
            PropertyValue::Vec2(v) => PinEditValue::Vec2(v.x.into_inner(), v.y.into_inner()),
            PropertyValue::Vec3(v) => {
                PinEditValue::Vec3(v.x.into_inner(), v.y.into_inner(), v.z.into_inner())
            }
            _ => PinEditValue::None,
        };

        // Determine data type from pin definition or node definition
        let data_type = self.get_pin_data_type(node_id, pin_name);

        Some(PinPropertyInfo {
            value: edit_value,
            data_type,
        })
    }
}

impl VideoEditorDataSource<'_> {
    fn get_pin_data_type(&self, node_id: Uuid, pin_name: &str) -> PinDataType {
        // Try graph node definition
        if let Some(graph_node) = self.project.get_graph_node(node_id) {
            if let Some(def) = self.plugin_manager.get_node_type(&graph_node.type_id) {
                for p in &def.inputs {
                    if p.name == pin_name {
                        return convert_pin_data_type(&p.data_type);
                    }
                }
            }
        }
        // Try clip property definitions
        if let Some(clip) = self.project.get_source(node_id) {
            let defs = library::project::source::SourceData::get_definitions_for_kind(&clip.kind);
            for def in &defs {
                if def.name() == pin_name {
                    return convert_pin_data_type(&def.ui_type().pin_data_type());
                }
            }
        }
        PinDataType::Any
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
    fn set_pin_value(
        &mut self,
        node_id: Uuid,
        pin_name: &str,
        value_str: &str,
    ) -> Result<(), String> {
        use library::project::property::PropertyValue;
        // Try to parse the string as a property value and update
        let value = PropertyValue::from_display_str(value_str);
        self.project_service
            .update_graph_node_property(node_id, pin_name, 0.0, value, None)
            .map_err(|e| e.to_string())
    }

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
        let from = library::project::connection::PinId::new(from_node, from_pin);
        let to = library::project::connection::PinId::new(to_node, to_pin);
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
