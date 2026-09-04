//! Explicit conversion of one ordinary Timeline source into a bounded Node Clip.

use super::*;

use crate::editor::authoring_factory::{AuthoringNodeFactory, ModuleNodeRequest};
use crate::editor::project_service::MediaNodeRequest;
use crate::model::authoring::{
    AutomationKeyframe, ModuleConnection, ModuleDefinitionSharing, ModulePortAddress,
    PublishedParameter, ShapeKind, ShapeSource, TextEnsembleOperation, property_value_type,
};
use crate::model::node::{NodeContent, PluginOperationContent};
use crate::model::project::asset::AssetKind;
use crate::model::project::{
    AUDIO_OUTPUT_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, PortDataType, SHAPE_INPUT_PORT,
    SHAPE_OUTPUT_PORT,
};
use crate::plugin::{
    EFFECT_APPLY_OPERATION, EFFECT_CATEGORY, PROPERTY_PORT_PREFIX, STYLE_APPLY_OPERATION,
    STYLE_CATEGORY,
};

const KEYFRAME_TIME_SCALE: u32 = 1_000_000_000;

struct PreparedConversion {
    definition: ModuleDefinition,
    output_id: ModuleOutputId,
    instance_id: ModuleInstanceId,
    parameter_overrides: HashMap<PublishedParameterId, PropertyValue>,
    automation_tracks: HashMap<PublishedParameterId, AutomationTrack>,
    source_property_keys: Vec<String>,
    moved_attachment_ids: Vec<AttachmentId>,
    retained_post_transform_effects: usize,
}

struct GraphBuilder<'a> {
    plugins: &'a PluginManager,
    definition: ModuleDefinition,
    output_id: ModuleOutputId,
    current: Option<ModulePortAddress>,
    next_column: f32,
    parameter_overrides: HashMap<PublishedParameterId, PropertyValue>,
    automation_tracks: HashMap<PublishedParameterId, AutomationTrack>,
}

/// A Timeline source that can be represented by one bounded Node Clip graph.
///
/// Constructing this view performs the conversion boundary check once, so the
/// graph builder cannot be called with a nested Timeline or an existing Module
/// invocation.
enum NodeClipSource<'a> {
    Asset {
        asset_id: uuid::Uuid,
    },
    Text {
        text: &'a String,
        ensemble_operations: &'a [TextEnsembleOperation],
    },
    Shape(&'a ShapeSource),
    Solid(&'a crate::model::frame::color::Color),
}

impl<'a> NodeClipSource<'a> {
    fn from_item(item: &'a TimelineItem) -> Result<Self, LibraryError> {
        match &item.source {
            SourceRef::Asset { asset_id } => Ok(Self::Asset {
                asset_id: *asset_id,
            }),
            SourceRef::Text {
                text,
                ensemble_operations,
            } => Ok(Self::Text {
                text,
                ensemble_operations,
            }),
            SourceRef::Shape { shape } => Ok(Self::Shape(shape)),
            SourceRef::Solid { color } => Ok(Self::Solid(color)),
            SourceRef::Composition(_) => Err(LibraryError::Validation(
                "Nested Timeline sources cannot live inside a Module graph".to_string(),
            )),
            SourceRef::Module(_) => Err(LibraryError::Validation(format!(
                "Timeline item {} is already a Node Clip",
                item.id
            ))),
        }
    }
}

impl TimelineEditorService {
    /// Converts exactly one user-selected source island. Timeline placement,
    /// hierarchy, time mapping, blend, and placement properties stay on the
    /// same item; no sibling or Timeline structure is expanded into Nodes.
    pub fn convert_source_to_node_clip(
        &self,
        plugins: &PluginManager,
        item_id: TimelineItemId,
    ) -> Result<NodeClipConversionResult, LibraryError> {
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_item(session.project(), item_id)?;
        let prepared = prepare_conversion(session.project(), plugins, item_id)?;
        let definition_id = prepared.definition.id;
        let instance_id = prepared.instance_id;
        let output_id = prepared.output_id;
        let moved_pre_transform_effects = prepared.moved_attachment_ids.len();
        let retained_post_transform_effects = prepared.retained_post_transform_effects;
        let (_, changes) = session
            .transact(
                vec![ProjectInvalidation::Item {
                    timeline_id,
                    item_id,
                }],
                move |project| apply_conversion(project, item_id, prepared),
            )
            .map_err(LibraryError::Validation)?;
        Ok(NodeClipConversionResult {
            item_id,
            definition_id,
            instance_id,
            output_id,
            moved_pre_transform_effects,
            retained_post_transform_effects,
            changes,
        })
    }
}

fn prepare_conversion(
    project: &AuthoringProject,
    plugins: &PluginManager,
    item_id: TimelineItemId,
) -> Result<PreparedConversion, LibraryError> {
    let item = project
        .items
        .get(&item_id)
        .ok_or_else(|| LibraryError::Validation(format!("Missing Timeline item {item_id}")))?;
    let source = NodeClipSource::from_item(item)?;
    let timeline_id = project
        .tracks
        .get(&item.track_id)
        .map(|track| track.timeline_id)
        .ok_or_else(|| LibraryError::Validation(format!("Missing Track {}", item.track_id)))?;
    let timeline = project
        .timelines
        .get(&timeline_id)
        .ok_or_else(|| LibraryError::Validation(format!("Missing Timeline {timeline_id}")))?;
    let (definition, output_id) = ModuleDefinition::new_image(
        format!("{} Source", item.name),
        ModuleDefinitionSharing::Private,
    );
    let mut builder = GraphBuilder {
        plugins,
        definition,
        output_id,
        current: None,
        next_column: 40.0,
        parameter_overrides: HashMap::new(),
        automation_tracks: HashMap::new(),
    };
    let mut source_property_keys = Vec::new();
    builder.add_source(
        project,
        item,
        source,
        timeline.width,
        timeline.height,
        &mut source_property_keys,
    )?;
    let (moved_attachment_ids, retained_post_transform_effects) =
        builder.add_pre_transform_effects(project, item_id)?;
    builder.connect_output()?;
    builder.definition.topology_revision = 2;
    if !builder.definition.interface.parameters.is_empty() {
        builder.definition.interface_version = 2;
    }
    builder
        .definition
        .validate()
        .map_err(LibraryError::Validation)?;
    Ok(PreparedConversion {
        definition: builder.definition,
        output_id,
        instance_id: ModuleInstanceId::new(),
        parameter_overrides: builder.parameter_overrides,
        automation_tracks: builder.automation_tracks,
        source_property_keys,
        moved_attachment_ids,
        retained_post_transform_effects,
    })
}

fn apply_conversion(
    project: &mut AuthoringProject,
    item_id: TimelineItemId,
    prepared: PreparedConversion,
) -> Result<(), String> {
    if project
        .module_definitions
        .contains_key(&prepared.definition.id)
    {
        return Err(format!(
            "Module definition {} already exists",
            prepared.definition.id
        ));
    }
    if project.module_instances.contains_key(&prepared.instance_id) {
        return Err(format!(
            "Module instance {} already exists",
            prepared.instance_id
        ));
    }
    let definition_id = prepared.definition.id;
    project
        .module_definitions
        .insert(definition_id, prepared.definition);
    project.module_instances.insert(
        prepared.instance_id,
        ModuleInstance {
            id: prepared.instance_id,
            definition_id,
            parameter_overrides: prepared.parameter_overrides,
        },
    );
    let item = project
        .items
        .get_mut(&item_id)
        .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
    if matches!(item.source, SourceRef::Module(_)) {
        return Err(format!("Timeline item {item_id} is already a Node Clip"));
    }
    for key in prepared.source_property_keys {
        item.authored_properties
            .remove(&key)
            .ok_or_else(|| format!("Source Property '{key}' changed during conversion"))?;
    }
    item.source = SourceRef::Module(ModuleInvocation {
        instance_id: prepared.instance_id,
        output_id: prepared.output_id,
        input_bindings: HashMap::new(),
        automation_tracks: prepared.automation_tracks,
    });
    for attachment_id in prepared.moved_attachment_ids {
        project
            .attachments
            .remove(&attachment_id)
            .ok_or_else(|| format!("Attachment {attachment_id} changed during conversion"))?;
    }
    Ok(())
}

impl GraphBuilder<'_> {
    fn add_source(
        &mut self,
        project: &AuthoringProject,
        item: &TimelineItem,
        source: NodeClipSource<'_>,
        width: u64,
        height: u64,
        removed_keys: &mut Vec<String>,
    ) -> Result<(), LibraryError> {
        match source {
            NodeClipSource::Asset { asset_id } => {
                let asset = project
                    .assets
                    .iter()
                    .find(|asset| asset.id == asset_id)
                    .ok_or_else(|| LibraryError::Validation(format!("Missing Asset {asset_id}")))?;
                let (request, output_port) = match asset.kind {
                    AssetKind::Image => (
                        MediaNodeRequest::Image {
                            asset_id: asset.id,
                            file_path: asset.path.clone(),
                        },
                        IMAGE_OUTPUT_PORT,
                    ),
                    AssetKind::Video => {
                        return Err(LibraryError::Validation(
                            "Video source conversion is disabled until Node Clip audio routing can preserve embedded sound"
                                .to_string(),
                        ));
                    }
                    AssetKind::Audio => (
                        MediaNodeRequest::Audio {
                            asset_id: asset.id,
                            file_path: asset.path.clone(),
                            audio_stream_index: asset.stream_index,
                        },
                        AUDIO_OUTPUT_PORT,
                    ),
                    AssetKind::Model3D | AssetKind::Other => {
                        return Err(LibraryError::Validation(format!(
                            "Asset kind {:?} has no Module source Node",
                            asset.kind
                        )));
                    }
                };
                let node = AuthoringNodeFactory::create_media(
                    self.plugins,
                    &item.name,
                    request,
                    width,
                    height,
                    asset.width.map(u64::from).unwrap_or(width),
                    asset.height.map(u64::from).unwrap_or(height),
                )?;
                self.push_source_node(node, output_port)?;
            }
            NodeClipSource::Text {
                text,
                ensemble_operations,
            } => {
                let mut node = AuthoringNodeFactory::create(
                    self.plugins,
                    ModuleNodeRequest::Text {
                        text: text.clone(),
                        font: "Arial".to_string(),
                    },
                    width,
                    height,
                )?;
                node.set_property(
                    "size".to_string(),
                    Property::constant(PropertyValue::from(48.0)),
                )
                .map_err(LibraryError::Validation)?;
                let node_id = node.id;
                self.push_source_node(node, SHAPE_OUTPUT_PORT)?;
                self.publish_literal(node_id, "text", "Text", PropertyValue::String(text.clone()))?;
                self.publish_item_property(
                    item,
                    node_id,
                    "font_family",
                    &["font_family", "font"],
                    "Font",
                    removed_keys,
                )?;
                self.publish_item_property(
                    item,
                    node_id,
                    "size",
                    &["size", "font_size"],
                    "Font Size",
                    removed_keys,
                )?;
                for operation in ensemble_operations {
                    self.add_ensemble_operation(operation)?;
                }
                let fill_id = self.add_fill_node()?;
                self.publish_item_property(
                    item,
                    fill_id,
                    &format!("{PROPERTY_PORT_PREFIX}color"),
                    &["color"],
                    "Color",
                    removed_keys,
                )?;
            }
            NodeClipSource::Shape(shape) => {
                validate_shape_parameters(shape)?;
                let path = shape_path(shape)?;
                let path_text = crate::model::path::write_legacy_svg_path_data(&path)
                    .map_err(|error| LibraryError::Validation(error.to_string()))?;
                let shape_width = positive_shape_extent(shape, "width", 100.0)?;
                let shape_height = positive_shape_extent(shape, "height", 100.0)?;
                let node = AuthoringNodeFactory::create(
                    self.plugins,
                    ModuleNodeRequest::Shape {
                        path: path_text,
                        width: shape_width,
                        height: shape_height,
                    },
                    width,
                    height,
                )?;
                let node_id = node.id;
                self.push_source_node(node, SHAPE_OUTPUT_PORT)?;
                self.publish_literal(node_id, "path", "Path", PropertyValue::Path(path))?;
                let fill_id = self.add_fill_node()?;
                let color = shape.parameters.get("color").cloned().unwrap_or_else(|| {
                    PropertyValue::Color(crate::model::frame::color::Color::white())
                });
                self.publish_literal(
                    fill_id,
                    &format!("{PROPERTY_PORT_PREFIX}color"),
                    "Color",
                    color,
                )?;
            }
            NodeClipSource::Solid(color) => {
                let node = AuthoringNodeFactory::create(
                    self.plugins,
                    ModuleNodeRequest::Solid {
                        color: color.clone(),
                    },
                    width,
                    height,
                )?;
                let node_id = node.id;
                self.push_source_node(node, IMAGE_OUTPUT_PORT)?;
                self.publish_literal(
                    node_id,
                    "color",
                    "Color",
                    PropertyValue::ColorValue(
                        crate::model::property::ColorValue::from_straight_srgba8(color),
                    ),
                )?;
            }
        }
        Ok(())
    }

    fn push_source_node(&mut self, mut node: Node, output_port: &str) -> Result<(), LibraryError> {
        self.position_node(&mut node);
        let node_id = node.id;
        if self.definition.graph.nodes.insert(node_id, node).is_some() {
            return Err(LibraryError::Validation(format!(
                "Module source Node {node_id} collides with an existing Node"
            )));
        }
        self.current = Some(ModulePortAddress {
            node_id,
            port: output_port.to_string(),
        });
        Ok(())
    }

    fn add_fill_node(&mut self) -> Result<uuid::Uuid, LibraryError> {
        let mut node = self.plugins.create_style_operation_node("fill")?;
        let NodeContent::PluginOperation(operation) = node.content() else {
            return Err(LibraryError::Validation(
                "Fill factory did not create a Plugin operation Node".to_string(),
            ));
        };
        if node.name != "Fill"
            || operation.category != STYLE_CATEGORY
            || operation.operation != STYLE_APPLY_OPERATION
        {
            return Err(LibraryError::Validation(
                "Node Clip conversion requires the built-in Fill Style implementation".to_string(),
            ));
        }
        let node_id = node.id;
        self.position_node(&mut node);
        self.connect_current_to(&node, SHAPE_INPUT_PORT, IMAGE_OUTPUT_PORT)?;
        Ok(node_id)
    }

    fn add_ensemble_operation(
        &mut self,
        operation: &TextEnsembleOperation,
    ) -> Result<(), LibraryError> {
        let mut node = self.plugins.create_text_ensemble_operation_node(
            &operation.operation.category,
            &operation.operation.component_id,
        )?;
        let NodeContent::PluginOperation(content) = node.content() else {
            return Err(LibraryError::Validation(format!(
                "Text Ensemble operation {} did not create a Plugin Node",
                operation.id
            )));
        };
        if content.declared_ports != operation.declared_ports {
            return Err(LibraryError::Validation(format!(
                "Text Ensemble operation {} no longer matches its plugin port contract",
                operation.id
            )));
        }
        node.id = operation.id;
        self.position_node(&mut node);
        self.connect_current_to(&node, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT)?;
        for (key, property) in operation.properties.iter() {
            self.move_property_to_node_parameter(
                &node,
                key,
                &format!("{PROPERTY_PORT_PREFIX}{key}"),
                key,
                property,
            )?;
        }
        Ok(())
    }

    fn add_pre_transform_effects(
        &mut self,
        project: &AuthoringProject,
        item_id: TimelineItemId,
    ) -> Result<(Vec<AttachmentId>, usize), LibraryError> {
        let owner = AttachmentOwner::Item { item_id };
        let mut attachments = project
            .attachments
            .values()
            .filter(|attachment| attachment.owner == owner)
            .collect::<Vec<_>>();
        attachments.sort_by_key(|attachment| (attachment.stage, attachment.order, attachment.id));
        let retained_post = attachments
            .iter()
            .filter(|attachment| attachment.stage == AttachmentStage::ItemPostTransform)
            .count();
        let mut moved = Vec::new();
        for attachment in attachments {
            if attachment.stage != AttachmentStage::ItemPreTransform {
                continue;
            }
            if !attachment.enabled {
                return Err(LibraryError::Validation(format!(
                    "Disabled pre-Transform Effect {} cannot be represented by Node disabled semantics without changing output",
                    attachment.id
                )));
            }
            let AttachmentProcessor::BuiltinEffect(effect) = &attachment.processor else {
                return Err(LibraryError::Validation(format!(
                    "Pre-Transform Module Effect {} cannot be flattened into another private Module",
                    attachment.id
                )));
            };
            if effect.contract.input_type != PortDataType::Image
                || effect.contract.output_type != PortDataType::Image
            {
                return Err(LibraryError::Validation(format!(
                    "Pre-Transform Effect {} is not Image to Image",
                    attachment.id
                )));
            }
            let mut node = self.plugins.create_operation_node(
                &effect.operation.category,
                &effect.operation.component_id,
                &effect.operation.operation,
            )?;
            let NodeContent::PluginOperation(content) = node.content() else {
                return Err(LibraryError::Validation(format!(
                    "Effect {} did not create a Plugin Node",
                    attachment.id
                )));
            };
            if content.category != EFFECT_CATEGORY
                || content.operation != EFFECT_APPLY_OPERATION
                || content.component_id != effect.operation.component_id
            {
                return Err(LibraryError::Validation(format!(
                    "Effect {} no longer matches its operation identity",
                    attachment.id
                )));
            }
            validate_effect_contract(content, effect, attachment.id)?;
            node.id = attachment.id.as_uuid();
            node.bypassed = attachment.bypassed;
            node.blend_mode = effect.blend_mode;
            self.position_node(&mut node);
            self.connect_current_to(&node, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT)?;
            for contract in &effect.contract.parameters {
                let parameter = effect.parameters.get(&contract.key).ok_or_else(|| {
                    LibraryError::Validation(format!(
                        "Effect {} is missing parameter '{}'",
                        attachment.id, contract.key
                    ))
                })?;
                self.move_effect_parameter(&node, &contract.key, parameter)?;
            }
            moved.push(attachment.id);
        }
        Ok((moved, retained_post))
    }

    fn move_effect_parameter(
        &mut self,
        node: &Node,
        key: &str,
        parameter: &crate::model::authoring::AutomatableParameter,
    ) -> Result<(), LibraryError> {
        let target = format!("{PROPERTY_PORT_PREFIX}{key}");
        let default = node
            .properties()
            .get(key)
            .and_then(Property::value)
            .cloned()
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Effect Node {} has no default for '{key}'",
                    node.id
                ))
            })?;
        let parameter_id = self.add_parameter(node.id, target, key, default)?;
        match &parameter.automation {
            Some(track) => {
                self.automation_tracks.insert(parameter_id, track.clone());
            }
            None => {
                self.parameter_overrides
                    .insert(parameter_id, parameter.value.clone());
            }
        }
        Ok(())
    }

    fn publish_item_property(
        &mut self,
        item: &TimelineItem,
        node_id: uuid::Uuid,
        target_port: &str,
        source_keys: &[&str],
        label: &str,
        removed_keys: &mut Vec<String>,
    ) -> Result<(), LibraryError> {
        let matches = source_keys
            .iter()
            .filter_map(|key| {
                item.authored_properties
                    .get(key)
                    .map(|property| ((*key).to_string(), property))
            })
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            return Err(LibraryError::Validation(format!(
                "Timeline item {} has ambiguous source Properties: {}",
                item.id,
                matches
                    .iter()
                    .map(|(key, _)| key.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        let node =
            self.definition.graph.nodes.get(&node_id).ok_or_else(|| {
                LibraryError::Validation(format!("Missing Module Node {node_id}"))
            })?;
        let property_key = target_port
            .strip_prefix(PROPERTY_PORT_PREFIX)
            .unwrap_or(target_port);
        let default = node
            .properties()
            .get(property_key)
            .and_then(Property::value)
            .cloned()
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Module Node {node_id} has no source Property '{property_key}'"
                ))
            })?;
        let parameter_id = self.add_parameter(node_id, target_port.to_string(), label, default)?;
        if let Some((key, property)) = matches.first() {
            self.move_property_value(parameter_id, property)?;
            removed_keys.push(key.clone());
        }
        Ok(())
    }

    fn publish_literal(
        &mut self,
        node_id: uuid::Uuid,
        target_port: &str,
        label: &str,
        value: PropertyValue,
    ) -> Result<(), LibraryError> {
        let parameter_id =
            self.add_parameter(node_id, target_port.to_string(), label, value.clone())?;
        self.parameter_overrides.insert(parameter_id, value);
        Ok(())
    }

    fn move_property_to_node_parameter(
        &mut self,
        node: &Node,
        property_key: &str,
        target_port: &str,
        label: &str,
        property: &Property,
    ) -> Result<(), LibraryError> {
        let default = node
            .properties()
            .get(property_key)
            .and_then(Property::value)
            .cloned()
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Module Node {} has no property '{property_key}'",
                    node.id
                ))
            })?;
        let parameter_id = self.add_parameter(node.id, target_port, label, default)?;
        self.move_property_value(parameter_id, property)
    }

    fn move_property_value(
        &mut self,
        parameter_id: PublishedParameterId,
        property: &Property,
    ) -> Result<(), LibraryError> {
        match property.evaluator.as_str() {
            "constant" => {
                let value = property.value().cloned().ok_or_else(|| {
                    LibraryError::Validation("Constant source Property has no value".to_string())
                })?;
                self.parameter_overrides.insert(parameter_id, value);
            }
            "keyframe" => {
                self.automation_tracks
                    .insert(parameter_id, property_automation(property)?);
            }
            evaluator => {
                return Err(LibraryError::Validation(format!(
                    "Source Property evaluator '{evaluator}' cannot be moved to Timeline-owned Module automation"
                )));
            }
        }
        Ok(())
    }

    fn add_parameter(
        &mut self,
        node_id: uuid::Uuid,
        target_port: impl Into<String>,
        label: &str,
        default_value: PropertyValue,
    ) -> Result<PublishedParameterId, LibraryError> {
        let target_port = target_port.into();
        let target = ModulePortAddress {
            node_id,
            port: target_port,
        };
        let port = self
            .definition
            .graph
            .port_definition(&target, crate::model::project::PortDirection::Input)
            .map_err(LibraryError::Validation)?;
        let value_type = property_value_type(&default_value);
        if !port.data_type.accepts(value_type) {
            return Err(LibraryError::Validation(format!(
                "Published source value '{label}' does not match {:?}",
                port.data_type
            )));
        }
        let id = PublishedParameterId::new();
        self.definition
            .interface
            .parameters
            .push(PublishedParameter {
                id,
                name: label.to_string(),
                data_type: port.data_type,
                default_value,
                target,
            });
        Ok(id)
    }

    fn connect_current_to(
        &mut self,
        node: &Node,
        input_port: &str,
        output_port: &str,
    ) -> Result<(), LibraryError> {
        let source = self.current.take().ok_or_else(|| {
            LibraryError::Validation("Node Clip source graph has no current output".to_string())
        })?;
        self.definition.graph.connections.push(ModuleConnection {
            id: ModuleConnectionId::new(),
            from: source,
            to: ModulePortAddress {
                node_id: node.id,
                port: input_port.to_string(),
            },
            order: 0,
            blend_mode: BlendMode::Normal,
        });
        let node_id = node.id;
        if self
            .definition
            .graph
            .nodes
            .insert(node_id, node.clone())
            .is_some()
        {
            return Err(LibraryError::Validation(format!(
                "Module Node {node_id} has a duplicate stable identity"
            )));
        }
        self.current = Some(ModulePortAddress {
            node_id,
            port: output_port.to_string(),
        });
        Ok(())
    }

    fn connect_output(&mut self) -> Result<(), LibraryError> {
        let source = self.current.take().ok_or_else(|| {
            LibraryError::Validation("Node Clip conversion produced no Image source".to_string())
        })?;
        let output = self
            .definition
            .output(self.output_id)
            .ok_or_else(|| LibraryError::Validation("Module Output is missing".to_string()))?;
        let source_type = self
            .definition
            .graph
            .port_definition(&source, crate::model::project::PortDirection::Output)
            .map_err(LibraryError::Validation)?
            .data_type;
        let target = output.target(source_type).ok_or_else(|| {
            LibraryError::Validation(format!("Module Output cannot accept {source_type:?} media"))
        })?;
        self.definition.graph.connections.push(ModuleConnection {
            id: ModuleConnectionId::new(),
            from: source,
            to: target,
            order: 0,
            blend_mode: BlendMode::Normal,
        });
        if let Some(output) = self
            .definition
            .graph
            .nodes
            .values_mut()
            .find(|node| matches!(node.content(), NodeContent::ModuleOutput(_)))
        {
            output.ui_position = [self.next_column, 120.0];
        }
        Ok(())
    }

    fn position_node(&mut self, node: &mut Node) {
        node.ui_position = [self.next_column, 120.0];
        self.next_column += 300.0;
    }
}

fn validate_effect_contract(
    content: &PluginOperationContent,
    effect: &crate::model::authoring::BuiltinEffectInstance,
    attachment_id: AttachmentId,
) -> Result<(), LibraryError> {
    use crate::model::project::PortDirection;

    let require_port = |key: &str, direction: PortDirection, data_type: PortDataType| {
        content
            .declared_ports
            .iter()
            .any(|port| {
                port.key == key && port.direction == direction && port.data_type == data_type
            })
            .then_some(())
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Effect {attachment_id} no longer matches its persisted '{key}' port contract"
                ))
            })
    };
    require_port(
        IMAGE_INPUT_PORT,
        PortDirection::Input,
        effect.contract.input_type,
    )?;
    require_port(
        IMAGE_OUTPUT_PORT,
        PortDirection::Output,
        effect.contract.output_type,
    )?;

    let current_property_ports = content
        .declared_ports
        .iter()
        .filter(|port| {
            port.direction == PortDirection::Input && port.key.starts_with(PROPERTY_PORT_PREFIX)
        })
        .collect::<Vec<_>>();
    if current_property_ports.len() != effect.contract.parameters.len()
        || effect.parameters.len() != effect.contract.parameters.len()
    {
        return Err(LibraryError::Validation(format!(
            "Effect {attachment_id} property contract changed; conversion would be lossy"
        )));
    }
    for parameter in &effect.contract.parameters {
        let port_key = format!("{PROPERTY_PORT_PREFIX}{}", parameter.key);
        require_port(&port_key, PortDirection::Input, parameter.data_type)?;
    }
    Ok(())
}

fn property_automation(property: &Property) -> Result<AutomationTrack, LibraryError> {
    let keyframes = property.keyframes();
    if keyframes.is_empty() {
        return Err(LibraryError::Validation(
            "Source Keyframe Property has no Keyframes".to_string(),
        ));
    }
    let keyframes = keyframes
        .into_iter()
        .map(|keyframe| {
            let seconds = keyframe.time.into_inner();
            let time = MediaTime::from_seconds_f64(seconds, KEYFRAME_TIME_SCALE)
                .map_err(LibraryError::Validation)?;
            if (time.to_seconds_f64() - seconds).abs() > 0.5 / f64::from(KEYFRAME_TIME_SCALE) {
                return Err(LibraryError::Validation(format!(
                    "Keyframe time {seconds} cannot be represented by Module automation without loss"
                )));
            }
            Ok(AutomationKeyframe {
                id: keyframe.id,
                time,
                value: keyframe.value,
                easing: keyframe.easing,
            })
        })
        .collect::<Result<Vec<_>, LibraryError>>()?;
    Ok(AutomationTrack { keyframes })
}

fn shape_path(shape: &ShapeSource) -> Result<crate::model::path::PathValue, LibraryError> {
    let width = shape_number(shape, "width", 100.0)?;
    let height = shape_number(shape, "height", 100.0)?;
    let path = match shape.shape_kind {
        ShapeKind::Rectangle => format!("M 0 0 H {width} V {height} H 0 Z"),
        ShapeKind::Ellipse => format!(
            "M {width} 0 A {} {} 0 1 1 0 0 A {} {} 0 1 1 {width} 0 Z",
            width / 2.0,
            height / 2.0,
            width / 2.0,
            height / 2.0
        ),
        ShapeKind::Path => {
            return match shape.parameters.get("path") {
                Some(PropertyValue::Path(path)) => Ok(path.clone()),
                _ => Err(LibraryError::Validation(
                    "Path source has no canonical Path value".to_string(),
                )),
            };
        }
    };
    crate::model::path::parse_legacy_svg_path_data(&path)
        .map_err(|error| LibraryError::Validation(error.to_string()))
}

fn validate_shape_parameters(shape: &ShapeSource) -> Result<(), LibraryError> {
    let allowed = match shape.shape_kind {
        ShapeKind::Rectangle | ShapeKind::Ellipse => ["width", "height", "color"].as_slice(),
        ShapeKind::Path => ["path", "width", "height", "color"].as_slice(),
    };
    let mut unsupported = shape
        .parameters
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    unsupported.sort();
    if unsupported.is_empty() {
        Ok(())
    } else {
        Err(LibraryError::Validation(format!(
            "Shape source has unsupported parameters that cannot be moved atomically: {}",
            unsupported.join(", ")
        )))
    }
}

fn shape_number(shape: &ShapeSource, key: &str, fallback: f64) -> Result<f64, LibraryError> {
    let value = match shape.parameters.get(key) {
        None => fallback,
        Some(PropertyValue::Number(value)) => value.into_inner(),
        Some(PropertyValue::Integer(value)) => *value as f64,
        Some(_) => {
            return Err(LibraryError::Validation(format!(
                "Shape source parameter '{key}' is not numeric"
            )));
        }
    };
    if !value.is_finite() || value <= 0.0 {
        return Err(LibraryError::Validation(format!(
            "Shape source parameter '{key}' must be positive and finite"
        )));
    }
    Ok(value)
}

fn positive_shape_extent(
    shape: &ShapeSource,
    key: &str,
    fallback: f64,
) -> Result<u64, LibraryError> {
    let value = shape_number(shape, key, fallback)?.ceil();
    if value > u64::MAX as f64 {
        return Err(LibraryError::Validation(format!(
            "Shape source parameter '{key}' is too large"
        )));
    }
    Ok(value as u64)
}
