//! Compilation of bounded render-stage Point fields.

use std::collections::{HashMap, HashSet};

use super::{
    CompiledPointInstruction, CompiledPointProgram, CompiledPointRenderer, CompiledPointSource,
    CompiledPointValueType,
};
use crate::model::authoring::{ModuleDefinition, ModulePortAddress};
use crate::model::node::{
    COLOR_RAMP_FACTOR_PORT, COLOR_RAMP_GRADIENT_PORT, COLOR_VALUE_PORT, CONDITION_INPUT_PORT,
    ColorContent, ConditionalNodeRole, NUMERIC_LENGTH_CATALOG_ID, NUMERIC_LENGTH_INPUT_PORT, Node,
    NodeContent, POINT_ATTRIBUTE_OUTPUT_PORT, POINT_ATTRIBUTE_VALUE_PORT, POINT_OFFSET_INPUT_PORT,
    POINT_POSITION_INPUT_PORT, POINT_SCALE_INPUT_PORT, POINT_SELECTION_INPUT_PORT, POINT_SIZE_PORT,
    POINT_SOURCE_PORT, PointNodeRole, SELECT_FALSE_INPUT_PORT, SELECT_TRUE_INPUT_PORT,
    SPRITE_COLOR_INPUT_PORT, SPRITE_SELECTION_INPUT_PORT,
};
use crate::model::point::{
    NumericBinaryOperation, POINT_MAX_INSTRUCTIONS, POINT_MAX_RAMPS, PointAttributeElementType,
    PointAttributeId, PointAttributeSchema,
};
use crate::model::project::NUMBER_RESULT_OUTPUT_PORT;

mod dependencies;
mod schema;
mod stream;

use dependencies::PointDependencyResolver;
pub(super) use dependencies::validate_point_field_consumers;
use schema::point_attribute_schema;
pub use schema::validate_module_node_name;
use stream::{PointStage, PointStreamTrace, trace_point_stream};

const POINT_AGE_OUTPUT_PORT: &str = "age";
const POINT_NORMALIZED_AGE_OUTPUT_PORT: &str = "normalized_age";
const POINT_POSITION_OUTPUT_PORT: &str = "position";
const POINT_RANDOM_OUTPUT_PORT: &str = "random";
#[derive(Clone, Copy)]
struct PointSourceCapabilities {
    age: bool,
}

struct PointSourceCompilation {
    source: CompiledPointSource,
    lineage: HashSet<ModulePortAddress>,
    capabilities: PointSourceCapabilities,
}

/// Immutable geometry snapshots associated with each Point-stream address.
/// Missing fields refer directly to the producer's geometry.
#[derive(Clone, Default)]
struct StreamGeometry {
    position: Option<CompiledValue>,
    size: Option<CompiledValue>,
}

impl StreamGeometry {
    fn get(&self, field: GeometryField) -> Option<&CompiledValue> {
        match field {
            GeometryField::Position => self.position.as_ref(),
            GeometryField::Size => self.size.as_ref(),
        }
    }

    fn set(&mut self, field: GeometryField, value: CompiledValue) {
        match field {
            GeometryField::Position => self.position = Some(value),
            GeometryField::Size => self.size = Some(value),
        }
    }

    fn register(&self, field: GeometryField) -> Option<u16> {
        self.get(field).map(|value| value.register)
    }
}

#[derive(Clone, Copy)]
enum GeometryField {
    Position,
    Size,
}

impl GeometryField {
    fn from_info_port(port: &str) -> Option<Self> {
        match port {
            POINT_POSITION_OUTPUT_PORT => Some(Self::Position),
            POINT_SIZE_PORT => Some(Self::Size),
            _ => None,
        }
    }

    fn element_type(self) -> PointAttributeElementType {
        match self {
            Self::Position => PointAttributeElementType::Vec3,
            Self::Size => PointAttributeElementType::Number,
        }
    }

    fn input_port(self) -> &'static str {
        match self {
            Self::Position => POINT_POSITION_INPUT_PORT,
            Self::Size => POINT_SIZE_PORT,
        }
    }

    fn modifier(self) -> (&'static str, NumericBinaryOperation) {
        match self {
            Self::Position => (POINT_OFFSET_INPUT_PORT, NumericBinaryOperation::Add),
            Self::Size => (POINT_SCALE_INPUT_PORT, NumericBinaryOperation::Multiply),
        }
    }

    fn source_instruction(self) -> CompiledPointInstruction {
        match self {
            Self::Position => CompiledPointInstruction::Position,
            Self::Size => CompiledPointInstruction::Size,
        }
    }
}

pub(super) fn compile_point_renderers(
    definition: &ModuleDefinition,
    active_nodes: &HashSet<uuid::Uuid>,
) -> Result<HashMap<uuid::Uuid, CompiledPointRenderer>, String> {
    validate_point_field_consumers(definition, active_nodes)?;
    let mut compiled = HashMap::new();
    let mut candidate_ids = active_nodes.iter().copied().collect::<Vec<_>>();
    candidate_ids.sort_unstable();
    for renderer_node_id in candidate_ids {
        let Some(renderer) = definition.graph.nodes.get(&renderer_node_id) else {
            continue;
        };
        if !particle_sprite(renderer) || !renderer.enabled || renderer.bypassed {
            continue;
        }
        let Some(trace) = trace_point_stream(definition, renderer_node_id)? else {
            continue;
        };
        let Some(source) = compile_point_source(definition, &trace.terminal_source)? else {
            continue;
        };
        let point_program = compile_point_program(
            definition,
            renderer_node_id,
            &trace,
            &source.lineage,
            source.capabilities,
        )?;
        compiled.insert(
            renderer_node_id,
            CompiledPointRenderer {
                source: source.source,
                point_program,
                renderer_node_id,
                // Each renderer branch owns independent derived GPU resources.
                state_slot_id: renderer_node_id,
            },
        );
    }
    Ok(compiled)
}

fn compile_point_source(
    definition: &ModuleDefinition,
    terminal: &ModulePortAddress,
) -> Result<Option<PointSourceCompilation>, String> {
    let Some(node) = definition.graph.nodes.get(&terminal.node_id) else {
        return Ok(None);
    };
    if point_role(node) == Some(PointNodeRole::Grid) {
        if terminal.port != POINT_SOURCE_PORT || !node.enabled || node.bypassed {
            return Ok(None);
        }
        return Ok(Some(PointSourceCompilation {
            source: CompiledPointSource::Grid { node_id: node.id },
            lineage: HashSet::from([terminal.clone()]),
            capabilities: PointSourceCapabilities { age: false },
        }));
    }
    let Some(particle) = super::particle::compile_particle_source(definition, terminal)? else {
        return Ok(None);
    };
    Ok(Some(PointSourceCompilation {
        source: CompiledPointSource::Particle(particle.source),
        lineage: particle.lineage,
        capabilities: PointSourceCapabilities { age: true },
    }))
}

fn compile_point_program(
    definition: &ModuleDefinition,
    renderer_node_id: uuid::Uuid,
    trace: &PointStreamTrace,
    source_lineage: &HashSet<ModulePortAddress>,
    capabilities: PointSourceCapabilities,
) -> Result<Option<CompiledPointProgram>, String> {
    let stores = trace.stores().collect::<Vec<_>>();
    let schema = point_attribute_schema(definition, &stores)?;
    let mut builder = PointProgramBuilder::new(definition, schema);
    let mut allowed_streams = source_lineage.clone();
    // Missing fields are producer-local geometry. Set stages replace only
    // their own field with an immutable SSA value for the downstream branch.
    let mut stream_geometry = source_lineage
        .iter()
        .cloned()
        .map(|stream| (stream, StreamGeometry::default()))
        .collect::<HashMap<_, _>>();
    let mut available_attributes = 0;
    let mut position_register = None;
    let mut size_register = None;

    for stage in &trace.stages {
        let stage_id = match stage {
            PointStage::StoreAttribute(node_id)
            | PointStage::SetPosition(node_id)
            | PointStage::SetSize(node_id)
            | PointStage::Passthrough(node_id) => *node_id,
        };
        let point_input = address(stage_id, POINT_SOURCE_PORT);
        let expected_stream = single_input_source(definition, &point_input).ok_or_else(|| {
            format!("Point operation Node {stage_id} requires one Point Source input")
        })?;
        if !allowed_streams.contains(&expected_stream) {
            return Err(format!(
                "Point operation Node {stage_id} is connected to a different Point domain"
            ));
        }
        let context = FieldContext {
            allowed_streams: &allowed_streams,
            stream_geometry: &stream_geometry,
            available_attributes,
            capabilities,
        };
        let mut output_geometry = context
            .stream_geometry
            .get(&expected_stream)
            .cloned()
            .ok_or_else(|| {
                format!("Point operation Node {stage_id} reads a different Point domain")
            })?;
        match stage {
            PointStage::StoreAttribute(store_id) => {
                let element_type = builder.schema.attributes()[available_attributes].element_type();
                let value = builder.compile_input(
                    &address(*store_id, POINT_ATTRIBUTE_VALUE_PORT),
                    element_type,
                    &context,
                )?;
                builder.emit(CompiledPointInstruction::StoreAttribute {
                    attribute: checked_u16(available_attributes, "Point attribute")?,
                    value: value.register,
                })?;
                available_attributes += 1;
            }
            PointStage::SetPosition(node_id) => {
                let result = builder.compile_set_geometry(
                    *node_id,
                    &expected_stream,
                    GeometryField::Position,
                    &context,
                )?;
                output_geometry.set(GeometryField::Position, result);
            }
            PointStage::SetSize(node_id) => {
                let result = builder.compile_set_geometry(
                    *node_id,
                    &expected_stream,
                    GeometryField::Size,
                    &context,
                )?;
                output_geometry.set(GeometryField::Size, result);
            }
            PointStage::Passthrough(_) => {}
        }
        position_register = output_geometry.register(GeometryField::Position);
        size_register = output_geometry.register(GeometryField::Size);
        let output_stream = address(stage_id, POINT_SOURCE_PORT);
        allowed_streams.insert(output_stream.clone());
        stream_geometry.insert(output_stream, output_geometry);
    }

    let color_target = address(renderer_node_id, SPRITE_COLOR_INPUT_PORT);
    let color_source = single_input_source(definition, &color_target);
    let varying_color = color_source
        .as_ref()
        .is_some_and(|source| builder.depends_on_point(source));
    let selection_target = address(renderer_node_id, SPRITE_SELECTION_INPUT_PORT);
    let varying_selection = single_input_source(definition, &selection_target)
        .as_ref()
        .is_some_and(|source| builder.depends_on_point(source));
    let has_executable_stage = trace
        .stages
        .iter()
        .any(|stage| !matches!(stage, PointStage::Passthrough(_)));
    if !has_executable_stage && !varying_color && !varying_selection {
        return Ok(None);
    }
    let context = FieldContext {
        allowed_streams: &allowed_streams,
        stream_geometry: &stream_geometry,
        available_attributes,
        capabilities,
    };
    let color = builder.compile_input(&color_target, PointAttributeElementType::Color, &context)?;
    let sprite_selection_register = if varying_selection {
        Some(
            builder
                .compile_input(
                    &selection_target,
                    PointAttributeElementType::Number,
                    &context,
                )?
                .register,
        )
    } else {
        None
    };
    let program = CompiledPointProgram {
        schema: builder.schema,
        instructions: builder.instructions,
        color_register: color.register,
        position_register,
        size_register,
        sprite_selection_register,
    };
    Ok(Some(program))
}

struct FieldContext<'a> {
    allowed_streams: &'a HashSet<ModulePortAddress>,
    stream_geometry: &'a HashMap<ModulePortAddress, StreamGeometry>,
    available_attributes: usize,
    capabilities: PointSourceCapabilities,
}

#[derive(Clone, Default)]
struct FieldDependencies {
    point_streams: HashSet<ModulePortAddress>,
    attributes: HashSet<usize>,
}

impl FieldDependencies {
    fn merge(&mut self, other: &Self) {
        self.point_streams
            .extend(other.point_streams.iter().cloned());
        self.attributes.extend(other.attributes.iter().copied());
    }

    fn validate(&self, context: &FieldContext<'_>) -> Result<(), String> {
        if let Some(stream) = self
            .point_streams
            .iter()
            .find(|stream| !context.allowed_streams.contains(*stream))
        {
            return Err(format!(
                "Per-Point field reads a different Point domain at {}:{}",
                stream.node_id, stream.port
            ));
        }
        if let Some(attribute) = self
            .attributes
            .iter()
            .find(|attribute| **attribute >= context.available_attributes)
        {
            return Err(format!(
                "Point attribute {attribute} is read before its Store in this stream"
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
struct CompiledValue {
    register: u16,
    value_type: CompiledPointValueType,
    dependencies: FieldDependencies,
}

struct PointProgramBuilder<'a> {
    definition: &'a ModuleDefinition,
    schema: PointAttributeSchema,
    instructions: Vec<CompiledPointInstruction>,
    values: HashMap<ModulePortAddress, CompiledValue>,
    uniforms: HashMap<(ModulePortAddress, CompiledPointValueType), CompiledValue>,
    dependency_resolver: PointDependencyResolver<'a>,
    ramp_count: usize,
}

impl<'a> PointProgramBuilder<'a> {
    fn new(definition: &'a ModuleDefinition, schema: PointAttributeSchema) -> Self {
        Self {
            definition,
            schema,
            instructions: Vec::new(),
            values: HashMap::new(),
            uniforms: HashMap::new(),
            dependency_resolver: PointDependencyResolver::new(definition),
            ramp_count: 0,
        }
    }

    fn depends_on_point(&mut self, source: &ModulePortAddress) -> bool {
        self.dependency_resolver.depends_on_point(source)
    }

    fn compile_set_geometry(
        &mut self,
        node_id: uuid::Uuid,
        stream: &ModulePortAddress,
        field: GeometryField,
        context: &FieldContext<'_>,
    ) -> Result<CompiledValue, String> {
        let upstream = self.compile_stream_geometry(stream, field, context)?;
        let target = address(node_id, field.input_port());
        let base = if single_input_source(self.definition, &target).is_some()
            || self
                .definition
                .interface
                .parameters
                .iter()
                .any(|parameter| parameter.target == target)
        {
            self.compile_input(&target, field.element_type(), context)?
        } else {
            upstream.clone()
        };
        let (modifier_port, operation) = field.modifier();
        let modifier = self.compile_input(
            &address(node_id, modifier_port),
            field.element_type(),
            context,
        )?;
        let selected = self.compile_input(
            &address(node_id, POINT_SELECTION_INPUT_PORT),
            PointAttributeElementType::Boolean,
            context,
        )?;
        let modified = self.emit_binary(operation, base, modifier)?;
        self.emit_select(field.element_type(), selected, modified, upstream)
    }

    fn compile_stream_geometry(
        &mut self,
        stream: &ModulePortAddress,
        field: GeometryField,
        context: &FieldContext<'_>,
    ) -> Result<CompiledValue, String> {
        match context.stream_geometry.get(stream) {
            Some(geometry) => match geometry.get(field) {
                Some(value) => Ok(value.clone()),
                None => {
                    let mut dependencies = FieldDependencies::default();
                    dependencies.point_streams.insert(stream.clone());
                    Ok(CompiledValue {
                        register: self.emit(field.source_instruction())?,
                        value_type: field.element_type().into(),
                        dependencies,
                    })
                }
            },
            None => Err(format!(
                "Point geometry reads a different Point domain at {}:{}",
                stream.node_id, stream.port
            )),
        }
    }

    fn emit_binary(
        &mut self,
        operation: crate::model::point::NumericBinaryOperation,
        left: CompiledValue,
        right: CompiledValue,
    ) -> Result<CompiledValue, String> {
        let value_type = left.value_type.binary_result(right.value_type)?;
        let mut dependencies = left.dependencies;
        dependencies.merge(&right.dependencies);
        Ok(CompiledValue {
            register: self.emit(CompiledPointInstruction::Binary {
                operation,
                left: left.register,
                right: right.register,
            })?,
            value_type,
            dependencies,
        })
    }

    fn emit_select(
        &mut self,
        element_type: PointAttributeElementType,
        condition: CompiledValue,
        when_true: CompiledValue,
        when_false: CompiledValue,
    ) -> Result<CompiledValue, String> {
        let mut dependencies = condition.dependencies;
        dependencies.merge(&when_true.dependencies);
        dependencies.merge(&when_false.dependencies);
        Ok(CompiledValue {
            register: self.emit(CompiledPointInstruction::Select {
                element_type,
                condition: condition.register,
                when_true: when_true.register,
                when_false: when_false.register,
            })?,
            value_type: element_type.into(),
            dependencies,
        })
    }

    fn compile_input(
        &mut self,
        target: &ModulePortAddress,
        value_type: impl Into<CompiledPointValueType>,
        context: &FieldContext<'_>,
    ) -> Result<CompiledValue, String> {
        let value_type = value_type.into();
        let source = single_input_source(self.definition, target);
        if let Some(source) = source.as_ref()
            && self.depends_on_point(source)
        {
            let value = self.compile_source(source, value_type, context)?;
            require_input_type(&value, value_type, target)?;
            return Ok(value);
        }
        let key = (target.clone(), value_type);
        if let Some(value) = self.uniforms.get(&key) {
            return Ok(value.clone());
        }
        let register = self.emit(CompiledPointInstruction::Uniform {
            node_id: target.node_id,
            port: target.port.clone(),
            value_type,
        })?;
        let value = CompiledValue {
            register,
            value_type,
            dependencies: FieldDependencies::default(),
        };
        self.uniforms.insert(key, value.clone());
        Ok(value)
    }

    fn compile_source(
        &mut self,
        source: &ModulePortAddress,
        expected_type: CompiledPointValueType,
        context: &FieldContext<'_>,
    ) -> Result<CompiledValue, String> {
        if let Some(value) = self.values.get(source) {
            value.dependencies.validate(context)?;
            return Ok(value.clone());
        }
        let node = self
            .definition
            .graph
            .nodes
            .get(&source.node_id)
            .cloned()
            .ok_or_else(|| format!("Point field reaches missing Node {}", source.node_id))?;
        if !node.enabled {
            return Err(format!(
                "Per-Point field reaches disabled Node {}",
                source.node_id
            ));
        }
        if node.bypassed {
            let input = node.bypass_input_for_output(&source.port).ok_or_else(|| {
                format!(
                    "Per-Point field Node {} has no bypass for '{}'",
                    node.id, source.port
                )
            })?;
            return self.compile_input(&address(node.id, input), expected_type, context);
        }

        let value = if let NodeContent::NativeOperation(operation) = node.content()
            && source.port == NUMBER_RESULT_OUTPUT_PORT
            && let Some(role) = ConditionalNodeRole::from_catalog_id(&operation.catalog_id)
        {
            self.compile_conditional(&node, role, context)?
        } else {
            match node.content() {
                NodeContent::NativeOperation(operation)
                    if operation.catalog_id == NUMERIC_LENGTH_CATALOG_ID
                        && source.port == NUMBER_RESULT_OUTPUT_PORT =>
                {
                    let input = self.compile_input(
                        &address(node.id, NUMERIC_LENGTH_INPUT_PORT),
                        CompiledPointValueType::Numeric,
                        context,
                    )?;
                    let register = self.emit(CompiledPointInstruction::Length {
                        value: input.register,
                    })?;
                    CompiledValue {
                        register,
                        value_type: PointAttributeElementType::Number.into(),
                        dependencies: input.dependencies,
                    }
                }
                NodeContent::NativeOperation(_) => match point_role(&node) {
                    Some(PointNodeRole::Info) => self.compile_point_info(&node, source, context)?,
                    Some(PointNodeRole::StoreAttribute(_)) => {
                        self.compile_attribute_load(&node, source, context)?
                    }
                    Some(
                        PointNodeRole::Grid | PointNodeRole::SetPosition | PointNodeRole::SetSize,
                    )
                    | None => {
                        return Err(unsupported_field_node(&node, source));
                    }
                },
                NodeContent::Value(operation) if source.port == NUMBER_RESULT_OUTPUT_PORT => {
                    let left = self.compile_input(
                        &address(node.id, operation.primary_input()),
                        CompiledPointValueType::Numeric,
                        context,
                    )?;
                    let right = self.compile_input(
                        &address(node.id, operation.secondary_input()),
                        CompiledPointValueType::Numeric,
                        context,
                    )?;
                    let value_type = left.value_type.binary_result(right.value_type)?;
                    let mut dependencies = left.dependencies.clone();
                    dependencies.merge(&right.dependencies);
                    let register = self.emit(CompiledPointInstruction::Binary {
                        operation: operation.numeric_operation(),
                        left: left.register,
                        right: right.register,
                    })?;
                    CompiledValue {
                        register,
                        value_type,
                        dependencies,
                    }
                }
                NodeContent::Color(ColorContent::ColorRamp) if source.port == COLOR_VALUE_PORT => {
                    let gradient = address(node.id, COLOR_RAMP_GRADIENT_PORT);
                    if single_input_source(self.definition, &gradient)
                        .as_ref()
                        .is_some_and(|source| self.depends_on_point(source))
                    {
                        return Err(format!(
                            "Point Color Ramp Node {} requires a frame-uniform Gradient",
                            node.id
                        ));
                    }
                    self.ramp_count += 1;
                    if self.ramp_count > POINT_MAX_RAMPS {
                        return Err(format!(
                            "Point program exceeds the maximum of {POINT_MAX_RAMPS} Color Ramps"
                        ));
                    }
                    let factor = self.compile_input(
                        &address(node.id, COLOR_RAMP_FACTOR_PORT),
                        PointAttributeElementType::Number,
                        context,
                    )?;
                    let register = self.emit(CompiledPointInstruction::ColorRamp {
                        gradient,
                        factor: factor.register,
                    })?;
                    CompiledValue {
                        register,
                        value_type: PointAttributeElementType::Color.into(),
                        dependencies: factor.dependencies,
                    }
                }
                _ => return Err(unsupported_field_node(&node, source)),
            }
        };
        value.dependencies.validate(context)?;
        self.values.insert(source.clone(), value.clone());
        Ok(value)
    }

    fn compile_conditional(
        &mut self,
        node: &Node,
        role: ConditionalNodeRole,
        context: &FieldContext<'_>,
    ) -> Result<CompiledValue, String> {
        let (instruction, value_type, inputs) = match role {
            ConditionalNodeRole::Compare(operation) => {
                let left = self.compile_input(
                    &address(node.id, "a"),
                    PointAttributeElementType::Number,
                    context,
                )?;
                let right = self.compile_input(
                    &address(node.id, "b"),
                    PointAttributeElementType::Number,
                    context,
                )?;
                (
                    CompiledPointInstruction::Compare {
                        operation,
                        left: left.register,
                        right: right.register,
                    },
                    PointAttributeElementType::Boolean,
                    vec![left, right],
                )
            }
            ConditionalNodeRole::Select(data_type) => {
                let kind = PointAttributeElementType::from_port_data_type(data_type)?;
                let condition = self.compile_input(
                    &address(node.id, CONDITION_INPUT_PORT),
                    PointAttributeElementType::Boolean,
                    context,
                )?;
                // Select is eager value selection, not a control-flow branch.
                // Both inputs belong to the same Point domain and must type-check.
                let when_true =
                    self.compile_input(&address(node.id, SELECT_TRUE_INPUT_PORT), kind, context)?;
                let when_false =
                    self.compile_input(&address(node.id, SELECT_FALSE_INPUT_PORT), kind, context)?;
                (
                    CompiledPointInstruction::Select {
                        element_type: kind,
                        condition: condition.register,
                        when_true: when_true.register,
                        when_false: when_false.register,
                    },
                    kind,
                    vec![condition, when_true, when_false],
                )
            }
        };
        let mut dependencies = FieldDependencies::default();
        for input in inputs {
            dependencies.merge(&input.dependencies);
        }
        Ok(CompiledValue {
            register: self.emit(instruction)?,
            value_type: value_type.into(),
            dependencies,
        })
    }

    fn compile_point_info(
        &mut self,
        node: &Node,
        source: &ModulePortAddress,
        context: &FieldContext<'_>,
    ) -> Result<CompiledValue, String> {
        let point_input = address(node.id, POINT_SOURCE_PORT);
        let point_stream = single_input_source(self.definition, &point_input).ok_or_else(|| {
            format!(
                "Point Info Node {} requires one Point Source input",
                node.id
            )
        })?;
        let mut dependencies = FieldDependencies::default();
        dependencies.point_streams.insert(point_stream.clone());
        dependencies.validate(context)?;
        if let Some(field) = GeometryField::from_info_port(&source.port) {
            return self.compile_stream_geometry(&point_stream, field, context);
        }
        let instruction = match source.port.as_str() {
            POINT_AGE_OUTPUT_PORT if context.capabilities.age => CompiledPointInstruction::Age,
            POINT_NORMALIZED_AGE_OUTPUT_PORT if context.capabilities.age => {
                CompiledPointInstruction::NormalizedAge
            }
            POINT_AGE_OUTPUT_PORT | POINT_NORMALIZED_AGE_OUTPUT_PORT => {
                return Err(format!(
                    "Point Info output '{}' requires a Particle source with lifetime data",
                    source.port
                ));
            }
            POINT_RANDOM_OUTPUT_PORT => CompiledPointInstruction::Random { channel: 0 },
            _ => {
                return Err(format!(
                    "Point Info Node {} has no field output '{}'",
                    node.id, source.port
                ));
            }
        };
        let register = self.emit(instruction)?;
        Ok(CompiledValue {
            register,
            value_type: PointAttributeElementType::Number.into(),
            dependencies,
        })
    }

    fn compile_attribute_load(
        &mut self,
        node: &Node,
        source: &ModulePortAddress,
        context: &FieldContext<'_>,
    ) -> Result<CompiledValue, String> {
        if source.port != POINT_ATTRIBUTE_OUTPUT_PORT {
            return Err(format!(
                "Point Store Node {} output '{}' is not a field value",
                node.id, source.port
            ));
        }
        let attribute_id = PointAttributeId::from_uuid(node.id);
        let attribute = self
            .schema
            .attributes()
            .iter()
            .position(|attribute| attribute.id() == attribute_id)
            .ok_or_else(|| {
                format!(
                    "Point Store Node {} is outside this Sprite's Point stream",
                    node.id
                )
            })?;
        let mut dependencies = FieldDependencies::default();
        dependencies.attributes.insert(attribute);
        dependencies.validate(context)?;
        let element_type = self.schema.attributes()[attribute].element_type();
        let register = self.emit(CompiledPointInstruction::LoadAttribute {
            attribute: checked_u16(attribute, "Point attribute")?,
        })?;
        Ok(CompiledValue {
            register,
            value_type: element_type.into(),
            dependencies,
        })
    }

    fn emit(&mut self, instruction: CompiledPointInstruction) -> Result<u16, String> {
        if self.instructions.len() >= POINT_MAX_INSTRUCTIONS {
            return Err(format!(
                "Point program exceeds the maximum of {POINT_MAX_INSTRUCTIONS} instructions"
            ));
        }
        let register = checked_u16(self.instructions.len(), "Point register")?;
        self.instructions.push(instruction);
        Ok(register)
    }
}

fn require_input_type(
    value: &CompiledValue,
    expected: CompiledPointValueType,
    target: &ModulePortAddress,
) -> Result<(), String> {
    if !expected.compatible_with(value.value_type) {
        return Err(format!(
            "Per-Point input {}:{} requires {expected}, received {}; implicit varying conversions are not supported",
            target.node_id, target.port, value.value_type
        ));
    }
    Ok(())
}

fn unsupported_field_node(node: &Node, source: &ModulePortAddress) -> String {
    format!(
        "Node {} ('{}') output '{}' cannot execute in a per-Point field",
        node.id, node.name, source.port
    )
}

fn point_role(node: &Node) -> Option<PointNodeRole> {
    match node.content() {
        NodeContent::NativeOperation(operation) => {
            PointNodeRole::from_catalog_id(&operation.catalog_id)
        }
        _ => None,
    }
}

fn particle_sprite(node: &Node) -> bool {
    matches!(
        node.content(),
        NodeContent::NativeOperation(operation)
            if operation.catalog_id == crate::model::node::PARTICLE_SPRITE_RENDERER_CATALOG_ID
    )
}

fn single_input_source(
    definition: &ModuleDefinition,
    target: &ModulePortAddress,
) -> Option<ModulePortAddress> {
    definition
        .graph
        .connections
        .iter()
        .find(|connection| connection.to == *target)
        .map(|connection| connection.from.clone())
}

fn address(node_id: uuid::Uuid, port: &str) -> ModulePortAddress {
    ModulePortAddress {
        node_id,
        port: port.to_string(),
    }
}

fn checked_u16(value: usize, label: &str) -> Result<u16, String> {
    u16::try_from(value).map_err(|_| format!("{label} index exceeds u16"))
}
