//! Shape evaluation used by bounded Module definitions.
//!
//! Timeline-to-Node-Clip conversion emits the same Text/Path generators and
//! descriptor-backed Ensemble operations as the production Node Editor. This
//! evaluator consumes those exact nodes; it never expands Timeline structure
//! or constructs a compatibility Project.

use super::frame_values::{required_number, required_string};
use super::*;
use crate::model::authoring::text_ensemble_direct_contract_is_compatible;
use crate::model::frame::runtime_shape::RuntimeShape;
use crate::model::node::{ELLIPSE_SHAPE_CATALOG_ID, RECTANGLE_SHAPE_CATALOG_ID};
use crate::model::project::{
    BACKGROUND_SHAPE_INPUT_PORT, EvalOutput, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use crate::plugin::{
    DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY,
    PATH_EFFECT_APPLY_OPERATION, PATH_EFFECT_CATEGORY, SHAPE_TRANSFORM_COMPONENT_ID,
    TRANSFORM_APPLY_OPERATION, TRANSFORM_CATEGORY,
};

impl ModuleImageRuntime<'_> {
    pub(super) fn style_shape_image(
        &mut self,
        node: &CompiledNode,
        operation: &crate::model::node::PluginOperationContent,
    ) -> Result<Option<FrameItem>, LibraryError> {
        let Some(source) = self.single_shape_source(node.id, SHAPE_INPUT_PORT)? else {
            return Ok(None);
        };
        let Some(shape) = self.evaluate_shape_output(&source)? else {
            return Ok(None);
        };
        let Some(style) = self.evaluate_style_config(node, operation)? else {
            return Ok(None);
        };
        let object = shape.into_styled_object(style, self.local_time.to_seconds_f64() as f32)?;
        Ok(Some(FrameItem::Group(FrameGroup {
            source_id: node.id,
            kind: FrameGroupKind::Node,
            width: self.width,
            height: self.height,
            background_color: transparent(),
            transform: Transform::default(),
            blend_mode: node.blend_mode,
            effect_time: OrderedFloat(self.local_time.to_seconds_f64()),
            effects: Vec::new(),
            items: vec![FrameItem::Object(object)],
        })))
    }

    pub(super) fn appearance_stack_image(
        &mut self,
        node: &CompiledNode,
    ) -> Result<Option<FrameItem>, LibraryError> {
        let Some(shape_source) = self.single_shape_source(node.id, SHAPE_INPUT_PORT)? else {
            return Ok(None);
        };
        let Some(shape) = self.evaluate_shape_output(&shape_source)? else {
            return Ok(None);
        };
        let target = ModulePortAddress {
            node_id: node.id,
            port: crate::model::project::APPEARANCE_STYLES_PORT.to_string(),
        };
        let mut sources = self
            .definition
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .map(|connection| (connection.order, connection.id, connection.from.clone()))
            .collect::<Vec<_>>();
        sources.sort_by_key(|(order, id, _)| (*order, *id));

        let mut styles = Vec::with_capacity(sources.len());
        for (_, _, source) in sources {
            if source.port != crate::model::project::STYLE_OUTPUT_PORT {
                return Err(LibraryError::Validation(format!(
                    "Appearance Stack {} received a non-Style output {}:{}",
                    node.id, source.node_id, source.port
                )));
            }
            let style_node = self
                .definition
                .nodes
                .get(&source.node_id)
                .cloned()
                .ok_or_else(|| {
                    LibraryError::Validation(format!(
                        "Appearance Stack {} reaches missing Style Node {}",
                        node.id, source.node_id
                    ))
                })?;
            if !style_node.enabled {
                continue;
            }
            if style_node.bypassed {
                return Err(LibraryError::Validation(format!(
                    "Style Node {} cannot bypass a Style value",
                    style_node.id
                )));
            }
            let NodeContent::PluginOperation(operation) = &style_node.content else {
                return Err(LibraryError::Validation(format!(
                    "Appearance Stack {} input {} is not a Style operation",
                    node.id, style_node.id
                )));
            };
            if operation.category != STYLE_CATEGORY
                || operation.operation != STYLE_APPLY_OPERATION
                || operation.component_id == IMAGE_OPACITY_STYLE_COMPONENT_ID
            {
                return Err(LibraryError::Validation(format!(
                    "Appearance Stack {} input {} has an incompatible operation contract",
                    node.id, style_node.id
                )));
            }
            if let Some(style) = self.evaluate_style_config(&style_node, operation)? {
                styles.push(style);
            }
        }
        if styles.is_empty() {
            return Ok(None);
        }
        let object =
            shape.into_appearance_object(styles, self.local_time.to_seconds_f64() as f32)?;
        Ok(Some(FrameItem::Group(FrameGroup {
            source_id: node.id,
            kind: FrameGroupKind::Node,
            width: self.width,
            height: self.height,
            background_color: transparent(),
            transform: Transform::default(),
            blend_mode: node.blend_mode,
            effect_time: OrderedFloat(self.local_time.to_seconds_f64()),
            effects: Vec::new(),
            items: vec![FrameItem::Object(object)],
        })))
    }

    fn evaluate_style_config(
        &mut self,
        node: &CompiledNode,
        operation: &crate::model::node::PluginOperationContent,
    ) -> Result<Option<StyleConfig>, LibraryError> {
        let descriptor = self.plugins.operation_descriptor(
            &operation.category,
            &operation.component_id,
            &operation.operation,
        )?;
        if !descriptor.is_execution_compatible_with_ports(&operation.declared_ports) {
            return Ok(None);
        }
        let values = self.node_values(node)?;
        match self.plugins.evaluate_style_operation_values(
            &operation.component_id,
            node.id,
            &values,
            self.local_time.to_seconds_f64(),
            self.evaluation_fps,
            (self.width, self.height),
        ) {
            EvalOutput::Produced(style) => Ok(Some(style)),
            EvalOutput::NoOutput => Err(LibraryError::Render(format!(
                "Style Module Node {} produced no Style for component '{}'",
                node.id, operation.component_id
            ))),
        }
    }

    pub(super) fn evaluate_shape_output(
        &mut self,
        source: &ModulePortAddress,
    ) -> Result<Option<RuntimeShape>, LibraryError> {
        if source.port != SHAPE_OUTPUT_PORT {
            return Err(LibraryError::Validation(format!(
                "Module Shape source {}:{} is not a Shape output",
                source.node_id, source.port
            )));
        }
        let key = (source.node_id, source.port.clone());
        if let Some(cached) = self.shape_memo.get(&key) {
            return Ok(cached.clone());
        }
        if !self.shape_path.insert(key.clone()) {
            return Err(LibraryError::Validation(format!(
                "Module Shape cycle reaches {}:{}",
                source.node_id, source.port
            )));
        }
        let result = self.evaluate_shape_output_inner(source);
        self.shape_path.remove(&key);
        if let Ok(shape) = &result {
            self.shape_memo.insert(key, shape.clone());
        }
        result
    }

    fn evaluate_shape_output_inner(
        &mut self,
        source: &ModulePortAddress,
    ) -> Result<Option<RuntimeShape>, LibraryError> {
        let node = self
            .definition
            .nodes
            .get(&source.node_id)
            .cloned()
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Compiled Module Shape reaches missing Node {}",
                    source.node_id
                ))
            })?;
        if !node.enabled {
            return Ok(None);
        }
        if node.bypassed {
            let input = node.bypass_routes.get(SHAPE_OUTPUT_PORT).ok_or_else(|| {
                LibraryError::Validation(format!("Node {} has no Shape bypass route", node.id))
            })?;
            let Some(source) = self.single_shape_source(node.id, input)? else {
                return Ok(None);
            };
            return self.evaluate_shape_output(&source);
        }
        match &node.content {
            NodeContent::Generator(GeneratorContent::Text) => {
                let values = self.node_values(&node)?;
                let text = required_string(&values, "text", "Text Generator")?;
                let font = required_string(&values, "font_family", "Text Generator")?;
                let size = required_number(&values, "size", "Text Generator")?;
                Ok(crate::plugin::entity_converter::runtime_text_shape(
                    node.id, &text, &font, size,
                ))
            }
            NodeContent::Generator(GeneratorContent::Shape) => {
                let values = self.node_values(&node)?;
                let Some(PropertyValue::Path(path)) = values.get("path") else {
                    return Err(LibraryError::Validation(format!(
                        "Shape Generator {} has no canonical Path",
                        node.id
                    )));
                };
                crate::plugin::entity_converter::runtime_path_shape(node.id, path.clone())
            }
            NodeContent::NativeOperation(operation)
                if matches!(
                    operation.catalog_id.as_str(),
                    RECTANGLE_SHAPE_CATALOG_ID | ELLIPSE_SHAPE_CATALOG_ID
                ) =>
            {
                let values = self.node_values(&node)?;
                let width = required_number(&values, "width", "Primitive Shape")?;
                let height = required_number(&values, "height", "Primitive Shape")?;
                let kind = if operation.catalog_id == RECTANGLE_SHAPE_CATALOG_ID {
                    crate::model::authoring::ShapeKind::Rectangle
                } else {
                    crate::model::authoring::ShapeKind::Ellipse
                };
                let path_data = crate::plugin::entity_converter::primitive_shape_path_data(
                    kind, width, height,
                )?;
                let path = crate::model::path::parse_legacy_svg_path_data(&path_data)
                    .map_err(|error| LibraryError::Render(error.to_string()))?;
                crate::plugin::entity_converter::runtime_path_shape(node.id, path)
            }
            NodeContent::PluginOperation(operation)
                if operation.category == EFFECTOR_CATEGORY
                    && operation.operation == EFFECTOR_APPLY_OPERATION =>
            {
                let Some(source) = self.single_shape_source(node.id, SHAPE_INPUT_PORT)? else {
                    return Ok(None);
                };
                let Some(mut shape) = self.evaluate_shape_output(&source)? else {
                    return Ok(None);
                };
                let values = self.node_values(&node)?;
                let config = match self.plugins.evaluate_effector_operation_values(
                    &operation.component_id,
                    node.id,
                    &values,
                    self.local_time.to_seconds_f64(),
                    self.evaluation_fps,
                    (self.width, self.height),
                ) {
                    EvalOutput::Produced(config) => config,
                    EvalOutput::NoOutput => return Ok(None),
                };
                shape.apply_effector(config, self.local_time.to_seconds_f64() as f32)?;
                Ok(Some(shape))
            }
            NodeContent::PluginOperation(operation)
                if operation.category == PATH_EFFECT_CATEGORY
                    && operation.operation == PATH_EFFECT_APPLY_OPERATION =>
            {
                let descriptor = self.plugins.operation_descriptor(
                    PATH_EFFECT_CATEGORY,
                    &operation.component_id,
                    PATH_EFFECT_APPLY_OPERATION,
                )?;
                if !descriptor.is_execution_compatible_with_ports(&operation.declared_ports) {
                    return Ok(None);
                }
                let Some(source) = self.single_shape_source(node.id, SHAPE_INPUT_PORT)? else {
                    return Ok(None);
                };
                let Some(mut shape) = self.evaluate_shape_output(&source)? else {
                    return Ok(None);
                };
                let values = self.node_values(&node)?;
                let effect = match self.plugins.evaluate_path_effect_values(
                    &operation.component_id,
                    &values,
                    self.local_time.to_seconds_f64(),
                    self.evaluation_fps,
                    (self.width, self.height),
                )? {
                    EvalOutput::Produced(effect) => effect,
                    EvalOutput::NoOutput => return Ok(None),
                };
                shape.apply_path_effect(node.id, effect)?;
                Ok(Some(shape))
            }
            NodeContent::PluginOperation(operation)
                if operation.category == TRANSFORM_CATEGORY
                    && operation.component_id == SHAPE_TRANSFORM_COMPONENT_ID
                    && operation.operation == TRANSFORM_APPLY_OPERATION =>
            {
                let descriptor = self.plugins.operation_descriptor(
                    TRANSFORM_CATEGORY,
                    SHAPE_TRANSFORM_COMPONENT_ID,
                    TRANSFORM_APPLY_OPERATION,
                )?;
                if !descriptor.is_execution_compatible_with_ports(&operation.declared_ports) {
                    return Ok(None);
                }
                let Some(source) = self.single_shape_source(node.id, SHAPE_INPUT_PORT)? else {
                    return Ok(None);
                };
                let Some(mut shape) = self.evaluate_shape_output(&source)? else {
                    return Ok(None);
                };
                let values = self.node_values(&node)?;
                let transform = match self
                    .plugins
                    .evaluate_shape_transform_operation_values(&operation.component_id, &values)
                {
                    EvalOutput::Produced(transform) => transform,
                    EvalOutput::NoOutput => return Ok(None),
                };
                shape.set_root_transform(node.id, transform)?;
                Ok(Some(shape))
            }
            NodeContent::PluginOperation(operation)
                if operation.category == DECORATOR_CATEGORY
                    && operation.operation == DECORATOR_APPLY_OPERATION =>
            {
                let inline = text_ensemble_direct_contract_is_compatible(&operation.declared_ports);
                let descriptor = if inline {
                    self.plugins.text_ensemble_operation_descriptor(
                        DECORATOR_CATEGORY,
                        &operation.component_id,
                    )?
                } else {
                    self.plugins.operation_descriptor(
                        DECORATOR_CATEGORY,
                        &operation.component_id,
                        DECORATOR_APPLY_OPERATION,
                    )?
                };
                if !descriptor.is_execution_compatible_with_ports(&operation.declared_ports) {
                    return Ok(None);
                }
                let Some(source) = self.single_shape_source(node.id, SHAPE_INPUT_PORT)? else {
                    return Ok(None);
                };
                let Some(shape) = self.evaluate_shape_output(&source)? else {
                    return Ok(None);
                };
                let values = self.node_values(&node)?;
                let evaluated = if inline {
                    self.plugins.evaluate_text_decorator_operation_values(
                        &operation.component_id,
                        node.id,
                        &values,
                        self.local_time.to_seconds_f64(),
                        self.evaluation_fps,
                        (self.width, self.height),
                    )
                } else {
                    self.plugins.evaluate_decorator_operation_values(
                        &operation.component_id,
                        node.id,
                        &values,
                        self.local_time.to_seconds_f64(),
                        self.evaluation_fps,
                        (self.width, self.height),
                    )
                };
                let config = match evaluated {
                    EvalOutput::Produced(config) => config,
                    EvalOutput::NoOutput => return Ok(None),
                };
                match config {
                    config @ crate::core::ensemble::types::DecoratorConfig::LegacyBackplate {
                        ..
                    } => {
                        let mut shape = shape;
                        shape.push_decorator(config);
                        Ok(Some(shape))
                    }
                    config @ crate::core::ensemble::types::DecoratorConfig::Backplate { .. } => {
                        if inline {
                            return Err(LibraryError::Render(format!(
                                "Inline Decorator Module Node {} returned a multi-Shape geometry contract",
                                node.id
                            )));
                        }
                        let Some(background_source) =
                            self.single_shape_source(node.id, BACKGROUND_SHAPE_INPUT_PORT)?
                        else {
                            return Ok(None);
                        };
                        let Some(background) = self.evaluate_shape_output(&background_source)?
                        else {
                            return Ok(None);
                        };
                        Ok(Some(shape.into_backplate_geometry(
                            node.id,
                            background,
                            config,
                            self.local_time.to_seconds_f64() as f32,
                        )?))
                    }
                }
            }
            _ => Err(LibraryError::Render(format!(
                "Module Node {} does not produce a supported Shape",
                node.id
            ))),
        }
    }

    pub(super) fn single_shape_source(
        &self,
        node_id: uuid::Uuid,
        port: &str,
    ) -> Result<Option<ModulePortAddress>, LibraryError> {
        let target = ModulePortAddress {
            node_id,
            port: port.to_string(),
        };
        let mut sources = self
            .definition
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .map(|connection| (connection.order, connection.id, connection.from.clone()))
            .collect::<Vec<_>>();
        sources.sort_by_key(|(order, id, _)| (*order, *id));
        if sources.len() > 1 {
            return Err(LibraryError::Validation(format!(
                "Module Shape input {node_id}:{port} resolves more than once"
            )));
        }
        Ok(sources.pop().map(|(_, _, source)| source))
    }
}
