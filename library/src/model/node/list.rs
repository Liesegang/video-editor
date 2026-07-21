//! Native heterogeneous List operations.
//!
//! Authored topology and order stay in the canonical Project connections.
//! Runtime arrays reuse `PropertyValue::Array`, so this module introduces no
//! intermediate graph model or persisted side table.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::model::project::connection::LIST_INDEX_INPUT_PORT;
use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};

static GET_ITEM_PROPERTY_DEFINITIONS: LazyLock<[PropertyDefinition; 1]> = LazyLock::new(|| {
    [PropertyDefinition::new(
        LIST_INDEX_INPUT_PORT,
        PropertyUiType::Integer {
            min: 0,
            max: i64::MAX,
            suffix: String::new(),
            min_hard_limit: true,
            max_hard_limit: false,
        },
        "Index",
        PropertyValue::Integer(0),
    )]
});

/// Stable persisted identity for first-party List operations.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListContent {
    Make,
    GetItem,
    Length,
}

impl ListContent {
    pub const ALL: [Self; 3] = [Self::Make, Self::GetItem, Self::Length];

    pub const fn catalog_id(self) -> &'static str {
        match self {
            Self::Make => "native.list.make",
            Self::GetItem => "native.list.get-item",
            Self::Length => "native.list.length",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Make => "Make List",
            Self::GetItem => "Get List Item",
            Self::Length => "List Length",
        }
    }

    /// Inspector metadata is authoritative here; the Node constructor uses
    /// the same definitions to materialize every required property.
    pub fn property_definitions(self) -> &'static [PropertyDefinition] {
        match self {
            Self::GetItem => GET_ITEM_PROPERTY_DEFINITIONS.as_slice(),
            Self::Make | Self::Length => &[],
        }
    }
}
