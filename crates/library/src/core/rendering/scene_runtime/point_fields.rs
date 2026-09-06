//! Trusted GPU lowering for bounded render-stage Point field programs.

use glow::HasContext;
use sha2::{Digest, Sha256};

use crate::error::LibraryError;
use crate::model::point::{
    NumericBinaryOperation, POINT_MAX_ATTRIBUTE_COUNT, POINT_MAX_INSTRUCTIONS,
    POINT_MAX_RAMP_STOPS, POINT_MAX_RAMPS, PointAttributeGpuDefault, PointColumnLayout,
    PointInstruction, PointRenderProgram,
};
use crate::model::property::{GradientSpread, PropertyValue};
use crate::rendering::gl_resources::link_program;

use super::gl_backend::{PARTICLE_WORKGROUP_SIZE, required_uniform};
use super::shaders::{PARTICLE_RANDOM_FUNCTIONS, PARTICLE_STRUCT_GLSL};
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
    pub source_hash: [u8; 32],
}

impl PointFieldPipeline {
    pub fn create(gl: &glow::Context, program: &PointRenderProgram) -> Result<Self, LibraryError> {
        program.validate().map_err(LibraryError::Validation)?;
        let source = compute_source(program)?;
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
            ))
        })();
        let (capacity, seed, attribute_offsets) = match uniforms {
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
            source_hash,
        })
    }

    pub fn evaluate(
        &self,
        gl: &glow::Context,
        program: &PointRenderProgram,
        buffers: &PointFieldBuffers,
        particle_buffer: glow::Buffer,
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
        let source_hash: [u8; 32] = Sha256::digest(compute_source(program)?.as_bytes()).into();
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
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 0, Some(particle_buffer));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 1, Some(buffers.columns));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 2, Some(buffers.colors));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 3, Some(self.program_data));
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
    program: Option<&PointRenderProgram>,
) -> Result<Option<[u8; 32]>, LibraryError> {
    program
        .map(|program| {
            program.validate().map_err(LibraryError::Validation)?;
            Ok(Sha256::digest(compute_source(program)?.as_bytes()).into())
        })
        .transpose()
}

pub(super) fn required_invocation_bytes(
    program: Option<&PointRenderProgram>,
    capacity: u32,
) -> Result<u64, LibraryError> {
    let particle_bytes = u64::from(capacity)
        .checked_mul(super::gl_backend::PARTICLE_STRIDE_BYTES)
        .ok_or_else(|| LibraryError::Render("GPU Particle state size overflow".to_string()))?;
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

fn compute_source(program: &PointRenderProgram) -> Result<String, LibraryError> {
    program.validate().map_err(LibraryError::Validation)?;
    let mut body =
        String::from("    bool valid = true;\n    pointColumns[slot] = particle.identity.x;\n");
    for (index, instruction) in program.instructions.iter().enumerate() {
        let register = format!("r{index}");
        match instruction {
            PointInstruction::Constant { .. } => {
                body.push_str(&format!("    vec4 {register} = pointData[{index}];\n"));
            }
            PointInstruction::Age => body.push_str(&format!(
                "    vec4 {register} = vec4(particle.position_age.w, 0.0, 0.0, 0.0);\n"
            )),
            PointInstruction::NormalizedAge => body.push_str(&format!(
                "    vec4 {register} = vec4(clamp(particle.position_age.w / particle.velocity_lifetime.w, 0.0, 1.0), 0.0, 0.0, 0.0);\n"
            )),
            PointInstruction::Random { channel } => body.push_str(&format!(
                "    vec4 {register} = vec4(random_01(uSeed, particle.identity.x, {channel}u), 0.0, 0.0, 0.0);\n"
            )),
            PointInstruction::LoadAttribute { attribute } => body.push_str(&format!(
                "    vec4 {register} = vec4(uintBitsToFloat(pointColumns[uAttributeOffsets[{attribute}] + slot]), 0.0, 0.0, 0.0);\n"
            )),
            PointInstruction::StoreNumber { attribute, value } => body.push_str(&format!(
                "    vec4 {register} = r{value};\n    pointColumns[uAttributeOffsets[{attribute}] + slot] = floatBitsToUint(valid ? {register}.x : 0.0);\n"
            )),
            PointInstruction::Binary {
                operation,
                left,
                right,
            } => {
                if *operation == NumericBinaryOperation::Fmod {
                    body.push_str(&format!(
                        "    if (valid && r{right}.x == 0.0) valid = false;\n    float {register}_quotient = valid ? trunc(r{left}.x / r{right}.x) : 0.0;\n    if (isnan({register}_quotient) || isinf({register}_quotient)) valid = false;\n    float {register}_value = valid ? r{left}.x - r{right}.x * {register}_quotient : 0.0;\n    if (isnan({register}_value) || isinf({register}_value)) valid = false;\n    vec4 {register} = vec4(valid ? {register}_value : 0.0, 0.0, 0.0, 0.0);\n"
                    ));
                    continue;
                }
                let expression = match operation {
                    NumericBinaryOperation::Add => format!("r{left}.x + r{right}.x"),
                    NumericBinaryOperation::Subtract => format!("r{left}.x - r{right}.x"),
                    NumericBinaryOperation::Multiply => format!("r{left}.x * r{right}.x"),
                    NumericBinaryOperation::Divide => format!("r{left}.x / r{right}.x"),
                    NumericBinaryOperation::Fmod => {
                        return Err(LibraryError::Validation(
                            "Point Fmod lowering entered an inconsistent branch".to_string(),
                        ));
                    }
                };
                if matches!(
                    operation,
                    NumericBinaryOperation::Divide | NumericBinaryOperation::Fmod
                ) {
                    body.push_str(&format!(
                        "    if (valid && r{right}.x == 0.0) valid = false;\n"
                    ));
                }
                body.push_str(&format!(
                    "    float {register}_value = valid ? {expression} : 0.0;\n    if (isnan({register}_value) || isinf({register}_value)) valid = false;\n    vec4 {register} = vec4(valid ? {register}_value : 0.0, 0.0, 0.0, 0.0);\n"
                ));
            }
            PointInstruction::ColorRamp { gradient, factor } => body.push_str(&format!(
                "    vec4 {register} = valid ? sample_point_ramp({gradient}u, r{factor}.x) : vec4(0.0);\n"
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
    Ok(format!(
        "#version 430 core\nlayout(local_size_x = 64) in;\n{PARTICLE_STRUCT_GLSL}\nlayout(std430, binding = 0) readonly buffer ParticleBuffer {{ Particle particles[]; }};\nlayout(std430, binding = 1) buffer PointColumns {{ uint pointColumns[]; }};\nlayout(std430, binding = 2) buffer PointColors {{ vec4 pointColors[]; }};\nlayout(std430, binding = 3) readonly buffer PointProgramData {{ vec4 pointData[]; }};\nuniform uint uCapacity;\nuniform uint uSeed;\nuniform uint uAttributeOffsets[{POINT_MAX_ATTRIBUTE_COUNT}];\n{PARTICLE_RANDOM_FUNCTIONS}\n{ramp_function}\nvoid main() {{\n    uint slot = gl_GlobalInvocationID.x;\n    if (slot >= uCapacity) return;\n    Particle particle = particles[slot];\n    if (particle.position_age.w < 0.0 || particle.position_age.w >= particle.velocity_lifetime.w) {{\n        pointColors[slot] = vec4(0.0);\n        return;\n    }}\n{body}}}\n"
    ))
}

fn ramp_source() -> String {
    format!(
        r#"
vec4 sample_point_ramp(uint rampIndex, float factor) {{
    vec4 header = pointData[{POINT_MAX_INSTRUCTIONS}u + rampIndex];
    uint count = uint(header.x);
    uint spread = uint(header.y);
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
    vec4 leftStop = pointData[base];
    vec4 leftColor = vec4(leftStop.yzw, pointData[base + 1u].x);
    if (parameter < leftStop.x) return leftColor;
    for (uint index = 1u; index < {POINT_MAX_RAMP_STOPS}u; ++index) {{
        if (index >= count) break;
        vec4 rightStop = pointData[base + index * 2u];
        vec4 rightColor = vec4(rightStop.yzw, pointData[base + index * 2u + 1u].x);
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
    let mut values = vec![[0.0_f32; 4]; PROGRAM_DATA_VEC4S];
    for (index, instruction) in program.instructions.iter().enumerate() {
        if let PointInstruction::Constant { value } = instruction {
            values[index] = match value {
                PropertyValue::Number(_) => packed_value(value)?,
                PropertyValue::ColorValue(_) => packed_value(value)?,
                _ => {
                    return Err(LibraryError::Validation(
                        "Point program contains an unsupported constant".to_string(),
                    ));
                }
            };
        }
    }
    for (ramp_index, ramp) in program.ramps.iter().enumerate() {
        values[POINT_MAX_INSTRUCTIONS + ramp_index] = [
            ramp.stops().len() as f32,
            match ramp.spread() {
                GradientSpread::Pad => 0.0,
                GradientSpread::Repeat => 1.0,
                GradientSpread::Reflect => 2.0,
            },
            0.0,
            0.0,
        ];
        let base = PROGRAM_HEADER_VEC4S + ramp_index * POINT_MAX_RAMP_STOPS * RAMP_STOP_VEC4S;
        for (stop_index, stop) in ramp.stops().iter().enumerate() {
            let color = packed_value(&PropertyValue::ColorValue(stop.color().clone()))?;
            values[base + stop_index * 2] = [stop.offset() as f32, color[0], color[1], color[2]];
            values[base + stop_index * 2 + 1] = [color[3], 0.0, 0.0, 0.0];
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

fn packed_value(value: &PropertyValue) -> Result<[f32; 4], LibraryError> {
    let kind = match value {
        PropertyValue::Number(_) => crate::model::point::PointAttributeElementType::Number,
        PropertyValue::ColorValue(_) => crate::model::point::PointAttributeElementType::Color,
        _ => {
            return Err(LibraryError::Validation(
                "Point program value is neither Number nor Color".to_string(),
            ));
        }
    };
    match kind.pack_value(value).map_err(LibraryError::Validation)? {
        PointAttributeGpuDefault::Number(value) => Ok([value, 0.0, 0.0, 0.0]),
        PointAttributeGpuDefault::Color(value) => Ok(value),
        _ => Err(LibraryError::Validation(
            "Point program packed an unexpected GPU value".to_string(),
        )),
    }
}
