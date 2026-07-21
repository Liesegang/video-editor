//! Evaluation of Nodes that produce Image output.
//!
//! This module owns raster generators/media, Merge ordering, Image Transform,
//! Image Style/Opacity, Effect, Shape-to-Image Style, and Composition Instance
//! rasterization. It consumes Shape and metadata evaluators through typed
//! boundaries and never owns their implementation.

use std::collections::HashSet;

use ordered_float::OrderedFloat;
use uuid::Uuid;

use super::evaluator::{FrameEvaluator, missing_error, transparent_background};
use super::scope::EvaluationScope;
use crate::error::LibraryError;
use crate::model::frame::entity::{FrameGroup, FrameGroupKind, FrameItem, FrameObject};
use crate::model::project::{
    EvalOutput, EvalResult, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, PortAddress,
    PortOwner,
};
use crate::model::{
    CompositionInstanceContent, GeneratorContent, Node, NodeContent, PluginOperationContent,
};
use crate::plugin::{
    EFFECT_APPLY_OPERATION, EFFECT_CATEGORY, IMAGE_OPACITY_STYLE_COMPONENT_ID,
    IMAGE_TRANSFORM_COMPONENT_ID, ResolvedNodeInputs, STYLE_APPLY_OPERATION, STYLE_CATEGORY,
    TRANSFORM_APPLY_OPERATION, TRANSFORM_CATEGORY,
};

impl FrameEvaluator<'_> {
    fn collect_node(
        &self,
        node_id: Uuid,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<FrameItem> {
        let owner = PortOwner::Node(node_id);
        let node = self
            .project
            .get_node(node_id)
            .ok_or_else(|| missing_error(owner))?;
        if !node.enabled {
            return Ok(EvalOutput::NoOutput);
        }
        if !path.insert(owner) {
            return Err(super::evaluator::cycle_error(owner));
        }
        if node.bypassed {
            let item = self.collect_bypassed_image_node(node, global_time, path);
            path.remove(&owner);
            return item;
        }
        if let NodeContent::PluginOperation(operation) = node.content() {
            let item = if operation.category == TRANSFORM_CATEGORY
                && operation.component_id == IMAGE_TRANSFORM_COMPONENT_ID
                && operation.operation == TRANSFORM_APPLY_OPERATION
            {
                self.collect_image_transform_operation(node, operation, scope, global_time, path)?
            } else if operation.category == EFFECT_CATEGORY
                && operation.operation == EFFECT_APPLY_OPERATION
            {
                self.collect_effect_operation(node, operation, scope, global_time, path)?
            } else if operation.category == STYLE_CATEGORY
                && operation.component_id == IMAGE_OPACITY_STYLE_COMPONENT_ID
                && operation.operation == STYLE_APPLY_OPERATION
            {
                self.collect_image_opacity_style_operation(
                    node,
                    operation,
                    scope,
                    global_time,
                    path,
                )?
            } else if operation.category == STYLE_CATEGORY
                && operation.operation == STYLE_APPLY_OPERATION
            {
                self.collect_style_operation(node, operation, scope, global_time, path)?
            } else {
                log::warn!(
                    "Plugin operation node {} ({}/{}/{}) has no image evaluator; producing NoOutput",
                    node.id,
                    operation.category,
                    operation.component_id,
                    operation.operation
                );
                EvalOutput::NoOutput
            };
            path.remove(&owner);
            return Ok(item);
        }
        let inputs = self.resolve_node_inputs(node.id, scope, global_time)?;
        if inputs
            .properties
            .values()
            .any(|value| value == &EvalOutput::NoOutput)
        {
            path.remove(&owner);
            return Ok(EvalOutput::NoOutput);
        }
        let item = match node.content() {
            NodeContent::CompositionInstance(instance) => {
                self.collect_composition_instance(node, instance, scope, path, &inputs)?
            }
            NodeContent::Merge => self.collect_merge(node, scope, global_time, path, &inputs)?,
            NodeContent::Value(_) => EvalOutput::NoOutput,
            _ => self.convert_node(node, scope, &inputs)?.map(|object| {
                FrameItem::Group(FrameGroup {
                    source_id: node.id,
                    kind: FrameGroupKind::Node,
                    width: scope.width,
                    height: scope.height,
                    background_color: transparent_background(),
                    transform: Default::default(),
                    blend_mode: node.blend_mode,
                    effect_time: OrderedFloat(scope.time),
                    effects: Vec::new(),
                    items: vec![FrameItem::Object(object)],
                })
            }),
        };
        path.remove(&owner);
        Ok(item)
    }

    fn collect_bypassed_image_node(
        &self,
        node: &Node,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<FrameItem> {
        let input = node
            .bypass_input_for_output(IMAGE_OUTPUT_PORT)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Node {} cannot bypass Image output: no unambiguous same-typed input",
                    node.id
                ))
            })?;
        let target = PortAddress::new(PortOwner::Node(node.id), input);
        let connection = match self.single_connection_to(&target)? {
            EvalOutput::Produced(connection) => connection,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        self.collect_owner_output(connection.from.owner, global_time, path)
    }

    fn collect_image_transform_operation(
        &self,
        node: &Node,
        operation: &PluginOperationContent,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<FrameItem> {
        if !self.operation_contract_matches(operation)? {
            return Ok(EvalOutput::NoOutput);
        }
        let inputs = self.resolve_node_inputs(node.id, scope, global_time)?;
        if inputs
            .properties
            .values()
            .any(|value| value == &EvalOutput::NoOutput)
        {
            return Ok(EvalOutput::NoOutput);
        }
        let composition = self
            .composition_for_owner(PortOwner::Node(node.id))
            .ok_or_else(|| missing_error(PortOwner::Node(node.id)))?;
        let context = self.context(composition, Some(&inputs));
        let transform = match self.plugin_manager.evaluate_transform_operation(
            &context,
            &operation.component_id,
            node.properties(),
            scope.time,
        ) {
            EvalOutput::Produced(transform) => transform,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };

        let target = PortAddress::new(PortOwner::Node(node.id), IMAGE_INPUT_PORT);
        let connection = match self.single_connection_to(&target)? {
            EvalOutput::Produced(connection) => connection,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let mut source =
            match self.collect_owner_output(connection.from.owner, global_time, path)? {
                EvalOutput::Produced(source) => source,
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            };
        let source_scope =
            match self.scope_for_owner(connection.from.owner, global_time, &mut HashSet::new())? {
                EvalOutput::Produced(scope) => scope,
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            };
        neutralize_root_blend(&mut source);
        Ok(EvalOutput::Produced(FrameItem::Group(FrameGroup {
            source_id: node.id,
            kind: FrameGroupKind::ImageTransform,
            width: source_scope.width,
            height: source_scope.height,
            background_color: transparent_background(),
            transform,
            blend_mode: node.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: Vec::new(),
            items: vec![source],
        })))
    }

    /// Native Image Opacity is a Style-owned raster boundary, not a spatial
    /// Transform. It preserves the upstream Image subtree and applies alpha
    /// exactly once after isolating that subtree.
    fn collect_image_opacity_style_operation(
        &self,
        node: &Node,
        operation: &PluginOperationContent,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<FrameItem> {
        let descriptor = match self.plugin_manager.operation_descriptor(
            &operation.category,
            &operation.component_id,
            &operation.operation,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Unavailable Image Opacity operation on Node {}: {}; producing NoOutput",
                    node.id,
                    error
                );
                return Ok(EvalOutput::NoOutput);
            }
        };
        if !descriptor.is_execution_compatible_with_ports(&operation.declared_ports) {
            log::warn!(
                "Image Opacity operation contract mismatch on Node {}; producing NoOutput",
                node.id
            );
            return Ok(EvalOutput::NoOutput);
        }

        let inputs = self.resolve_node_inputs(node.id, scope, global_time)?;
        if inputs
            .properties
            .values()
            .any(|value| value == &EvalOutput::NoOutput)
        {
            return Ok(EvalOutput::NoOutput);
        }
        let composition = self
            .composition_for_owner(PortOwner::Node(node.id))
            .ok_or_else(|| missing_error(PortOwner::Node(node.id)))?;
        let context = self.context(composition, Some(&inputs));
        let opacity = match self.plugin_manager.evaluate_image_opacity_style_operation(
            &context,
            node.properties(),
            scope.time,
        ) {
            EvalOutput::Produced(opacity) => opacity,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };

        let target = PortAddress::new(PortOwner::Node(node.id), IMAGE_INPUT_PORT);
        let connection = match self.single_connection_to(&target)? {
            EvalOutput::Produced(connection) => connection,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let mut source =
            match self.collect_owner_output(connection.from.owner, global_time, path)? {
                EvalOutput::Produced(source) => source,
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            };
        let source_scope =
            match self.scope_for_owner(connection.from.owner, global_time, &mut HashSet::new())? {
                EvalOutput::Produced(scope) => scope,
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            };
        neutralize_root_blend(&mut source);
        let transform = crate::model::frame::transform::Transform {
            opacity,
            ..Default::default()
        };
        Ok(EvalOutput::Produced(FrameItem::Group(FrameGroup {
            source_id: node.id,
            kind: FrameGroupKind::ImageStyle,
            width: source_scope.width,
            height: source_scope.height,
            background_color: transparent_background(),
            transform,
            blend_mode: node.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: Vec::new(),
            items: vec![source],
        })))
    }

    fn collect_effect_operation(
        &self,
        node: &Node,
        operation: &PluginOperationContent,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<FrameItem> {
        let descriptor = match self.plugin_manager.operation_descriptor(
            &operation.category,
            &operation.component_id,
            &operation.operation,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Unavailable Effect operation {}/{}/{} on Node {}: {}; producing NoOutput",
                    operation.category,
                    operation.component_id,
                    operation.operation,
                    node.id,
                    error
                );
                return Ok(EvalOutput::NoOutput);
            }
        };
        if !descriptor.is_execution_compatible_with_ports(&operation.declared_ports) {
            log::warn!(
                "Effect operation contract mismatch on Node {}; producing NoOutput",
                node.id
            );
            return Ok(EvalOutput::NoOutput);
        }

        // Scalar/keyframed inputs are resolved before touching the potentially
        // expensive upstream image graph.
        let inputs = self.resolve_node_inputs(node.id, scope, global_time)?;
        if inputs
            .properties
            .values()
            .any(|value| value == &EvalOutput::NoOutput)
        {
            return Ok(EvalOutput::NoOutput);
        }
        let composition = match self.composition_for_owner(PortOwner::Node(node.id)) {
            Some(composition) => composition,
            None => {
                log::warn!(
                    "Effect operation Node {} has no containing Composition; producing NoOutput",
                    node.id
                );
                return Ok(EvalOutput::NoOutput);
            }
        };
        let context = self.context(composition, Some(&inputs));
        let Some(effect) = context.build_operation_effect(
            descriptor.component_id(),
            descriptor.properties(),
            node.properties(),
            scope.time,
        ) else {
            log::warn!(
                "Effect operation Node {} has incomplete properties; producing NoOutput",
                node.id
            );
            return Ok(EvalOutput::NoOutput);
        };

        let target = PortAddress::new(PortOwner::Node(node.id), IMAGE_INPUT_PORT);
        let connection = match self.single_connection_to(&target)? {
            EvalOutput::Produced(connection) => connection,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let mut source =
            match self.collect_owner_output(connection.from.owner, global_time, path)? {
                EvalOutput::Produced(source) => source,
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            };
        let source_scope =
            match self.scope_for_owner(connection.from.owner, global_time, &mut HashSet::new())? {
                EvalOutput::Produced(scope) => scope,
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            };
        neutralize_root_blend(&mut source);
        Ok(EvalOutput::Produced(FrameItem::Group(FrameGroup {
            source_id: node.id,
            kind: FrameGroupKind::Effect,
            width: source_scope.width,
            height: source_scope.height,
            background_color: transparent_background(),
            transform: Default::default(),
            blend_mode: node.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: vec![effect],
            items: vec![source],
        })))
    }

    /// Style is the only Shape -> Image boundary. It pulls one RuntimeShape,
    /// resolves its own properties at the Style node's explicit Time, and
    /// materializes exactly one renderer object for this branch.
    fn collect_style_operation(
        &self,
        node: &Node,
        operation: &PluginOperationContent,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<FrameItem> {
        let descriptor = match self.plugin_manager.operation_descriptor(
            &operation.category,
            &operation.component_id,
            &operation.operation,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Unavailable Style operation {}/{}/{} on Node {}: {}; producing NoOutput",
                    operation.category,
                    operation.component_id,
                    operation.operation,
                    node.id,
                    error
                );
                return Ok(EvalOutput::NoOutput);
            }
        };
        if !descriptor.is_execution_compatible_with_ports(&operation.declared_ports) {
            log::warn!(
                "Style operation contract mismatch on Node {}; producing NoOutput",
                node.id
            );
            return Ok(EvalOutput::NoOutput);
        }

        let inputs = self.resolve_node_inputs(node.id, scope, global_time)?;
        if inputs
            .properties
            .values()
            .any(|value| value == &EvalOutput::NoOutput)
        {
            return Ok(EvalOutput::NoOutput);
        }
        let shape_input = PortAddress::new(
            PortOwner::Node(node.id),
            crate::model::project::SHAPE_INPUT_PORT,
        );
        let connection = match self.single_connection_to(&shape_input)? {
            EvalOutput::Produced(connection) => connection,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let shape = match self.evaluate_shape_output(&connection.from, global_time, path)? {
            EvalOutput::Produced(shape) => shape,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let composition = self
            .composition_for_owner(PortOwner::Node(node.id))
            .ok_or_else(|| missing_error(PortOwner::Node(node.id)))?;
        let context = self.context(composition, Some(&inputs));
        let style = match self.plugin_manager.evaluate_style_operation(
            &context,
            descriptor.component_id(),
            node.id,
            node.properties(),
            scope.time,
        ) {
            EvalOutput::Produced(style) => style,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let objects = shape.into_styled_objects(style, scope.time as f32)?;
        Ok(EvalOutput::Produced(FrameItem::Group(FrameGroup {
            source_id: node.id,
            kind: FrameGroupKind::Node,
            width: scope.width,
            height: scope.height,
            background_color: transparent_background(),
            transform: Default::default(),
            blend_mode: node.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: Vec::new(),
            items: objects.into_iter().map(FrameItem::Object).collect(),
        })))
    }

    fn collect_merge(
        &self,
        node: &Node,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
        _inputs: &ResolvedNodeInputs,
    ) -> EvalResult<FrameItem> {
        let owner = PortOwner::Node(node.id);
        let target = PortAddress::new(owner, MERGE_IMAGES_PORT);
        let mut connections = self
            .project
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .collect::<Vec<_>>();
        connections.sort_by_key(|connection| (connection.order, connection.id));
        if connections.is_empty() {
            return Ok(EvalOutput::NoOutput);
        }
        for pair in connections.windows(2) {
            if pair[0].order == pair[1].order {
                return Err(LibraryError::Validation(format!(
                    "Merge input {target:?} has duplicate order {}",
                    pair[0].order
                )));
            }
        }
        let mut items = Vec::new();
        for connection in connections {
            let errors = self.project.validate_connection(connection);
            if !errors.is_empty() {
                return Err(LibraryError::Validation(
                    errors
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; "),
                ));
            }
            let mut source =
                match self.collect_owner_output(connection.from.owner, global_time, path)? {
                    EvalOutput::Produced(source) => source,
                    EvalOutput::NoOutput => continue,
                };
            let source_scope = match self.scope_for_owner(
                connection.from.owner,
                global_time,
                &mut HashSet::new(),
            )? {
                EvalOutput::Produced(scope) => scope,
                EvalOutput::NoOutput => continue,
            };
            neutralize_root_blend(&mut source);
            let blend_mode = connection
                .blend_mode
                .effective_over_empty_backdrop(items.is_empty());
            items.push(FrameItem::Group(FrameGroup {
                source_id: connection.id,
                kind: FrameGroupKind::ConnectedImage,
                width: source_scope.width,
                height: source_scope.height,
                background_color: transparent_background(),
                transform: Default::default(),
                blend_mode,
                effect_time: OrderedFloat(scope.time),
                effects: Vec::new(),
                items: vec![source],
            }));
        }
        if items.is_empty() {
            return Ok(EvalOutput::NoOutput);
        }
        Ok(EvalOutput::Produced(FrameItem::Group(FrameGroup {
            source_id: node.id,
            kind: FrameGroupKind::Merge,
            width: scope.width,
            height: scope.height,
            background_color: transparent_background(),
            transform: Default::default(),
            blend_mode: node.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: Vec::new(),
            items,
        })))
    }

    pub(super) fn collect_owner_output(
        &self,
        owner: PortOwner,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<FrameItem> {
        match owner {
            PortOwner::Composition(id) => {
                let composition = self
                    .project
                    .get_composition(id)
                    .ok_or_else(|| missing_error(owner))?;
                let scope = match self.scope_for_owner(owner, global_time, &mut HashSet::new())? {
                    EvalOutput::Produced(scope) => scope,
                    EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                };
                // A Composition is a configured raster boundary: once its
                // evaluation scope is active, an empty child graph still
                // produces its background (including a fully transparent
                // one). Track and Clip intentionally keep propagating
                // NoOutput when none of their children produce an image.
                let items = match self.collect_composition_items(composition, global_time, path)? {
                    EvalOutput::Produced(items) => items,
                    EvalOutput::NoOutput => Vec::new(),
                };
                let inputs = ResolvedNodeInputs::from_metadata(scope.as_inputs());
                let context = self.context(composition, Some(&inputs));
                Ok(EvalOutput::Produced(FrameItem::Group(FrameGroup {
                    source_id: id,
                    kind: FrameGroupKind::Composition,
                    width: scope.width,
                    height: scope.height,
                    background_color: composition.background_color.clone(),
                    transform: context.build_transform(&composition.properties, scope.time),
                    blend_mode: composition.blend_mode,
                    effect_time: OrderedFloat(scope.time),
                    effects: Vec::new(),
                    items,
                })))
            }
            PortOwner::Track(id) => self.collect_track(id, global_time, path),
            PortOwner::Clip(id) => self.collect_clip(id, global_time, path),
            PortOwner::Node(id) => {
                let node = self
                    .project
                    .get_node(id)
                    .ok_or_else(|| missing_error(owner))?;
                if !node.enabled {
                    return Ok(EvalOutput::NoOutput);
                }
                match self.scope_for_node(id, global_time)? {
                    EvalOutput::Produced(scope) => self.collect_node(id, scope, global_time, path),
                    EvalOutput::NoOutput => Ok(EvalOutput::NoOutput),
                }
            }
        }
    }

    fn convert_node(
        &self,
        node: &Node,
        scope: EvaluationScope,
        inputs: &ResolvedNodeInputs,
    ) -> EvalResult<FrameObject> {
        let owner = PortOwner::Node(node.id);
        let composition = self
            .composition_for_owner(owner)
            .ok_or_else(|| missing_error(owner))?;
        let kind = match node.content() {
            NodeContent::Media(media) => {
                let asset = self.project.get_asset(media.asset_id).ok_or_else(|| {
                    LibraryError::Project(format!("Asset {} not found", media.asset_id))
                })?;
                match asset.kind {
                    crate::model::asset::AssetKind::Video => "video",
                    crate::model::asset::AssetKind::Image => "image",
                    crate::model::asset::AssetKind::Audio => "audio",
                    _ => "unknown",
                }
            }
            NodeContent::Generator(generator) => match generator {
                GeneratorContent::Shape => "shape",
                GeneratorContent::Text => "text",
                GeneratorContent::Solid => "solid",
                GeneratorContent::SkSL => "sksl",
            },
            NodeContent::CompositionInstance(_) => return Ok(EvalOutput::NoOutput),
            NodeContent::PluginOperation(_) => return Ok(EvalOutput::NoOutput),
            NodeContent::Value(_) => return Ok(EvalOutput::NoOutput),
            NodeContent::Merge => "merge",
        };
        let converter = self
            .plugin_manager
            .get_entity_converter(kind)
            .ok_or_else(|| {
                LibraryError::Plugin(format!("No entity converter registered for {kind}"))
            })?;
        let context = self.context(composition, Some(inputs));
        if kind == "video"
            && let NodeContent::Media(media) = node.content()
            && let Some(asset) = self.project.get_asset(media.asset_id)
            && (!scope.time.is_finite()
                || scope.time < 0.0
                || asset
                    .duration
                    .is_some_and(|duration| scope.time >= duration))
        {
            return Ok(EvalOutput::NoOutput);
        }
        converter
            .convert_entity(&context, node, scope.time)
            .map(EvalOutput::Produced)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Entity converter {kind} failed to produce Node {}",
                    node.id
                ))
            })
    }

    fn collect_composition_instance(
        &self,
        node: &Node,
        instance: &CompositionInstanceContent,
        scope: EvaluationScope,
        path: &mut HashSet<PortOwner>,
        _inputs: &ResolvedNodeInputs,
    ) -> EvalResult<FrameItem> {
        let target = self
            .project
            .get_composition(instance.composition_id)
            .ok_or_else(|| missing_error(PortOwner::Composition(instance.composition_id)))?;
        let mut item =
            match self.collect_owner_output(PortOwner::Composition(target.id), scope.time, path)? {
                EvalOutput::Produced(item) => item,
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            };
        let target_scope = match self.scope_for_owner(
            PortOwner::Composition(target.id),
            scope.time,
            &mut HashSet::new(),
        )? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        neutralize_root_blend(&mut item);
        Ok(EvalOutput::Produced(FrameItem::Group(FrameGroup {
            source_id: node.id,
            kind: FrameGroupKind::CompositionInstance,
            width: target_scope.width,
            height: target_scope.height,
            background_color: transparent_background(),
            transform: Default::default(),
            blend_mode: node.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: Vec::new(),
            items: vec![item],
        })))
    }
}

fn neutralize_root_blend(item: &mut FrameItem) {
    if let FrameItem::Group(group) = item {
        group.blend_mode = crate::model::BlendMode::Normal;
    }
}
