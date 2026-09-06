//! Compilation of bounded render-stage Point fields.

use std::collections::{HashMap, HashSet};

use ordered_float::OrderedFloat;

use crate::model::authoring::{ModuleDefinition, ModulePortAddress};
use crate::model::node::{
    COLOR_RAMP_FACTOR_PORT, COLOR_RAMP_GRADIENT_PORT, COLOR_VALUE_PORT, ColorContent, Node,
    NodeContent, PARTICLE_SYSTEM_PORT, POINT_ATTRIBUTE_OUTPUT_PORT, POINT_ATTRIBUTE_VALUE_PORT,
    POINT_SOURCE_PORT, PointNodeRole,
};
use crate::model::point::{
    POINT_MAX_INSTRUCTIONS, POINT_MAX_RAMPS, PointAttributeDefinition, PointAttributeElementType,
    PointAttributeId, PointAttributeSchema,
};
use crate::model::project::NUMBER_RESULT_OUTPUT_PORT;
use crate::model::property::PropertyValue;

use super::{CompiledPointInstruction, CompiledPointProgram};

const POINT_AGE_OUTPUT_PORT: &str = "age";
const POINT_NORMALIZED_AGE_OUTPUT_PORT: &str = "normalized_age";
const POINT_RANDOM_OUTPUT_PORT: &str = "random";
const SPRITE_COLOR_INPUT_PORT: &str = "color";

/// Point stream selected by one Sprite endpoint before Particle stage
/// recognition. Stores are ordered upstream-to-downstream.
pub(super) struct PointStreamTrace {
    pub particle_source: ModulePortAddress,
    stores: Vec<uuid::Uuid>,
}

pub(super) fn trace_point_stream(
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
        if point_role(node) != Some(PointNodeRole::StoreNumberAttribute) {
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
    if source.port != PARTICLE_SYSTEM_PORT {
        return Ok(None);
    }
    stores.reverse();
    Ok(Some(PointStreamTrace {
        particle_source: source,
        stores,
    }))
}

pub(super) fn compile_point_program(
    definition: &ModuleDefinition,
    renderer_node_id: uuid::Uuid,
    trace: &PointStreamTrace,
    particle_lineage: &HashSet<ModulePortAddress>,
) -> Result<Option<CompiledPointProgram>, String> {
    let mut definitions = Vec::with_capacity(trace.stores.len());
    for store_id in &trace.stores {
        let store =
            definition.graph.nodes.get(store_id).ok_or_else(|| {
                format!("Point Store Node {store_id} disappeared during compilation")
            })?;
        definitions.push(PointAttributeDefinition::new(
            PointAttributeId::from_uuid(store.id),
            store.name.clone(),
            PointAttributeElementType::Number,
            PropertyValue::Number(OrderedFloat(0.0)),
        )?);
    }
    let schema = PointAttributeSchema::new(definitions)?;
    let mut builder = PointProgramBuilder::new(definition, schema);
    let mut allowed_streams = particle_lineage.clone();

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
        };
        let value = builder.compile_input(
            &address(*store_id, POINT_ATTRIBUTE_VALUE_PORT),
            PointAttributeElementType::Number,
            &context,
        )?;
        builder.emit(CompiledPointInstruction::StoreNumber {
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
            NodeContent::NativeOperation(_) => match point_role(target) {
                Some(PointNodeRole::StoreNumberAttribute) => {
                    connection.to.port == POINT_ATTRIBUTE_VALUE_PORT
                }
                Some(PointNodeRole::Info) => false,
                None => particle_sprite(target) && connection.to.port == SPRITE_COLOR_INPUT_PORT,
            },
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
    element_type: PointAttributeElementType,
    dependencies: FieldDependencies,
}

struct PointProgramBuilder<'a> {
    definition: &'a ModuleDefinition,
    schema: PointAttributeSchema,
    instructions: Vec<CompiledPointInstruction>,
    values: HashMap<ModulePortAddress, CompiledValue>,
    uniforms: HashMap<(ModulePortAddress, PointAttributeElementType), CompiledValue>,
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
        element_type: PointAttributeElementType,
        context: &FieldContext<'_>,
    ) -> Result<CompiledValue, String> {
        let source = single_input_source(self.definition, target);
        if let Some(source) = source.as_ref()
            && self.depends_on_point(source)
        {
            return self.compile_source(source, context);
        }
        let key = (target.clone(), element_type);
        if let Some(value) = self.uniforms.get(&key) {
            return Ok(value.clone());
        }
        let register = self.emit(CompiledPointInstruction::Uniform {
            node_id: target.node_id,
            port: target.port.clone(),
            element_type,
        })?;
        let value = CompiledValue {
            register,
            element_type,
            dependencies: FieldDependencies::default(),
        };
        self.uniforms.insert(key, value.clone());
        Ok(value)
    }

    fn compile_source(
        &mut self,
        source: &ModulePortAddress,
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
            return self.compile_input(
                &address(node.id, input),
                expected_output_type(self.definition, source)?,
                context,
            );
        }

        let value = match node.content() {
            NodeContent::NativeOperation(_) => match point_role(&node) {
                Some(PointNodeRole::Info) => self.compile_point_info(&node, source, context)?,
                Some(PointNodeRole::StoreNumberAttribute) => {
                    self.compile_attribute_load(&node, source, context)?
                }
                None => return Err(unsupported_field_node(&node, source)),
            },
            NodeContent::Value(operation) if source.port == NUMBER_RESULT_OUTPUT_PORT => {
                let left = self.compile_input(
                    &address(node.id, operation.primary_input()),
                    PointAttributeElementType::Number,
                    context,
                )?;
                let right = self.compile_input(
                    &address(node.id, operation.secondary_input()),
                    PointAttributeElementType::Number,
                    context,
                )?;
                require_type(&left, PointAttributeElementType::Number, node.id)?;
                require_type(&right, PointAttributeElementType::Number, node.id)?;
                let mut dependencies = left.dependencies.clone();
                dependencies.merge(&right.dependencies);
                let register = self.emit(CompiledPointInstruction::Binary {
                    operation: operation.numeric_operation(),
                    left: left.register,
                    right: right.register,
                })?;
                CompiledValue {
                    register,
                    element_type: PointAttributeElementType::Number,
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
                require_type(&factor, PointAttributeElementType::Number, node.id)?;
                let register = self.emit(CompiledPointInstruction::ColorRamp {
                    gradient,
                    factor: factor.register,
                })?;
                CompiledValue {
                    register,
                    element_type: PointAttributeElementType::Color,
                    dependencies: factor.dependencies,
                }
            }
            _ => return Err(unsupported_field_node(&node, source)),
        };
        value.dependencies.validate(context)?;
        self.values.insert(source.clone(), value.clone());
        Ok(value)
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
            POINT_AGE_OUTPUT_PORT => CompiledPointInstruction::Age,
            POINT_NORMALIZED_AGE_OUTPUT_PORT => CompiledPointInstruction::NormalizedAge,
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
            element_type: PointAttributeElementType::Number,
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
        let register = self.emit(CompiledPointInstruction::LoadAttribute {
            attribute: checked_u16(attribute, "Point attribute")?,
        })?;
        Ok(CompiledValue {
            register,
            element_type: PointAttributeElementType::Number,
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
                | crate::model::project::PortDataType::Integer
                | crate::model::project::PortDataType::Numeric
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
                POINT_AGE_OUTPUT_PORT | POINT_NORMALIZED_AGE_OUTPUT_PORT | POINT_RANDOM_OUTPUT_PORT
            ),
            PointNodeRole::StoreNumberAttribute => source.port == POINT_ATTRIBUTE_OUTPUT_PORT,
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

fn require_type(
    value: &CompiledValue,
    expected: PointAttributeElementType,
    node_id: uuid::Uuid,
) -> Result<(), String> {
    if value.element_type != expected {
        return Err(format!(
            "Point field Node {node_id} requires {expected:?}, received {:?}",
            value.element_type
        ));
    }
    Ok(())
}

fn expected_output_type(
    definition: &ModuleDefinition,
    source: &ModulePortAddress,
) -> Result<PointAttributeElementType, String> {
    let data_type = definition
        .graph
        .port_definition(source, crate::model::project::PortDirection::Output)?
        .data_type;
    match data_type {
        crate::model::project::PortDataType::Number
        | crate::model::project::PortDataType::Integer
        | crate::model::project::PortDataType::Numeric => Ok(PointAttributeElementType::Number),
        crate::model::project::PortDataType::Color => Ok(PointAttributeElementType::Color),
        _ => Err(format!(
            "Per-Point output {}:{} has unsupported type {data_type:?}",
            source.node_id, source.port
        )),
    }
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
