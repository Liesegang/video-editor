//! Authored Point attribute schema and its bounded GPU column layout.
//!
//! A Point collection remains a runtime value: this module persists only the
//! typed attribute definitions that are authored by Nodes. GPU handles,
//! compiled programs, and per-Point arrays deliberately stay outside the
//! Project model.

mod layout;
mod program;
mod schema;

pub use crate::model::numeric::NumericBinaryOperation;
pub use program::{
    POINT_MAX_INSTRUCTIONS, POINT_MAX_RAMP_STOPS, POINT_MAX_RAMPS, PointInstruction,
    PointRenderProgram,
};

pub use layout::{
    POINT_MAX_CAPACITY, POINT_MAX_COLUMN_BYTES, PointAttributeColumnLayout,
    PointAttributeGpuDefault, PointColumnLayout,
};
pub use schema::{
    POINT_ATTRIBUTE_NAME_MAX_BYTES, POINT_MAX_ATTRIBUTE_COUNT, PointAttributeDefinition,
    PointAttributeElementType, PointAttributeId, PointAttributeSchema, PointId, PointProducerId,
};

#[cfg(test)]
mod program_tests;
#[cfg(test)]
mod tests;
