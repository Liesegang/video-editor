//! GPU line rasterization for compact mutual Point connections.

use glow::HasContext;

use super::gl_backend::{SceneTextureFormat, required_uniform};
use super::gl_operation_result;
use super::prefix_scan::PrefixScanBuffers;
use super::render::{PointDrawRequest, begin_point_target};
use super::shaders::{LINEAR_TO_SRGB, POINT_PROJECTION_GLSL};
use super::source::{PointSourceKind, PointSourceRequirements, PointSourceUniforms};
use crate::error::LibraryError;
use crate::model::frame::point::PointConnectionParameters;
use crate::rendering::gl_resources::link_program;

#[derive(Clone)]
pub(super) struct PointLinePipeline {
    program: glow::Program,
    vertex_array: glow::VertexArray,
    source: PointSourceUniforms,
    logical_size: glow::UniformLocation,
    target_size: glow::UniformLocation,
    affine_x: glow::UniformLocation,
    affine_y: glow::UniformLocation,
    focal_length: glow::UniformLocation,
    width: glow::UniformLocation,
    max_distance: glow::UniformLocation,
    fade: glow::UniformLocation,
    color: Option<glow::UniformLocation>,
    output_srgba: glow::UniformLocation,
}

impl PointLinePipeline {
    pub fn create(
        gl: &glow::Context,
        source_kind: PointSourceKind,
        geometry: bool,
        point_colors: bool,
    ) -> Result<Self, LibraryError> {
        let vertex = LINE_VERTEX
            .replace("// POINT_ACCESS", &source_kind.render_shader(geometry))
            .replace("// POINT_PROJECTION", POINT_PROJECTION_GLSL)
            .replace(
                "// POINT_COLORS",
                if point_colors {
                    r#"layout(std430, binding = 2) readonly buffer PointColors {
    vec4 pointColors[];
};

vec4 endpoint_color(uint slot) {
    vec4 color = pointColors[slot];
    return vec4(color.rgb * color.a, color.a);
}"#
                } else {
                    r#"uniform vec4 uPremultipliedColor;

vec4 endpoint_color(uint slot) {
    return uPremultipliedColor;
}"#
                },
            );
        let fragment = LINE_FRAGMENT.replace("// LINEAR_TO_SRGB", LINEAR_TO_SRGB);
        let program = link_program(
            gl,
            &[
                (glow::VERTEX_SHADER, &vertex),
                (glow::FRAGMENT_SHADER, &fragment),
            ],
            "Point Line renderer",
        )?;
        // SAFETY: the current context exclusively owns the new program.
        let vertex_array = match unsafe { gl.create_vertex_array() } {
            Ok(array) => array,
            Err(error) => {
                // SAFETY: program has not escaped this constructor.
                unsafe { gl.delete_program(program) };
                return Err(LibraryError::Render(format!(
                    "Cannot create Point Line vertex array: {error}"
                )));
            }
        };
        let uniforms = (|| {
            Ok(Self {
                program,
                vertex_array,
                source: PointSourceUniforms::new(
                    gl,
                    program,
                    source_kind,
                    if geometry {
                        PointSourceRequirements::Optional
                    } else {
                        PointSourceRequirements::Position
                    },
                )?,
                logical_size: required_uniform(gl, program, "uLogicalSize")?,
                target_size: required_uniform(gl, program, "uTargetSize")?,
                affine_x: required_uniform(gl, program, "uAffineX")?,
                affine_y: required_uniform(gl, program, "uAffineY")?,
                focal_length: required_uniform(gl, program, "uFocalLength")?,
                width: required_uniform(gl, program, "uLineWidth")?,
                max_distance: required_uniform(gl, program, "uMaxDistance")?,
                fade: required_uniform(gl, program, "uDistanceFade")?,
                color: (!point_colors)
                    .then(|| required_uniform(gl, program, "uPremultipliedColor"))
                    .transpose()?,
                output_srgba: required_uniform(gl, program, "uOutputSrgba")?,
            })
        })();
        if uniforms.is_err() {
            // SAFETY: neither object escaped after uniform lookup failed.
            unsafe {
                gl.delete_vertex_array(vertex_array);
                gl.delete_program(program);
            }
        }
        uniforms
    }

    #[cfg_attr(
        test,
        allow(
            clippy::too_many_arguments,
            reason = "Test-only timing adds one argument to the unchanged production draw boundary"
        )
    )]
    pub fn draw(
        &self,
        gl: &glow::Context,
        request: PointDrawRequest<'_>,
        connections: &PointConnectionParameters,
        width: f32,
        fade: f32,
        scan: &PrefixScanBuffers,
        #[cfg(test)] mut profiler: Option<&mut super::profiling::PointGpuProfiler>,
    ) -> Result<(), LibraryError> {
        let fields = request.point_fields;
        #[cfg(test)]
        if let Some(profiler) = profiler.as_deref_mut() {
            profiler.begin_stage(gl, super::profiling::PointProfileStage::Draw)?;
        }
        // SAFETY: every resource belongs to this current context. The compact
        // edge count was generated from buffers sized for this invocation.
        unsafe {
            begin_point_target(gl, request.target);
            let determinant = request.transform.scale_x * request.transform.scale_y
                - request.transform.skew_x * request.transform.skew_y;
            if determinant.abs() <= f64::EPSILON {
                gl.memory_barrier(glow::FRAMEBUFFER_BARRIER_BIT | glow::TEXTURE_FETCH_BARRIER_BIT);
                #[cfg(test)]
                if let Some(profiler) = profiler.as_deref_mut() {
                    profiler.end_stage(gl, super::profiling::PointProfileStage::Draw)?;
                }
                return gl_operation_result(gl, "singular Point Line clear");
            }
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vertex_array));
            request.point_source.bind(gl, &self.source)?;
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 1, Some(scan.compact_edges));
            if let Some(fields) = fields {
                gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 2, Some(fields.colors));
                if let Some(geometry) = fields.geometry {
                    gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 4, Some(geometry));
                }
            }
            gl.uniform_2_f32(
                Some(&self.logical_size),
                request.logical_size.0 as f32,
                request.logical_size.1 as f32,
            );
            gl.uniform_2_f32(
                Some(&self.target_size),
                request.target.width as f32,
                request.target.height as f32,
            );
            gl.uniform_3_f32(
                Some(&self.affine_x),
                request.transform.scale_x as f32,
                request.transform.skew_x as f32,
                request.transform.translate_x as f32,
            );
            gl.uniform_3_f32(
                Some(&self.affine_y),
                request.transform.skew_y as f32,
                request.transform.scale_y as f32,
                request.transform.translate_y as f32,
            );
            gl.uniform_1_f32(
                Some(&self.focal_length),
                request.logical_size.1.max(1) as f32,
            );
            gl.uniform_1_f32(Some(&self.width), width);
            gl.uniform_1_f32(Some(&self.max_distance), connections.max_distance.0);
            gl.uniform_1_f32(Some(&self.fade), fade);
            if let Some(location) = &self.color {
                gl.uniform_4_f32(
                    Some(location),
                    request.premultiplied_color[0],
                    request.premultiplied_color[1],
                    request.premultiplied_color[2],
                    request.premultiplied_color[3],
                );
            }
            gl.uniform_1_i32(
                Some(&self.output_srgba),
                i32::from(request.target.format == SceneTextureFormat::Srgba8),
            );
            gl.bind_buffer(glow::DRAW_INDIRECT_BUFFER, Some(scan.indirect));
            gl.draw_arrays_indirect_offset(glow::TRIANGLES, 0);
            gl.memory_barrier(glow::FRAMEBUFFER_BARRIER_BIT | glow::TEXTURE_FETCH_BARRIER_BIT);
        }
        #[cfg(test)]
        if let Some(profiler) = profiler {
            profiler.end_stage(gl, super::profiling::PointProfileStage::Draw)?;
        }
        gl_operation_result(gl, "Line render")
    }

    pub fn destroy(self, gl: &glow::Context) {
        // SAFETY: cache entry uniquely owns these objects.
        unsafe {
            gl.delete_vertex_array(self.vertex_array);
            gl.delete_program(self.program);
        }
    }
}

const LINE_VERTEX: &str = r#"#version 430 core
// POINT_ACCESS
// POINT_COLORS
layout(std430, binding = 1) readonly buffer CompactEdges {
    uvec2 compactEdges[];
};
uniform vec2 uLogicalSize;
uniform vec2 uTargetSize;
uniform vec3 uAffineX;
uniform vec3 uAffineY;
uniform float uFocalLength;
uniform float uLineWidth;
uniform float uMaxDistance;
uniform float uDistanceFade;
// POINT_PROJECTION
out vec4 vPremultipliedLinearColor;
const uvec2 CORNERS[6] = uvec2[6](
    uvec2(0, 0),
    uvec2(0, 1),
    uvec2(1, 1),
    uvec2(0, 0),
    uvec2(1, 1),
    uvec2(1, 0)
);

void main() {
    uvec2 edge = compactEdges[uint(gl_VertexID) / 6u];
    uvec2 corner = CORNERS[uint(gl_VertexID) % 6u];
    RenderPoint a = load_final_render_point(edge.x);
    RenderPoint b = load_final_render_point(edge.y);
    float perspectiveA = uFocalLength /
        max(1.0, uFocalLength + a.position_size.z);
    float perspectiveB = uFocalLength /
        max(1.0, uFocalLength + b.position_size.z);
    vec2 localA = uLogicalSize * 0.5 + a.position_size.xy * perspectiveA;
    vec2 localB = uLogicalSize * 0.5 + b.position_size.xy * perspectiveB;
    vec2 direction = localB - localA;
    float length2 = dot(direction, direction);
    if (length2 <= 1e-12) {
        gl_Position = vec4(2, 2, 1, 1);
        vPremultipliedLinearColor = vec4(0);
        return;
    }
    vec2 normal = vec2(-direction.y, direction.x) *
        inversesqrt(length2) * uLineWidth * 0.5;
    vec2 local = mix(localA, localB, float(corner.x)) +
        (corner.y == 0u ? -normal : normal);
    float z = mix(a.position_size.z, b.position_size.z, float(corner.x));
    gl_Position = point_clip_position(local, z);
    float distance3 = length(b.position_size.xyz - a.position_size.xyz);
    float attenuation = mix(
        1.0,
        clamp(1.0 - distance3 / uMaxDistance, 0.0, 1.0),
        uDistanceFade
    );
    vPremultipliedLinearColor = mix(
        endpoint_color(edge.x),
        endpoint_color(edge.y),
        float(corner.x)
    ) * attenuation;
}
"#;

const LINE_FRAGMENT: &str = r#"#version 430 core
in vec4 vPremultipliedLinearColor;
uniform bool uOutputSrgba;
layout(location = 0) out vec4 output_color;
// LINEAR_TO_SRGB
void main() {
    if (vPremultipliedLinearColor.a <= 0.0) {
        discard;
    }
    vec3 straight = vPremultipliedLinearColor.rgb /
        vPremultipliedLinearColor.a;
    vec3 rgb = uOutputSrgba
        ? vec3(
            linear_to_srgb(straight.r),
            linear_to_srgb(straight.g),
            linear_to_srgb(straight.b)
        )
        : straight;
    output_color = vec4(
        rgb * vPremultipliedLinearColor.a,
        vPremultipliedLinearColor.a
    );
}
"#;
