//! Runtime evaluation of reusable canonical Path data operations.
//!
//! This path is intentionally independent from Shape Path Effects: a native
//! operation returns `PropertyValue::Path`, so its result can be listed,
//! branched, saved, or consumed by later geometry nodes before rasterization.

use std::collections::HashSet;

use skia_safe::{OpBuilder, Path, PathOp};
use uuid::Uuid;

use super::evaluator::{FrameEvaluator, cycle_error, missing_error};
use crate::core::rendering::path_geometry::{from_skia_boolean_path, to_skia_path};
use crate::model::path::{FillRule, PathValue};
use crate::model::project::connection::{PATH_OUTPUT_PORT, PATHS_INPUT_PORT};
use crate::model::project::{EvalOutput, EvalResult, PortAddress, PortOwner};
use crate::model::property::PropertyValue;
use crate::model::{Node, PathOperationContent};

impl FrameEvaluator<'_> {
    pub(super) fn evaluate_path_operation_output(
        &self,
        node_id: Uuid,
        operation: PathOperationContent,
        output_port: &str,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let owner = PortOwner::Node(node_id);
        let node = self
            .project
            .get_node(node_id)
            .ok_or_else(|| missing_error(owner))?;
        if !node.enabled || node.bypassed || output_port != PATH_OUTPUT_PORT {
            return Ok(EvalOutput::NoOutput);
        }
        match self.scope_for_owner(owner, global_time, path)? {
            EvalOutput::Produced(_) => {}
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        }
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        let result = match operation {
            PathOperationContent::Union => self.evaluate_union_path(node, global_time, path),
        };
        path.remove(&owner);
        result
    }

    fn evaluate_union_path(
        &self,
        node: &Node,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let target = PortAddress::new(PortOwner::Node(node.id), PATHS_INPUT_PORT);
        let connection = match self.single_connection_to(&target)? {
            EvalOutput::Produced(connection) => connection,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let values = match self.resolve_metadata_value(&connection.from, global_time, path)? {
            EvalOutput::Produced(PropertyValue::Array(values)) => values,
            EvalOutput::Produced(_) | EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        if values.is_empty() {
            return Ok(EvalOutput::Produced(PropertyValue::Path(PathValue::empty(
                FillRule::NonZero,
            ))));
        }

        let mut builder = OpBuilder::default();
        for value in values {
            let PropertyValue::Path(value) = value else {
                return Ok(EvalOutput::NoOutput);
            };
            let backend = match to_skia_path(&value) {
                Ok(backend) => backend,
                Err(error) => {
                    log::warn!(
                        "Union Path input cannot cross the native Path boundary: {error}; producing NoOutput"
                    );
                    return Ok(EvalOutput::NoOutput);
                }
            };
            builder.add(&backend, PathOp::Union);
        }
        Ok(finalize_boolean_path_result(builder.resolve()))
    }
}

/// Normalize one backend Boolean operation result at the explicit
/// Skia-to-Project boundary. Keeping this step separate makes backend failure
/// semantics testable: native failure is absence, never an unchanged input or
/// another plausible-but-wrong fallback Path.
pub(super) fn finalize_boolean_path_result(result: Option<Path>) -> EvalOutput<PropertyValue> {
    let Some(result) = result else {
        log::warn!("Union Path backend failed to resolve its inputs; producing NoOutput");
        return EvalOutput::NoOutput;
    };
    match from_skia_boolean_path(&result) {
        Ok(result) => EvalOutput::Produced(PropertyValue::Path(result)),
        Err(error) => {
            log::warn!(
                "Union Path result cannot cross the canonical Path boundary: {error}; producing NoOutput"
            );
            EvalOutput::NoOutput
        }
    }
}
