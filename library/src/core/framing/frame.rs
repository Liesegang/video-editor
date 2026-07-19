use log::debug;
use ordered_float::OrderedFloat;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

use crate::error::LibraryError;
use crate::model::frame::color::Color;
use crate::model::frame::entity::{
    FrameGroup, FrameGroupKind, FrameItem, FrameObject, StyleConfig,
};
use crate::model::frame::frame::{FrameInfo, Region};
use crate::model::project::{
    Composition, DECORATOR_OUTPUT_PORT, DURATION_PORT, EFFECTOR_OUTPUT_PORT, EvalOutput,
    EvalResult, FPS_PORT, FRAME_PORT, IMAGE_INPUT_PORT, MERGE_IMAGES_PORT, NodeContainer,
    PortAddress, PortDataType, PortDirection, PortMultiplicity, PortOwner, Project,
    ProjectConnection, RESOLUTION_PORT, TIME_PORT,
};
use crate::model::property::{PropertyValue, Vec2};
use crate::model::{GeneratorContent, Node, NodeContent};
use crate::plugin::{
    DECORATOR_CATEGORY, DECORATOR_PRODUCE_OPERATION, EFFECT_APPLY_OPERATION, EFFECT_CATEGORY,
    EFFECTOR_CATEGORY, EFFECTOR_PRODUCE_OPERATION, FrameEvaluationContext, PluginManager,
    PropertyEvaluatorRegistry, ResolvedNodeInputs, STYLE_CATEGORY, STYLE_PRODUCE_OPERATION,
    property_name_from_port,
};
use crate::util::timing::ScopedTimer;

#[derive(Clone, Copy, Debug)]
struct EvaluationScope {
    time: f64,
    fps: f64,
    duration: f64,
    width: u64,
    height: u64,
}

impl EvaluationScope {
    fn value(self, port: &str) -> Option<PropertyValue> {
        match port {
            TIME_PORT => Some(PropertyValue::Number(OrderedFloat(self.time))),
            FRAME_PORT => Some(PropertyValue::Integer(frame_at_time(self.time, self.fps))),
            FPS_PORT => Some(PropertyValue::Number(OrderedFloat(self.fps))),
            DURATION_PORT => Some(PropertyValue::Number(OrderedFloat(self.duration))),
            RESOLUTION_PORT => Some(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(self.width as f64),
                y: OrderedFloat(self.height as f64),
            })),
            _ => None,
        }
    }

    fn as_inputs(self) -> HashMap<String, EvalOutput<PropertyValue>> {
        [
            TIME_PORT,
            FRAME_PORT,
            FPS_PORT,
            DURATION_PORT,
            RESOLUTION_PORT,
        ]
        .into_iter()
        .filter_map(|port| {
            self.value(port)
                .map(|value| (port.to_string(), EvalOutput::Produced(value)))
        })
        .collect()
    }
}

pub struct FrameEvaluator<'a> {
    project: &'a Project,
    composition: &'a Composition,
    property_evaluators: Arc<PropertyEvaluatorRegistry>,
    plugin_manager: Arc<PluginManager>,
}

impl<'a> FrameEvaluator<'a> {
    pub fn new(
        project: &'a Project,
        composition: &'a Composition,
        property_evaluators: Arc<PropertyEvaluatorRegistry>,
        plugin_manager: Arc<PluginManager>,
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
        let scope = match self.scope_for_owner(owner, global_time, &mut HashSet::new())? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => {
                path.remove(&owner);
                return Ok(EvalOutput::NoOutput);
            }
        };
        let items = self.collect_container_image_items(owner, scope, global_time, path);
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
        let items = match self.collect_container_image_items(owner, scope, global_time, path)? {
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
        let context = self.context(composition, Some(&inputs));
        let item = FrameItem::Group(FrameGroup {
            source_id: track.id,
            kind: FrameGroupKind::Track,
            width: scope.width,
            height: scope.height,
            background_color: transparent(),
            transform: context.build_transform(&track.properties, scope.time),
            blend_mode: track.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: context.build_image_effects(&track.effects, scope.time),
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
        let items = match self.collect_container_image_items(owner, scope, global_time, path)? {
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
        let context = self.context(composition, Some(&inputs));
        let item = FrameItem::Group(FrameGroup {
            source_id: clip.id,
            kind: FrameGroupKind::Clip,
            width: scope.width,
            height: scope.height,
            background_color: transparent(),
            transform: context.build_transform(&clip.properties, scope.time),
            blend_mode: clip.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: context.build_image_effects(&clip.effects, scope.time),
            items,
        });
        path.remove(&owner);
        Ok(EvalOutput::Produced(item))
    }

    fn collect_container_image_items(
        &self,
        owner: PortOwner,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<Vec<FrameItem>> {
        let mut candidates = Vec::new();
        for source in self.project.container_image_sources(owner) {
            let item = match source.source {
                PortOwner::Node(node_id) => self.collect_node(node_id, scope, global_time, path)?,
                source_owner => self.collect_owner_output(source_owner, global_time, path)?,
            };
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
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        if let NodeContent::PluginOperation(operation) = &node.content {
            let item = if operation.category == EFFECT_CATEGORY
                && operation.operation == EFFECT_APPLY_OPERATION
            {
                self.collect_effect_operation(node, operation, scope, global_time, path)?
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
        let item = match &node.content {
            NodeContent::Reference(reference) => {
                self.collect_reference(node, reference, scope, global_time, path, &inputs)?
            }
            NodeContent::Merge => self.collect_merge(node, scope, global_time, path, &inputs)?,
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
            &node.properties,
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

    fn collect_reference(
        &self,
        node: &Node,
        reference: &crate::model::ReferenceContent,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
        inputs: &ResolvedNodeInputs,
    ) -> EvalResult<FrameItem> {
        let owner = PortOwner::Node(node.id);
        let input = PortAddress::new(owner, IMAGE_INPUT_PORT);
        let (mut item, width, height) = match self.single_connection_to(&input)? {
            EvalOutput::Produced(connection) => {
                let item =
                    match self.collect_owner_output(connection.from.owner, global_time, path)? {
                        EvalOutput::Produced(item) => item,
                        EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                    };
                let source_scope = match self.scope_for_owner(
                    connection.from.owner,
                    global_time,
                    &mut HashSet::new(),
                )? {
                    EvalOutput::Produced(scope) => scope,
                    EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                };
                (item, source_scope.width, source_scope.height)
            }
            EvalOutput::NoOutput => {
                // Only the absence of a connection enables Reference fallback.
                let target = self
                    .project
                    .get_composition(reference.target_id)
                    .ok_or_else(|| missing_error(PortOwner::Composition(reference.target_id)))?;
                let target_time = if reference.sync_global_time {
                    global_time
                } else {
                    scope.time
                };
                let item = match self.collect_owner_output(
                    PortOwner::Composition(target.id),
                    target_time,
                    path,
                )? {
                    EvalOutput::Produced(item) => item,
                    EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                };
                let target_scope = match self.scope_for_owner(
                    PortOwner::Composition(target.id),
                    target_time,
                    &mut HashSet::new(),
                )? {
                    EvalOutput::Produced(scope) => scope,
                    EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                };
                (item, target_scope.width, target_scope.height)
            }
        };
        neutralize_root_blend(&mut item);
        let composition = self
            .composition_for_owner(owner)
            .ok_or_else(|| missing_error(owner))?;
        let context = self.context(composition, Some(inputs));
        Ok(EvalOutput::Produced(FrameItem::Group(FrameGroup {
            source_id: node.id,
            kind: FrameGroupKind::ConnectedImage,
            width,
            height,
            background_color: transparent(),
            transform: context.build_transform(&node.properties, scope.time),
            blend_mode: node.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: context.build_image_effects(&node.effects, scope.time),
            items: vec![item],
        })))
    }

    fn collect_merge(
        &self,
        node: &Node,
        scope: EvaluationScope,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
        inputs: &ResolvedNodeInputs,
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
            let blend_mode = if items.is_empty() {
                crate::model::BlendMode::Normal
            } else {
                self.blend_mode_for_owner(connection.from.owner)
            };
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
        let composition = self
            .composition_for_owner(owner)
            .ok_or_else(|| missing_error(owner))?;
        let context = self.context(composition, Some(inputs));
        Ok(EvalOutput::Produced(FrameItem::Group(FrameGroup {
            source_id: node.id,
            kind: FrameGroupKind::Merge,
            width: scope.width,
            height: scope.height,
            background_color: transparent(),
            transform: context.build_transform(&node.properties, scope.time),
            blend_mode: node.blend_mode,
            effect_time: OrderedFloat(scope.time),
            effects: context.build_image_effects(&node.effects, scope.time),
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
                    effects: context.build_image_effects(&composition.effects, scope.time),
                    items,
                })))
            }
            PortOwner::Track(id) => self.collect_track(id, global_time, path),
            PortOwner::Clip(id) => self.collect_clip(id, global_time, path),
            PortOwner::Node(id) => match self.scope_for_node(id, global_time)? {
                EvalOutput::Produced(scope) => self.collect_node(id, scope, global_time, path),
                EvalOutput::NoOutput => Ok(EvalOutput::NoOutput),
            },
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
        let kind = match &node.content {
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
            NodeContent::Reference(_) => "reference",
            NodeContent::PluginOperation(_) => return Ok(EvalOutput::NoOutput),
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
            && let NodeContent::Media(media) = &node.content
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
                PortDataType::Image => continue,
                PortDataType::Style => {
                    let connections = self.ordered_variadic_connections_to(&target)?;
                    if connections.is_empty() {
                        // No map entry is the legacy fallback signal. A map
                        // entry with one or more NoOutput values is not.
                        continue;
                    }
                    let mut styles = Vec::with_capacity(connections.len());
                    for connection in connections {
                        styles.push(self.evaluate_style_source(&connection.from, global_time)?);
                    }
                    // The entry itself records wire presence. An all-NoOutput
                    // vector must not fall back to legacy Node::styles.
                    values.styles.insert(target.port, styles);
                    continue;
                }
                PortDataType::Effector => {
                    let connections = self.ordered_variadic_connections_to(&target)?;
                    if connections.is_empty() {
                        continue;
                    }
                    let mut effectors = Vec::with_capacity(connections.len());
                    for connection in connections {
                        effectors
                            .push(self.evaluate_effector_source(&connection.from, global_time)?);
                    }
                    // Wire presence remains authoritative even when every
                    // producer yields NoOutput.
                    values.effectors.insert(target.port, effectors);
                    continue;
                }
                PortDataType::Decorator => {
                    let connections = self.ordered_variadic_connections_to(&target)?;
                    if connections.is_empty() {
                        continue;
                    }
                    let mut decorators = Vec::with_capacity(connections.len());
                    for connection in connections {
                        decorators
                            .push(self.evaluate_decorator_source(&connection.from, global_time)?);
                    }
                    values.decorators.insert(target.port, decorators);
                    continue;
                }
                _ => {}
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

    fn ordered_variadic_connections_to<'b>(
        &'b self,
        target: &PortAddress,
    ) -> Result<Vec<&'b ProjectConnection>, LibraryError> {
        let definition = self
            .project
            .port_definition(target, PortDirection::Input)
            .ok_or_else(|| LibraryError::Validation(format!("Missing input port {target:?}")))?;
        if definition.multiplicity != PortMultiplicity::Variadic {
            return Err(LibraryError::Validation(format!(
                "Expected variadic input {target:?}"
            )));
        }
        let mut connections = self
            .project
            .connections
            .iter()
            .filter(|connection| &connection.to == target)
            .collect::<Vec<_>>();
        connections.sort_by_key(|connection| (connection.order, connection.id));
        for pair in connections.windows(2) {
            if pair[0].order == pair[1].order {
                return Err(LibraryError::Validation(format!(
                    "Variadic input {target:?} has duplicate order {}",
                    pair[0].order
                )));
            }
        }
        for connection in &connections {
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
        }
        Ok(connections)
    }

    fn evaluate_style_source(
        &self,
        source: &PortAddress,
        global_time: f64,
    ) -> EvalResult<StyleConfig> {
        let PortOwner::Node(node_id) = source.owner else {
            log::warn!("Style source {source:?} is not a Node; producing NoOutput");
            return Ok(EvalOutput::NoOutput);
        };
        let scope = match self.scope_for_node(node_id, global_time)? {
            EvalOutput::Produced(scope) => scope,
            // Activity is checked before any plugin descriptor or authored
            // property evaluation.
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let Some(node) = self.project.get_node(node_id) else {
            log::warn!("Style source Node {node_id} is unavailable; producing NoOutput");
            return Ok(EvalOutput::NoOutput);
        };
        let NodeContent::PluginOperation(operation) = &node.content else {
            log::warn!("Style source Node {node_id} is not a plugin operation; producing NoOutput");
            return Ok(EvalOutput::NoOutput);
        };
        if operation.category != STYLE_CATEGORY || operation.operation != STYLE_PRODUCE_OPERATION {
            log::warn!(
                "Unknown Style operation {}/{}/{} on Node {}; producing NoOutput",
                operation.category,
                operation.component_id,
                operation.operation,
                node.id
            );
            return Ok(EvalOutput::NoOutput);
        }
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
        let Some(composition) = self.composition_for_owner(source.owner) else {
            log::warn!(
                "Style source Node {} has no containing Composition; producing NoOutput",
                node.id
            );
            return Ok(EvalOutput::NoOutput);
        };
        let context = self.context(composition, Some(&inputs));
        match self.plugin_manager.evaluate_style_operation(
            &context,
            &operation.component_id,
            node.id,
            &node.properties,
            scope.time,
        ) {
            EvalOutput::Produced(style) => Ok(EvalOutput::Produced(style)),
            EvalOutput::NoOutput => {
                log::warn!(
                    "Style operation {}/{}/{} on Node {} produced NoOutput",
                    operation.category,
                    operation.component_id,
                    operation.operation,
                    node.id
                );
                Ok(EvalOutput::NoOutput)
            }
        }
    }

    fn evaluate_effector_source(
        &self,
        source: &PortAddress,
        global_time: f64,
    ) -> EvalResult<crate::core::ensemble::types::EffectorConfig> {
        let PortOwner::Node(node_id) = source.owner else {
            log::warn!("Effector source {source:?} is not a Node; producing NoOutput");
            return Ok(EvalOutput::NoOutput);
        };
        if source.port != EFFECTOR_OUTPUT_PORT {
            log::warn!(
                "Effector source {source:?} is not the canonical output; producing NoOutput"
            );
            return Ok(EvalOutput::NoOutput);
        }
        let scope = match self.scope_for_node(node_id, global_time)? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let Some(node) = self.project.get_node(node_id) else {
            log::warn!("Effector source Node {node_id} is unavailable; producing NoOutput");
            return Ok(EvalOutput::NoOutput);
        };
        let NodeContent::PluginOperation(operation) = &node.content else {
            log::warn!(
                "Effector source Node {node_id} is not a plugin operation; producing NoOutput"
            );
            return Ok(EvalOutput::NoOutput);
        };
        if operation.category != EFFECTOR_CATEGORY
            || operation.operation != EFFECTOR_PRODUCE_OPERATION
        {
            log::warn!(
                "Unknown Effector operation {}/{}/{} on Node {}; producing NoOutput",
                operation.category,
                operation.component_id,
                operation.operation,
                node.id
            );
            return Ok(EvalOutput::NoOutput);
        }
        let descriptor = match self.plugin_manager.operation_descriptor(
            &operation.category,
            &operation.component_id,
            &operation.operation,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Unavailable Effector operation {}/{}/{} on Node {}: {}; producing NoOutput",
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
                "Effector operation contract mismatch on Node {}; producing NoOutput",
                node.id
            );
            return Ok(EvalOutput::NoOutput);
        }
        let inputs = self.resolve_node_inputs(node.id, scope, global_time)?;
        let Some(composition) = self.composition_for_owner(source.owner) else {
            log::warn!(
                "Effector source Node {} has no containing Composition; producing NoOutput",
                node.id
            );
            return Ok(EvalOutput::NoOutput);
        };
        let context = self.context(composition, Some(&inputs));
        Ok(self.plugin_manager.evaluate_effector_operation(
            &context,
            &operation.component_id,
            node.id,
            &node.properties,
            scope.time,
        ))
    }

    fn evaluate_decorator_source(
        &self,
        source: &PortAddress,
        global_time: f64,
    ) -> EvalResult<crate::core::ensemble::types::DecoratorConfig> {
        let PortOwner::Node(node_id) = source.owner else {
            log::warn!("Decorator source {source:?} is not a Node; producing NoOutput");
            return Ok(EvalOutput::NoOutput);
        };
        if source.port != DECORATOR_OUTPUT_PORT {
            log::warn!(
                "Decorator source {source:?} is not the canonical output; producing NoOutput"
            );
            return Ok(EvalOutput::NoOutput);
        }
        let scope = match self.scope_for_node(node_id, global_time)? {
            EvalOutput::Produced(scope) => scope,
            EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
        };
        let Some(node) = self.project.get_node(node_id) else {
            log::warn!("Decorator source Node {node_id} is unavailable; producing NoOutput");
            return Ok(EvalOutput::NoOutput);
        };
        let NodeContent::PluginOperation(operation) = &node.content else {
            log::warn!(
                "Decorator source Node {node_id} is not a plugin operation; producing NoOutput"
            );
            return Ok(EvalOutput::NoOutput);
        };
        if operation.category != DECORATOR_CATEGORY
            || operation.operation != DECORATOR_PRODUCE_OPERATION
        {
            log::warn!(
                "Unknown Decorator operation {}/{}/{} on Node {}; producing NoOutput",
                operation.category,
                operation.component_id,
                operation.operation,
                node.id
            );
            return Ok(EvalOutput::NoOutput);
        }
        let descriptor = match self.plugin_manager.operation_descriptor(
            &operation.category,
            &operation.component_id,
            &operation.operation,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Unavailable Decorator operation {}/{}/{} on Node {}: {}; producing NoOutput",
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
                "Decorator operation contract mismatch on Node {}; producing NoOutput",
                node.id
            );
            return Ok(EvalOutput::NoOutput);
        }
        let inputs = self.resolve_node_inputs(node.id, scope, global_time)?;
        let Some(composition) = self.composition_for_owner(source.owner) else {
            log::warn!(
                "Decorator source Node {} has no containing Composition; producing NoOutput",
                node.id
            );
            return Ok(EvalOutput::NoOutput);
        };
        let context = self.context(composition, Some(&inputs));
        Ok(self.plugin_manager.evaluate_decorator_operation(
            &context,
            &operation.component_id,
            node.id,
            &node.properties,
            scope.time,
        ))
    }

    fn scope_for_node(&self, node_id: Uuid, global_time: f64) -> EvalResult<EvaluationScope> {
        let container = self
            .project
            .find_node_container(node_id)
            .ok_or_else(|| missing_error(PortOwner::Node(node_id)))?;
        let owner = match container {
            NodeContainer::Composition(id) => PortOwner::Composition(id),
            NodeContainer::Track(id) => PortOwner::Track(id),
            NodeContainer::Clip(id) => PortOwner::Clip(id),
        };
        self.scope_for_owner(owner, global_time, &mut HashSet::new())
    }

    fn scope_for_owner(
        &self,
        owner: PortOwner,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<EvaluationScope> {
        if !path.insert(owner) {
            return Err(cycle_error(owner));
        }
        let result = (|| {
            let mut scope = match owner {
                PortOwner::Composition(id) => {
                    let composition = self
                        .project
                        .get_composition(id)
                        .ok_or_else(|| missing_error(owner))?;
                    EvaluationScope {
                        time: global_time,
                        fps: composition.fps,
                        duration: composition.duration,
                        width: composition.width,
                        height: composition.height,
                    }
                }
                PortOwner::Track(id) => {
                    let composition_id = self
                        .project
                        .find_composition_for_track(id)
                        .ok_or_else(|| missing_error(owner))?;
                    match self.scope_for_owner(
                        PortOwner::Composition(composition_id),
                        global_time,
                        path,
                    )? {
                        EvalOutput::Produced(scope) => scope,
                        EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                    }
                }
                PortOwner::Clip(id) => {
                    let clip = self
                        .project
                        .get_clip(id)
                        .ok_or_else(|| missing_error(owner))?;
                    let track_id = self
                        .project
                        .find_track_for_clip(id)
                        .ok_or_else(|| missing_error(owner))?;
                    let mut inherited = match self.scope_for_owner(
                        PortOwner::Track(track_id),
                        global_time,
                        path,
                    )? {
                        EvalOutput::Produced(scope) => scope,
                        EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                    };
                    // Clip activity is start-inclusive and end-exclusive. No Node,
                    // shader, effect, media request, or value output is evaluated
                    // while this test is false.
                    if inherited.time < clip.start_time.into_inner()
                        || inherited.time >= clip.end_time()
                    {
                        return Ok(EvalOutput::NoOutput);
                    }
                    inherited.duration = clip.duration.into_inner();
                    match self.apply_metadata_inputs(owner, global_time, path, &mut inherited)? {
                        EvalOutput::Produced(()) => {}
                        EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                    }
                    inherited.time = clip.local_time(inherited.time);
                    return Ok(EvalOutput::Produced(inherited));
                }
                PortOwner::Node(id) => {
                    let container = self
                        .project
                        .find_node_container(id)
                        .ok_or_else(|| missing_error(owner))?;
                    let container_owner = match container {
                        NodeContainer::Composition(id) => PortOwner::Composition(id),
                        NodeContainer::Track(id) => PortOwner::Track(id),
                        NodeContainer::Clip(id) => PortOwner::Clip(id),
                    };
                    match self.scope_for_owner(container_owner, global_time, path)? {
                        EvalOutput::Produced(scope) => scope,
                        EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
                    }
                }
            };
            match self.apply_metadata_inputs(owner, global_time, path, &mut scope)? {
                EvalOutput::Produced(()) => {}
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            }
            Ok(EvalOutput::Produced(scope))
        })();
        path.remove(&owner);
        result
    }

    fn apply_metadata_inputs(
        &self,
        owner: PortOwner,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
        scope: &mut EvaluationScope,
    ) -> EvalResult<()> {
        for port in [
            FPS_PORT,
            DURATION_PORT,
            RESOLUTION_PORT,
            FRAME_PORT,
            TIME_PORT,
        ] {
            let target = PortAddress::new(owner, port);
            let connection = match self.single_connection_to(&target)? {
                EvalOutput::Produced(connection) => connection,
                EvalOutput::NoOutput => continue,
            };
            let value = match self.resolve_metadata_value(&connection.from, global_time, path)? {
                EvalOutput::Produced(value) => value,
                EvalOutput::NoOutput => return Ok(EvalOutput::NoOutput),
            };
            match (port, value) {
                (TIME_PORT, value) => {
                    scope.time = required_number(value, port)?;
                }
                (FRAME_PORT, value) => {
                    let frame = value.get_as::<i64>().ok_or_else(|| invalid_value(port))?;
                    scope.time = frame as f64 / scope.fps;
                }
                (FPS_PORT, value) => {
                    let fps = required_number(value, port)?;
                    if !fps.is_finite() || fps <= 0.0 {
                        return Err(invalid_value(port));
                    }
                    scope.fps = fps;
                }
                (DURATION_PORT, value) => scope.duration = required_number(value, port)?,
                (RESOLUTION_PORT, PropertyValue::Vec2(value)) => {
                    scope.width = value.x.into_inner().max(1.0) as u64;
                    scope.height = value.y.into_inner().max(1.0) as u64;
                }
                _ => return Err(invalid_value(port)),
            }
        }
        Ok(EvalOutput::Produced(()))
    }

    fn resolve_metadata_value(
        &self,
        source: &PortAddress,
        global_time: f64,
        path: &mut HashSet<PortOwner>,
    ) -> EvalResult<PropertyValue> {
        let definition = self
            .project
            .port_definition(source, PortDirection::Output)
            .ok_or_else(|| LibraryError::Validation(format!("Missing output port {source:?}")))?;
        if definition.data_type == PortDataType::Image {
            return Err(LibraryError::Validation(format!(
                "Image port {source:?} cannot be resolved as a value"
            )));
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

    fn blend_mode_for_owner(&self, owner: PortOwner) -> crate::model::BlendMode {
        match owner {
            PortOwner::Composition(id) => {
                self.project.get_composition(id).map(|item| item.blend_mode)
            }
            PortOwner::Track(id) => self.project.get_track(id).map(|item| item.blend_mode),
            PortOwner::Clip(id) => self.project.get_clip(id).map(|item| item.blend_mode),
            PortOwner::Node(id) => self.project.get_node(id).map(|item| item.blend_mode),
        }
        .unwrap_or_default()
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
            plugin_manager: &self.plugin_manager,
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
        Arc::clone(plugin_manager),
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
