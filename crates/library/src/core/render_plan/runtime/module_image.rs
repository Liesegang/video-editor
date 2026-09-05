//! Stateless evaluation of one compiled Image Module invocation.

use super::frame_values::{
    neutralize_root_blend, required_color, required_number, required_string, solid_item,
    transparent,
};
use super::*;
use crate::core::render_plan::CompiledModuleOutput;
use crate::model::authoring::PublishedParameterId;
use crate::model::frame::entity::{
    FrameTransition, FrameTransitionKind, FrameTransitionSource, NormalizedProgress16,
};
use crate::model::node::{
    TRANSITION_IMAGE_INPUT_NODE_ID, TRANSITION_IMAGE_MIX_NODE_ID, TRANSITION_PROGRESS_INPUT_NODE_ID,
};
use crate::model::project::{
    TRANSITION_FROM_INPUT_PORT, TRANSITION_PROGRESS_INPUT_PORT, TRANSITION_TO_INPUT_PORT,
};
use crate::plugin::EvaluationContext;

#[derive(Clone, Copy)]
pub(super) struct TransitionImageSourceContext {
    pub(super) item_id: uuid::Uuid,
    pub(super) source_time: OrderedFloat<f64>,
}

#[derive(Clone, Copy)]
pub(super) struct TransitionImageContext {
    pub(super) transition_id: uuid::Uuid,
    pub(super) timeline_time: OrderedFloat<f64>,
    pub(super) from: TransitionImageSourceContext,
    pub(super) to: TransitionImageSourceContext,
}

pub(super) struct ModuleImageRuntime<'a> {
    pub(super) project: &'a AuthoringProject,
    pub(super) definition: &'a CompiledModuleDefinition,
    pub(super) invocation: &'a CompiledModuleInvocation,
    pub(super) instance_path: &'a InstancePath,
    pub(super) local_time: MediaTime,
    pub(super) width: u64,
    pub(super) height: u64,
    pub(super) evaluation_fps: f64,
    pub(super) plugins: &'a crate::plugin::PluginManager,
    pub(super) external_images: HashMap<ModulePortAddress, FrameItem>,
    /// Values owned by the invoking host. Transition Progress is injected
    /// here so neither instance overrides nor automation can replace the
    /// Timeline's normalized clock.
    pub(super) host_parameters: HashMap<PublishedParameterId, PropertyValue>,
    pub(super) transition_context: Option<TransitionImageContext>,
    pub(super) image_memo: HashMap<(uuid::Uuid, String), Option<FrameItem>>,
    pub(super) image_path: HashSet<(uuid::Uuid, String)>,
    pub(super) shape_memo:
        HashMap<(uuid::Uuid, String), Option<crate::model::frame::runtime_shape::RuntimeShape>>,
    pub(super) shape_path: HashSet<(uuid::Uuid, String)>,
    pub(super) value_memo: HashMap<(uuid::Uuid, String), Option<PropertyValue>>,
    pub(super) value_path: HashSet<(uuid::Uuid, String)>,
}

impl ModuleImageRuntime<'_> {
    #[expect(
        clippy::too_many_arguments,
        reason = "one Module invocation keeps its compiled graph, host values, media, and render context explicit"
    )]
    pub(super) fn new<'a>(
        project: &'a AuthoringProject,
        definition: &'a CompiledModuleDefinition,
        invocation: &'a CompiledModuleInvocation,
        instance_path: &'a InstancePath,
        local_time: MediaTime,
        width: u64,
        height: u64,
        evaluation_fps: f64,
        plugins: &'a crate::plugin::PluginManager,
        external_images: HashMap<ModulePortAddress, FrameItem>,
        host_parameters: HashMap<PublishedParameterId, PropertyValue>,
        transition_context: Option<TransitionImageContext>,
    ) -> ModuleImageRuntime<'a> {
        ModuleImageRuntime {
            project,
            definition,
            invocation,
            instance_path,
            local_time,
            width,
            height,
            evaluation_fps,
            plugins,
            external_images,
            host_parameters,
            transition_context,
            image_memo: HashMap::new(),
            image_path: HashSet::new(),
            shape_memo: HashMap::new(),
            shape_path: HashSet::new(),
            value_memo: HashMap::new(),
            value_path: HashSet::new(),
        }
    }

    pub(super) fn evaluate_terminal(
        &mut self,
        output: &CompiledModuleOutput,
    ) -> Result<Option<FrameItem>, LibraryError> {
        let image_target = output
            .terminal
            .target(crate::model::project::PortDataType::Image)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Module Output {} has no Image input",
                    output.terminal.id
                ))
            })?;
        if let Some(external) = self.external_images.get(&image_target) {
            return Ok(Some(external.clone()));
        }
        let Some(source) = output.source(crate::model::project::PortDataType::Image) else {
            return Ok(None);
        };
        self.evaluate_image_output(source)
    }

    pub(super) fn evaluate_image_output(
        &mut self,
        source: &ModulePortAddress,
    ) -> Result<Option<FrameItem>, LibraryError> {
        let key = (source.node_id, source.port.clone());
        if let Some(cached) = self.image_memo.get(&key) {
            return Ok(cached.clone());
        }
        if !self.image_path.insert(key.clone()) {
            return Err(LibraryError::Validation(format!(
                "Module Image cycle reaches {}:{}",
                source.node_id, source.port
            )));
        }
        let result = self.evaluate_image_output_inner(source);
        self.image_path.remove(&key);
        if let Ok(value) = &result {
            self.image_memo.insert(key, value.clone());
        }
        result
    }

    fn evaluate_image_output_inner(
        &mut self,
        source: &ModulePortAddress,
    ) -> Result<Option<FrameItem>, LibraryError> {
        let node = self
            .definition
            .nodes
            .get(&source.node_id)
            .cloned()
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Compiled Module output reaches missing Node {}",
                    source.node_id
                ))
            })?;
        if !node.enabled {
            return Ok(None);
        }
        if node.bypassed {
            if let Some(input) = node.bypass_routes.get(&source.port) {
                return self.single_image_input(node.id, input);
            }
            if matches!(
                &node.content,
                NodeContent::NativeOperation(operation)
                    if operation.catalog_id
                        == crate::model::node::PARTICLE_SPRITE_RENDERER_CATALOG_ID
            ) {
                // ParticleSystem cannot be passed through an Image output.
                // Its endpoint bypass state therefore has the same stable
                // no-output result as a disabled endpoint.
                return Ok(None);
            }
            return Err(LibraryError::Validation(format!(
                "Node {} has no bypass route for '{}'",
                node.id, source.port
            )));
        }
        match &node.content {
            NodeContent::ModuleOutput(_) => Err(LibraryError::Validation(format!(
                "Module Output Node {} reached the executable Node evaluator",
                node.id
            ))),
            NodeContent::Generator(generator) => self.generator_image(&node, *generator),
            NodeContent::Media(media) => self.module_media_image(&node, media),
            NodeContent::Merge => self.merge_image(&node),
            NodeContent::PluginOperation(operation)
                if operation.category == STYLE_CATEGORY
                    && operation.operation == STYLE_APPLY_OPERATION
                    && operation.component_id != IMAGE_OPACITY_STYLE_COMPONENT_ID =>
            {
                self.style_shape_image(&node, operation)
            }
            NodeContent::PluginOperation(operation) => {
                self.plugin_operation_image(&node, operation)
            }
            NodeContent::NativeOperation(operation) => {
                self.native_operation_image(&node, &operation.catalog_id)
            }
            NodeContent::CompositionInstance(_) => Err(LibraryError::Validation(
                "Composition instances are not permitted inside Module definitions".to_string(),
            )),
            other => Err(LibraryError::Render(format!(
                "Module Node {} ({other:?}) does not produce a stateless Image",
                node.id
            ))),
        }
    }

    fn generator_image(
        &mut self,
        node: &CompiledNode,
        generator: GeneratorContent,
    ) -> Result<Option<FrameItem>, LibraryError> {
        let values = self.node_values(node)?;
        let item = match generator {
            GeneratorContent::Solid => {
                let color = required_color(&values, "color", "Solid Generator")?;
                solid_item(node.id, self.width, self.height, color, node.blend_mode)
            }
            GeneratorContent::SkSL => {
                let shader = required_string(&values, "shader", "SkSL Generator")?;
                let width = required_number(&values, "width", "SkSL Generator")? as f32;
                let height = required_number(&values, "height", "SkSL Generator")? as f32;
                FrameItem::Group(FrameGroup {
                    source_id: node.id,
                    kind: FrameGroupKind::Node,
                    width: self.width,
                    height: self.height,
                    background_color: transparent(),
                    transform: Transform::default(),
                    blend_mode: node.blend_mode,
                    effect_time: OrderedFloat(self.local_time.to_seconds_f64()),
                    effects: Vec::new(),
                    items: vec![FrameItem::Object(FrameObject {
                        source_node_id: node.id,
                        spatial_transform_node_id: None,
                        spatial_transform: Box::default(),
                        content_bounds: Some(FrameBounds::new(0.0, 0.0, width, height)),
                        content: FrameContent::SkSL {
                            shader,
                            resolution: (width, height),
                            color_domain: SkSLColorDomain::ProjectWorkingLinear,
                            effects: Vec::new(),
                            transform: Transform::default(),
                        },
                    })],
                })
            }
            GeneratorContent::Text | GeneratorContent::Shape => {
                return Err(LibraryError::Render(format!(
                    "Generator Node {} produces Shape; add an Image Style before publishing it as Image",
                    node.id
                )));
            }
        };
        Ok(Some(item))
    }

    fn module_media_image(
        &self,
        node: &CompiledNode,
        media: &crate::model::node::MediaContent,
    ) -> Result<Option<FrameItem>, LibraryError> {
        let asset = self
            .project
            .assets
            .iter()
            .find(|asset| asset.id == media.asset_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Module Media Node {} refers to missing Asset {}",
                    node.id, media.asset_id
                ))
            })?;
        if !matches!(asset.kind, AssetKind::Image | AssetKind::Video) {
            return Ok(None);
        }
        let seconds = self.local_time.to_seconds_f64();
        if let Some(frame) = asset.source_frame_number_at(seconds, self.evaluation_fps)
            && !asset.contains_source_frame(frame)
        {
            return Ok(None);
        }
        let surface = ImageSurface {
            asset_id: Some(asset.id),
            file_path: asset.path.clone(),
            effects: Vec::new(),
            input_color_space: None,
            output_color_space: None,
            transform: Transform::default(),
        };
        let content = match asset.kind {
            AssetKind::Image => FrameContent::Image { surface },
            AssetKind::Video => FrameContent::Video {
                surface,
                source_time: seconds,
                stream_index: media.stream_index.or(asset.stream_index),
            },
            AssetKind::Audio | AssetKind::Model3D | AssetKind::Other => return Ok(None),
        };
        Ok(Some(FrameItem::Group(FrameGroup {
            source_id: node.id,
            kind: FrameGroupKind::Node,
            width: self.width,
            height: self.height,
            background_color: transparent(),
            transform: Transform::default(),
            blend_mode: node.blend_mode,
            effect_time: OrderedFloat(seconds),
            effects: Vec::new(),
            items: vec![FrameItem::Object(FrameObject {
                source_node_id: node.id,
                spatial_transform_node_id: None,
                spatial_transform: Box::default(),
                content_bounds: match (asset.width, asset.height) {
                    (Some(width), Some(height)) => {
                        Some(FrameBounds::new(0.0, 0.0, width as f32, height as f32))
                    }
                    _ => None,
                },
                content,
            })],
        })))
    }

    fn merge_image(&mut self, node: &CompiledNode) -> Result<Option<FrameItem>, LibraryError> {
        let items = self.image_inputs(node.id, MERGE_IMAGES_PORT)?;
        if items.is_empty() {
            return Ok(None);
        }
        Ok(Some(FrameItem::Group(FrameGroup {
            source_id: node.id,
            kind: FrameGroupKind::Merge,
            width: self.width,
            height: self.height,
            background_color: transparent(),
            transform: Transform::default(),
            blend_mode: node.blend_mode,
            effect_time: OrderedFloat(self.local_time.to_seconds_f64()),
            effects: Vec::new(),
            items,
        })))
    }

    fn plugin_operation_image(
        &mut self,
        node: &CompiledNode,
        operation: &crate::model::node::PluginOperationContent,
    ) -> Result<Option<FrameItem>, LibraryError> {
        let mut source = match self.single_image_input(node.id, IMAGE_INPUT_PORT)? {
            Some(source) => source,
            None => return Ok(None),
        };
        neutralize_root_blend(&mut source);
        let values = self.node_values(node)?;
        let seconds = self.local_time.to_seconds_f64();
        if operation.category == EFFECT_CATEGORY && operation.operation == EFFECT_APPLY_OPERATION {
            return Ok(Some(FrameItem::Group(FrameGroup {
                source_id: node.id,
                kind: FrameGroupKind::Effect,
                width: self.width,
                height: self.height,
                background_color: transparent(),
                transform: Transform::default(),
                blend_mode: node.blend_mode,
                effect_time: OrderedFloat(seconds),
                effects: vec![ImageEffect {
                    effect_type: operation.component_id.clone(),
                    properties: values,
                }],
                items: vec![source],
            })));
        }
        if operation.category == TRANSFORM_CATEGORY
            && operation.component_id == IMAGE_TRANSFORM_COMPONENT_ID
            && operation.operation == TRANSFORM_APPLY_OPERATION
        {
            return Ok(Some(FrameItem::Group(FrameGroup {
                source_id: node.id,
                kind: FrameGroupKind::ImageTransform,
                width: self.width,
                height: self.height,
                background_color: transparent(),
                transform: crate::plugin::transforms::transform_from_values(&values).ok_or_else(
                    || {
                        LibraryError::Render(format!(
                            "Image Transform Module Node {} has invalid resolved properties",
                            node.id
                        ))
                    },
                )?,
                blend_mode: node.blend_mode,
                effect_time: OrderedFloat(seconds),
                effects: Vec::new(),
                items: vec![source],
            })));
        }
        if operation.category == STYLE_CATEGORY
            && operation.component_id == IMAGE_OPACITY_STYLE_COMPONENT_ID
            && operation.operation == STYLE_APPLY_OPERATION
        {
            let opacity = required_number(&values, "opacity", "Image Opacity")?;
            return Ok(Some(FrameItem::Group(FrameGroup {
                source_id: node.id,
                kind: FrameGroupKind::ImageStyle,
                width: self.width,
                height: self.height,
                background_color: transparent(),
                transform: Transform {
                    opacity,
                    ..Transform::default()
                },
                blend_mode: node.blend_mode,
                effect_time: OrderedFloat(seconds),
                effects: Vec::new(),
                items: vec![source],
            })));
        }
        Err(LibraryError::Render(format!(
            "Plugin operation {}/{}/{} has no stateless Image runtime",
            operation.category, operation.component_id, operation.operation
        )))
    }

    fn native_operation_image(
        &mut self,
        node: &CompiledNode,
        catalog_id: &str,
    ) -> Result<Option<FrameItem>, LibraryError> {
        match catalog_id {
            crate::model::node::APPEARANCE_STACK_CATALOG_ID => self.appearance_stack_image(node),
            crate::model::node::PARTICLE_SPRITE_RENDERER_CATALOG_ID => {
                let Some(particle) = self.definition.particle_renderers.get(&node.id).cloned()
                else {
                    // The compiler validates Particle topology even when a
                    // chain is inactive. A disabled stage, or an endpoint
                    // with no type-preserving bypass, deterministically
                    // produces no Image like every other disabled Module path.
                    return Ok(None);
                };
                self.evaluate_particle_renderer(self.invocation.output_id, &particle)
                    .map(Some)
            }
            TRANSITION_IMAGE_INPUT_NODE_ID => self.single_image_input(node.id, IMAGE_INPUT_PORT),
            TRANSITION_IMAGE_MIX_NODE_ID => self.transition_image_mix(node),
            _ => Err(LibraryError::Render(format!(
                "Native Module operation '{catalog_id}' has no stateless Image runtime"
            ))),
        }
    }

    fn transition_image_mix(
        &mut self,
        node: &CompiledNode,
    ) -> Result<Option<FrameItem>, LibraryError> {
        let from = self
            .single_image_input(node.id, TRANSITION_FROM_INPUT_PORT)?
            .ok_or_else(|| {
                LibraryError::Render(format!(
                    "Transition Image Mix Node {} received no A image",
                    node.id
                ))
            })?;
        let to = self
            .single_image_input(node.id, TRANSITION_TO_INPUT_PORT)?
            .ok_or_else(|| {
                LibraryError::Render(format!(
                    "Transition Image Mix Node {} received no B image",
                    node.id
                ))
            })?;
        let progress = self
            .value_input(node.id, TRANSITION_PROGRESS_INPUT_PORT)?
            .ok_or_else(|| {
                LibraryError::Render(format!(
                    "Transition Image Mix Node {} received no Progress value",
                    node.id
                ))
            })?;
        let PropertyValue::Number(progress) = progress else {
            return Err(LibraryError::Render(format!(
                "Transition Image Mix Node {} requires numeric Progress",
                node.id
            )));
        };
        let progress = NormalizedProgress16::new(progress.into_inner() as f32)
            .map_err(LibraryError::Render)?;
        let context = self.transition_context.ok_or_else(|| {
            LibraryError::Render(format!(
                "Transition Image Mix Node {} can only run in a Timeline Transition host",
                node.id
            ))
        })?;
        Ok(Some(FrameItem::Transition(Box::new(FrameTransition {
            transition_id: context.transition_id,
            timeline_time: context.timeline_time,
            kind: FrameTransitionKind::CrossDissolve,
            width: self.width,
            height: self.height,
            // Mix is internal processing. Timeline placement blend is applied
            // once at the enclosing TransitionOutput boundary.
            blend_mode: BlendMode::Normal,
            progress,
            from: FrameTransitionSource {
                item_id: context.from.item_id,
                source_time: context.from.source_time,
                item: from,
            },
            to: FrameTransitionSource {
                item_id: context.to.item_id,
                source_time: context.to.source_time,
                item: to,
            },
        }))))
    }

    fn single_image_input(
        &mut self,
        node_id: uuid::Uuid,
        port: &str,
    ) -> Result<Option<FrameItem>, LibraryError> {
        let mut inputs = self.image_inputs(node_id, port)?;
        if inputs.len() > 1 {
            return Err(LibraryError::Validation(format!(
                "Module Image input {node_id}:{port} resolved more than once"
            )));
        }
        Ok(inputs.pop())
    }

    fn image_inputs(
        &mut self,
        node_id: uuid::Uuid,
        port: &str,
    ) -> Result<Vec<FrameItem>, LibraryError> {
        let target = ModulePortAddress {
            node_id,
            port: port.to_string(),
        };
        if let Some(external) = self.external_images.get(&target) {
            return Ok(vec![external.clone()]);
        }
        let mut sources = self
            .definition
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .map(|connection| {
                (
                    connection.order,
                    connection.id,
                    connection.blend_mode,
                    connection.from.clone(),
                )
            })
            .collect::<Vec<_>>();
        sources.sort_by_key(|(order, id, _, _)| (*order, *id));
        let mut result = Vec::new();
        for (_, connection_id, blend_mode, source) in sources {
            if let Some(mut image) = self.evaluate_image_output(&source)? {
                neutralize_root_blend(&mut image);
                result.push(FrameItem::Group(FrameGroup {
                    source_id: connection_id.as_uuid(),
                    kind: FrameGroupKind::ConnectedImage,
                    width: self.width,
                    height: self.height,
                    background_color: transparent(),
                    transform: Transform::default(),
                    blend_mode: blend_mode.effective_over_empty_backdrop(result.is_empty()),
                    effect_time: OrderedFloat(self.local_time.to_seconds_f64()),
                    effects: Vec::new(),
                    items: vec![image],
                }));
            }
        }
        Ok(result)
    }

    pub(super) fn node_values(
        &mut self,
        node: &CompiledNode,
    ) -> Result<HashMap<String, PropertyValue>, LibraryError> {
        let mut values = node
            .properties
            .iter()
            .map(|(key, property)| {
                self.evaluate_node_property(node, key, property)
                    .map(|value| (key.clone(), value))
            })
            .collect::<Result<HashMap<_, _>, _>>()?;

        let parameters = self
            .definition
            .parameters
            .values()
            .filter(|parameter| parameter.target.node_id == node.id)
            .cloned()
            .collect::<Vec<_>>();
        for parameter in parameters {
            let key = property_name_from_port(&parameter.target.port)
                .unwrap_or(&parameter.target.port)
                .to_string();
            values.insert(key, self.effective_parameter(parameter.id)?);
        }

        let connected = self
            .definition
            .connections
            .iter()
            .filter(|connection| connection.to.node_id == node.id)
            .filter_map(|connection| {
                let key =
                    property_name_from_port(&connection.to.port).unwrap_or(&connection.to.port);
                (node.properties.get(key).is_some()).then(|| {
                    (
                        key.to_string(),
                        connection.order,
                        connection.id,
                        connection.from.clone(),
                    )
                })
            })
            .collect::<Vec<_>>();
        for (key, _, _, source) in connected {
            let value = self.evaluate_value_output(&source)?.ok_or_else(|| {
                LibraryError::Render(format!(
                    "Module property input {}:{} produced no value",
                    node.id, key
                ))
            })?;
            values.insert(key, value);
        }
        Ok(values)
    }

    fn evaluate_node_property(
        &self,
        node: &CompiledNode,
        key: &str,
        property: &crate::model::property::Property,
    ) -> Result<PropertyValue, LibraryError> {
        let seconds = self.local_time.to_seconds_f64();
        let context = EvaluationContext::new(
            &node.properties,
            self.evaluation_fps,
            (self.width, self.height),
        );
        self.plugins
            .get_property_evaluators()
            .evaluate_with_diagnostics(property, seconds, &context)
            .map(|outcome| {
                if let Some(diagnostic) = outcome.diagnostic() {
                    log::warn!(
                        "Recovered Module Node {} property {key:?} through {} fallback: {}",
                        node.id,
                        diagnostic.evaluator(),
                        diagnostic.message(),
                    );
                }
                outcome.into_value()
            })
            .map_err(|error| {
                LibraryError::Render(format!(
                    "Cannot evaluate Module Node {} property {key:?}: {error}",
                    node.id
                ))
            })
    }

    fn effective_parameter(
        &self,
        parameter_id: crate::model::authoring::PublishedParameterId,
    ) -> Result<PropertyValue, LibraryError> {
        let parameter = self
            .definition
            .parameters
            .get(&parameter_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Compiled Module has no parameter {parameter_id}"))
            })?;
        if let Some(value) = self.host_parameters.get(&parameter_id) {
            return Ok(value.clone());
        }
        if self.invocation.definition_id != self.definition.id {
            return Err(LibraryError::Validation(format!(
                "Module instance {} changed definition after compilation",
                self.invocation.instance_id
            )));
        }
        if let Some(track) = self.invocation.automation_tracks.get(&parameter_id) {
            return track.evaluate_at(self.local_time);
        }
        Ok(self
            .invocation
            .parameter_overrides
            .get(&parameter_id)
            .unwrap_or(&parameter.default_value)
            .clone())
    }

    fn evaluate_value_output(
        &mut self,
        source: &ModulePortAddress,
    ) -> Result<Option<PropertyValue>, LibraryError> {
        let key = (source.node_id, source.port.clone());
        if let Some(cached) = self.value_memo.get(&key) {
            return Ok(cached.clone());
        }
        if !self.value_path.insert(key.clone()) {
            return Err(LibraryError::Validation(format!(
                "Module value cycle reaches {}:{}",
                source.node_id, source.port
            )));
        }
        let result = self.evaluate_value_output_inner(source);
        self.value_path.remove(&key);
        if let Ok(value) = &result {
            self.value_memo.insert(key, value.clone());
        }
        result
    }

    fn evaluate_value_output_inner(
        &mut self,
        source: &ModulePortAddress,
    ) -> Result<Option<PropertyValue>, LibraryError> {
        let node = self
            .definition
            .nodes
            .get(&source.node_id)
            .cloned()
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Module value output reaches missing Node {}",
                    source.node_id
                ))
            })?;
        if !node.enabled {
            return Ok(None);
        }
        if node.bypassed {
            let input = node.bypass_routes.get(&source.port).ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Node {} has no bypass route for '{}'",
                    node.id, source.port
                ))
            })?;
            return self.value_input(node.id, input);
        }
        match node.content {
            NodeContent::NativeOperation(operation)
                if operation.catalog_id == TRANSITION_PROGRESS_INPUT_NODE_ID
                    && source.port == NUMBER_RESULT_OUTPUT_PORT =>
            {
                self.value_input(node.id, TRANSITION_PROGRESS_INPUT_PORT)
            }
            NodeContent::Data(_) if source.port == DATA_VALUE_OUTPUT_PORT => {
                self.value_input(node.id, DATA_VALUE_PROPERTY)
            }
            NodeContent::Value(operation) if source.port == NUMBER_RESULT_OUTPUT_PORT => {
                let left = self.value_input(node.id, operation.primary_input())?;
                let right = self.value_input(node.id, operation.secondary_input())?;
                let (Some(left), Some(right)) = (left, right) else {
                    return Ok(None);
                };
                crate::model::numeric::evaluate_numeric_binary(
                    operation.numeric_operation(),
                    &left,
                    &right,
                )
                .map(Some)
                .map_err(|error| {
                    LibraryError::Render(format!(
                        "Numeric Module Node {} failed: {error:?}",
                        node.id
                    ))
                })
            }
            _ => Err(LibraryError::Render(format!(
                "Module Node {} output '{}' has no stateless value runtime",
                node.id, source.port
            ))),
        }
    }

    fn value_input(
        &mut self,
        node_id: uuid::Uuid,
        port: &str,
    ) -> Result<Option<PropertyValue>, LibraryError> {
        if port == TIME_PORT {
            return Ok(Some(PropertyValue::Number(OrderedFloat(
                self.local_time.to_seconds_f64(),
            ))));
        }
        if let Some(parameter) =
            self.definition.parameters.values().find(|parameter| {
                parameter.target.node_id == node_id && parameter.target.port == port
            })
        {
            return self.effective_parameter(parameter.id).map(Some);
        }
        let target = ModulePortAddress {
            node_id,
            port: port.to_string(),
        };
        let mut connections = self
            .definition
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .map(|connection| (connection.order, connection.id, connection.from.clone()))
            .collect::<Vec<_>>();
        connections.sort_by_key(|(order, id, _)| (*order, *id));
        if connections.len() > 1 {
            return Err(LibraryError::Validation(format!(
                "Scalar Module input {node_id}:{port} has multiple connections"
            )));
        }
        if let Some((_, _, source)) = connections.pop() {
            return self.evaluate_value_output(&source);
        }
        let node = self.definition.nodes.get(&node_id).ok_or_else(|| {
            LibraryError::Validation(format!("Compiled Module Node {node_id} is missing"))
        })?;
        let property_name = property_name_from_port(port).unwrap_or(port);
        node.properties
            .get(property_name)
            .map(|property| {
                self.evaluate_node_property(node, property_name, property)
                    .map(Some)
            })
            .unwrap_or(Ok(None))
    }
}
