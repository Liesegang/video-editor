use log::debug;
use ordered_float::OrderedFloat;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

use crate::error::LibraryError;
use crate::model::frame::color::Color;
use crate::model::frame::entity::{FrameGroup, FrameGroupKind, FrameItem, FrameObject};
use crate::model::frame::frame::{FrameInfo, Region};
use crate::model::frame::runtime_shape::RuntimeShape;
use crate::model::numeric::evaluate_numeric_binary;
use crate::model::project::{
    Composition, DURATION_PORT, EvalOutput, EvalResult, FPS_PORT, FRAME_PORT, IMAGE_INPUT_PORT,
    IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NUMBER_RESULT_OUTPUT_PORT, NodeContainer, PortAddress,
    PortDataType, PortDirection, PortMultiplicity, PortOwner, Project, ProjectConnection,
    RESOLUTION_PORT, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
};
use crate::model::property::{PropertyValue, Vec2};
use crate::model::{GeneratorContent, Node, NodeContent, ValueContent};
use crate::plugin::{
    DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, EFFECT_APPLY_OPERATION, EFFECT_CATEGORY,
    EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY, FrameEvaluationContext,
    IMAGE_OPACITY_STYLE_COMPONENT_ID, IMAGE_TRANSFORM_COMPONENT_ID, PATH_EFFECT_APPLY_OPERATION,
    PATH_EFFECT_CATEGORY, PluginManager, PropertyEvaluatorRegistry, ResolvedNodeInputs,
    STYLE_APPLY_OPERATION, STYLE_CATEGORY, TRANSFORM_APPLY_OPERATION, TRANSFORM_CATEGORY,
    property_name_from_port,
};
use crate::util::timing::ScopedTimer;

mod composition_instances;
mod decorator;
mod property_evaluation;
mod scope;

use scope::EvaluationScope;

pub struct FrameEvaluator<'a> {
    project: &'a Project,
    composition: &'a Composition,
    property_evaluators: Arc<PropertyEvaluatorRegistry>,
    plugin_manager: &'a PluginManager,
}

impl<'a> FrameEvaluator<'a> {
    pub fn new(
        project: &'a Project,
        composition: &'a Composition,
        property_evaluators: Arc<PropertyEvaluatorRegistry>,
        plugin_manager: &'a PluginManager,
    ) -> Self {
        Self {
            project,
            composition,
            property_evaluators,
            plugin_manager,
        }
    }

    pub fn evaluate(
        &self,
        frame_number: u64,
        render_scale: f64,
        region: Option<Region>,
    ) -> Result<FrameInfo, LibraryError> {
        if let Some(error) = self.project.validate_connections().into_iter().next() {
            return Err(LibraryError::Validation(error.to_string()));
        }
        let global_time = frame_number as f64 / self.composition.fps;
        let mut frame = FrameInfo {
            width: self.composition.width,
            height: self.composition.height,
            // The root Composition is the only boundary that materializes a
            // normal NoOutput as its configured background/transparent canvas.
            background_color: self.composition.background_color.clone(),
            color_profile: self.composition.color_profile.clone(),
            render_scale: OrderedFloat(render_scale),
            now_time: OrderedFloat(global_time),
            region,
            items: Vec::new(),
        };
        frame.items = match self.collect_composition_items(
            self.composition,
            global_time,
            &mut HashSet::new(),
        )? {
            EvalOutput::Produced(items) => items,
            EvalOutput::NoOutput => Vec::new(),
        };
        Ok(frame)
    }

    fn collect_composition_items(
        &self,
        composition: &Composition,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<Vec<FrameItem>> {
        let owner = PortOwner::Composition(composition.id);
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        match self.scope_for_owner(owner, global_time, &mut HashSet::new())? {
            EvalOutput::Produced(_) => {}
            EvalOutput::NoOutput => {
                path.remove(&owner);
                return Ok(EvalOutput::NoOutput);
            }
        }
        let items = self.collect_container_image_items(owner, global_time, path);
        path.remove(&owner);
        items
    }

    fn collect_track(
        &self,
        track_id: Uuid,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<FrameItem> {
        let owner = PortOwner::Track(track_id);
        let track = self
            .project
            .get_track(track_id)
            .ok_or_else(|| missing_error(owner))?;
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        let scope = match self.scope_for_owner(owner, global_time, &mut HashSet::new())? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => {
                path.remove(&owner);
                return Ok(EvalOutput::NoOutput);
            }
        };
        let items = match self.collect_container_image_items(owner, global_time, path)? {
            EvalOutput::Produced(items) => items,
            EvalOutput::NoOutput => {
                path.remove(&owner);
                return Ok(EvalOutput::NoOutput);
            }
        };
        let composition = self
            .composition_for_owner(owner)
            .ok_or_else(|| missing_error(owner))?;
        let inputs = ResolvedNodeInputs::from_metadata(scope.as_inputs());
        let item = FrameItem::Group(FrameGroup {
            source_id: track.id,
            kind: FrameGroupKind::Track,
            width: scope.width,
            height: scope.height,
            background_color: transparent(),
            transform: self
                .context(composition, Some(&inputs))
                .build_transform(&track.properties, scope.time),
            blend_mode: track.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: Vec::new(),
            items,
        });
        path.remove(&owner);
        Ok(EvalOutput::Produced(item))
    }

    fn collect_clip(
        &self,
        clip_id: Uuid,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<FrameItem> {
        let owner = PortOwner::Clip(clip_id);
        let clip = self
            .project
            .get_clip(clip_id)
            .ok_or_else(|| missing_error(owner))?;
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        let scope = match self.scope_for_owner(owner, global_time, &mut HashSet::new())? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => {
                path.remove(&owner);
                return Ok(EvalOutput::NoOutput);
            }
        };
        let items = match self.collect_container_image_items(owner, global_time, path)? {
            EvalOutput::Produced(items) => items,
            EvalOutput::NoOutput => {
                path.remove(&owner);
                return Ok(EvalOutput::NoOutput);
            }
        };
        let composition = self
            .composition_for_owner(owner)
            .ok_or_else(|| missing_error(owner))?;
        let inputs = ResolvedNodeInputs::from_metadata(scope.as_inputs());
        let item = FrameItem::Group(FrameGroup {
            source_id: clip.id,
            kind: FrameGroupKind::Clip,
            width: scope.width,
            height: scope.height,
            background_color: transparent(),
            transform: self
                .context(composition, Some(&inputs))
                .build_transform(&clip.properties, scope.time),
            blend_mode: clip.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: Vec::new(),
            items,
        });
        path.remove(&owner);
        Ok(EvalOutput::Produced(item))
    }

    fn collect_container_image_items(
        &self,
        owner: PortOwner,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<Vec<FrameItem>> {
        let mut candidates = Vec::new();
        for source in self.project.container_image_sources(owner) {
            // Every child, including an explicitly bound direct Node, goes
            // through its own authoritative owner scope. Passing the
            // container scope directly here used to bypass the Node's Time
            // input only for direct output bindings.
            let item = self.collect_owner_output(source.source, global_time, path)?;
            candidates.push(item);
        }
        Ok(aggregate_outputs(candidates))
    }

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
            return Err(cycle_error(owner));
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
                    background_color: transparent(),
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
        operation: &crate::model::PluginOperationContent,
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
            background_color: transparent(),
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
        operation: &crate::model::PluginOperationContent,
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
            background_color: transparent(),
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
        operation: &crate::model::PluginOperationContent,
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
            background_color: transparent(),
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
        operation: &crate::model::PluginOperationContent,
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
        let shape_input = PortAddress::new(PortOwner::Node(node.id), SHAPE_INPUT_PORT);
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
            background_color: transparent(),
            transform: Default::default(),
            blend_mode: node.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: Vec::new(),
            items: objects.into_iter().map(FrameItem::Object).collect(),
        })))
    }

    /// Pull a transient Shape value from an exact output address. Shape values
    /// are never persisted and are cloned only by real graph fan-out.
    fn evaluate_shape_output(
        &self,
        source: &PortAddress,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<RuntimeShape> {
        let definition = self
            .project
            .port_definition(source, PortDirection::Output)
            .ok_or_else(|| LibraryError::Validation(format!("Missing output port {source:?}")))?;
        if definition.data_type != PortDataType::Shape {
            return Err(LibraryError::Validation(format!(
                "Port {source:?} does not produce Shape"
            )));
        }
        let PortOwner::Node(node_id) = source.owner else {
            return Ok(EvalOutput::NoOutput);
        };
        if source.port != SHAPE_OUTPUT_PORT {
            return Ok(EvalOutput::NoOutput);
        }
        let owner = PortOwner::Node(node_id);
        let node = self
            .project
            .get_node(node_id)
            .ok_or_else(|| missing_error(owner))?;
        // Disabled is a graph gate. It is checked before cycle detection,
        // scope/Time evaluation, descriptor lookup, properties, or upstream.
        if !node.enabled {
            return Ok(EvalOutput::NoOutput);
        }
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        if node.bypassed {
            let input = node
                .bypass_input_for_output(SHAPE_OUTPUT_PORT)
                .ok_or_else(|| {
                    LibraryError::Validation(format!(
                        "Node {} cannot bypass Shape output: no unambiguous same-typed input",
                        node.id
                    ))
                })?;
            let result = self.pull_shape_input_from_port(node.id, input, global_time, path);
            path.remove(&owner);
            return result;
        }
        let result = (|| {
            let scope = match self.scope_for_node(node_id, global_time)? {
                EvalOutput::Produced(scope) => scope,
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            };
            match node.content() {
                NodeContent::Generator(GeneratorContent::Text | GeneratorContent::Shape) => {
                    self.convert_shape_node(node, scope, global_time)
                }
                NodeContent::PluginOperation(operation)
                    if operation.category == EFFECTOR_CATEGORY
                        && operation.operation == EFFECTOR_APPLY_OPERATION =>
                {
                    self.apply_effector_to_shape(node, operation, scope, global_time, path)
                }
                NodeContent::PluginOperation(operation)
                    if operation.category == PATH_EFFECT_CATEGORY
                        && operation.operation == PATH_EFFECT_APPLY_OPERATION =>
                {
                    self.apply_path_effect_to_shape(node, operation, scope, global_time, path)
                }
                NodeContent::PluginOperation(operation)
                    if operation.category == DECORATOR_CATEGORY
                        && operation.operation == DECORATOR_APPLY_OPERATION =>
                {
                    self.apply_decorator_to_shape(node, operation, scope, global_time, path)
                }
                NodeContent::PluginOperation(operation)
                    if operation.category == TRANSFORM_CATEGORY
                        && operation.operation == TRANSFORM_APPLY_OPERATION =>
                {
                    self.apply_root_transform_to_shape(node, operation, scope, global_time, path)
                }
                _ => Ok(EvalOutput::NoOutput),
            }
        })();
        path.remove(&owner);
        result
    }

    fn convert_shape_node(
        &self,
        node: &Node,
        scope: EvaluationScope,
        global_time: f64,
    ) -> EvalResult<RuntimeShape> {
        let kind = match node.content() {
            NodeContent::Generator(GeneratorContent::Text) => "text",
            NodeContent::Generator(GeneratorContent::Shape) => "shape",
            _ => return Ok(EvalOutput::NoOutput),
        };
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
        let converter = self
            .plugin_manager
            .get_entity_converter(kind)
            .ok_or_else(|| LibraryError::Plugin(format!("No entity converter for {kind}")))?;
        let context = self.context(composition, Some(&inputs));
        Ok(match converter.convert_shape(&context, node, scope.time) {
            Some(shape) => EvalOutput::Produced(shape),
            None => EvalOutput::NoOutput,
        })
    }

    fn apply_effector_to_shape(
        &self,
        node: &Node,
        operation: &crate::model::PluginOperationContent,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<RuntimeShape> {
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
        let mut shape = match self.pull_shape_input(node.id, global_time, path)? {
            EvalOutput::Produced(shape) => shape,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let composition = self
            .composition_for_owner(PortOwner::Node(node.id))
            .ok_or_else(|| missing_error(PortOwner::Node(node.id)))?;
        let context = self.context(composition, Some(&inputs));
        let config = match self.plugin_manager.evaluate_effector_operation(
            &context,
            &operation.component_id,
            node.id,
            node.properties(),
            scope.time,
        ) {
            EvalOutput::Produced(config) => config,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        shape.apply_effector(config, scope.time as f32)?;
        Ok(EvalOutput::Produced(shape))
    }

    fn apply_path_effect_to_shape(
        &self,
        node: &Node,
        operation: &crate::model::PluginOperationContent,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<RuntimeShape> {
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
        let mut shape = match self.pull_shape_input(node.id, global_time, path)? {
            EvalOutput::Produced(shape) => shape,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let composition = self
            .composition_for_owner(PortOwner::Node(node.id))
            .ok_or_else(|| missing_error(PortOwner::Node(node.id)))?;
        let context = self.context(composition, Some(&inputs));
        let effect = match self.plugin_manager.evaluate_path_effect_operation(
            &context,
            &operation.component_id,
            node.properties(),
            scope.time,
        )? {
            EvalOutput::Produced(effect) => effect,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        shape.apply_path_effect(node.id, effect)?;
        Ok(EvalOutput::Produced(shape))
    }

    fn apply_root_transform_to_shape(
        &self,
        node: &Node,
        operation: &crate::model::PluginOperationContent,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<RuntimeShape> {
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
        let mut shape = match self.pull_shape_input(node.id, global_time, path)? {
            EvalOutput::Produced(shape) => shape,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
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
        shape.set_root_transform(node.id, transform)?;
        Ok(EvalOutput::Produced(shape))
    }

    fn operation_contract_matches(
        &self,
        operation: &crate::model::PluginOperationContent,
    ) -> Result<bool, LibraryError> {
        let descriptor = match self.plugin_manager.operation_descriptor(
            &operation.category,
            &operation.component_id,
            &operation.operation,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Unavailable operation {}/{}/{}: {error}; producing NoOutput",
                    operation.category,
                    operation.component_id,
                    operation.operation
                );
                return Ok(false);
            }
        };
        Ok(descriptor.is_execution_compatible_with_ports(&operation.declared_ports))
    }

    fn pull_shape_input(
        &self,
        node_id: Uuid,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<RuntimeShape> {
        self.pull_shape_input_from_port(node_id, SHAPE_INPUT_PORT, global_time, path)
    }

    fn pull_shape_input_from_port(
        &self,
        node_id: Uuid,
        port: &str,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<RuntimeShape> {
        let target = PortAddress::new(PortOwner::Node(node_id), port);
        let connection = match self.single_connection_to(&target)? {
            EvalOutput::Produced(connection) => connection,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        self.evaluate_shape_output(&connection.from, global_time, path)
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
                background_color: transparent(),
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
            background_color: transparent(),
            transform: Default::default(),
            blend_mode: node.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: Vec::new(),
            items,
        })))
    }

    fn collect_owner_output(
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

    fn resolve_node_inputs(
        &self,
        node_id: Uuid,
        scope: EvaluationScope,
        global_time: f64,
    ) -> Result<ResolvedNodeInputs, LibraryError> {
        let mut values = ResolvedNodeInputs::from_metadata(scope.as_inputs());
        let owner = PortOwner::Node(node_id);
        let targets = self
            .project
            .connections
            .iter()
            .filter(|connection| connection.to.owner == owner)
            .map(|connection| connection.to.clone())
            .collect::<HashSet<_>>();
        for target in targets {
            let target_definition = self
                .project
                .port_definition(&target, PortDirection::Input)
                .ok_or_else(|| {
                    LibraryError::Validation(format!("Missing input port {target:?}"))
                })?;
            match target_definition.data_type {
                PortDataType::Image | PortDataType::Shape => continue,
                _ => {}
            }
            if matches!(
                target.port.as_str(),
                TIME_PORT | DURATION_PORT | RESOLUTION_PORT
            ) {
                // Authored scope overrides have already been applied by
                // scope_for_owner. Keeping a second copy in the property map
                // both re-evaluates the graph and obscures which Time is
                // authoritative.
                continue;
            }
            let connection = match self.single_connection_to(&target)? {
                EvalOutput::Produced(connection) => connection,
                EvalOutput::NoOutput => continue,
            };
            let value =
                self.resolve_metadata_value(&connection.from, global_time, &mut HashSet::new())?;
            let logical_key = property_name_from_port(&target.port).unwrap_or(&target.port);
            values.properties.insert(logical_key.to_string(), value);
        }
        Ok(values)
    }

    fn resolve_metadata_value(
        &self,
        source: &PortAddress,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let source_node = if let PortOwner::Node(node_id) = source.owner {
            let node = self
                .project
                .get_node(node_id)
                .ok_or_else(|| missing_error(source.owner))?;
            if !node.enabled {
                return Ok(EvalOutput::NoOutput);
            }
            Some(node)
        } else {
            None
        };
        let definition = self
            .project
            .port_definition(source, PortDirection::Output)
            .ok_or_else(|| LibraryError::Validation(format!("Missing output port {source:?}")))?;
        if matches!(
            definition.data_type,
            PortDataType::Image | PortDataType::Audio
        ) {
            return Err(LibraryError::Validation(format!(
                "Typed media port {source:?} cannot be resolved as a value"
            )));
        }
        if let Some(node) = source_node
            && node.bypassed
        {
            if !path.insert(source.owner) {
                return Err(cycle_error(source.owner));
            }
            let result = (|| {
                let input = node.bypass_input_for_output(&source.port).ok_or_else(|| {
                    LibraryError::Validation(format!(
                        "Node {} cannot bypass output {:?}: no unambiguous same-typed input",
                        node.id, source.port
                    ))
                })?;
                let target = PortAddress::new(source.owner, input);
                let connection = match self.single_connection_to(&target)? {
                    EvalOutput::Produced(connection) => connection,
                    EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                };
                self.resolve_metadata_value(&connection.from, global_time, path)
            })();
            path.remove(&source.owner);
            return result;
        }
        if let Some(NodeContent::CompositionInstance(instance)) = source_node.map(Node::content) {
            return match self.composition_instance_target_scope(
                source.owner.id(),
                instance,
                global_time,
                path,
            )? {
                EvalOutput::Produced(scope) => scope
                    .value(&source.port)
                    .map(EvalOutput::Produced)
                    .ok_or_else(|| {
                        LibraryError::Validation(format!(
                            "Unsupported Composition Instance metadata output {source:?}"
                        ))
                    }),
                EvalOutput::NoOutput => Ok(EvalOutput::NoOutput),
            };
        }
        if let PortOwner::Node(node_id) = source.owner
            && matches!(source_node.map(Node::content), Some(NodeContent::Value(_)))
        {
            return self.evaluate_value_node_output(node_id, &source.port, global_time, path);
        }
        if let Some(NodeContent::PluginOperation(operation)) = source_node.map(Node::content) {
            let descriptor = match self.plugin_manager.operation_descriptor(
                &operation.category,
                &operation.component_id,
                &operation.operation,
            ) {
                Ok(descriptor) => descriptor,
                Err(_) => return Ok(EvalOutput::NoOutput),
            };
            if !descriptor.is_execution_compatible_with_ports(&operation.declared_ports) {
                return Ok(EvalOutput::NoOutput);
            }
        }
        match self.scope_for_owner(source.owner, global_time, path)? {
            EvalOutput::Produced(scope) => scope
                .value(&source.port)
                .map(EvalOutput::Produced)
                .ok_or_else(|| {
                    LibraryError::Validation(format!("Unsupported value output port {source:?}"))
                }),
            EvalOutput::NoOutput => Ok(EvalOutput::NoOutput),
        }
    }

    fn evaluate_value_node_output(
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
        if !node.enabled {
            return Ok(EvalOutput::NoOutput);
        }
        if output_port != NUMBER_RESULT_OUTPUT_PORT {
            return Err(LibraryError::Validation(format!(
                "Unsupported value output port {owner:?}.{output_port}"
            )));
        }
        let scope = match self.scope_for_owner(owner, global_time, path)? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        let result = match node.content() {
            NodeContent::Value(value) => {
                self.evaluate_numeric_binary_node(node, *value, scope, global_time, path)
            }
            _ => Ok(EvalOutput::NoOutput),
        };
        path.remove(&owner);
        result
    }

    fn evaluate_numeric_binary_node(
        &self,
        node: &Node,
        value: ValueContent,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let left = match self.resolve_value_input(
            node,
            value.primary_input(),
            None,
            scope,
            global_time,
            path,
        )? {
            EvalOutput::Produced(value) => value,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let right = match self.resolve_value_input(
            node,
            value.secondary_input(),
            Some(value.secondary_input()),
            scope,
            global_time,
            path,
        )? {
            EvalOutput::Produced(value) => value,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        Ok(
            evaluate_numeric_binary(value.numeric_operation(), &left, &right)
                .map_or(EvalOutput::NoOutput, EvalOutput::Produced),
        )
    }

    fn resolve_value_input(
        &self,
        node: &Node,
        port: &str,
        property_fallback: Option<&str>,
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
                let Some(property_key) = property_fallback else {
                    return Ok(EvalOutput::NoOutput);
                };
                let Some(property) = node.properties().get(property_key) else {
                    return Ok(EvalOutput::NoOutput);
                };
                let composition = self
                    .composition_for_owner(PortOwner::Node(node.id))
                    .ok_or_else(|| missing_error(PortOwner::Node(node.id)))?;
                let inputs = ResolvedNodeInputs::from_metadata(scope.as_inputs());
                let context = self.context(composition, Some(&inputs));
                let properties = node.properties();
                let value = context.evaluate_property_value(property, properties, scope.time);
                Ok(property_evaluation::output(value, node.id, property_key))
            }
        }
    }

    fn single_connection_to<'b>(
        &'b self,
        target: &PortAddress,
    ) -> EvalResult<&'b ProjectConnection> {
        let connections = self
            .project
            .connections
            .iter()
            .filter(|connection| &connection.to == target)
            .collect::<Vec<_>>();
        if connections.is_empty() {
            return Ok(EvalOutput::NoOutput);
        }
        let definition = self
            .project
            .port_definition(target, PortDirection::Input)
            .ok_or_else(|| LibraryError::Validation(format!("Missing input port {target:?}")))?;
        if definition.multiplicity != PortMultiplicity::Single || connections.len() != 1 {
            return Err(LibraryError::Validation(format!(
                "Expected one connection to {target:?}, got {}",
                connections.len()
            )));
        }
        let connection = connections[0];
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
        Ok(EvalOutput::Produced(connection))
    }

    fn composition_for_owner(&self, owner: PortOwner) -> Option<&Composition> {
        let id = match owner {
            PortOwner::Composition(id) => id,
            PortOwner::Track(id) => self.project.find_composition_for_track(id)?,
            PortOwner::Clip(id) | PortOwner::Node(id) => {
                self.project.find_containing_composition(id)?
            }
        };
        self.project.get_composition(id)
    }

    fn context<'b>(
        &'b self,
        composition: &'b Composition,
        inputs: Option<&'b ResolvedNodeInputs>,
    ) -> FrameEvaluationContext<'b> {
        FrameEvaluationContext {
            project: self.project,
            composition,
            property_evaluators: &self.property_evaluators,
            plugin_manager: self.plugin_manager,
            resolved_inputs: inputs,
        }
    }
}

fn aggregate_outputs(items: Vec<EvalOutput<FrameItem>>) -> EvalOutput<Vec<FrameItem>> {
    let items = items
        .into_iter()
        .filter_map(|item| match item {
            EvalOutput::Produced(item) => Some(item),
            EvalOutput::NoOutput => None,
        })
        .collect::<Vec<_>>();
    if items.is_empty() {
        EvalOutput::NoOutput
    } else {
        EvalOutput::Produced(items)
    }
}

fn missing_error(owner: PortOwner) -> LibraryError {
    LibraryError::Project(format!("Graph owner {owner:?} not found"))
}

fn cycle_error(owner: PortOwner) -> LibraryError {
    LibraryError::Validation(format!("Evaluation cycle at {owner:?}"))
}

fn invalid_value(port: &str) -> LibraryError {
    LibraryError::Validation(format!("Invalid value for graph port {port}"))
}

fn required_number(value: PropertyValue, port: &str) -> Result<f64, LibraryError> {
    value.get_as::<f64>().ok_or_else(|| invalid_value(port))
}

fn frame_at_time(time: f64, fps: f64) -> i64 {
    let scaled = time * fps;
    let epsilon = scaled.abs().max(1.0) * f64::EPSILON * 8.0;
    (scaled + epsilon).floor() as i64
}

fn transparent() -> Color {
    Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }
}

fn neutralize_root_blend(item: &mut FrameItem) {
    if let FrameItem::Group(group) = item {
        group.blend_mode = crate::model::BlendMode::Normal;
    }
}

pub fn evaluate_composition_frame(
    project: &Project,
    composition: &Composition,
    frame_number: u64,
    render_scale: f64,
    region: Option<Region>,
    property_evaluators: &Arc<PropertyEvaluatorRegistry>,
    plugin_manager: &Arc<PluginManager>,
) -> Result<FrameInfo, LibraryError> {
    FrameEvaluator::new(
        project,
        composition,
        Arc::clone(property_evaluators),
        plugin_manager.as_ref(),
    )
    .evaluate(frame_number, render_scale, region)
}

pub fn get_frame_from_project(
    project: &Project,
    composition_index: usize,
    frame_number: u64,
    render_scale: f64,
    region: Option<Region>,
    property_evaluators: &Arc<PropertyEvaluatorRegistry>,
    plugin_manager: &Arc<PluginManager>,
) -> Result<FrameInfo, LibraryError> {
    let composition = project
        .compositions
        .get(composition_index)
        .ok_or(LibraryError::InvalidCompositionIndex(composition_index))?;
    let _timer = log::log_enabled!(log::Level::Debug).then(|| {
        ScopedTimer::debug(format!(
            "Frame assembly comp={composition_index} frame={frame_number}"
        ))
    });
    let frame = evaluate_composition_frame(
        project,
        composition,
        frame_number,
        render_scale,
        region,
        property_evaluators,
        plugin_manager,
    )?;
    debug!(
        "Frame {frame_number} summary: objects={}",
        frame.object_count()
    );
    Ok(frame)
}

#[cfg(test)]
mod tests;
