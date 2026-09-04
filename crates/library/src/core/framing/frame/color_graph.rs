//! Lossless runtime evaluation for canonical Color metadata Nodes.

use std::collections::HashSet;

use uuid::Uuid;

use super::evaluator::{FrameEvaluator, cycle_error, missing_error};
use super::scope::EvaluationScope;
use crate::error::LibraryError;
use crate::model::project::{EvalOutput, EvalResult, PortAddress, PortOwner};
use crate::model::property::{ColorSpaceRef, ColorValue, PropertyValue};
use crate::model::{
    COLOR_ALPHA_PORT, COLOR_BLUE_PORT, COLOR_GREEN_PORT, COLOR_MIX_FACTOR_PORT,
    COLOR_MIX_LEFT_PORT, COLOR_MIX_RIGHT_PORT, COLOR_RED_PORT, COLOR_SPACE_PORT,
    COLOR_TARGET_SPACE_PORT, COLOR_VALUE_PORT, ColorContent, Node, NodeContent,
};
use crate::plugin::ResolvedNodeInputs;

impl FrameEvaluator<'_> {
    pub(super) fn evaluate_color_node_output(
        &self,
        node_id: Uuid,
        output_port: &str,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let owner = PortOwner::Node(node_id);
        let node = self
            .project
            .get_node(node_id)
            .ok_or_else(|| missing_error(owner))?;
        let NodeContent::Color(operation) = node.content() else {
            return Ok(EvalOutput::NoOutput);
        };
        if !node.enabled || !supports_output(*operation, output_port) {
            return Ok(EvalOutput::NoOutput);
        }
        // The shared metadata dispatcher handles a valid bypass before this
        // operation-specific evaluator and pulls the canonical Color input
        // connection. A direct call with a forward-loaded invalid bypass flag
        // must stay harmless.
        if node.bypassed {
            return Ok(EvalOutput::NoOutput);
        }
        let scope = match self.scope_for_owner(owner, global_time, path)? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        let result = match operation {
            ColorContent::Compose => self.evaluate_compose_color(node, scope, global_time, path),
            ColorContent::Split => {
                self.evaluate_split_color(node, output_port, scope, global_time, path)
            }
            ColorContent::Mix => self.evaluate_mix_color(node, scope, global_time, path),
            ColorContent::ConvertSpace => {
                self.evaluate_convert_color_space(node, scope, global_time, path)
            }
        };
        path.remove(&owner);
        result
    }

    fn evaluate_compose_color(
        &self,
        node: &Node,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let space = match self.resolve_color_property_input(
            node,
            COLOR_SPACE_PORT,
            scope,
            global_time,
            path,
        )? {
            EvalOutput::Produced(PropertyValue::String(value)) => value,
            EvalOutput::Produced(_) | EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let Some(r) = self.resolve_color_number(node, COLOR_RED_PORT, scope, global_time, path)?
        else {
            return Ok(EvalOutput::NoOutput);
        };
        let Some(g) =
            self.resolve_color_number(node, COLOR_GREEN_PORT, scope, global_time, path)?
        else {
            return Ok(EvalOutput::NoOutput);
        };
        let Some(b) = self.resolve_color_number(node, COLOR_BLUE_PORT, scope, global_time, path)?
        else {
            return Ok(EvalOutput::NoOutput);
        };
        let Some(a) =
            self.resolve_color_number(node, COLOR_ALPHA_PORT, scope, global_time, path)?
        else {
            return Ok(EvalOutput::NoOutput);
        };
        let Ok(space) = ColorSpaceRef::new(space) else {
            return Ok(EvalOutput::NoOutput);
        };
        Ok(ColorValue::new(space, [r, g, b, a])
            .map(PropertyValue::ColorValue)
            .map_or(EvalOutput::NoOutput, EvalOutput::Produced))
    }

    fn evaluate_split_color(
        &self,
        node: &Node,
        output_port: &str,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let color = match self.resolve_color(node, COLOR_VALUE_PORT, scope, global_time, path)? {
            EvalOutput::Produced(color) => color,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let [r, g, b, a] = color.rgba();
        let value = match output_port {
            COLOR_SPACE_PORT => PropertyValue::String(color.color_space().to_string()),
            COLOR_RED_PORT => PropertyValue::from(r),
            COLOR_GREEN_PORT => PropertyValue::from(g),
            COLOR_BLUE_PORT => PropertyValue::from(b),
            COLOR_ALPHA_PORT => PropertyValue::from(a),
            _ => return Ok(EvalOutput::NoOutput),
        };
        Ok(EvalOutput::Produced(value))
    }

    fn evaluate_mix_color(
        &self,
        node: &Node,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let left = match self.resolve_color(node, COLOR_MIX_LEFT_PORT, scope, global_time, path)? {
            EvalOutput::Produced(color) => color,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let right =
            match self.resolve_color(node, COLOR_MIX_RIGHT_PORT, scope, global_time, path)? {
                EvalOutput::Produced(color) => color,
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            };
        let Some(factor) =
            self.resolve_color_number(node, COLOR_MIX_FACTOR_PORT, scope, global_time, path)?
        else {
            return Ok(EvalOutput::NoOutput);
        };
        if !(0.0..=1.0).contains(&factor) {
            return Ok(EvalOutput::NoOutput);
        }
        Ok(left
            .interpolate_same_space(&right, factor)
            .map(PropertyValue::ColorValue)
            .map_or(EvalOutput::NoOutput, EvalOutput::Produced))
    }

    fn evaluate_convert_color_space(
        &self,
        node: &Node,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let source = match self.resolve_color(node, COLOR_VALUE_PORT, scope, global_time, path)? {
            EvalOutput::Produced(color) => color,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let target = match self.resolve_color_property_input(
            node,
            COLOR_TARGET_SPACE_PORT,
            scope,
            global_time,
            path,
        )? {
            EvalOutput::Produced(PropertyValue::String(target)) => target,
            EvalOutput::Produced(_) | EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let Ok(target) = ColorSpaceRef::new(target) else {
            return Ok(EvalOutput::NoOutput);
        };
        match crate::color_management::transform_color(&source, &target) {
            Ok(color) => Ok(EvalOutput::Produced(PropertyValue::ColorValue(color))),
            Err(error) => {
                log::debug!(
                    "Color Space conversion on Node '{}' produced no output: {error}",
                    node.id
                );
                Ok(EvalOutput::NoOutput)
            }
        }
    }

    fn resolve_color_number(
        &self,
        node: &Node,
        port: &str,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> Result<Option<f64>, LibraryError> {
        let value = match self.resolve_color_property_input(node, port, scope, global_time, path)? {
            EvalOutput::Produced(value) => value,
            EvalOutput::NoOutput => return Ok(None),
        };
        let value = match value {
            PropertyValue::Number(value) => value.into_inner(),
            PropertyValue::Integer(value) => value as f64,
            _ => return Ok(None),
        };
        Ok(value.is_finite().then_some(value))
    }

    fn resolve_color(
        &self,
        node: &Node,
        port: &str,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<ColorValue> {
        match self.resolve_color_property_input(node, port, scope, global_time, path)? {
            EvalOutput::Produced(PropertyValue::ColorValue(color)) => {
                Ok(EvalOutput::Produced(color))
            }
            EvalOutput::Produced(PropertyValue::Color(color)) => Ok(EvalOutput::Produced(
                ColorValue::from_straight_srgba8(&color),
            )),
            EvalOutput::Produced(_) | EvalOutput::NoOutput => Ok(EvalOutput::NoOutput),
        }
    }

    fn resolve_color_property_input(
        &self,
        node: &Node,
        port: &str,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let target = PortAddress::new(PortOwner::Node(node.id), port);
        match self.single_connection_to(&target)? {
            EvalOutput::Produced(connection) => {
                self.resolve_metadata_value(&connection.from, global_time, path)
            }
            EvalOutput::NoOutput => {
                let Some(property) = node.properties().get(port) else {
                    return Ok(EvalOutput::NoOutput);
                };
                let composition = self
                    .composition_for_owner(PortOwner::Node(node.id))
                    .ok_or_else(|| missing_error(PortOwner::Node(node.id)))?;
                let inputs = ResolvedNodeInputs::from_metadata(scope.as_inputs());
                match self
                    .context(composition, Some(&inputs))
                    .evaluate_property_value(property, node.properties(), scope.time)
                {
                    Ok(value) => Ok(EvalOutput::Produced(value)),
                    Err(error) => {
                        log::error!(
                            "Color Node '{}' property '{port}' produced no output: {error}",
                            node.id
                        );
                        Ok(EvalOutput::NoOutput)
                    }
                }
            }
        }
    }
}

fn supports_output(operation: ColorContent, output: &str) -> bool {
    match operation {
        ColorContent::Compose | ColorContent::Mix | ColorContent::ConvertSpace => {
            output == COLOR_VALUE_PORT
        }
        ColorContent::Split => matches!(
            output,
            COLOR_SPACE_PORT
                | COLOR_RED_PORT
                | COLOR_GREEN_PORT
                | COLOR_BLUE_PORT
                | COLOR_ALPHA_PORT
        ),
    }
}
