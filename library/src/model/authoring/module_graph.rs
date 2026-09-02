use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::node::{Node, NodeContent};
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
    pub graph: ModuleGraph,
    pub published_parameters: Vec<PublishedParameter>,
    pub published_signals: Vec<PublishedSignal>,
    pub published_actions: Vec<PublishedAction>,
    pub version: u64,
}

impl ModuleDefinition {
    pub fn validate(&self) -> Result<(), String> {
        for node in self.graph.nodes.values() {
            if matches!(node.content(), NodeContent::CompositionInstance(_)) {
                return Err("A Module graph cannot contain a Composition instance".to_string());
            }
        }
        self.graph.validate()?;
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
