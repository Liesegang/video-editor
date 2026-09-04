//! First-party operations over canonical [`PathValue`] graph data.
//!
//! These operations consume and produce Project-authoritative path values.
//! They are deliberately separate from Shape Path Effects, which annotate a
//! transient render Shape rather than computing a reusable Path value.

use crate::model::property::PropertyDefinition;
use serde::{Deserialize, Serialize};

/// Stable executable identity for native canonical-Path operations.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathOperationContent {
    /// Boolean union of every canonical Path in one ordered `List<Path>`.
    Union,
}

impl PathOperationContent {
    pub const ALL: [Self; 1] = [Self::Union];

    pub const fn catalog_id(self) -> &'static str {
        match self {
            Self::Union => "native.path.union",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Union => "Union Path",
        }
    }

    pub const fn property_definitions(self) -> &'static [PropertyDefinition] {
        match self {
            Self::Union => &[],
        }
    }
}
