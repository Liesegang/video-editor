use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::node::Node;
use crate::model::project::connection::PortDataType;
use crate::model::project::property::PropertyValue;

use super::{
    ModuleConnectionId, ModuleDefinitionId, ModuleInstanceId, PublishedActionId,
    PublishedParameterId, PublishedSignalId,
};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ModuleDefinition {
    pub id: ModuleDefinitionId,
    pub name: String,
    pub role: ModuleRole,
    pub graph: ModuleGraph,
    /// Explicit result of this reusable processing graph. Nodes that cannot
    /// reach this output remain editable but are not part of the RenderPlan.
    pub output_node_id: Option<uuid::Uuid>,
    pub published_parameters: Vec<PublishedParameter>,
    pub published_signals: Vec<PublishedSignal>,
    pub published_actions: Vec<PublishedAction>,
    pub version: u64,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ModuleRole {
    Effect,
    Generator,
    Behavior,
    Analyzer,
}

impl ModuleDefinition {
    pub fn validate(&self) -> Result<(), String> {
        self.graph.validate()?;
        match self.output_node_id {
            Some(node_id) if !self.graph.nodes.contains_key(&node_id) => {
                return Err(format!("Module output refers to missing Node {node_id}"));
            }
            None if !self.graph.nodes.is_empty() => {
                return Err("A non-empty Module graph must select an output Node".to_string());
            }
            _ => {}
        }
        let mut interface_ids = std::collections::HashSet::new();
        for parameter in &self.published_parameters {
            if !interface_ids.insert(parameter.id.as_uuid()) {
                return Err("A Module has duplicate Published Interface IDs".to_string());
            }
            self.graph.require_address(&parameter.target)?;
        }
        for signal in &self.published_signals {
            if !interface_ids.insert(signal.id.as_uuid()) {
                return Err("A Module has duplicate Published Interface IDs".to_string());
            }
            self.graph.require_address(&signal.source)?;
        }
        for action in &self.published_actions {
            if !interface_ids.insert(action.id.as_uuid()) {
                return Err("A Module has duplicate Published Interface IDs".to_string());
            }
            self.graph.require_address(&action.target)?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct ModuleGraph {
    pub nodes: HashMap<uuid::Uuid, Node>,
    pub connections: Vec<ModuleConnection>,
}

impl ModuleGraph {
    pub fn validate(&self) -> Result<(), String> {
        let mut indegree: HashMap<_, usize> = self
            .nodes
            .keys()
            .copied()
            .map(|node_id| (node_id, 0))
            .collect();
        let mut outgoing: HashMap<_, Vec<_>> = HashMap::new();
        for connection in &self.connections {
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
            *indegree.get_mut(&connection.to.node_id).expect("checked") += 1;
            outgoing
                .entry(connection.from.node_id)
                .or_default()
                .push(connection.to.node_id);
        }
        let mut ready: Vec<_> = indegree
            .iter()
            .filter_map(|(node_id, degree)| (*degree == 0).then_some(*node_id))
            .collect();
        let mut visited = 0;
        while let Some(node_id) = ready.pop() {
            visited += 1;
            for target in outgoing.get(&node_id).into_iter().flatten() {
                let degree = indegree.get_mut(target).expect("checked");
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

    fn require_address(&self, address: &ModulePortAddress) -> Result<(), String> {
        if self.nodes.contains_key(&address.node_id) {
            Ok(())
        } else {
            Err(format!(
                "Published Interface refers to missing Node {}",
                address.node_id
            ))
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct ModuleConnection {
    pub id: ModuleConnectionId,
    pub from: ModulePortAddress,
    pub to: ModulePortAddress,
    pub order: i64,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(deny_unknown_fields)]
pub struct ModulePortAddress {
    pub node_id: uuid::Uuid,
    pub port: String,
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
