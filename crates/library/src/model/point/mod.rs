//! Authored Point attribute schema and its bounded GPU column layout.
//!
//! A Point collection remains a runtime value: this module persists only the
//! typed attribute definitions that are authored by Nodes. GPU handles,
//! compiled programs, and per-Point arrays deliberately stay outside the
//! Project model.

mod layout;
mod schema;

pub use layout::{
    POINT_MAX_CAPACITY, POINT_MAX_COLUMN_BYTES, PointAttributeColumnLayout,
    PointAttributeGpuDefault, PointColumnLayout,
};
pub use schema::{
    POINT_ATTRIBUTE_NAME_MAX_BYTES, POINT_MAX_ATTRIBUTE_COUNT, PointAttributeDefinition,
    PointAttributeElementType, PointAttributeId, PointAttributeSchema, PointId, PointProducerId,
};

#[cfg(test)]
mod tests;
