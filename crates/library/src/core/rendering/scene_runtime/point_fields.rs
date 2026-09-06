//! Trusted GPU lowering for bounded render-stage Point field programs.

use glow::HasContext;
use sha2::{Digest, Sha256};

use crate::error::LibraryError;
use crate::model::point::{
    NumericBinaryOperation, POINT_MAX_ATTRIBUTE_COUNT, POINT_MAX_INSTRUCTIONS,
    POINT_MAX_RAMP_STOPS, POINT_MAX_RAMPS, PointAttributeElementType, PointAttributeGpuDefault,
    PointColumnLayout, PointInstruction, PointRenderProgram,
};
use crate::model::property::{GradientSpread, PropertyValue};
use crate::rendering::gl_resources::link_program;

use super::gl_backend::{PARTICLE_WORKGROUP_SIZE, required_uniform};
use super::shaders::{PARTICLE_RANDOM_FUNCTIONS, PARTICLE_STRUCT_GLSL};
use super::source::{PointSourceBinding, PointSourceKind, PointSourceUniforms, RENDER_POINT_GLSL};
use super::{drain_gl_errors, gl_operation_result};

const COLOR_STRIDE_BYTES: u64 = 16;
const PROGRAM_HEADER_VEC4S: usize = POINT_MAX_INSTRUCTIONS + POINT_MAX_RAMPS;
const RAMP_STOP_VEC4S: usize = 2;
const PROGRAM_DATA_VEC4S: usize =
    PROGRAM_HEADER_VEC4S + POINT_MAX_RAMPS * POINT_MAX_RAMP_STOPS * RAMP_STOP_VEC4S;
const PROGRAM_DATA_BYTES: usize = PROGRAM_DATA_VEC4S * 16;

pub(super) struct PointFieldBuffers {
    pub columns: glow::Buffer,
    pub colors: glow::Buffer,
    pub layout: PointColumnLayout,
}

impl PointFieldBuffers {
    pub fn create(
        gl: &glow::Context,
        program: &PointRenderProgram,
        capacity: u32,
    ) -> Result<Self, LibraryError> {
        let layout = PointColumnLayout::derive(&program.schema, capacity)
            .map_err(LibraryError::Validation)?;
        let color_bytes = color_bytes(capacity)?;
        let columns = allocate_buffer(gl, layout.byte_len, glow::DYNAMIC_COPY, "Point columns")?;
        let colors = match allocate_buffer(gl, color_bytes, glow::DYNAMIC_COPY, "Point colors") {
            Ok(buffer) => buffer,
            Err(error) => {
                // SAFETY: `columns` was created above and has not escaped.
                unsafe { gl.delete_buffer(columns) };
                return Err(error);
            }
        };
        Ok(Self {
            columns,
            colors,
            layout,
        })
    }

    pub fn byte_len(&self) -> u64 {
        self.layout.byte_len + u64::from(self.layout.capacity) * COLOR_STRIDE_BYTES
    }

    pub fn matches(&self, program: &PointRenderProgram) -> bool {
        PointColumnLayout::derive(&program.schema, self.layout.capacity)
            .is_ok_and(|layout| layout == self.layout)
    }

    pub fn destroy(self, gl: &glow::Context) {
        // SAFETY: both buffers are uniquely owned by this value and belong to
        // the current SceneRuntime context.
        unsafe {
            gl.delete_buffer(self.columns);
            gl.delete_buffer(self.colors);
        }
    }
}

#[derive(Clone)]
pub(super) struct PointFieldPipeline {
    compute_program: glow::Program,
    program_data: glow::Buffer,
    capacity: glow::UniformLocation,
    seed: Option<glow::UniformLocation>,
    attribute_offsets: Option<glow::UniformLocation>,
    source: PointSourceUniforms,
    pub source_kind: PointSourceKind,
    pub source_hash: [u8; 32],
}

impl PointFieldPipeline {
    pub fn create(
        gl: &glow::Context,
        source_kind: PointSourceKind,
        program: &PointRenderProgram,
    ) -> Result<Self, LibraryError> {
        program.validate().map_err(LibraryError::Validation)?;
        let source = compute_source(source_kind, program)?;
        let source_hash = Sha256::digest(source.as_bytes()).into();
        let compute_program = link_program(
            gl,
            &[(glow::COMPUTE_SHADER, &source)],
            "Point field compute",
        )?;
        let program_data = match allocate_buffer(
            gl,
            PROGRAM_DATA_BYTES as u64,
            glow::DYNAMIC_DRAW,
            "Point program data",
        ) {
            Ok(buffer) => buffer,
            Err(error) => {
                // SAFETY: program construction succeeded above and this error
                // path remains its sole owner.
                unsafe { gl.delete_program(compute_program) };
                return Err(error);
            }
        };
        let uniforms = (|| {
            Ok((
                required_uniform(gl, compute_program, "uCapacity")?,
                program
                    .instructions
                    .iter()
                    .any(|instruction| matches!(instruction, PointInstruction::Random { .. }))
                    .then(|| required_uniform(gl, compute_program, "uSeed"))
                    .transpose()?,
                if program.schema.attributes().is_empty() {
                    None
                } else {
                    Some(required_uniform(
                        gl,
                        compute_program,
                        "uAttributeOffsets[0]",
                    )?)
                },
                PointSourceUniforms::new(gl, compute_program, source_kind, false)?,
            ))
        })();
        let (capacity, seed, attribute_offsets, source) = match uniforms {
            Ok(uniforms) => uniforms,
            Err(error) => {
                // SAFETY: both resources were created by this context and
                // have not escaped the failed constructor.
                unsafe {
                    gl.delete_buffer(program_data);
                    gl.delete_program(compute_program);
                }
                return Err(error);
            }
        };
        Ok(Self {
            compute_program,
            program_data,
            capacity,
            seed,
            attribute_offsets,
            source,
            source_kind,
            source_hash,
        })
    }

    pub fn evaluate(
        &self,
        gl: &glow::Context,
        program: &PointRenderProgram,
        buffers: &PointFieldBuffers,
        point_source: &PointSourceBinding<'_>,
        seed: u32,
    ) -> Result<(), LibraryError> {
        program.validate().map_err(LibraryError::Validation)?;
        let expected = PointColumnLayout::derive(&program.schema, buffers.layout.capacity)
            .map_err(LibraryError::Validation)?;
        if expected != buffers.layout {
            return Err(LibraryError::Validation(
                "Point field schema changed without rebuilding its GPU columns".to_string(),
            ));
        }
        if point_source.kind() != self.source_kind {
            return Err(LibraryError::Validation(
                "Point source changed without rebuilding its field pipeline".to_string(),
            ));
        }
        let source_hash: [u8; 32] =
            Sha256::digest(compute_source(self.source_kind, program)?.as_bytes()).into();
        if source_hash != self.source_hash {
            return Err(LibraryError::Validation(
                "Point field instruction shape changed without rebuilding its GPU pipeline"
                    .to_string(),
            ));
        }
        let data = program_data(program)?;
        let mut offsets = [0_u32; POINT_MAX_ATTRIBUTE_COUNT];
        for (target, attribute) in offsets.iter_mut().zip(&buffers.layout.attributes) {
            *target = u32::try_from(attribute.offset_bytes / 4).map_err(|_| {
                LibraryError::Validation("Point attribute offset exceeds GPU range".to_string())
            })?;
        }
        // SAFETY: every buffer/program belongs to this live current context;
        // validation bounds the dispatch, registers, columns, and upload.
        unsafe {
            gl.bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(self.program_data));
            gl.buffer_sub_data_u8_slice(glow::SHADER_STORAGE_BUFFER, 0, &data);
            gl.use_program(Some(self.compute_program));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 1, Some(buffers.columns));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 2, Some(buffers.colors));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 3, Some(self.program_data));
            point_source.bind(gl, &self.source)?;
            gl.uniform_1_u32(Some(&self.capacity), buffers.layout.capacity);
            if let Some(location) = &self.seed {
                gl.uniform_1_u32(Some(location), seed);
            }
            if let Some(location) = &self.attribute_offsets {
                gl.uniform_1_u32_slice(Some(location), &offsets);
            }
            gl.dispatch_compute(
                buffers.layout.capacity.div_ceil(PARTICLE_WORKGROUP_SIZE),
                1,
                1,
            );
            gl.memory_barrier(
                glow::SHADER_STORAGE_BARRIER_BIT | glow::VERTEX_ATTRIB_ARRAY_BARRIER_BIT,
            );
        }
        gl_operation_result(gl, "render-stage Point fields")
    }

    pub fn destroy(self, gl: &glow::Context) {
        // SAFETY: the cache entry uniquely owns both resources.
        unsafe {
            gl.delete_buffer(self.program_data);
            gl.delete_program(self.compute_program);
        }
    }
}

pub(super) fn source_hash(
    source_kind: PointSourceKind,
    program: Option<&PointRenderProgram>,
) -> Result<Option<[u8; 32]>, LibraryError> {
    program
        .map(|program| {
            program.validate().map_err(LibraryError::Validation)?;
            Ok(Sha256::digest(compute_source(source_kind, program)?.as_bytes()).into())
        })
        .transpose()
}

pub(super) fn required_invocation_bytes(
    has_particle_state: bool,
    program: Option<&PointRenderProgram>,
    capacity: u32,
) -> Result<u64, LibraryError> {
    let particle_bytes = if has_particle_state {
        u64::from(capacity)
            .checked_mul(super::gl_backend::PARTICLE_STRIDE_BYTES)
            .ok_or_else(|| LibraryError::Render("GPU Particle state size overflow".to_string()))?
    } else {
        0
    };
    let Some(program) = program else {
        return Ok(particle_bytes);
    };
    let layout =
        PointColumnLayout::derive(&program.schema, capacity).map_err(LibraryError::Validation)?;
    particle_bytes
        .checked_add(layout.byte_len)
        .and_then(|bytes| {
            color_bytes(capacity)
                .ok()
                .and_then(|color| bytes.checked_add(color))
        })
        .ok_or_else(|| LibraryError::Render("GPU Point field state size overflow".to_string()))
}

fn color_bytes(capacity: u32) -> Result<u64, LibraryError> {
    u64::from(capacity)
        .checked_mul(COLOR_STRIDE_BYTES)
        .ok_or_else(|| LibraryError::Render("GPU Point color buffer size overflow".to_string()))
}

fn allocate_buffer(
    gl: &glow::Context,
    bytes: u64,
    usage: u32,
    label: &str,
) -> Result<glow::Buffer, LibraryError> {
    let bytes = i32::try_from(bytes)
        .map_err(|_| LibraryError::Render(format!("{label} size exceeds the GPU range")))?;
    // SAFETY: SceneRuntime owns the current context and the checked allocation
    // size is non-negative and representable by this OpenGL binding.
    let buffer = unsafe { gl.create_buffer() }
        .map_err(|error| LibraryError::Render(format!("Cannot create {label}: {error}")))?;
    // SAFETY: `buffer` is the live handle returned above and this context
    // remains current through the checked allocation.
    unsafe {
        gl.bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(buffer));
        gl.buffer_data_size(glow::SHADER_STORAGE_BUFFER, bytes, usage);
    }
    let errors = drain_gl_errors(gl);
    if errors.is_empty() {
        Ok(buffer)
    } else {
        // SAFETY: failed allocation retains sole ownership of this name.
        unsafe { gl.delete_buffer(buffer) };
        Err(LibraryError::Render(format!(
            "{label} allocation failed (OpenGL errors {})",
            errors
                .iter()
                .map(|error| format!("0x{error:04x}"))
                .collect::<Vec<_>>()
                .join(", ")
        )))
    }
}

fn compute_source(
    source_kind: PointSourceKind,
    program: &PointRenderProgram,
) -> Result<String, LibraryError> {
    let register_types = program.register_types().map_err(LibraryError::Validation)?;
    if source_kind == PointSourceKind::Grid
        && program.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                PointInstruction::Age | PointInstruction::NormalizedAge
            )
        })
    {
        return Err(LibraryError::Validation(
            "Point Age and Normalized Age require a Particle source".to_string(),
        ));
    }
    let mut body = String::from("    bool valid = true;\n    pointColumns[slot] = point.serial;\n");
    for (index, instruction) in program.instructions.iter().enumerate() {
        let register = format!("r{index}");
        match instruction {
            PointInstruction::Constant { .. } => {
                let kind = register_types[index];
                body.push_str(&constant_register_source(&register, index, kind));
            }
            PointInstruction::Age => body.push_str(&format!(
                "    float {register} = 0.0;\n    if (!load_point_age(slot, {register})) valid = false;\n"
            )),
            PointInstruction::NormalizedAge => body.push_str(&format!(
                "    float {register} = 0.0;\n    if (!load_point_normalized_age(slot, {register})) valid = false;\n"
            )),
            PointInstruction::Position => {
                body.push_str(&format!("    vec3 {register} = point.position_size.xyz;\n"));
            }
            PointInstruction::Random { channel } => body.push_str(&format!(
                "    float {register} = random_01(uSeed, point.serial, {channel}u);\n"
            )),
            PointInstruction::LoadAttribute { attribute } => {
                let kind = attribute_type(program, *attribute)?;
                body.push_str(&load_attribute_source(&register, *attribute, kind));
            }
            PointInstruction::StoreAttribute { attribute, value } => {
                let kind = attribute_type(program, *attribute)?;
                body.push_str(&store_attribute_source(
                    &register,
                    *attribute,
                    *value,
                    kind,
                ));
            }
            PointInstruction::Binary {
                operation,
                left,
                right,
            } => body.push_str(&binary_register_source(
                &register,
                *operation,
                *left,
                *right,
                register_types[usize::from(*right)],
                register_types[index],
            )?),
            PointInstruction::Length { value } => body.push_str(&length_register_source(
                &register,
                *value,
                register_types[usize::from(*value)],
            )?),
            PointInstruction::ColorRamp { gradient, factor } => body.push_str(&format!(
                "    vec4 {register} = valid ? sample_point_ramp({gradient}u, r{factor}) : vec4(0.0);\n"
            )),
        }
    }
    body.push_str(&format!(
        "    vec4 result = r{};\n    if (any(isnan(result)) || any(isinf(result))) valid = false;\n    pointColors[slot] = valid ? result : vec4(0.0);\n",
        program.color_register
    ));
    let ramp_function = program
        .instructions
        .iter()
        .any(|instruction| matches!(instruction, PointInstruction::ColorRamp { .. }))
        .then(ramp_source)
        .unwrap_or_default();
    let point_source = source_kind
        .shader()
        .replace("// PARTICLE_STRUCT", PARTICLE_STRUCT_GLSL);
    Ok(format!(
        "#version 430 core\nlayout(local_size_x = 64) in;\n{RENDER_POINT_GLSL}\n{point_source}\nlayout(std430, binding = 1) buffer PointColumns {{ uint pointColumns[]; }};\nlayout(std430, binding = 2) buffer PointColors {{ vec4 pointColors[]; }};\nlayout(std430, binding = 3) readonly buffer PointProgramData {{ uvec4 pointData[]; }};\nuniform uint uCapacity;\nuniform uint uSeed;\nuniform uint uAttributeOffsets[{POINT_MAX_ATTRIBUTE_COUNT}];\n{PARTICLE_RANDOM_FUNCTIONS}\n{ramp_function}\nvoid main() {{\n    uint slot = gl_GlobalInvocationID.x;\n    if (slot >= uCapacity) return;\n    RenderPoint point = load_render_point(slot);\n    if (!point.alive) {{\n        pointColors[slot] = vec4(0.0);\n        return;\n    }}\n{body}}}\n"
    ))
}

fn attribute_type(
    program: &PointRenderProgram,
    attribute: u16,
) -> Result<PointAttributeElementType, LibraryError> {
    program
        .schema
        .attributes()
        .get(usize::from(attribute))
        .map(|attribute| attribute.element_type())
        .ok_or_else(|| {
            LibraryError::Validation("Point instruction references a missing attribute".to_string())
        })
}

fn glsl_type(kind: PointAttributeElementType) -> &'static str {
    match kind {
        PointAttributeElementType::Number => "float",
        PointAttributeElementType::Integer => "int",
        PointAttributeElementType::Vec2 => "vec2",
        PointAttributeElementType::Vec3 => "vec3",
        PointAttributeElementType::Vec4 | PointAttributeElementType::Color => "vec4",
    }
}

fn component_count(kind: PointAttributeElementType) -> usize {
    match kind {
        PointAttributeElementType::Number | PointAttributeElementType::Integer => 1,
        PointAttributeElementType::Vec2 => 2,
        PointAttributeElementType::Vec3 => 3,
        PointAttributeElementType::Vec4 | PointAttributeElementType::Color => 4,
    }
}

fn zero_literal(kind: PointAttributeElementType) -> Result<String, LibraryError> {
    Ok(match kind {
        PointAttributeElementType::Number => "0.0".to_string(),
        PointAttributeElementType::Vec2
        | PointAttributeElementType::Vec3
        | PointAttributeElementType::Vec4 => format!("{}(0.0)", glsl_type(kind)),
        PointAttributeElementType::Integer | PointAttributeElementType::Color => {
            return Err(LibraryError::Validation(
                "Point arithmetic lowering received a non-numeric type".to_string(),
            ));
        }
    })
}

fn any_zero(expression: &str, kind: PointAttributeElementType) -> Result<String, LibraryError> {
    Ok(match kind {
        PointAttributeElementType::Number => format!("{expression} == 0.0"),
        PointAttributeElementType::Vec2
        | PointAttributeElementType::Vec3
        | PointAttributeElementType::Vec4 => {
            format!("any(equal({expression}, {}(0.0)))", glsl_type(kind))
        }
        PointAttributeElementType::Integer | PointAttributeElementType::Color => {
            return Err(LibraryError::Validation(
                "Point divisor lowering received a non-numeric type".to_string(),
            ));
        }
    })
}

fn non_finite(expression: &str, kind: PointAttributeElementType) -> Result<String, LibraryError> {
    Ok(match kind {
        PointAttributeElementType::Number => {
            format!("isnan({expression}) || isinf({expression})")
        }
        PointAttributeElementType::Vec2
        | PointAttributeElementType::Vec3
        | PointAttributeElementType::Vec4 => {
            format!("any(isnan({expression})) || any(isinf({expression}))")
        }
        PointAttributeElementType::Integer | PointAttributeElementType::Color => {
            return Err(LibraryError::Validation(
                "Point arithmetic lowering received a non-numeric type".to_string(),
            ));
        }
    })
}

fn binary_register_source(
    register: &str,
    operation: NumericBinaryOperation,
    left: u16,
    right: u16,
    right_kind: PointAttributeElementType,
    result_kind: PointAttributeElementType,
) -> Result<String, LibraryError> {
    let result_type = glsl_type(result_kind);
    let zero = zero_literal(result_kind)?;
    let mut source = String::new();
    if matches!(
        operation,
        NumericBinaryOperation::Divide | NumericBinaryOperation::Fmod
    ) {
        source.push_str(&format!(
            "    if (valid && {}) valid = false;\n",
            any_zero(&format!("r{right}"), right_kind)?
        ));
    }
    let expression = match operation {
        NumericBinaryOperation::Add => format!("r{left} + r{right}"),
        NumericBinaryOperation::Subtract => format!("r{left} - r{right}"),
        NumericBinaryOperation::Multiply => format!("r{left} * r{right}"),
        NumericBinaryOperation::Divide => format!("r{left} / r{right}"),
        NumericBinaryOperation::Fmod => {
            source.push_str(&format!(
                "    {result_type} {register}_quotient = valid ? trunc(r{left} / r{right}) : {zero};\n    if ({}) valid = false;\n",
                non_finite(&format!("{register}_quotient"), result_kind)?
            ));
            format!("r{left} - r{right} * {register}_quotient")
        }
    };
    source.push_str(&format!(
        "    {result_type} {register}_value = valid ? {expression} : {zero};\n    if ({}) valid = false;\n    {result_type} {register} = valid ? {register}_value : {zero};\n",
        non_finite(&format!("{register}_value"), result_kind)?
    ));
    Ok(source)
}

fn length_register_source(
    register: &str,
    value: u16,
    kind: PointAttributeElementType,
) -> Result<String, LibraryError> {
    if kind == PointAttributeElementType::Number {
        return Ok(format!(
            "    float {register}_value = valid ? abs(r{value}) : 0.0;\n    if (isnan({register}_value) || isinf({register}_value)) valid = false;\n    float {register} = valid ? {register}_value : 0.0;\n"
        ));
    }
    let scale = match kind {
        PointAttributeElementType::Vec2 => format!("max(abs(r{value}.x), abs(r{value}.y))"),
        PointAttributeElementType::Vec3 => {
            format!("max(max(abs(r{value}.x), abs(r{value}.y)), abs(r{value}.z))")
        }
        PointAttributeElementType::Vec4 => format!(
            "max(max(abs(r{value}.x), abs(r{value}.y)), max(abs(r{value}.z), abs(r{value}.w)))"
        ),
        PointAttributeElementType::Number
        | PointAttributeElementType::Integer
        | PointAttributeElementType::Color => {
            return Err(LibraryError::Validation(
                "Point Length lowering received a non-numeric type".to_string(),
            ));
        }
    };
    Ok(format!(
        "    float {register}_scale = valid ? {scale} : 0.0;\n    float {register}_value = {register}_scale == 0.0 ? 0.0 : {register}_scale * length(r{value} / {register}_scale);\n    if (isnan({register}_value) || isinf({register}_value)) valid = false;\n    float {register} = valid ? {register}_value : 0.0;\n"
    ))
}

fn constant_register_source(
    register: &str,
    index: usize,
    kind: PointAttributeElementType,
) -> String {
    let expression = match kind {
        PointAttributeElementType::Number => format!("uintBitsToFloat(pointData[{index}].x)"),
        // GLSL 4.30 section 5.4.1 defines int(uint)/uint(int) to preserve the
        // source bit pattern. Float bitcasts are not safe transport because
        // section 8.3 permits NaN encodings to become unspecified.
        // https://registry.khronos.org/OpenGL/specs/gl/GLSLangSpec.4.30.pdf
        PointAttributeElementType::Integer => format!("int(pointData[{index}].x)"),
        PointAttributeElementType::Vec2 => format!("uintBitsToFloat(pointData[{index}].xy)"),
        PointAttributeElementType::Vec3 => format!("uintBitsToFloat(pointData[{index}].xyz)"),
        PointAttributeElementType::Vec4 | PointAttributeElementType::Color => {
            format!("uintBitsToFloat(pointData[{index}])")
        }
    };
    format!("    {} {register} = {expression};\n", glsl_type(kind))
}

fn load_attribute_source(
    register: &str,
    attribute: u16,
    kind: PointAttributeElementType,
) -> String {
    let stride_words = kind.gpu_layout().1 / 4;
    let base = format!("uAttributeOffsets[{attribute}] + slot * {stride_words}u");
    let raw = match kind {
        PointAttributeElementType::Number | PointAttributeElementType::Integer => {
            format!("pointColumns[{base}]")
        }
        PointAttributeElementType::Vec2 => {
            format!("uvec2(pointColumns[{base}], pointColumns[{base} + 1u])")
        }
        PointAttributeElementType::Vec3 => format!(
            "uvec3(pointColumns[{base}], pointColumns[{base} + 1u], pointColumns[{base} + 2u])"
        ),
        PointAttributeElementType::Vec4 | PointAttributeElementType::Color => format!(
            "uvec4(pointColumns[{base}], pointColumns[{base} + 1u], pointColumns[{base} + 2u], pointColumns[{base} + 3u])"
        ),
    };
    let expression = if kind == PointAttributeElementType::Integer {
        format!("int({raw})")
    } else {
        format!("uintBitsToFloat({raw})")
    };
    format!("    {} {register} = {expression};\n", glsl_type(kind))
}

fn store_attribute_source(
    register: &str,
    attribute: u16,
    value: u16,
    kind: PointAttributeElementType,
) -> String {
    let stride_words = kind.gpu_layout().1 / 4;
    let base = format!("uAttributeOffsets[{attribute}] + slot * {stride_words}u");
    let mut source = format!("    {} {register} = r{value};\n", glsl_type(kind));
    let components = ["x", "y", "z", "w"];
    for (index, component) in components.iter().take(component_count(kind)).enumerate() {
        let value = if kind == PointAttributeElementType::Integer {
            format!("uint(valid ? {register} : 0)")
        } else {
            let access = if component_count(kind) == 1 {
                register.to_string()
            } else {
                format!("{register}.{component}")
            };
            format!("floatBitsToUint(valid ? {access} : 0.0)")
        };
        let suffix = if index == 0 {
            String::new()
        } else {
            format!(" + {index}u")
        };
        source.push_str(&format!("    pointColumns[{base}{suffix}] = {value};\n"));
    }
    if kind == PointAttributeElementType::Vec3 {
        source.push_str(&format!("    pointColumns[{base} + 3u] = 0u;\n"));
    }
    source
}

fn ramp_source() -> String {
    format!(
        r#"
vec4 sample_point_ramp(uint rampIndex, float factor) {{
    uvec4 header = pointData[{POINT_MAX_INSTRUCTIONS}u + rampIndex];
    uint count = header.x;
    uint spread = header.y;
    float parameter = factor;
    if (spread == 0u) {{
        parameter = clamp(parameter, 0.0, 1.0);
    }} else if (spread == 1u) {{
        parameter = parameter - floor(parameter);
    }} else {{
        float reflected = parameter - floor(parameter / 2.0) * 2.0;
        parameter = reflected <= 1.0 ? reflected : 2.0 - reflected;
    }}
    uint base = {PROGRAM_HEADER_VEC4S}u + rampIndex * {ramp_stride}u;
    vec4 leftStop = uintBitsToFloat(pointData[base]);
    vec4 leftColor = vec4(leftStop.yzw, uintBitsToFloat(pointData[base + 1u].x));
    if (parameter < leftStop.x) return leftColor;
    for (uint index = 1u; index < {POINT_MAX_RAMP_STOPS}u; ++index) {{
        if (index >= count) break;
        vec4 rightStop = uintBitsToFloat(pointData[base + index * 2u]);
        vec4 rightColor = vec4(
            rightStop.yzw,
            uintBitsToFloat(pointData[base + index * 2u + 1u].x)
        );
        if (parameter < rightStop.x) {{
            float amount = (parameter - leftStop.x) / (rightStop.x - leftStop.x);
            return mix(leftColor, rightColor, amount);
        }}
        leftStop = rightStop;
        leftColor = rightColor;
    }}
    return leftColor;
}}
"#,
        ramp_stride = POINT_MAX_RAMP_STOPS * RAMP_STOP_VEC4S
    )
}

fn program_data(program: &PointRenderProgram) -> Result<Vec<u8>, LibraryError> {
    let mut values = vec![[0_u32; 4]; PROGRAM_DATA_VEC4S];
    for (index, instruction) in program.instructions.iter().enumerate() {
        if let PointInstruction::Constant { value } = instruction {
            values[index] = packed_value(value)?;
        }
    }
    for (ramp_index, ramp) in program.ramps.iter().enumerate() {
        values[POINT_MAX_INSTRUCTIONS + ramp_index] = [
            ramp.stops().len() as u32,
            match ramp.spread() {
                GradientSpread::Pad => 0,
                GradientSpread::Repeat => 1,
                GradientSpread::Reflect => 2,
            },
            0,
            0,
        ];
        let base = PROGRAM_HEADER_VEC4S + ramp_index * POINT_MAX_RAMP_STOPS * RAMP_STOP_VEC4S;
        for (stop_index, stop) in ramp.stops().iter().enumerate() {
            let color = packed_value(&PropertyValue::ColorValue(stop.color().clone()))?;
            values[base + stop_index * 2] = [
                (stop.offset() as f32).to_bits(),
                color[0],
                color[1],
                color[2],
            ];
            values[base + stop_index * 2 + 1] = [color[3], 0, 0, 0];
        }
    }
    let mut bytes = Vec::with_capacity(PROGRAM_DATA_BYTES);
    for value in values {
        for component in value {
            bytes.extend_from_slice(&component.to_ne_bytes());
        }
    }
    Ok(bytes)
}

fn packed_value(value: &PropertyValue) -> Result<[u32; 4], LibraryError> {
    let kind =
        PointAttributeElementType::from_property_value(value).map_err(LibraryError::Validation)?;
    match kind.pack_value(value).map_err(LibraryError::Validation)? {
        PointAttributeGpuDefault::Number(value) => Ok([value.to_bits(), 0, 0, 0]),
        PointAttributeGpuDefault::Integer(value) => Ok([value as u32, 0, 0, 0]),
        PointAttributeGpuDefault::Vec2(value) => Ok([value[0].to_bits(), value[1].to_bits(), 0, 0]),
        PointAttributeGpuDefault::Vec3(value) => Ok([
            value[0].to_bits(),
            value[1].to_bits(),
            value[2].to_bits(),
            0,
        ]),
        PointAttributeGpuDefault::Vec4(value) | PointAttributeGpuDefault::Color(value) => {
            Ok(value.map(f32::to_bits))
        }
    }
}
