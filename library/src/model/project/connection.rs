use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::model::{GeneratorContent, NodeContent};

use super::{NodeContainer, Project, ProjectGraphError};

pub const IMAGE_OUTPUT_PORT: &str = "image";
pub const IMAGE_INPUT_PORT: &str = "image_in";
pub const MERGE_IMAGES_PORT: &str = "images";
pub const SHAPE_OUTPUT_PORT: &str = "shape";
pub const SHAPE_INPUT_PORT: &str = "shape_in";
pub const TIME_PORT: &str = "time";
pub const FRAME_PORT: &str = "frame";
pub const FPS_PORT: &str = "fps";
pub const DURATION_PORT: &str = "duration";
pub const RESOLUTION_PORT: &str = "resolution";

/// The normal result of evaluating a graph port. `NoOutput` is not an error
/// and is deliberately distinct from transparent pixels, zero, false, and
/// every other valid value in `T`.
#[derive(Clone, PartialEq, Debug)]
pub enum EvalOutput<T> {
    Produced(T),
    NoOutput,
}

impl<T> EvalOutput<T> {
    pub fn as_ref(&self) -> EvalOutput<&T> {
        match self {
            Self::Produced(value) => EvalOutput::Produced(value),
            Self::NoOutput => EvalOutput::NoOutput,
        }
    }

    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> EvalOutput<U> {
        match self {
            Self::Produced(value) => EvalOutput::Produced(map(value)),
            Self::NoOutput => EvalOutput::NoOutput,
        }
    }
}

pub type EvaluationError = crate::error::LibraryError;
pub type EvalResult<T> = Result<EvalOutput<T>, EvaluationError>;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(tag = "owner_type", content = "owner_id")]
pub enum PortOwner {
    Composition(Uuid),
    Track(Uuid),
    Clip(Uuid),
    Node(Uuid),
}

impl PortOwner {
    pub fn id(self) -> Uuid {
        match self {
            Self::Composition(id) | Self::Track(id) | Self::Clip(id) | Self::Node(id) => id,
        }
    }
}

/// Why a container exposes a particular owner as part of its image output.
/// This is a derived graph projection, not persisted project state.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ContainerImageSourceKind {
    /// The container's `output_node_id` selects one directly contained Node.
    OutputBinding,
    /// No output binding exists, so the ordered direct children are composed.
    DerivedChild,
}

/// One ordered dependency of a Composition, Track, or Clip image output.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ContainerImageSource {
    pub source: PortOwner,
    pub kind: ContainerImageSourceKind,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
pub struct PortAddress {
    pub owner: PortOwner,
    pub port: String,
}

impl PortAddress {
    pub fn new(owner: PortOwner, port: impl Into<String>) -> Self {
        Self {
            owner,
            port: port.into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PortDirection {
    Input,
    Output,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PortSide {
    Left,
    Right,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PortExposure {
    Graph,
    Internal,
    External,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum PortMultiplicity {
    #[default]
    Single,
    Variadic,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PortDataType {
    Any,
    Image,
    /// Render-time vector/typographic value. This is distinct from `Path`,
    /// which is only an authored scalar SVG path string.
    Shape,
    Audio,
    Number,
    Integer,
    Boolean,
    String,
    Color,
    Path,
    Vec2,
    Vec3,
    Vec4,
}

impl PortDataType {
    pub fn accepts(self, source: Self) -> bool {
        self == Self::Any
            || source == Self::Any
            || self == source
            || (self == Self::Number && source == Self::Integer)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct PortDefinition {
    pub key: String,
    pub label: String,
    pub direction: PortDirection,
    pub side: PortSide,
    pub exposure: PortExposure,
    pub data_type: PortDataType,
    pub multiplicity: PortMultiplicity,
}

impl PortDefinition {
    pub fn input(key: &str, label: &str, data_type: PortDataType) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            direction: PortDirection::Input,
            side: PortSide::Left,
            exposure: PortExposure::Graph,
            data_type,
            multiplicity: PortMultiplicity::Single,
        }
    }

    pub fn output(
        key: &str,
        label: &str,
        data_type: PortDataType,
        side: PortSide,
        exposure: PortExposure,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            direction: PortDirection::Output,
            side,
            exposure,
            data_type,
            multiplicity: PortMultiplicity::Single,
        }
    }

    pub fn variadic(mut self) -> Self {
        self.multiplicity = PortMultiplicity::Variadic;
        self
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ProjectConnection {
    pub id: Uuid,
    pub from: PortAddress,
    pub to: PortAddress,
    /// Stable evaluation order for variadic inputs. It is independent of UI
    /// pin indices and remains meaningful after layout changes.
    pub order: i64,
}

impl ProjectConnection {
    pub fn new(from: PortAddress, to: PortAddress, order: i64) -> Self {
        Self {
            id: Uuid::new_v4(),
            from,
            to,
            order,
        }
    }
}

fn metadata_catalog(direction: PortDirection, exposure: PortExposure) -> Vec<PortDefinition> {
    let ports: &[(&str, &str, PortDataType)] = match direction {
        // Time, Duration, and Resolution remain the authored container
        // overrides. FPS and Frame are derived, read-only context values.
        PortDirection::Input => &[
            (TIME_PORT, "Time", PortDataType::Number),
            (DURATION_PORT, "Duration", PortDataType::Number),
            (RESOLUTION_PORT, "Resolution", PortDataType::Vec2),
        ],
        PortDirection::Output => &[
            (TIME_PORT, "Time", PortDataType::Number),
            (FRAME_PORT, "Frame", PortDataType::Integer),
            (FPS_PORT, "FPS", PortDataType::Number),
            (DURATION_PORT, "Duration", PortDataType::Number),
            (RESOLUTION_PORT, "Resolution", PortDataType::Vec2),
        ],
    };
    ports
        .iter()
        .cloned()
        .map(|(key, label, data_type)| match direction {
            PortDirection::Input => PortDefinition {
                exposure,
                ..PortDefinition::input(key, label, data_type)
            },
            PortDirection::Output => {
                PortDefinition::output(key, label, data_type, PortSide::Left, exposure)
            }
        })
        .collect()
}

fn container_ports() -> Vec<PortDefinition> {
    let mut ports = metadata_catalog(PortDirection::Input, PortExposure::External);
    ports.extend(metadata_catalog(
        PortDirection::Output,
        PortExposure::Internal,
    ));
    ports.push(PortDefinition::output(
        IMAGE_OUTPUT_PORT,
        "Image",
        PortDataType::Image,
        PortSide::Right,
        PortExposure::External,
    ));
    ports
}

fn node_ports(node: &crate::model::Node) -> Vec<PortDefinition> {
    let mut ports = Vec::new();
    let time_input = || PortDefinition::input(TIME_PORT, "Time", PortDataType::Number);
    let image_output = || {
        PortDefinition::output(
            IMAGE_OUTPUT_PORT,
            "Image",
            PortDataType::Image,
            PortSide::Right,
            PortExposure::Graph,
        )
    };
    let mut include_property_inputs = true;
    match &node.content {
        NodeContent::Generator(GeneratorContent::Text) => {
            ports.extend([
                time_input(),
                PortDefinition::input("text", "Text", PortDataType::String),
                PortDefinition::input("font_family", "Font", PortDataType::String),
                PortDefinition::input("size", "Size", PortDataType::Number),
            ]);
            ports.push(PortDefinition::output(
                SHAPE_OUTPUT_PORT,
                "Shape",
                PortDataType::Shape,
                PortSide::Right,
                PortExposure::Graph,
            ));
        }
        NodeContent::Generator(GeneratorContent::Solid) => {
            ports.push(time_input());
            ports.push(PortDefinition::input("color", "Color", PortDataType::Color));
            ports.push(image_output());
        }
        NodeContent::Generator(GeneratorContent::Shape) => {
            ports.extend([
                time_input(),
                PortDefinition::input("path", "Path", PortDataType::Path),
            ]);
            ports.push(PortDefinition::output(
                SHAPE_OUTPUT_PORT,
                "Shape",
                PortDataType::Shape,
                PortSide::Right,
                PortExposure::Graph,
            ));
        }
        NodeContent::Generator(GeneratorContent::SkSL) => {
            ports.push(time_input());
            ports.push(PortDefinition::input(
                "shader",
                "Shader",
                PortDataType::String,
            ));
            ports.push(image_output());
        }
        NodeContent::Media(_) => {
            ports.extend([
                time_input(),
                PortDefinition::input("opacity", "Opacity", PortDataType::Number),
                PortDefinition::input("audio", "Audio", PortDataType::Audio),
            ]);
            ports.push(image_output());
        }
        NodeContent::Reference(_) => {
            ports.push(time_input());
            ports.push(PortDefinition::input(
                IMAGE_INPUT_PORT,
                "Image",
                PortDataType::Image,
            ));
            ports.push(image_output());
        }
        NodeContent::PluginOperation(operation) => {
            include_property_inputs = false;
            ports.extend(operation.declared_ports.iter().cloned());
        }
        NodeContent::Merge => {
            ports.push(time_input());
            ports.push(
                PortDefinition::input(MERGE_IMAGES_PORT, "Images", PortDataType::Image).variadic(),
            );
            ports.push(image_output());
        }
    }
    if include_property_inputs {
        for (key, property) in node.properties.iter() {
            if ports
                .iter()
                .any(|port| port.key == *key && port.direction == PortDirection::Input)
            {
                continue;
            }
            let data_type = property
                .value()
                .map(property_value_data_type)
                .unwrap_or(PortDataType::Any);
            ports.push(PortDefinition::input(
                key,
                &humanize_port_key(key),
                data_type,
            ));
        }
    }
    ports
}

fn property_value_data_type(value: &crate::model::property::PropertyValue) -> PortDataType {
    use crate::model::property::PropertyValue;
    match value {
        PropertyValue::Number(_) => PortDataType::Number,
        PropertyValue::Integer(_) => PortDataType::Integer,
        PropertyValue::String(_) => PortDataType::String,
        PropertyValue::Boolean(_) => PortDataType::Boolean,
        PropertyValue::Vec2(_) => PortDataType::Vec2,
        PropertyValue::Color(_) => PortDataType::Color,
        PropertyValue::Vec3(_) => PortDataType::Vec3,
        PropertyValue::Vec4(_) => PortDataType::Vec4,
        PropertyValue::Array(_) | PropertyValue::Map(_) => PortDataType::Any,
    }
}

fn humanize_port_key(key: &str) -> String {
    let mut result = String::new();
    let mut uppercase = true;
    for character in key.chars() {
        if matches!(character, '_' | '-') {
            result.push(' ');
            uppercase = true;
        } else if uppercase {
            result.extend(character.to_uppercase());
            uppercase = false;
        } else {
            result.push(character);
        }
    }
    result
}

impl Project {
    /// Return the authoritative, ordered image dependencies for a container.
    ///
    /// An explicit output binding always replaces fallback composition. Without
    /// one, a Composition derives from its ordered Tracks and a Track derives
    /// from its ordered Clips. Direct Nodes are an internal graph implementation
    /// detail and never become an implicit image output; a Clip therefore needs
    /// an explicit output binding. Missing owners and leaf Nodes have no sources.
    pub fn container_image_sources(&self, owner: PortOwner) -> Vec<ContainerImageSource> {
        self.container_image_sources_with_connections(owner, &self.connections)
    }

    fn container_image_sources_with_connections(
        &self,
        owner: PortOwner,
        _connections: &[ProjectConnection],
    ) -> Vec<ContainerImageSource> {
        let derived = |source| ContainerImageSource {
            source,
            kind: ContainerImageSourceKind::DerivedChild,
        };
        let bound = |node_id| {
            vec![ContainerImageSource {
                source: PortOwner::Node(node_id),
                kind: ContainerImageSourceKind::OutputBinding,
            }]
        };

        let (container, output_node_id) = match owner {
            PortOwner::Composition(id) => {
                let Some(composition) = self.get_composition(id) else {
                    return Vec::new();
                };
                (NodeContainer::Composition(id), composition.output_node_id)
            }
            PortOwner::Track(id) => {
                let Some(track) = self.get_track(id) else {
                    return Vec::new();
                };
                (NodeContainer::Track(id), track.output_node_id)
            }
            PortOwner::Clip(id) => {
                let Some(clip) = self.get_clip(id) else {
                    return Vec::new();
                };
                (NodeContainer::Clip(id), clip.output_node_id)
            }
            PortOwner::Node(_) => return Vec::new(),
        };
        if let Some(output_node_id) = output_node_id {
            return bound(output_node_id);
        }

        self.direct_child_owners(container)
            .into_iter()
            .filter(|source| self.owner_has_image_output(*source))
            .map(derived)
            .collect()
    }

    pub fn port_definitions(&self, owner: PortOwner) -> Vec<PortDefinition> {
        match owner {
            PortOwner::Composition(id) if self.get_composition(id).is_some() => container_ports(),
            PortOwner::Track(id) if self.get_track(id).is_some() => container_ports(),
            PortOwner::Clip(id) if self.get_clip(id).is_some() => container_ports(),
            PortOwner::Node(id) => self.get_node(id).map(node_ports).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn direct_child_owners(&self, container: NodeContainer) -> Vec<PortOwner> {
        match container {
            NodeContainer::Composition(id) => self
                .get_composition(id)
                .map(|composition| {
                    composition
                        .track_ids
                        .iter()
                        .copied()
                        .map(PortOwner::Track)
                        .collect()
                })
                .unwrap_or_default(),
            NodeContainer::Track(id) => self
                .get_track(id)
                .map(|track| {
                    track
                        .clip_ids
                        .iter()
                        .copied()
                        .map(PortOwner::Clip)
                        .collect()
                })
                .unwrap_or_default(),
            NodeContainer::Clip(_) => Vec::new(),
        }
    }

    fn owner_has_image_output(&self, owner: PortOwner) -> bool {
        let address = PortAddress::new(owner, IMAGE_OUTPUT_PORT);
        self.port_definition(&address, PortDirection::Output)
            .is_some_and(|port| port.data_type == PortDataType::Image)
    }

    pub fn port_definition(
        &self,
        address: &PortAddress,
        direction: PortDirection,
    ) -> Option<PortDefinition> {
        self.port_definitions(address.owner)
            .into_iter()
            .find(|port| port.key == address.port && port.direction == direction)
    }

    pub fn connect_ports(
        &mut self,
        from: PortAddress,
        to: PortAddress,
    ) -> Result<Uuid, ProjectGraphError> {
        let source = self
            .port_definition(&from, PortDirection::Output)
            .ok_or_else(|| ProjectGraphError::PortNotFound(from.clone()))?;
        let target = self
            .port_definition(&to, PortDirection::Input)
            .ok_or_else(|| ProjectGraphError::PortNotFound(to.clone()))?;
        if !target.data_type.accepts(source.data_type) {
            return Err(ProjectGraphError::IncompatiblePortTypes {
                source_type: source.data_type,
                target_type: target.data_type,
            });
        }
        if source.exposure == PortExposure::Internal
            && !self.is_internal_port_target(from.owner, to.owner)
        {
            return Err(ProjectGraphError::InternalPortEscapesContainer {
                source_owner: from.owner,
                target_owner: to.owner,
            });
        }
        if !is_render_evaluable_type(source.data_type) {
            return Err(ProjectGraphError::UnsupportedConnectionType {
                connection_id: Uuid::nil(),
                data_type: source.data_type,
            });
        }
        if let Some(existing) = self
            .connections
            .iter()
            .find(|item| item.from == from && item.to == to)
        {
            return Ok(existing.id);
        }

        let order = match target.multiplicity {
            PortMultiplicity::Single => 0,
            PortMultiplicity::Variadic => {
                self.connections
                    .iter()
                    .filter(|item| item.to == to)
                    .map(|item| item.order)
                    .max()
                    .unwrap_or(-1)
                    + 1
            }
        };
        let connection = ProjectConnection::new(from, to, order);
        let mut prospective = self.connections.clone();
        if target.multiplicity == PortMultiplicity::Single {
            prospective.retain(|item| item.to != connection.to);
        }
        prospective.push(connection.clone());
        if source.exposure != PortExposure::Internal
            && is_render_evaluable_type(source.data_type)
            && self.connection_is_cyclic(&connection, &prospective)
        {
            return Err(ProjectGraphError::ConnectionCycle {
                from: connection.from.owner,
                to: connection.to.owner,
            });
        }
        if target.multiplicity == PortMultiplicity::Single {
            self.connections.retain(|item| item.to != connection.to);
        }
        let id = connection.id;
        self.connections.push(connection);
        Ok(id)
    }

    pub fn reorder_connection(
        &mut self,
        connection_id: Uuid,
        new_order: i64,
    ) -> Result<(), ProjectGraphError> {
        let index = self
            .connections
            .iter()
            .position(|item| item.id == connection_id)
            .ok_or(ProjectGraphError::ConnectionNotFound(connection_id))?;
        let target = self.connections[index].to.clone();
        let definition = self
            .port_definition(&target, PortDirection::Input)
            .ok_or_else(|| ProjectGraphError::PortNotFound(target.clone()))?;
        if definition.multiplicity != PortMultiplicity::Variadic {
            self.connections[index].order = 0;
            return Ok(());
        }
        let mut ids = self
            .connections
            .iter()
            .filter(|item| item.to == target)
            .map(|item| (item.order, item.id))
            .collect::<Vec<_>>();
        ids.sort_by_key(|item| item.0);
        let mut ids = ids.into_iter().map(|item| item.1).collect::<Vec<_>>();
        ids.retain(|id| *id != connection_id);
        let insert_at = new_order.max(0) as usize;
        ids.insert(insert_at.min(ids.len()), connection_id);
        for (order, id) in ids.into_iter().enumerate() {
            let connection = self
                .connections
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or(ProjectGraphError::ConnectionNotFound(id))?;
            connection.order = order as i64;
        }
        Ok(())
    }

    pub fn disconnect_connection(&mut self, id: Uuid) -> bool {
        let old_len = self.connections.len();
        self.connections.retain(|item| item.id != id);
        self.normalize_connection_orders();
        old_len != self.connections.len()
    }

    /// Move either end of one canonical connection without changing its
    /// persistent identity. The mutation is transactional and retains the
    /// original variadic order whenever the target is unchanged.
    pub fn reconnect_connection(
        &mut self,
        id: Uuid,
        from: PortAddress,
        to: PortAddress,
    ) -> Result<(), ProjectGraphError> {
        let original_index = self
            .connections
            .iter()
            .position(|connection| connection.id == id)
            .ok_or(ProjectGraphError::ConnectionNotFound(id))?;
        let original = self.connections[original_index].clone();
        if original.from == from && original.to == to {
            return Ok(());
        }

        let target = self
            .port_definition(&to, PortDirection::Input)
            .ok_or_else(|| ProjectGraphError::PortNotFound(to.clone()))?;
        let baseline = self.validate_connections();
        let mut candidate = self.clone();
        let mut moved = candidate.connections.remove(original_index);
        moved.from = from;
        moved.to = to.clone();

        if original.to != to {
            candidate.normalize_connection_orders();
            moved.order = match target.multiplicity {
                PortMultiplicity::Single => {
                    candidate
                        .connections
                        .retain(|connection| connection.to != to);
                    0
                }
                PortMultiplicity::Variadic => {
                    let count = candidate
                        .connections
                        .iter()
                        .filter(|connection| connection.to == to)
                        .count() as i64;
                    let insertion_order = original.order.clamp(0, count);
                    for connection in candidate
                        .connections
                        .iter_mut()
                        .filter(|connection| connection.to == to)
                    {
                        if connection.order >= insertion_order {
                            connection.order += 1;
                        }
                    }
                    insertion_order
                }
            };
        }

        candidate
            .connections
            .insert(original_index.min(candidate.connections.len()), moved);
        if let Some(error) =
            super::first_new_project_validation_error(&baseline, candidate.validate_connections())
        {
            return Err(error);
        }
        *self = candidate;
        Ok(())
    }

    /// Insert an already-contained Node (or another compatible owner) into an
    /// existing connection. The original connection becomes the downstream
    /// edge and therefore keeps its UUID, target and variadic order; only one
    /// new upstream connection is allocated.
    pub fn splice_connection(
        &mut self,
        connection_id: Uuid,
        via_input: PortAddress,
        via_output: PortAddress,
    ) -> Result<Uuid, ProjectGraphError> {
        let original = self
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .cloned()
            .ok_or(ProjectGraphError::ConnectionNotFound(connection_id))?;
        let via_input_definition = self
            .port_definition(&via_input, PortDirection::Input)
            .ok_or_else(|| ProjectGraphError::PortNotFound(via_input.clone()))?;
        if via_input_definition.multiplicity == PortMultiplicity::Single
            && self
                .connections
                .iter()
                .any(|connection| connection.to == via_input)
        {
            return Err(ProjectGraphError::SpliceInputOccupied { target: via_input });
        }
        let baseline = self.validate_connections();
        let mut candidate = self.clone();
        let upstream_id = candidate.connect_ports(original.from.clone(), via_input)?;
        let downstream = candidate
            .connections
            .iter_mut()
            .find(|connection| connection.id == connection_id)
            .ok_or(ProjectGraphError::ConnectionNotFound(connection_id))?;
        downstream.from = via_output;

        if let Some(error) =
            super::first_new_project_validation_error(&baseline, candidate.validate_connections())
        {
            return Err(error);
        }
        *self = candidate;
        Ok(upstream_id)
    }

    pub fn disconnect_ports(&mut self, from: &PortAddress, to: &PortAddress) -> bool {
        let old_len = self.connections.len();
        self.connections
            .retain(|item| &item.from != from || &item.to != to);
        self.normalize_connection_orders();
        old_len != self.connections.len()
    }

    fn normalize_connection_orders(&mut self) {
        let targets = self
            .connections
            .iter()
            .map(|item| item.to.clone())
            .collect::<HashSet<_>>();
        for target in targets {
            let mut ids = self
                .connections
                .iter()
                .filter(|item| item.to == target)
                .map(|item| (item.order, item.id))
                .collect::<Vec<_>>();
            ids.sort_by_key(|item| item.0);
            for (order, (_, id)) in ids.into_iter().enumerate() {
                if let Some(connection) = self.connections.iter_mut().find(|item| item.id == id) {
                    connection.order = order as i64;
                }
            }
        }
    }

    pub fn validate_connections(&self) -> Vec<ProjectGraphError> {
        let mut errors = self.validate_containment();
        errors.extend(self.validate_plugin_operation_contracts());
        errors.extend(self.validate_container_output_ports());
        let mut targets: HashMap<&PortAddress, Vec<&ProjectConnection>> = HashMap::new();
        for connection in &self.connections {
            targets.entry(&connection.to).or_default().push(connection);
            errors.extend(self.validate_connection(connection));
        }
        for (target, mut inputs) in targets {
            let Some(definition) = self.port_definition(target, PortDirection::Input) else {
                continue;
            };
            match definition.multiplicity {
                PortMultiplicity::Single if inputs.len() > 1 => {
                    errors.push(ProjectGraphError::DuplicateInputConnection {
                        target: target.clone(),
                    })
                }
                PortMultiplicity::Variadic => {
                    inputs.sort_by_key(|item| item.order);
                    for pair in inputs.windows(2) {
                        if pair[0].order == pair[1].order {
                            errors.push(ProjectGraphError::DuplicateConnectionOrder {
                                target: target.clone(),
                                order: pair[0].order,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        errors
    }

    fn validate_container_output_ports(&self) -> Vec<ProjectGraphError> {
        self.compositions
            .iter()
            .filter_map(|composition| {
                composition
                    .output_node_id
                    .map(|node_id| (NodeContainer::Composition(composition.id), node_id))
            })
            .chain(self.tracks.values().filter_map(|track| {
                track
                    .output_node_id
                    .map(|node_id| (NodeContainer::Track(track.id), node_id))
            }))
            .chain(self.clips.values().filter_map(|clip| {
                clip.output_node_id
                    .map(|node_id| (NodeContainer::Clip(clip.id), node_id))
            }))
            .filter_map(|(container, node_id)| {
                self.get_node(node_id)
                    .is_some_and(|_| !self.owner_has_image_output(PortOwner::Node(node_id)))
                    .then_some(ProjectGraphError::OutputNodeHasNoImagePort { node_id, container })
            })
            .collect()
    }

    fn validate_plugin_operation_contracts(&self) -> Vec<ProjectGraphError> {
        let mut errors = Vec::new();
        for node in self.nodes.values() {
            let NodeContent::PluginOperation(operation) = &node.content else {
                continue;
            };
            for (field, value) in [
                ("category", operation.category.as_str()),
                ("component_id", operation.component_id.as_str()),
                ("operation", operation.operation.as_str()),
            ] {
                if value.trim().is_empty() {
                    errors.push(ProjectGraphError::EmptyPluginOperationIdentifier {
                        node_id: node.id,
                        field: field.to_string(),
                    });
                }
            }

            let mut port_keys = HashSet::new();
            // PluginOperation ports are a complete persisted execution
            // contract. Metadata such as Time must be declared explicitly by
            // the operation; injecting a hidden catalog here would create a
            // second port authority and duplicate the persisted Time input.
            for port in operation.declared_ports.iter().cloned() {
                if port.key.trim().is_empty() {
                    errors.push(ProjectGraphError::EmptyNodePortKey {
                        node_id: node.id,
                        direction: port.direction,
                    });
                    continue;
                }
                if !port_keys.insert((port.direction, port.key.clone())) {
                    errors.push(ProjectGraphError::DuplicateNodePort {
                        node_id: node.id,
                        key: port.key.clone(),
                        direction: port.direction,
                    });
                }
                if port.exposure != PortExposure::Graph {
                    errors.push(ProjectGraphError::InvalidNodePortExposure {
                        node_id: node.id,
                        key: port.key.clone(),
                        exposure: port.exposure,
                    });
                }
                if port.direction == PortDirection::Output
                    && port.multiplicity == PortMultiplicity::Variadic
                {
                    errors.push(ProjectGraphError::VariadicNodeOutput {
                        node_id: node.id,
                        key: port.key,
                    });
                }
            }
        }
        errors
    }

    /// Returns structured, serializable diagnostics for the complete
    /// authoritative Project without mutating or repairing it. Besides typed
    /// connections this includes containment, entity identity, Composition
    /// settings/work-area invariants, and duplicate persistent IDs.
    pub fn validation_issues(&self) -> Vec<ProjectGraphError> {
        self.validate_connections()
    }

    pub fn validate_connection(&self, connection: &ProjectConnection) -> Vec<ProjectGraphError> {
        let mut errors = Vec::new();
        let Some(source) = self.port_definition(&connection.from, PortDirection::Output) else {
            errors.push(ProjectGraphError::PortNotFound(connection.from.clone()));
            return errors;
        };
        let Some(target) = self.port_definition(&connection.to, PortDirection::Input) else {
            errors.push(ProjectGraphError::PortNotFound(connection.to.clone()));
            return errors;
        };
        if !target.data_type.accepts(source.data_type) {
            errors.push(ProjectGraphError::IncompatiblePortTypes {
                source_type: source.data_type,
                target_type: target.data_type,
            });
        }
        if source.exposure == PortExposure::Internal
            && !self.is_internal_port_target(connection.from.owner, connection.to.owner)
        {
            errors.push(ProjectGraphError::InternalPortEscapesContainer {
                source_owner: connection.from.owner,
                target_owner: connection.to.owner,
            });
        }
        if !is_render_evaluable_type(source.data_type) {
            errors.push(ProjectGraphError::UnsupportedConnectionType {
                connection_id: connection.id,
                data_type: source.data_type,
            });
        }
        if source.exposure != PortExposure::Internal
            && is_render_evaluable_type(source.data_type)
            && self.connection_is_cyclic(connection, &self.connections)
        {
            errors.push(ProjectGraphError::ConnectionCycle {
                from: connection.from.owner,
                to: connection.to.owner,
            });
        }
        errors
    }

    fn is_internal_port_target(&self, source: PortOwner, target: PortOwner) -> bool {
        match source {
            PortOwner::Composition(id) => match target {
                PortOwner::Track(track) => self.find_composition_for_track(track) == Some(id),
                PortOwner::Node(node) => {
                    self.find_node_container(node) == Some(NodeContainer::Composition(id))
                }
                _ => false,
            },
            PortOwner::Track(id) => match target {
                PortOwner::Clip(clip) => self.find_track_for_clip(clip) == Some(id),
                PortOwner::Node(node) => {
                    self.find_node_container(node) == Some(NodeContainer::Track(id))
                }
                _ => false,
            },
            PortOwner::Clip(id) => {
                matches!(target, PortOwner::Node(node) if self.find_node_container(node) == Some(NodeContainer::Clip(id)))
            }
            PortOwner::Node(_) => false,
        }
    }

    fn connection_is_cyclic(
        &self,
        candidate: &ProjectConnection,
        connections: &[ProjectConnection],
    ) -> bool {
        if self.descendant_scope_override_is_cyclic(candidate) {
            return true;
        }
        let dependencies = self.graph_dependencies(connections, Some(candidate.id));
        is_reachable(
            candidate.from.owner,
            candidate.to.owner,
            &dependencies,
            &mut HashSet::new(),
        )
    }

    /// A Node output is evaluated in the scope inherited from its containing
    /// Clip/Track/Composition. Feeding that value back into a metadata input
    /// on the same container or one of its ancestors makes the source scope
    /// depend on itself even when the Node does not produce an Image.
    fn descendant_scope_override_is_cyclic(&self, connection: &ProjectConnection) -> bool {
        if !matches!(
            connection.to.port.as_str(),
            TIME_PORT | DURATION_PORT | RESOLUTION_PORT
        ) {
            return false;
        }
        let PortOwner::Node(source_node_id) = connection.from.owner else {
            return false;
        };
        match connection.to.owner {
            PortOwner::Composition(composition_id) => {
                self.find_containing_composition(source_node_id) == Some(composition_id)
            }
            PortOwner::Track(track_id) => self.find_parent_track(source_node_id) == Some(track_id),
            PortOwner::Clip(clip_id) => self.find_parent_clip(source_node_id) == Some(clip_id),
            PortOwner::Node(_) => false,
        }
    }

    /// Build owner-level evaluation dependencies for rendered image flow and
    /// typed/value graph connections. Container `Internal` outputs are scope
    /// metadata roots (time, frame, FPS, duration, resolution), so they do not
    /// depend on the container's rendered children and must not create a false
    /// container <-> child cycle here.
    fn graph_dependencies(
        &self,
        connections: &[ProjectConnection],
        skipped: Option<Uuid>,
    ) -> HashMap<PortOwner, Vec<PortOwner>> {
        let mut dependencies: HashMap<PortOwner, Vec<PortOwner>> = HashMap::new();
        for composition in &self.compositions {
            let owner = PortOwner::Composition(composition.id);
            dependencies.entry(owner).or_default().extend(
                self.container_image_sources_with_connections(owner, connections)
                    .into_iter()
                    .map(|source| source.source),
            );
        }
        for track in self.tracks.values() {
            let owner = PortOwner::Track(track.id);
            dependencies.entry(owner).or_default().extend(
                self.container_image_sources_with_connections(owner, connections)
                    .into_iter()
                    .map(|source| source.source),
            );
        }
        for clip in self.clips.values() {
            let owner = PortOwner::Clip(clip.id);
            dependencies.entry(owner).or_default().extend(
                self.container_image_sources_with_connections(owner, connections)
                    .into_iter()
                    .map(|source| source.source),
            );
        }
        for connection in connections {
            if Some(connection.id) == skipped {
                continue;
            }
            let creates_dependency = self
                .port_definition(&connection.from, PortDirection::Output)
                .is_some_and(|port| {
                    port.exposure != PortExposure::Internal
                        && is_render_evaluable_type(port.data_type)
                });
            if creates_dependency {
                dependencies
                    .entry(connection.to.owner)
                    .or_default()
                    .push(connection.from.owner);
            }
        }
        for node in self.nodes.values() {
            let NodeContent::Reference(reference) = &node.content else {
                continue;
            };
            let input = PortAddress::new(PortOwner::Node(node.id), IMAGE_INPUT_PORT);
            if !connections.iter().any(|item| item.to == input) {
                dependencies
                    .entry(PortOwner::Node(node.id))
                    .or_default()
                    .push(PortOwner::Composition(reference.target_id));
            }
        }
        dependencies
    }
}

fn is_render_evaluable_type(data_type: PortDataType) -> bool {
    !matches!(data_type, PortDataType::Any | PortDataType::Audio)
}

fn is_reachable(
    current: PortOwner,
    target: PortOwner,
    dependencies: &HashMap<PortOwner, Vec<PortOwner>>,
    visited: &mut HashSet<PortOwner>,
) -> bool {
    if current == target {
        return true;
    }
    if !visited.insert(current) {
        return false;
    }
    dependencies.get(&current).is_some_and(|items| {
        items
            .iter()
            .any(|item| is_reachable(*item, target, dependencies, visited))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::project::Composition;
    use crate::model::{Clip, Node, ReferenceContent};

    fn add_node(project: &mut Project, container: NodeContainer, name: &str) -> Uuid {
        let node = Node::new(name, NodeContent::Merge);
        let node_id = node.id;
        project.add_node(node);
        project
            .attach_node_to_container(container, node_id)
            .unwrap();
        node_id
    }

    fn add_reference_node(project: &mut Project, container: NodeContainer, name: &str) -> Uuid {
        let node = Node::new(
            name,
            NodeContent::Reference(ReferenceContent {
                target_id: Uuid::new_v4(),
                sync_global_time: false,
            }),
        );
        let node_id = node.id;
        project.add_node(node);
        project
            .attach_node_to_container(container, node_id)
            .unwrap();
        node_id
    }

    #[test]
    fn container_image_sources_preserve_fallback_order_and_binding_precedence() {
        let mut project = Project::new("container sources");
        let (composition, track) = Composition::new("composition", 320, 180, 30.0, 10.0);
        let composition_id = composition.id;
        let track_id = track.id;
        project.add_track(track);
        project.add_composition(composition);

        let clip = Clip::new("clip", 0.0, 10.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let composition_node = add_node(
            &mut project,
            NodeContainer::Composition(composition_id),
            "composition node",
        );
        let track_node = add_node(&mut project, NodeContainer::Track(track_id), "track node");
        let _first_clip_node = add_node(
            &mut project,
            NodeContainer::Clip(clip_id),
            "first clip node",
        );
        let second_clip_node = add_node(
            &mut project,
            NodeContainer::Clip(clip_id),
            "second clip node",
        );
        let derived = ContainerImageSourceKind::DerivedChild;
        assert_eq!(
            project.container_image_sources(PortOwner::Composition(composition_id)),
            vec![ContainerImageSource {
                source: PortOwner::Track(track_id),
                kind: derived,
            }]
        );
        assert_eq!(
            project.container_image_sources(PortOwner::Track(track_id)),
            vec![ContainerImageSource {
                source: PortOwner::Clip(clip_id),
                kind: derived,
            }]
        );
        assert!(
            project
                .container_image_sources(PortOwner::Clip(clip_id))
                .is_empty(),
            "Clip nodes are internal graph values until an output is bound"
        );

        project
            .connect_ports(
                PortAddress::new(PortOwner::Clip(clip_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(track_node), MERGE_IMAGES_PORT),
            )
            .unwrap();
        project
            .connect_ports(
                PortAddress::new(PortOwner::Track(track_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(composition_node), MERGE_IMAGES_PORT),
            )
            .unwrap();
        assert_eq!(
            project.container_image_sources(PortOwner::Track(track_id)),
            vec![ContainerImageSource {
                source: PortOwner::Clip(clip_id),
                kind: derived,
            }],
            "wiring a child Clip to an unbound helper Node must not suppress natural composition"
        );
        assert_eq!(
            project.container_image_sources(PortOwner::Composition(composition_id)),
            vec![ContainerImageSource {
                source: PortOwner::Track(track_id),
                kind: derived,
            }],
            "wiring a child Track to an unbound helper Node must not suppress natural composition"
        );

        project
            .set_output_node(NodeContainer::Clip(clip_id), Some(second_clip_node))
            .unwrap();
        assert_eq!(
            project.container_image_sources(PortOwner::Clip(clip_id)),
            vec![ContainerImageSource {
                source: PortOwner::Node(second_clip_node),
                kind: ContainerImageSourceKind::OutputBinding,
            }]
        );
        project
            .set_output_node(NodeContainer::Track(track_id), Some(track_node))
            .unwrap();
        assert_eq!(
            project.container_image_sources(PortOwner::Track(track_id)),
            vec![ContainerImageSource {
                source: PortOwner::Node(track_node),
                kind: ContainerImageSourceKind::OutputBinding,
            }]
        );
        project
            .set_output_node(
                NodeContainer::Composition(composition_id),
                Some(composition_node),
            )
            .unwrap();
        assert_eq!(
            project.container_image_sources(PortOwner::Composition(composition_id)),
            vec![ContainerImageSource {
                source: PortOwner::Node(composition_node),
                kind: ContainerImageSourceKind::OutputBinding,
            }]
        );
        assert!(
            project
                .container_image_sources(PortOwner::Node(second_clip_node))
                .is_empty()
        );
        assert!(
            project
                .container_image_sources(PortOwner::Track(Uuid::new_v4()))
                .is_empty()
        );
    }

    #[test]
    fn reconnect_and_splice_keep_the_downstream_connection_identity_order_and_target() {
        let mut project = Project::new("connection editing");
        let (composition, track) = Composition::new("composition", 320, 180, 30.0, 10.0);
        let composition_id = composition.id;
        project.add_track(track);
        project.add_composition(composition);
        let container = NodeContainer::Composition(composition_id);
        let source = add_node(&mut project, container, "source");
        let alternate_source = add_node(&mut project, container, "alternate source");
        let sibling = add_node(&mut project, container, "sibling");
        let via = add_node(&mut project, container, "via");
        let target = add_node(&mut project, container, "target");

        let target_address = PortAddress::new(PortOwner::Node(target), MERGE_IMAGES_PORT);
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(sibling), IMAGE_OUTPUT_PORT),
                target_address.clone(),
            )
            .unwrap();
        let connection_id = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(source), IMAGE_OUTPUT_PORT),
                target_address.clone(),
            )
            .unwrap();
        let original_order = project
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .unwrap()
            .order;

        project
            .reconnect_connection(
                connection_id,
                PortAddress::new(PortOwner::Node(alternate_source), IMAGE_OUTPUT_PORT),
                target_address.clone(),
            )
            .unwrap();
        let reconnected = project
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .unwrap();
        assert_eq!(reconnected.to, target_address);
        assert_eq!(reconnected.order, original_order);

        let upstream_id = project
            .splice_connection(
                connection_id,
                PortAddress::new(PortOwner::Node(via), MERGE_IMAGES_PORT),
                PortAddress::new(PortOwner::Node(via), IMAGE_OUTPUT_PORT),
            )
            .unwrap();
        let downstream = project
            .connections
            .iter()
            .find(|connection| connection.id == connection_id)
            .unwrap();
        assert_eq!(downstream.from.owner, PortOwner::Node(via));
        assert_eq!(downstream.to, target_address);
        assert_eq!(downstream.order, original_order);
        let upstream = project
            .connections
            .iter()
            .find(|connection| connection.id == upstream_id)
            .unwrap();
        assert_eq!(upstream.from.owner, PortOwner::Node(alternate_source));
        assert_eq!(upstream.to.owner, PortOwner::Node(via));
        assert!(project.validate_connections().is_empty());
    }

    #[test]
    fn reconnect_is_atomic_replaces_single_inputs_and_normalizes_variadic_orders() {
        let mut project = Project::new("reconnect contracts");
        let (composition, track) = Composition::new("composition", 320, 180, 30.0, 10.0);
        let composition_id = composition.id;
        project.add_track(track);
        project.add_composition(composition);
        let container = NodeContainer::Composition(composition_id);
        let sources = (0..5)
            .map(|index| add_node(&mut project, container, &format!("source {index}")))
            .collect::<Vec<_>>();
        let target_a = add_node(&mut project, container, "target a");
        let target_b = add_node(&mut project, container, "target b");
        let single_a = add_reference_node(&mut project, container, "single a");
        let single_b = add_reference_node(&mut project, container, "single b");

        let single_a_input = PortAddress::new(PortOwner::Node(single_a), IMAGE_INPUT_PORT);
        let occupied_id = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(sources[0]), IMAGE_OUTPUT_PORT),
                single_a_input.clone(),
            )
            .unwrap();
        let moving_id = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(sources[1]), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(single_b), IMAGE_INPUT_PORT),
            )
            .unwrap();
        project
            .reconnect_connection(
                moving_id,
                PortAddress::new(PortOwner::Node(sources[1]), IMAGE_OUTPUT_PORT),
                single_a_input.clone(),
            )
            .unwrap();
        assert!(
            !project
                .connections
                .iter()
                .any(|item| item.id == occupied_id)
        );
        assert_eq!(
            project
                .connections
                .iter()
                .find(|item| item.id == moving_id)
                .map(|item| (&item.to, item.order)),
            Some((&single_a_input, 0))
        );

        let target_a_input = PortAddress::new(PortOwner::Node(target_a), MERGE_IMAGES_PORT);
        let target_b_input = PortAddress::new(PortOwner::Node(target_b), MERGE_IMAGES_PORT);
        for source in &sources[..3] {
            project
                .connect_ports(
                    PortAddress::new(PortOwner::Node(*source), IMAGE_OUTPUT_PORT),
                    target_a_input.clone(),
                )
                .unwrap();
        }
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(sources[3]), IMAGE_OUTPUT_PORT),
                target_b_input.clone(),
            )
            .unwrap();
        let moved_variadic = project
            .connections
            .iter()
            .find(|item| {
                item.from.owner == PortOwner::Node(sources[2]) && item.to == target_a_input
            })
            .unwrap()
            .id;
        project
            .reconnect_connection(
                moved_variadic,
                PortAddress::new(PortOwner::Node(sources[2]), IMAGE_OUTPUT_PORT),
                target_b_input.clone(),
            )
            .unwrap();
        let orders = |project: &Project, target: &PortAddress| {
            let mut orders = project
                .connections
                .iter()
                .filter(|item| &item.to == target)
                .map(|item| item.order)
                .collect::<Vec<_>>();
            orders.sort_unstable();
            orders
        };
        assert_eq!(orders(&project, &target_a_input), vec![0, 1]);
        assert_eq!(orders(&project, &target_b_input), vec![0, 1]);

        let before_invalid = project.clone();
        let error = project
            .reconnect_connection(
                moved_variadic,
                PortAddress::new(PortOwner::Node(sources[2]), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(target_b), TIME_PORT),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            ProjectGraphError::IncompatiblePortTypes { .. }
        ));
        assert_eq!(project, before_invalid, "failed reconnect must roll back");
    }

    #[test]
    fn reconnect_allows_cross_container_graph_ports_but_rejects_internal_escape_and_cycles() {
        let mut project = Project::new("reconnect scope contracts");
        let (composition, track) = Composition::new("composition", 320, 180, 30.0, 10.0);
        let composition_id = composition.id;
        let track_id = track.id;
        project.add_track(track);
        project.add_composition(composition);
        let clip = Clip::new("clip", 0.0, 10.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let composition_source = add_node(
            &mut project,
            NodeContainer::Composition(composition_id),
            "composition source",
        );
        let clip_source = add_node(&mut project, NodeContainer::Clip(clip_id), "clip source");
        let clip_target = add_node(&mut project, NodeContainer::Clip(clip_id), "clip target");
        let image_connection = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(clip_source), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(clip_target), MERGE_IMAGES_PORT),
            )
            .unwrap();
        project
            .reconnect_connection(
                image_connection,
                PortAddress::new(PortOwner::Node(composition_source), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(clip_target), MERGE_IMAGES_PORT),
            )
            .unwrap();

        let time_connection = project
            .connect_ports(
                PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
                PortAddress::new(PortOwner::Node(clip_source), TIME_PORT),
            )
            .unwrap();
        let before_escape = project.clone();
        let error = project
            .reconnect_connection(
                time_connection,
                PortAddress::new(PortOwner::Composition(composition_id), TIME_PORT),
                PortAddress::new(PortOwner::Node(clip_source), TIME_PORT),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            ProjectGraphError::InternalPortEscapesContainer { .. }
        ));
        assert_eq!(project, before_escape);

        let a = add_node(
            &mut project,
            NodeContainer::Composition(composition_id),
            "a",
        );
        let b = add_node(
            &mut project,
            NodeContainer::Composition(composition_id),
            "b",
        );
        let c = add_node(
            &mut project,
            NodeContainer::Composition(composition_id),
            "c",
        );
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(a), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(b), MERGE_IMAGES_PORT),
            )
            .unwrap();
        let movable = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(c), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(clip_target), MERGE_IMAGES_PORT),
            )
            .unwrap();
        let before_cycle = project.clone();
        let error = project
            .reconnect_connection(
                movable,
                PortAddress::new(PortOwner::Node(b), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(a), MERGE_IMAGES_PORT),
            )
            .unwrap_err();
        assert!(matches!(error, ProjectGraphError::ConnectionCycle { .. }));
        assert_eq!(project, before_cycle);
    }

    #[test]
    fn splice_rejects_occupied_single_input_and_any_validation_failure_without_mutation() {
        let mut project = Project::new("splice rollback");
        let (composition, track) = Composition::new("composition", 320, 180, 30.0, 10.0);
        let composition_id = composition.id;
        project.add_track(track);
        project.add_composition(composition);
        let container = NodeContainer::Composition(composition_id);
        let source = add_node(&mut project, container, "source");
        let occupant = add_node(&mut project, container, "occupant");
        let via = add_reference_node(&mut project, container, "via");
        let target = add_node(&mut project, container, "target");
        let connection_id = project
            .connect_ports(
                PortAddress::new(PortOwner::Node(source), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(target), MERGE_IMAGES_PORT),
            )
            .unwrap();
        let via_input = PortAddress::new(PortOwner::Node(via), IMAGE_INPUT_PORT);
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(occupant), IMAGE_OUTPUT_PORT),
                via_input.clone(),
            )
            .unwrap();
        let before_occupied = project.clone();
        assert_eq!(
            project
                .splice_connection(
                    connection_id,
                    via_input.clone(),
                    PortAddress::new(PortOwner::Node(via), IMAGE_OUTPUT_PORT),
                )
                .unwrap_err(),
            ProjectGraphError::SpliceInputOccupied { target: via_input }
        );
        assert_eq!(project, before_occupied);

        let empty_via = add_reference_node(&mut project, container, "empty via");
        let before_invalid = project.clone();
        let error = project
            .splice_connection(
                connection_id,
                PortAddress::new(PortOwner::Node(empty_via), IMAGE_INPUT_PORT),
                PortAddress::new(PortOwner::Composition(composition_id), TIME_PORT),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            ProjectGraphError::IncompatiblePortTypes { .. }
                | ProjectGraphError::InternalPortEscapesContainer { .. }
        ));
        assert_eq!(project, before_invalid);
    }
}
