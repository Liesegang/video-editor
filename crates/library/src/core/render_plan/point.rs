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
    NodeContent, PARTICLE_SYSTEM_PORT, POINT_ATTRIBUTE_OUTPUT_PORT, POINT_ATTRIBUTE_VALUE_PORT,
    POINT_SOURCE_PORT, PointNodeRole, SELECT_FALSE_INPUT_PORT, SELECT_TRUE_INPUT_PORT,
};
use crate::model::point::{
    POINT_MAX_INSTRUCTIONS, POINT_MAX_RAMPS, PointAttributeDefinition, PointAttributeElementType,
    PointAttributeId, PointAttributeSchema,
};
use crate::model::project::NUMBER_RESULT_OUTPUT_PORT;

const POINT_AGE_OUTPUT_PORT: &str = "age";
const POINT_NORMALIZED_AGE_OUTPUT_PORT: &str = "normalized_age";
const POINT_POSITION_OUTPUT_PORT: &str = "position";
const POINT_RANDOM_OUTPUT_PORT: &str = "random";
const SPRITE_COLOR_INPUT_PORT: &str = "color";

/// Point stream selected by one Sprite endpoint before Particle stage
/// recognition. Stores are ordered upstream-to-downstream.
struct PointStreamTrace {
    pub terminal_source: ModulePortAddress,
    stores: Vec<uuid::Uuid>,
}

#[derive(Clone, Copy)]
struct PointSourceCapabilities {
    age: bool,
}

struct PointSourceCompilation {
    source: CompiledPointSource,
    lineage: HashSet<ModulePortAddress>,
    capabilities: PointSourceCapabilities,
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

fn trace_point_stream(
    definition: &ModuleDefinition,
    renderer_node_id: uuid::Uuid,
) -> Result<Option<PointStreamTrace>, String> {
    let renderer_input = address(renderer_node_id, PARTICLE_SYSTEM_PORT);
    let Some(mut source) = single_input_source(definition, &renderer_input) else {
        return Ok(None);
    };
    let mut stores = Vec::new();
    let mut visited = HashSet::new();
    loop {
        let Some(node) = definition.graph.nodes.get(&source.node_id) else {
            return Ok(None);
        };
        if point_role(node)
            .and_then(PointNodeRole::attribute_type)
            .is_none()
        {
            break;
        }
        if source.port != POINT_SOURCE_PORT || !visited.insert(node.id) {
            return Ok(None);
        }
        if !node.enabled || node.bypassed {
            return Ok(None);
        }
        stores.push(node.id);
        let store_input = address(node.id, POINT_SOURCE_PORT);
        let Some(upstream) = single_input_source(definition, &store_input) else {
            return Ok(None);
        };
        source = upstream;
    }
    stores.reverse();
    Ok(Some(PointStreamTrace {
        terminal_source: source,
        stores,
    }))
}

fn compile_point_program(
    definition: &ModuleDefinition,
    renderer_node_id: uuid::Uuid,
    trace: &PointStreamTrace,
    source_lineage: &HashSet<ModulePortAddress>,
    capabilities: PointSourceCapabilities,
) -> Result<Option<CompiledPointProgram>, String> {
    let mut definitions = Vec::with_capacity(trace.stores.len());
    for store_id in &trace.stores {
        let store =
            definition.graph.nodes.get(store_id).ok_or_else(|| {
                format!("Point Store Node {store_id} disappeared during compilation")
            })?;
        let element_type = point_role(store)
            .and_then(PointNodeRole::attribute_type)
            .ok_or_else(|| format!("Point Store Node {store_id} has no attribute type"))?;
        definitions.push(PointAttributeDefinition::new(
            PointAttributeId::from_uuid(store.id),
            store.name.clone(),
            element_type,
            element_type.default_value(),
        )?);
    }
    let schema = PointAttributeSchema::new(definitions)?;
    let mut builder = PointProgramBuilder::new(definition, schema);
    let mut allowed_streams = source_lineage.clone();

    for (attribute, store_id) in trace.stores.iter().enumerate() {
        let point_input = address(*store_id, POINT_SOURCE_PORT);
        let expected_stream = single_input_source(definition, &point_input).ok_or_else(|| {
            format!("Point Store Node {store_id} requires one Point Source input")
        })?;
        if !allowed_streams.contains(&expected_stream) {
            return Err(format!(
                "Point Store Node {store_id} is connected to a different Point domain"
            ));
        }
        let context = FieldContext {
            allowed_streams: &allowed_streams,
            available_attributes: attribute,
            capabilities,
        };
        let element_type = builder.schema.attributes()[attribute].element_type();
        let value = builder.compile_input(
            &address(*store_id, POINT_ATTRIBUTE_VALUE_PORT),
            element_type,
            &context,
        )?;
        builder.emit(CompiledPointInstruction::StoreAttribute {
            attribute: checked_u16(attribute, "Point attribute")?,
            value: value.register,
        })?;
        allowed_streams.insert(address(*store_id, POINT_SOURCE_PORT));
    }

    let color_target = address(renderer_node_id, SPRITE_COLOR_INPUT_PORT);
    let color_source = single_input_source(definition, &color_target);
    let varying_color = color_source
        .as_ref()
        .is_some_and(|source| builder.depends_on_point(source));
    if trace.stores.is_empty() && !varying_color {
        return Ok(None);
    }
    let context = FieldContext {
        allowed_streams: &allowed_streams,
        available_attributes: trace.stores.len(),
        capabilities,
    };
    let color = builder.compile_input(&color_target, PointAttributeElementType::Color, &context)?;
    let program = CompiledPointProgram {
        schema: builder.schema,
        instructions: builder.instructions,
        color_register: color.register,
    };
    Ok(Some(program))
}

/// Reject a varying Point value before the stateless value runtime can mistake
/// it for one frame-wide PropertyValue. Dead editor branches remain harmless.
pub(super) fn validate_point_field_consumers(
    definition: &ModuleDefinition,
    active_nodes: &HashSet<uuid::Uuid>,
) -> Result<(), String> {
    let mut dependencies = PointDependencyResolver::new(definition);
    for connection in &definition.graph.connections {
        if !active_nodes.contains(&connection.to.node_id)
            || !dependencies.depends_on_point(&connection.from)
        {
            continue;
        }
        let target = definition
            .graph
            .nodes
            .get(&connection.to.node_id)
            .ok_or_else(|| "Point field reaches a missing consumer".to_string())?;
        let supported = match target.content() {
            NodeContent::Value(operation) => {
                connection.to.port == operation.primary_input()
                    || connection.to.port == operation.secondary_input()
            }
            NodeContent::Color(ColorContent::ColorRamp) => {
                connection.to.port == COLOR_RAMP_FACTOR_PORT
            }
            NodeContent::NativeOperation(operation) => {
                if let Some(role) = ConditionalNodeRole::from_catalog_id(&operation.catalog_id) {
                    match role {
                        ConditionalNodeRole::Compare(_) => {
                            matches!(connection.to.port.as_str(), "a" | "b")
                        }
                        ConditionalNodeRole::Select(_) => matches!(
                            connection.to.port.as_str(),
                            CONDITION_INPUT_PORT | SELECT_TRUE_INPUT_PORT | SELECT_FALSE_INPUT_PORT
                        ),
                    }
                } else if operation.catalog_id == NUMERIC_LENGTH_CATALOG_ID {
                    connection.to.port == NUMERIC_LENGTH_INPUT_PORT
                } else {
                    match point_role(target) {
                        Some(PointNodeRole::StoreAttribute(_)) => {
                            connection.to.port == POINT_ATTRIBUTE_VALUE_PORT
                        }
                        Some(PointNodeRole::Info) => false,
                        Some(PointNodeRole::Grid) => false,
                        None => {
                            particle_sprite(target) && connection.to.port == SPRITE_COLOR_INPUT_PORT
                        }
                    }
                }
            }
            _ => false,
        };
        if !supported {
            return Err(format!(
                "Per-Point value {}:{} cannot drive unsupported input {}:{}",
                connection.from.node_id,
                connection.from.port,
                connection.to.node_id,
                connection.to.port
            ));
        }
    }
    Ok(())
}

struct FieldContext<'a> {
    allowed_streams: &'a HashSet<ModulePortAddress>,
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
                    Some(PointNodeRole::Grid) | None => {
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
        dependencies.point_streams.insert(point_stream);
        dependencies.validate(context)?;
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
            POINT_POSITION_OUTPUT_PORT => CompiledPointInstruction::Position,
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
            value_type: if source.port == POINT_POSITION_OUTPUT_PORT {
                PointAttributeElementType::Vec3
            } else {
                PointAttributeElementType::Number
            }
            .into(),
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

struct PointDependencyResolver<'a> {
    definition: &'a ModuleDefinition,
    memo: HashMap<ModulePortAddress, bool>,
    visiting: HashSet<ModulePortAddress>,
}

impl<'a> PointDependencyResolver<'a> {
    fn new(definition: &'a ModuleDefinition) -> Self {
        Self {
            definition,
            memo: HashMap::new(),
            visiting: HashSet::new(),
        }
    }

    fn depends_on_point(&mut self, source: &ModulePortAddress) -> bool {
        if let Some(result) = self.memo.get(source) {
            return *result;
        }
        if !self.visiting.insert(source.clone()) {
            return true;
        }
        let result = self.depends_on_point_inner(source);
        self.visiting.remove(source);
        self.memo.insert(source.clone(), result);
        result
    }

    fn depends_on_point_inner(&mut self, source: &ModulePortAddress) -> bool {
        let Ok(port) = self
            .definition
            .graph
            .port_definition(source, crate::model::project::PortDirection::Output)
        else {
            return false;
        };
        if !matches!(
            port.data_type,
            crate::model::project::PortDataType::Number
                | crate::model::project::PortDataType::Boolean
                | crate::model::project::PortDataType::Integer
                | crate::model::project::PortDataType::Numeric
                | crate::model::project::PortDataType::Vec2
                | crate::model::project::PortDataType::Vec3
                | crate::model::project::PortDataType::Vec4
                | crate::model::project::PortDataType::Color
        ) {
            return false;
        }
        let Some(node) = self.definition.graph.nodes.get(&source.node_id).cloned() else {
            return false;
        };
        if point_role(&node).is_some_and(|role| match role {
            PointNodeRole::Info => matches!(
                source.port.as_str(),
                POINT_AGE_OUTPUT_PORT
                    | POINT_NORMALIZED_AGE_OUTPUT_PORT
                    | POINT_RANDOM_OUTPUT_PORT
                    | POINT_POSITION_OUTPUT_PORT
            ),
            PointNodeRole::StoreAttribute(_) => source.port == POINT_ATTRIBUTE_OUTPUT_PORT,
            PointNodeRole::Grid => false,
        }) {
            return true;
        }
        if node.bypassed
            && let Some(input) = node.bypass_input_for_output(&source.port)
        {
            return single_input_source(self.definition, &address(node.id, input))
                .is_some_and(|source| self.depends_on_point(&source));
        }
        let inputs = self
            .definition
            .graph
            .connections
            .iter()
            .filter(|connection| connection.to.node_id == node.id)
            .map(|connection| connection.from.clone())
            .collect::<Vec<_>>();
        inputs.iter().any(|source| self.depends_on_point(source))
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
