use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::model::BlendMode;
use crate::model::node::{Node, NodeContent, native_node_descriptor_for_node};
use crate::model::project::connection::{
    AUDIO_OUTPUT_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, PortDataType,
    PortDefinition, PortDirection, PortExposure, PortMultiplicity, PortSide, SOUND_INPUT_PORT,
    TIME_PORT,
};
use crate::model::project::property::PropertyValue;

use super::{
    ModuleConnectionId, ModuleDefinitionId, ModuleHostContract, ModuleInstanceId, ModuleOutputId,
    PublishedActionId, PublishedMediaInputId, PublishedParameterId, PublishedSignalId,
};

#[path = "module_graph/parameter_value_validation.rs"]
mod parameter_value_validation;

/// Runtime sampling support for one Published Module parameter.
///
/// This is derived from the parameter's target port rather than persisted in
/// the Published Interface, so changing native runtime support cannot leave a
/// stale second authority in Project files.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PublishedParameterAutomationCapability {
    /// The runtime evaluates the Published value independently for each frame.
    FrameSampled,
    /// The runtime needs one value for state history and cannot consume
    /// Timeline keyframes until the stated scheduling contract exists.
    ConstantOnly { reason: &'static str },
}

/// Reusable media-processing logic. Render boundaries are dedicated Output
/// Nodes in `graph`; `interface` contains only externally supplied controls
/// and host inputs, never a second output-routing source of truth.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ModuleDefinition {
    pub id: ModuleDefinitionId,
    pub name: String,
    pub sharing: ModuleDefinitionSharing,
    pub graph: ModuleGraph,
    pub interface: ModuleInterface,
    pub host_contract: ModuleHostContract,
    pub topology_revision: u64,
    pub interface_version: u64,
}

/// Derived ownership of one Module input port.
///
/// An externally-driven input cannot simultaneously accept an authored graph
/// connection. Module Output terminal inputs are deliberately not part of the
/// Published Interface, so they remain [`Self::Internal`] and connectable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModuleInputPortOwnership {
    Internal,
    Published,
    HostProtected,
}

impl ModuleInputPortOwnership {
    pub const fn is_externally_driven(self) -> bool {
        !matches!(self, Self::Internal)
    }

    pub const fn is_host_protected(self) -> bool {
        matches!(self, Self::HostProtected)
    }
}

impl ModuleDefinition {
    /// Creates the smallest valid media-processing Module: one stable Output
    /// boundary with Image and Sound inputs and no authored processing Nodes.
    /// UI surfaces and importers use this model constructor so the starter
    /// topology has one authority.
    pub fn new_image(
        name: impl Into<String>,
        sharing: ModuleDefinitionSharing,
    ) -> (Self, ModuleOutputId) {
        let output_id = ModuleOutputId::new();
        let mut output = Node::new_module_output("Output", output_id);
        output.ui_position = [360.0, 120.0];
        let output_node_id = output.id;
        (
            Self {
                id: ModuleDefinitionId::new(),
                name: name.into(),
                sharing,
                graph: ModuleGraph {
                    nodes: HashMap::from([(output_node_id, output)]),
                    connections: Vec::new(),
                },
                interface: ModuleInterface::default(),
                host_contract: ModuleHostContract::General,
                topology_revision: 1,
                interface_version: 1,
            },
            output_id,
        )
    }

    pub fn new_project_image(name: impl Into<String>) -> (Self, ModuleOutputId) {
        Self::new_image(
            name,
            ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err(format!("Module definition {} has no name", self.id));
        }
        if self.topology_revision == 0 || self.interface_version == 0 {
            return Err(format!(
                "Module definition {} has an invalid revision",
                self.id
            ));
        }
        self.sharing.validate()?;
        self.graph.validate()?;
        self.validate_outputs()?;
        self.interface.validate(&self.graph)?;
        self.validate_parameter_overrides(&HashMap::new())?;
        if let ModuleHostContract::Transition(contract) = &self.host_contract {
            contract.validate_definition(self)?;
        }
        Ok(())
    }

    /// Returns the render terminals derived directly from dedicated Output
    /// Nodes. There is no second persisted list to synchronize with topology.
    pub fn outputs(&self) -> impl Iterator<Item = ModuleOutput> + '_ {
        let mut outputs = self
            .graph
            .nodes
            .values()
            .filter_map(|node| {
                let NodeContent::ModuleOutput(output) = node.content() else {
                    return None;
                };
                Some(ModuleOutput {
                    id: output.id,
                    node_id: node.id,
                    name: node.name.clone(),
                })
            })
            .collect::<Vec<_>>();
        outputs.sort_by_key(|output| output.id);
        outputs.into_iter()
    }

    pub fn output(&self, output_id: ModuleOutputId) -> Option<ModuleOutput> {
        self.outputs().find(|output| output.id == output_id)
    }

    /// One authority for Node Editor connection and inline-control policy.
    /// Only Published Interface *input targets* are externally driven;
    /// Published signals and the dedicated Output terminal remain graph ports.
    pub fn input_port_ownership(&self, address: &ModulePortAddress) -> ModuleInputPortOwnership {
        if let Some(parameter) = self
            .interface
            .parameters
            .iter()
            .find(|parameter| parameter.target == *address)
        {
            return if self.host_contract.protects_parameter(parameter.id) {
                ModuleInputPortOwnership::HostProtected
            } else {
                ModuleInputPortOwnership::Published
            };
        }
        if let Some(input) = self
            .interface
            .media_inputs
            .iter()
            .find(|input| input.target == *address)
        {
            return if self.host_contract.protects_media_input(input.id) {
                ModuleInputPortOwnership::HostProtected
            } else {
                ModuleInputPortOwnership::Published
            };
        }
        if self
            .interface
            .actions
            .iter()
            .any(|action| action.target == *address)
        {
            return ModuleInputPortOwnership::Published;
        }
        ModuleInputPortOwnership::Internal
    }

    /// Whether the production Module runtime can consume a graph edge at this
    /// input. Published/host inputs and native constant-only inputs remain
    /// directly editable but never advertise a connect gesture.
    pub fn input_port_accepts_connection(&self, address: &ModulePortAddress) -> bool {
        if self
            .graph
            .port_definition(address, PortDirection::Input)
            .is_err()
            || self.input_port_ownership(address).is_externally_driven()
        {
            return false;
        }
        self.graph
            .nodes
            .get(&address.node_id)
            .and_then(native_node_descriptor_for_node)
            .and_then(|descriptor| descriptor.dynamic_input_disabled_reason(&address.port))
            .is_none()
    }

    /// Resolve the one model-side automation contract for a Published
    /// parameter. Plugin and general native inputs remain frame-sampled unless
    /// their canonical native descriptor explicitly narrows the capability.
    pub fn parameter_automation_capability(
        &self,
        parameter_id: PublishedParameterId,
    ) -> Result<PublishedParameterAutomationCapability, String> {
        let parameter = self
            .interface
            .parameters
            .iter()
            .find(|parameter| parameter.id == parameter_id)
            .ok_or_else(|| {
                format!(
                    "Module definition {} has no Published parameter {parameter_id}",
                    self.id
                )
            })?;
        self.graph
            .port_definition(&parameter.target, PortDirection::Input)
            .map_err(|error| {
                format!("Published parameter {parameter_id} has an invalid target: {error}")
            })?;
        let node = self
            .graph
            .nodes
            .get(&parameter.target.node_id)
            .ok_or_else(|| {
                format!(
                    "Published parameter {parameter_id} targets missing Node {}",
                    parameter.target.node_id
                )
            })?;
        Ok(native_node_descriptor_for_node(node).map_or(
            PublishedParameterAutomationCapability::FrameSampled,
            |descriptor| descriptor.input_automation_capability(&parameter.target.port),
        ))
    }

    /// Reject Timeline automation before a RenderPlan can observe a parameter
    /// whose runtime needs constant simulation history.
    pub fn require_parameter_automation(
        &self,
        parameter_id: PublishedParameterId,
    ) -> Result<(), String> {
        let parameter = self
            .interface
            .parameters
            .iter()
            .find(|parameter| parameter.id == parameter_id)
            .ok_or_else(|| {
                format!(
                    "Module definition {} has no Published parameter {parameter_id}",
                    self.id
                )
            })?;
        let capability = self.parameter_automation_capability(parameter_id)?;
        let PublishedParameterAutomationCapability::ConstantOnly { reason } = capability else {
            return Ok(());
        };
        Err(format!(
            "Published parameter '{}' ({parameter_id}) is constant-only and cannot use Timeline automation: {reason}",
            parameter.name
        ))
    }

    fn validate_outputs(&self) -> Result<(), String> {
        let outputs = self.outputs().collect::<Vec<_>>();
        if outputs.is_empty() {
            return Err(format!(
                "Module definition {} requires at least one dedicated Output Node",
                self.id
            ));
        }
        let mut ids = HashSet::with_capacity(outputs.len());
        for output in outputs {
            if !ids.insert(output.id) {
                return Err(format!("Module repeats Output identity {}", output.id));
            }
            require_interface_name(&output.name, "Module Output")?;
            let node = self
                .graph
                .nodes
                .get(&output.node_id)
                .ok_or_else(|| format!("Module Output {} has no Node", output.id))?;
            if !node.enabled || node.bypassed || node.blend_mode != BlendMode::Normal {
                return Err(format!(
                    "Module Output Node {} cannot be disabled, bypassed, or blended",
                    node.id
                ));
            }
            if node.properties().iter().next().is_some() {
                return Err(format!(
                    "Module Output Node {} cannot own authored properties",
                    node.id
                ));
            }
        }
        Ok(())
    }
}

/// Derived description of an input-only Module render terminal.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ModuleOutput {
    pub id: ModuleOutputId,
    pub node_id: uuid::Uuid,
    pub name: String,
}

impl ModuleOutput {
    pub fn target(&self, data_type: PortDataType) -> Option<ModulePortAddress> {
        module_output_port(data_type).map(|port| ModulePortAddress {
            node_id: self.node_id,
            port: port.key.to_string(),
        })
    }

    pub fn supports(&self, data_type: PortDataType) -> bool {
        module_output_port(data_type).is_some()
    }

    pub fn targets(&self) -> impl Iterator<Item = (PortDataType, ModulePortAddress)> + '_ {
        MODULE_OUTPUT_PORTS.iter().map(|port| {
            (
                port.data_type,
                ModulePortAddress {
                    node_id: self.node_id,
                    port: port.key.to_string(),
                },
            )
        })
    }
}

#[derive(Clone, Copy)]
struct ModuleOutputPort {
    key: &'static str,
    label: &'static str,
    data_type: PortDataType,
}

const MODULE_OUTPUT_PORTS: [ModuleOutputPort; 2] = [
    ModuleOutputPort {
        key: IMAGE_INPUT_PORT,
        label: "Image",
        data_type: PortDataType::Image,
    },
    ModuleOutputPort {
        key: SOUND_INPUT_PORT,
        label: "Audio",
        data_type: PortDataType::Audio,
    },
];

fn module_output_port(data_type: PortDataType) -> Option<ModuleOutputPort> {
    MODULE_OUTPUT_PORTS
        .iter()
        .find(|port| port.data_type == data_type)
        .copied()
}

/// Persisted edit-sharing policy. A private definition belongs to exactly one
/// instance. A reusable definition is immutable through ordinary instance
/// editing, even while it currently has only one placement.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(
    tag = "kind",
    content = "origin",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ModuleDefinitionSharing {
    Private,
    /// Project-local sharing created by split/duplicate. It is not exposed as
    /// a reusable Asset and ordinary edits still use copy-on-write.
    SharedLocal,
    ReusableTemplate(ModuleTemplateOrigin),
}

impl ModuleDefinitionSharing {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Private
            | Self::SharedLocal
            | Self::ReusableTemplate(ModuleTemplateOrigin::Project) => Ok(()),
            Self::ReusableTemplate(ModuleTemplateOrigin::External { locator, version }) => {
                if locator.trim().is_empty() || version.trim().is_empty() {
                    Err("Reusable Module origin is incomplete".to_string())
                } else {
                    Ok(())
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModuleTemplateOrigin {
    Project,
    External { locator: String, version: String },
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct ModuleGraph {
    pub nodes: HashMap<uuid::Uuid, Node>,
    pub connections: Vec<ModuleConnection>,
}

impl ModuleGraph {
    pub fn validate(&self) -> Result<(), String> {
        let mut indegree = self
            .nodes
            .keys()
            .copied()
            .map(|node_id| (node_id, 0_usize))
            .collect::<HashMap<_, _>>();
        let mut outgoing: HashMap<uuid::Uuid, Vec<uuid::Uuid>> = HashMap::new();
        let mut connection_ids = HashSet::new();
        let mut contracts = HashMap::with_capacity(self.nodes.len());
        for (key, node) in &self.nodes {
            if *key != node.id {
                return Err(format!(
                    "Module Node map key {key} does not match {}",
                    node.id
                ));
            }
            if matches!(node.content(), NodeContent::CompositionInstance(_)) {
                return Err("Module graph cannot contain a Composition instance".to_string());
            }
            if matches!(node.content(), NodeContent::NativeOperation(_))
                && let Some(descriptor) = native_node_descriptor_for_node(node)
            {
                descriptor.validate_native_properties(node.properties())?;
            }
            contracts.insert(*key, ModuleNodePortContract::resolve(node)?);
        }
        let mut target_orders: HashMap<ModulePortAddress, Vec<i64>> = HashMap::new();
        for connection in &self.connections {
            if !connection_ids.insert(connection.id) {
                return Err(format!("Module graph repeats connection {}", connection.id));
            }
            if !self.nodes.contains_key(&connection.from.node_id) {
                return Err(format!(
                    "Module connection {} has a missing source",
                    connection.id
                ));
            }
            if !self.nodes.contains_key(&connection.to.node_id) {
                return Err(format!(
                    "Module connection {} has a missing target",
                    connection.id
                ));
            }
            if connection.from.node_id == connection.to.node_id {
                return Err(format!(
                    "Module connection {} is a self-cycle",
                    connection.id
                ));
            }
            if connection.order < 0 {
                return Err(format!(
                    "Module connection {} has a negative input order",
                    connection.id
                ));
            }
            let source = contracts
                .get(&connection.from.node_id)
                .ok_or_else(|| format!("Module connection {} has a missing source", connection.id))?
                .require(&connection.from.port, PortDirection::Output)?;
            let target = contracts
                .get(&connection.to.node_id)
                .ok_or_else(|| format!("Module connection {} has a missing target", connection.id))?
                .require(&connection.to.port, PortDirection::Input)?;
            if let Some(reason) = self
                .nodes
                .get(&connection.to.node_id)
                .and_then(native_node_descriptor_for_node)
                .and_then(|descriptor| {
                    descriptor.dynamic_input_disabled_reason(&connection.to.port)
                })
            {
                return Err(format!(
                    "Module connection {} cannot drive constant-only input {}:{}: {reason}",
                    connection.id, connection.to.node_id, connection.to.port
                ));
            }
            if !target.data_type.accepts(source.data_type) {
                return Err(format!(
                    "Module connection {} cannot connect {:?} to {:?}",
                    connection.id, source.data_type, target.data_type
                ));
            }
            if connection.blend_mode != BlendMode::Normal
                && (source.data_type != PortDataType::Image
                    || connection.to.port != MERGE_IMAGES_PORT
                    || !matches!(
                        self.nodes.get(&connection.to.node_id).map(Node::content),
                        Some(NodeContent::Merge)
                    ))
            {
                return Err(format!(
                    "Module connection {} can use a non-Normal Blend only on an Image Merge input",
                    connection.id
                ));
            }
            target_orders
                .entry(connection.to.clone())
                .or_default()
                .push(connection.order);
            *indegree.get_mut(&connection.to.node_id).ok_or_else(|| {
                format!("Module connection {} has a missing target", connection.id)
            })? += 1;
            outgoing
                .entry(connection.from.node_id)
                .or_default()
                .push(connection.to.node_id);
        }
        for (target, mut orders) in target_orders {
            let port = contracts
                .get(&target.node_id)
                .ok_or_else(|| format!("Module input {} has a missing Node", target.node_id))?
                .require(&target.port, PortDirection::Input)?;
            orders.sort_unstable();
            match port.multiplicity {
                PortMultiplicity::Single if orders.as_slice() != [0] => {
                    return Err(format!(
                        "Single Module input {}:{} must have exactly one connection at order 0",
                        target.node_id, target.port
                    ));
                }
                PortMultiplicity::Variadic
                    if orders
                        .iter()
                        .enumerate()
                        .any(|(expected, actual)| *actual != expected as i64) =>
                {
                    return Err(format!(
                        "Variadic Module input {}:{} must use contiguous orders from zero",
                        target.node_id, target.port
                    ));
                }
                _ => {}
            }
        }
        let mut ready = indegree
            .iter()
            .filter_map(|(node_id, degree)| (*degree == 0).then_some(*node_id))
            .collect::<Vec<_>>();
        let mut visited = 0;
        while let Some(node_id) = ready.pop() {
            visited += 1;
            for target in outgoing.get(&node_id).into_iter().flatten() {
                let degree = indegree
                    .get_mut(target)
                    .ok_or_else(|| "Module graph traversal reached a missing target".to_string())?;
                *degree -= 1;
                if *degree == 0 {
                    ready.push(*target);
                }
            }
        }
        if visited != self.nodes.len() {
            return Err("Module graph contains a cycle".to_string());
        }
        Ok(())
    }

    pub fn port_definition(
        &self,
        address: &ModulePortAddress,
        direction: PortDirection,
    ) -> Result<PortDefinition, String> {
        let node = self
            .nodes
            .get(&address.node_id)
            .ok_or_else(|| format!("Module port refers to missing Node {}", address.node_id))?;
        ModuleNodePortContract::resolve(node)?
            .require(&address.port, direction)
            .cloned()
    }

    fn has_connection_to(&self, address: &ModulePortAddress) -> bool {
        self.connections
            .iter()
            .any(|connection| connection.to == *address)
    }
}

/// Canonical typed port contract for a Node persisted inside a Module.
/// Plugin contracts are persisted with the Node; first-party contracts are
/// resolved through the native catalog. Unknown native identities fail closed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ModuleNodePortContract {
    pub ports: Vec<PortDefinition>,
}

impl ModuleNodePortContract {
    pub fn resolve(node: &Node) -> Result<Self, String> {
        let ports = match node.content() {
            NodeContent::ModuleOutput(_) => MODULE_OUTPUT_PORTS
                .iter()
                .map(|port| PortDefinition::input(port.key, port.label, port.data_type))
                .collect(),
            NodeContent::PluginOperation(operation) => operation.declared_ports.clone(),
            NodeContent::Media(_) => vec![
                PortDefinition::input(TIME_PORT, "Time", PortDataType::Number),
                PortDefinition::output(
                    IMAGE_OUTPUT_PORT,
                    "Image",
                    PortDataType::Image,
                    PortSide::Right,
                    PortExposure::Graph,
                ),
                PortDefinition::output(
                    AUDIO_OUTPUT_PORT,
                    "Audio",
                    PortDataType::Audio,
                    PortSide::Right,
                    PortExposure::Graph,
                ),
            ],
            NodeContent::CompositionInstance(_) => {
                return Err("Module graph cannot contain a Composition instance".to_string());
            }
            _ => native_node_descriptor_for_node(node)
                .ok_or_else(|| {
                    format!(
                        "Module Node {} has no persisted or canonical port contract",
                        node.id
                    )
                })?
                .ports()
                .to_vec(),
        };
        let mut addresses = HashSet::new();
        for port in &ports {
            if port.key.trim().is_empty() {
                return Err(format!(
                    "Module Node {} declares an empty port key",
                    node.id
                ));
            }
            if !addresses.insert((port.direction, port.key.as_str())) {
                return Err(format!(
                    "Module Node {} repeats {:?} port '{}'",
                    node.id, port.direction, port.key
                ));
            }
            if port.direction == PortDirection::Output
                && port.multiplicity != PortMultiplicity::Single
            {
                return Err(format!(
                    "Module Node {} output '{}' cannot be variadic",
                    node.id, port.key
                ));
            }
        }
        Ok(Self { ports })
    }

    pub fn require(&self, key: &str, direction: PortDirection) -> Result<&PortDefinition, String> {
        self.ports
            .iter()
            .find(|port| port.key == key && port.direction == direction)
            .ok_or_else(|| format!("Module Node has no {direction:?} port '{key}'"))
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ModuleConnection {
    pub id: ModuleConnectionId,
    pub from: ModulePortAddress,
    pub to: ModulePortAddress,
    pub order: i64,
    /// Per-edge compositing is part of Module topology. This permits one
    /// source to feed different Merge inputs with independent modes.
    pub blend_mode: BlendMode,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(deny_unknown_fields)]
pub struct ModulePortAddress {
    pub node_id: uuid::Uuid,
    pub port: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct ModuleInterface {
    pub parameters: Vec<PublishedParameter>,
    pub media_inputs: Vec<PublishedMediaInput>,
    pub signals: Vec<PublishedSignal>,
    pub actions: Vec<PublishedAction>,
}

impl ModuleInterface {
    fn validate(&self, graph: &ModuleGraph) -> Result<(), String> {
        let mut ids = HashSet::new();
        let mut published_targets = HashSet::new();
        for parameter in &self.parameters {
            require_interface_id(&mut ids, parameter.id.as_uuid())?;
            require_interface_name(&parameter.name, "Published parameter")?;
            let target = graph.port_definition(&parameter.target, PortDirection::Input)?;
            require_unambiguous_published_target(graph, &mut published_targets, &parameter.target)?;
            if !authored_parameter_value_is_compatible(
                parameter.data_type,
                &parameter.default_value,
            ) {
                return Err(format!(
                    "Published parameter {} has a mismatched default",
                    parameter.id
                ));
            }
            if !target.data_type.accepts(parameter.data_type) {
                return Err(format!(
                    "Published parameter {} type does not match its target",
                    parameter.id
                ));
            }
        }
        let mut primary_media_inputs = 0;
        for input in &self.media_inputs {
            require_interface_id(&mut ids, input.id.as_uuid())?;
            require_interface_name(&input.name, "Published media input")?;
            let target = graph.port_definition(&input.target, PortDirection::Input)?;
            require_unambiguous_published_target(graph, &mut published_targets, &input.target)?;
            if !is_media_type(input.data_type) {
                return Err(format!("Published media input {} is not media", input.id));
            }
            if !target.data_type.accepts(input.data_type) {
                return Err(format!(
                    "Published media input {} type does not match its target",
                    input.id
                ));
            }
            primary_media_inputs += usize::from(input.primary);
        }
        if primary_media_inputs > 1 {
            return Err("A Module may publish at most one primary media input".to_string());
        }
        for signal in &self.signals {
            require_interface_id(&mut ids, signal.id.as_uuid())?;
            require_interface_name(&signal.name, "Published signal")?;
            let source = graph.port_definition(&signal.source, PortDirection::Output)?;
            if !signal.data_type.accepts(source.data_type) {
                return Err(format!(
                    "Published signal {} type does not match its source",
                    signal.id
                ));
            }
        }
        for action in &self.actions {
            require_interface_id(&mut ids, action.id.as_uuid())?;
            require_interface_name(&action.name, "Published action")?;
            graph.port_definition(&action.target, PortDirection::Input)?;
            require_unambiguous_published_target(graph, &mut published_targets, &action.target)?;
        }
        Ok(())
    }
}

fn require_interface_name(name: &str, label: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        Err(format!("{label} has no name"))
    } else {
        Ok(())
    }
}

fn require_unambiguous_published_target(
    graph: &ModuleGraph,
    published_targets: &mut HashSet<ModulePortAddress>,
    target: &ModulePortAddress,
) -> Result<(), String> {
    if graph.has_connection_to(target) {
        return Err(format!(
            "Published target {}:{} is also driven by a Module connection",
            target.node_id, target.port
        ));
    }
    if !published_targets.insert(target.clone()) {
        return Err(format!(
            "Module publishes target {}:{} more than once",
            target.node_id, target.port
        ));
    }
    Ok(())
}

fn require_interface_id(ids: &mut HashSet<uuid::Uuid>, id: uuid::Uuid) -> Result<(), String> {
    if ids.insert(id) {
        Ok(())
    } else {
        Err("A Module has duplicate Published Interface IDs".to_string())
    }
}

fn is_media_type(data_type: PortDataType) -> bool {
    matches!(data_type, PortDataType::Image | PortDataType::Audio)
}

pub(crate) fn property_value_type(value: &PropertyValue) -> PortDataType {
    match value {
        PropertyValue::Integer(_) => PortDataType::Integer,
        PropertyValue::Number(_) => PortDataType::Number,
        PropertyValue::String(_) => PortDataType::String,
        PropertyValue::Boolean(_) => PortDataType::Boolean,
        PropertyValue::Vec2(_) => PortDataType::Vec2,
        PropertyValue::Vec3(_) => PortDataType::Vec3,
        PropertyValue::Vec4(_) => PortDataType::Vec4,
        PropertyValue::ColorValue(_) | PropertyValue::Color(_) => PortDataType::Color,
        PropertyValue::Path(_) => PortDataType::Path,
        PropertyValue::Array(_) => PortDataType::List,
        PropertyValue::Map(_) | PropertyValue::OpaqueJson(_) => PortDataType::Any,
    }
}

/// Strict persisted-value compatibility for Published parameters and their
/// Timeline automation. `Any` remains dynamic for graph edge typing, but a
/// concrete authored Map/OpaqueJson must not masquerade as Number, Color, or
/// another typed public control.
pub(crate) fn authored_parameter_value_is_compatible(
    target: PortDataType,
    value: &PropertyValue,
) -> bool {
    let source = property_value_type(value);
    if source == PortDataType::Any {
        target == PortDataType::Any
    } else {
        target.accepts(source)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct PublishedParameter {
    pub id: PublishedParameterId,
    pub name: String,
    pub data_type: PortDataType,
    pub default_value: PropertyValue,
    pub target: ModulePortAddress,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct PublishedMediaInput {
    pub id: PublishedMediaInputId,
    pub name: String,
    pub data_type: PortDataType,
    pub target: ModulePortAddress,
    pub required: bool,
    pub primary: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct PublishedSignal {
    pub id: PublishedSignalId,
    pub name: String,
    pub data_type: PortDataType,
    pub source: ModulePortAddress,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct PublishedAction {
    pub id: PublishedActionId,
    pub name: String,
    pub target: ModulePortAddress,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ModuleInstance {
    pub id: ModuleInstanceId,
    pub definition_id: ModuleDefinitionId,
    pub parameter_overrides: HashMap<PublishedParameterId, PropertyValue>,
}
