//! Exact bounded GPU proximity topology for shared Point streams.

use glow::HasContext;

use super::gl_backend::{PARTICLE_WORKGROUP_SIZE, required_uniform};
use super::gl_operation_result;
use super::point_buffer_alloc::allocate_buffer;
use super::point_fields::PointFieldBuffers;
use super::prefix_scan::{PrefixScanBuffers, PrefixScanPipeline};
use super::source::{
    PointSourceBinding, PointSourceKind, PointSourceRequirements, PointSourceUniforms,
};
use crate::error::LibraryError;
use crate::model::frame::point::PointConnectionParameters;
use crate::rendering::gl_resources::link_program;

pub(super) const MAX_CANDIDATES_PER_POINT: u32 = 4_096;
pub(super) const MAX_CANDIDATE_TESTS: u32 = 32_000_000;

const STATUS_BYTES: u64 = 4 * size_of::<u32>() as u64;
pub(super) struct PointConnectionBuffers {
    pub buckets: glow::Buffer,
    pub records: glow::Buffer,
    pub neighbors: glow::Buffer,
    pub candidates: glow::Buffer,
    pub edge_counts: glow::Buffer,
    pub status: glow::Buffer,
    pub scan: PrefixScanBuffers,
    pub capacity: u32,
    pub max_neighbors: u32,
    pub bucket_count: u32,
    byte_len: u64,
    #[cfg(test)]
    pub candidate_tests: std::cell::Cell<u32>,
    #[cfg(test)]
    pub max_point_candidates: std::cell::Cell<u32>,
    #[cfg(test)]
    pub compact_edge_count: std::cell::Cell<u32>,
}

impl PointConnectionBuffers {
    pub fn required_bytes(capacity: u32, max_neighbors: u32) -> Result<u64, LibraryError> {
        let bucket_count = capacity
            .checked_mul(2)
            .and_then(|value| value.checked_next_power_of_two())
            .ok_or_else(|| LibraryError::Render("Point spatial hash size overflow".into()))?;
        let edge_slots = capacity
            .checked_mul(max_neighbors)
            .ok_or_else(|| LibraryError::Render("Point neighbor slot count overflow".into()))?;
        [
            u64::from(bucket_count) * 8,
            u64::from(capacity) * 16,
            u64::from(edge_slots) * 4,
            u64::from(edge_slots) * 4,
            u64::from(capacity) * 4,
            STATUS_BYTES,
        ]
        .into_iter()
        .try_fold(
            PrefixScanBuffers::required_bytes(capacity, max_neighbors)?,
            |total, bytes| total.checked_add(bytes),
        )
        .ok_or_else(|| LibraryError::Render("Point connection state size overflow".into()))
    }

    pub fn create(
        gl: &glow::Context,
        capacity: u32,
        max_neighbors: u32,
    ) -> Result<Self, LibraryError> {
        let bucket_count = capacity
            .checked_mul(2)
            .and_then(|value| value.checked_next_power_of_two())
            .ok_or_else(|| LibraryError::Render("Point spatial hash size overflow".into()))?;
        let edge_slots = capacity
            .checked_mul(max_neighbors)
            .ok_or_else(|| LibraryError::Render("Point neighbor slot count overflow".into()))?;
        let byte_len = Self::required_bytes(capacity, max_neighbors)?;
        let specs = [
            (u64::from(bucket_count) * 8, "Point spatial buckets"),
            (u64::from(capacity) * 16, "Point spatial records"),
            (u64::from(edge_slots) * 4, "Point nearest neighbors"),
            (u64::from(edge_slots) * 4, "Point mutual edge candidates"),
            (u64::from(capacity) * 4, "Point mutual edge counts"),
            (STATUS_BYTES, "Point proximity status"),
        ];
        let mut owned = Vec::new();
        let result = (|| {
            for (bytes, label) in specs {
                owned.push(allocate_buffer(gl, bytes, glow::DYNAMIC_COPY, label)?);
            }
            let scan = PrefixScanBuffers::create(gl, capacity, max_neighbors)?;
            Ok(Self {
                buckets: owned[0],
                records: owned[1],
                neighbors: owned[2],
                candidates: owned[3],
                edge_counts: owned[4],
                status: owned[5],
                scan,
                capacity,
                max_neighbors,
                bucket_count,
                byte_len,
                #[cfg(test)]
                candidate_tests: std::cell::Cell::new(0),
                #[cfg(test)]
                max_point_candidates: std::cell::Cell::new(0),
                #[cfg(test)]
                compact_edge_count: std::cell::Cell::new(0),
            })
        })();
        if result.is_err() {
            // SAFETY: construction failed before any buffer escaped.
            unsafe {
                for buffer in owned {
                    gl.delete_buffer(buffer);
                }
            }
        }
        result
    }

    pub fn matches(&self, capacity: u32, max_neighbors: u32) -> bool {
        self.capacity == capacity && self.max_neighbors == max_neighbors
    }

    pub fn byte_len(&self) -> u64 {
        self.byte_len
    }

    pub fn destroy(self, gl: &glow::Context) {
        self.scan.destroy(gl);
        // SAFETY: every buffer is uniquely owned by this invocation.
        unsafe {
            gl.delete_buffer(self.buckets);
            gl.delete_buffer(self.records);
            gl.delete_buffer(self.neighbors);
            gl.delete_buffer(self.candidates);
            gl.delete_buffer(self.edge_counts);
            gl.delete_buffer(self.status);
        }
    }
}

#[derive(Clone)]
pub(super) struct PointConnectionPipeline {
    clear: glow::Program,
    clear_bucket_count: glow::UniformLocation,
    clear_capacity: glow::UniformLocation,
    build: glow::Program,
    build_capacity: glow::UniformLocation,
    build_bucket_mask: glow::UniformLocation,
    build_distance: glow::UniformLocation,
    build_source: PointSourceUniforms,
    budget: glow::Program,
    budget_capacity: glow::UniformLocation,
    budget_bucket_mask: glow::UniformLocation,
    search: glow::Program,
    search_capacity: glow::UniformLocation,
    search_bucket_mask: glow::UniformLocation,
    search_min_distance: glow::UniformLocation,
    search_max_distance: glow::UniformLocation,
    search_neighbors: glow::UniformLocation,
    search_source: PointSourceUniforms,
    mutual: glow::Program,
    mutual_capacity: glow::UniformLocation,
    mutual_neighbors: glow::UniformLocation,
    mutual_source: PointSourceUniforms,
    pub scan: PrefixScanPipeline,
    pub source_kind: PointSourceKind,
    geometry: bool,
    validity: bool,
}

impl PointConnectionPipeline {
    pub fn create(
        gl: &glow::Context,
        source_kind: PointSourceKind,
        geometry: bool,
        validity: bool,
    ) -> Result<Self, LibraryError> {
        let access = source_kind.render_shader(geometry);
        let validity_source = validity_declaration(validity);
        let build_source = BUILD_SHADER
            .replace("// POINT_ACCESS", &access)
            .replace("// VALIDITY", validity_source)
            .replace("HASH_SOURCE", HASH_GLSL);
        let budget_source = BUDGET_SHADER.replace("HASH_SOURCE", HASH_GLSL);
        let search_source = SEARCH_SHADER
            .replace("// POINT_ACCESS", &access)
            .replace("HASH_SOURCE", HASH_GLSL);
        let mutual_source = MUTUAL_SHADER.replace("// POINT_ACCESS", &access);
        let mut programs = Vec::new();
        let result = (|| {
            for (source, label) in [
                (CLEAR_SHADER, "Point spatial clear"),
                (&build_source, "Point spatial build"),
                (&budget_source, "Point spatial budget"),
                (&search_source, "Point nearest neighbor search"),
                (&mutual_source, "Point mutual neighbor filter"),
            ] {
                programs.push(link_program(gl, &[(glow::COMPUTE_SHADER, source)], label)?);
            }
            let requirements = if geometry {
                PointSourceRequirements::Alive
            } else {
                PointSourceRequirements::Position
            };
            let clear_bucket_count = required_uniform(gl, programs[0], "uBucketCount")?;
            let clear_capacity = required_uniform(gl, programs[0], "uCapacity")?;
            let build_capacity = required_uniform(gl, programs[1], "uCapacity")?;
            let build_bucket_mask = required_uniform(gl, programs[1], "uBucketMask")?;
            let build_distance = required_uniform(gl, programs[1], "uMaxDistance")?;
            let build_source =
                PointSourceUniforms::new(gl, programs[1], source_kind, requirements)?;
            let budget_capacity = required_uniform(gl, programs[2], "uCapacity")?;
            let budget_bucket_mask = required_uniform(gl, programs[2], "uBucketMask")?;
            let search_capacity = required_uniform(gl, programs[3], "uCapacity")?;
            let search_bucket_mask = required_uniform(gl, programs[3], "uBucketMask")?;
            let search_min_distance = required_uniform(gl, programs[3], "uMinDistance")?;
            let search_max_distance = required_uniform(gl, programs[3], "uMaxDistance")?;
            let search_neighbors = required_uniform(gl, programs[3], "uMaxNeighbors")?;
            let search_source =
                PointSourceUniforms::new(gl, programs[3], source_kind, requirements)?;
            let mutual_capacity = required_uniform(gl, programs[4], "uCapacity")?;
            let mutual_neighbors = required_uniform(gl, programs[4], "uMaxNeighbors")?;
            let mutual_source = PointSourceUniforms::new(
                gl,
                programs[4],
                source_kind,
                PointSourceRequirements::Alive,
            )?;
            // Create the owned scan programs last: no fallible construction
            // follows, so an earlier uniform error cannot leak them.
            let scan = PrefixScanPipeline::create(gl)?;
            Ok(Self {
                clear: programs[0],
                clear_bucket_count,
                clear_capacity,
                build: programs[1],
                build_capacity,
                build_bucket_mask,
                build_distance,
                build_source,
                budget: programs[2],
                budget_capacity,
                budget_bucket_mask,
                search: programs[3],
                search_capacity,
                search_bucket_mask,
                search_min_distance,
                search_max_distance,
                search_neighbors,
                search_source,
                mutual: programs[4],
                mutual_capacity,
                mutual_neighbors,
                mutual_source,
                scan,
                source_kind,
                geometry,
                validity,
            })
        })();
        if result.is_err() {
            // SAFETY: no program escaped the failed constructor.
            unsafe {
                for program in programs {
                    gl.delete_program(program);
                }
            }
        }
        result
    }

    pub fn evaluate(
        &self,
        gl: &glow::Context,
        parameters: &PointConnectionParameters,
        buffers: &PointConnectionBuffers,
        fields: Option<&PointFieldBuffers>,
        source: &PointSourceBinding<'_>,
    ) -> Result<(), LibraryError> {
        if source.kind() != self.source_kind
            || fields.and_then(|fields| fields.geometry).is_some() != self.geometry
            || fields.and_then(|fields| fields.validity).is_some() != self.validity
            || !buffers.matches(buffers.capacity, parameters.max_neighbors)
        {
            return Err(LibraryError::Validation(
                "Point connection GPU layout changed without rebuilding".into(),
            ));
        }
        if parameters.max_distance.0 == 0.0 {
            let result = clear_indirect(gl, buffers.scan.indirect);
            #[cfg(test)]
            if result.is_ok() {
                buffers.candidate_tests.set(0);
                buffers.max_point_candidates.set(0);
                buffers.compact_edge_count.set(0);
            }
            return result;
        }
        reset_status(gl, buffers.status)?;
        let groups = buffers.capacity.div_ceil(PARTICLE_WORKGROUP_SIZE);
        // SAFETY: all resources belong to the current context; model and
        // allocation validation bound every dispatch and indexed access.
        unsafe {
            gl.use_program(Some(self.clear));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 0, Some(buffers.buckets));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 1, Some(buffers.records));
            gl.uniform_1_u32(Some(&self.clear_bucket_count), buffers.bucket_count);
            gl.uniform_1_u32(Some(&self.clear_capacity), buffers.capacity);
            gl.dispatch_compute(
                buffers
                    .bucket_count
                    .max(buffers.capacity)
                    .div_ceil(PARTICLE_WORKGROUP_SIZE),
                1,
                1,
            );
            gl.memory_barrier(glow::SHADER_STORAGE_BARRIER_BIT);

            gl.use_program(Some(self.build));
            source.bind(gl, &self.build_source)?;
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 1, Some(buffers.buckets));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 2, Some(buffers.records));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 3, Some(buffers.status));
            bind_derived(gl, fields);
            gl.uniform_1_u32(Some(&self.build_capacity), buffers.capacity);
            gl.uniform_1_u32(Some(&self.build_bucket_mask), buffers.bucket_count - 1);
            gl.uniform_1_f32(Some(&self.build_distance), parameters.max_distance.0);
            gl.dispatch_compute(groups, 1, 1);
            gl.memory_barrier(glow::SHADER_STORAGE_BARRIER_BIT);

            gl.use_program(Some(self.budget));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 0, Some(buffers.buckets));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 1, Some(buffers.records));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 2, Some(buffers.status));
            gl.uniform_1_u32(Some(&self.budget_capacity), buffers.capacity);
            gl.uniform_1_u32(Some(&self.budget_bucket_mask), buffers.bucket_count - 1);
            gl.dispatch_compute(groups, 1, 1);
            gl.memory_barrier(glow::SHADER_STORAGE_BARRIER_BIT | glow::BUFFER_UPDATE_BARRIER_BIT);
        }
        let status = read_status(gl, buffers.status)?;
        #[cfg(test)]
        {
            buffers.candidate_tests.set(status[0]);
            buffers.max_point_candidates.set(status[1]);
        }
        if status[2] & 2 != 0 {
            return Err(LibraryError::Render(
                "Point proximity cell coordinates exceed the GPU integer range; increase Maximum Distance or reduce Point coordinates".into(),
            ));
        }
        if status[2] != 0 || status[0] > MAX_CANDIDATE_TESTS || status[1] > MAX_CANDIDATES_PER_POINT
        {
            return Err(LibraryError::Render(format!(
                "Point proximity requires {} candidate tests (maximum {}, per-point {} / maximum {}); reduce Maximum Distance or Point density",
                status[0], MAX_CANDIDATE_TESTS, status[1], MAX_CANDIDATES_PER_POINT
            )));
        }
        // SAFETY: the programs and buffers belong to this current context;
        // the fixed work budget was checked before these bounded dispatches.
        unsafe {
            gl.use_program(Some(self.search));
            source.bind(gl, &self.search_source)?;
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 1, Some(buffers.buckets));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 2, Some(buffers.records));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 3, Some(buffers.neighbors));
            bind_derived(gl, fields);
            gl.uniform_1_u32(Some(&self.search_capacity), buffers.capacity);
            gl.uniform_1_u32(Some(&self.search_bucket_mask), buffers.bucket_count - 1);
            gl.uniform_1_f32(Some(&self.search_min_distance), parameters.min_distance.0);
            gl.uniform_1_f32(Some(&self.search_max_distance), parameters.max_distance.0);
            gl.uniform_1_u32(Some(&self.search_neighbors), parameters.max_neighbors);
            gl.dispatch_compute(groups, 1, 1);
            gl.memory_barrier(glow::SHADER_STORAGE_BARRIER_BIT);

            gl.use_program(Some(self.mutual));
            source.bind(gl, &self.mutual_source)?;
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 1, Some(buffers.neighbors));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 2, Some(buffers.candidates));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 3, Some(buffers.edge_counts));
            gl.uniform_1_u32(Some(&self.mutual_capacity), buffers.capacity);
            gl.uniform_1_u32(Some(&self.mutual_neighbors), parameters.max_neighbors);
            gl.dispatch_compute(groups, 1, 1);
            gl.memory_barrier(glow::SHADER_STORAGE_BARRIER_BIT);
        }
        self.scan.compact(
            gl,
            buffers.edge_counts,
            buffers.candidates,
            parameters.max_neighbors,
            &buffers.scan,
        )?;
        #[cfg(test)]
        buffers
            .compact_edge_count
            .set(read_indirect_count(gl, buffers.scan.indirect)?);
        Ok(())
    }

    pub fn destroy(self, gl: &glow::Context) {
        self.scan.destroy(gl);
        // SAFETY: the pipeline cache uniquely owns these programs.
        unsafe {
            for program in [
                self.clear,
                self.build,
                self.budget,
                self.search,
                self.mutual,
            ] {
                gl.delete_program(program);
            }
        }
    }
}

fn bind_derived(gl: &glow::Context, fields: Option<&PointFieldBuffers>) {
    // SAFETY: caller owns the current context and buffers remain live through dispatch.
    unsafe {
        if let Some(geometry) = fields.and_then(|fields| fields.geometry) {
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 4, Some(geometry));
        }
        if let Some(validity) = fields.and_then(|fields| fields.validity) {
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 5, Some(validity));
        }
    }
}

fn reset_status(gl: &glow::Context, buffer: glow::Buffer) -> Result<(), LibraryError> {
    // SAFETY: fixed upload exactly covers the owned status allocation.
    unsafe {
        gl.bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(buffer));
        gl.buffer_sub_data_u8_slice(
            glow::SHADER_STORAGE_BUFFER,
            0,
            bytemuck::bytes_of(&[0_u32; 4]),
        );
    }
    gl_operation_result(gl, "proximity status reset")
}

fn read_status(gl: &glow::Context, buffer: glow::Buffer) -> Result<[u32; 4], LibraryError> {
    let mut status = [0_u32; 4];
    // SAFETY: fixed read exactly covers the owned status allocation.
    unsafe {
        gl.bind_buffer(glow::SHADER_STORAGE_BUFFER, Some(buffer));
        gl.get_buffer_sub_data(
            glow::SHADER_STORAGE_BUFFER,
            0,
            bytemuck::bytes_of_mut(&mut status),
        );
    }
    gl_operation_result(gl, "proximity budget readback")?;
    Ok(status)
}

fn clear_indirect(gl: &glow::Context, buffer: glow::Buffer) -> Result<(), LibraryError> {
    // SAFETY: count zero and instance count one form a valid empty command.
    unsafe {
        gl.bind_buffer(glow::DRAW_INDIRECT_BUFFER, Some(buffer));
        gl.buffer_sub_data_u8_slice(
            glow::DRAW_INDIRECT_BUFFER,
            0,
            bytemuck::bytes_of(&[0_u32, 1, 0, 0]),
        );
    }
    gl_operation_result(gl, "empty Point line command")
}

#[cfg(test)]
pub(super) fn read_indirect_count(
    gl: &glow::Context,
    buffer: glow::Buffer,
) -> Result<u32, LibraryError> {
    let mut command = [0_u32; 4];
    // SAFETY: the test-only read exactly covers the owned indirect command.
    unsafe {
        gl.bind_buffer(glow::DRAW_INDIRECT_BUFFER, Some(buffer));
        gl.get_buffer_sub_data(
            glow::DRAW_INDIRECT_BUFFER,
            0,
            bytemuck::bytes_of_mut(&mut command),
        );
    }
    gl_operation_result(gl, "Point line command readback")?;
    if command[0] % 6 != 0 || command[1] != 1 || command[2] != 0 || command[3] != 0 {
        return Err(LibraryError::Render(
            "Point line indirect command is invalid".into(),
        ));
    }
    Ok(command[0] / 6)
}

fn validity_declaration(validity: bool) -> &'static str {
    if validity {
        r#"layout(std430, binding = 5) readonly buffer PointValidity {
    uint pointValidity[];
};

bool point_is_valid(uint slot) {
    return pointValidity[slot] != 0u;
}"#
    } else {
        r#"bool point_is_valid(uint slot) {
    return true;
}"#
    }
}

const HASH_GLSL: &str = r#"
uint hash_cell(ivec3 cell, uint mask) {
    uvec3 value = uvec3(cell);
    uint hash = value.x * 0x8da6b343u ^ value.y * 0xd8163841u ^ value.z * 0xcb1ab31fu;
    hash ^= hash >> 16u;
    hash *= 0x7feb352du;
    hash ^= hash >> 15u;
    return hash & mask;
}
"#;

const CLEAR_SHADER: &str = r#"#version 430 core
layout(local_size_x = 64) in;
layout(std430, binding = 0) buffer Buckets {
    uvec2 buckets[];
};
layout(std430, binding = 1) buffer Records {
    ivec4 records[];
};
uniform uint uBucketCount;
uniform uint uCapacity;

void main() {
    uint index = gl_GlobalInvocationID.x;
    if (index < uBucketCount) {
        buckets[index] = uvec2(0xffffffffu, 0u);
    }
    if (index < uCapacity) {
        records[index] = ivec4((-2147483647 - 1), 0, 0, -1);
    }
}
"#;

const BUILD_SHADER: &str = r#"#version 430 core
layout(local_size_x = 64) in;
// POINT_ACCESS
// VALIDITY
layout(std430, binding = 1) buffer Buckets {
    uvec2 buckets[];
};
layout(std430, binding = 2) buffer Records {
    ivec4 records[];
};
layout(std430, binding = 3) buffer Status {
    uint status[];
};
uniform uint uCapacity;
uniform uint uBucketMask;
uniform float uMaxDistance;
HASH_SOURCE

void main() {
    uint slot = gl_GlobalInvocationID.x;
    if (slot >= uCapacity) {
        return;
    }
    RenderPoint point = load_final_render_point(slot);
    if (!point.alive || !point_is_valid(slot)) {
        return;
    }
    vec3 scaled = floor(point.position_size.xyz / uMaxDistance);
    if (
        any(isnan(scaled)) ||
        any(isinf(scaled)) ||
        any(greaterThan(abs(scaled), vec3(2147483000.0)))
    ) {
        atomicOr(status[2], 2u);
        return;
    }
    ivec3 cell = ivec3(scaled);
    uint bucket = hash_cell(cell, uBucketMask);
    uint previous = atomicExchange(buckets[bucket].x, slot);
    atomicAdd(buckets[bucket].y, 1u);
    records[slot] = ivec4(cell, int(previous));
}
"#;

const BUDGET_SHADER: &str = r#"#version 430 core
layout(local_size_x = 64) in;
layout(std430, binding = 0) readonly buffer Buckets {
    uvec2 buckets[];
};
layout(std430, binding = 1) readonly buffer Records {
    ivec4 records[];
};
layout(std430, binding = 2) buffer Status {
    uint status[];
};
uniform uint uCapacity;
uniform uint uBucketMask;
HASH_SOURCE

void saturated_add(uint amount) {
    uint old = status[0];
    for (;;) {
        uint next = min(32000001u, old + min(amount, 32000001u - old));
        uint seen = atomicCompSwap(status[0], old, next);
        if (seen == old) {
            return;
        }
        old = seen;
    }
}

void main() {
    uint slot = gl_GlobalInvocationID.x;
    if (slot >= uCapacity || records[slot].x == (-2147483647 - 1)) {
        return;
    }
    ivec3 cell = records[slot].xyz;
    uint unique[27];
    uint count = 0u;
    uint work = 0u;
    for (int z = -1; z <= 1; z++) {
        for (int y = -1; y <= 1; y++) {
            for (int x = -1; x <= 1; x++) {
                uint bucket = hash_cell(cell + ivec3(x, y, z), uBucketMask);
                bool seen = false;
                for (uint index = 0u; index < count; index++) {
                    seen = seen || unique[index] == bucket;
                }
                if (!seen) {
                    unique[count++] = bucket;
                    work += buckets[bucket].y;
                }
            }
        }
    }
    atomicMax(status[1], work);
    if (work > 4096u) {
        atomicOr(status[2], 1u);
    }
    saturated_add(work);
}
"#;

const SEARCH_SHADER: &str = r#"#version 430 core
layout(local_size_x = 64) in;
// POINT_ACCESS
layout(std430, binding = 1) readonly buffer Buckets {
    uvec2 buckets[];
};
layout(std430, binding = 2) readonly buffer Records {
    ivec4 records[];
};
layout(std430, binding = 3) writeonly buffer Neighbors {
    uint neighbors[];
};
uniform uint uCapacity;
uniform uint uBucketMask;
uniform float uMinDistance;
uniform float uMaxDistance;
uniform uint uMaxNeighbors;
HASH_SOURCE

bool better(
    float distance,
    uint serial,
    float otherDistance,
    uint otherSerial
) {
    return distance < otherDistance ||
        (distance == otherDistance && serial < otherSerial);
}

void main() {
    uint slot = gl_GlobalInvocationID.x;
    if (slot >= uCapacity) {
        return;
    }
    for (uint rank = 0u; rank < uMaxNeighbors; rank++) {
        neighbors[slot * uMaxNeighbors + rank] = 0xffffffffu;
    }
    if (records[slot].x == (-2147483647 - 1)) {
        return;
    }

    RenderPoint point = load_final_render_point(slot);
    ivec3 cell = records[slot].xyz;
    float bestDistance[32];
    uint bestSerial[32];
    uint bestSlot[32];
    for (uint rank = 0u; rank < 32u; rank++) {
        bestDistance[rank] = 3.402823466e+38;
        bestSerial[rank] = 0xffffffffu;
        bestSlot[rank] = 0xffffffffu;
    }

    uint unique[27];
    uint uniqueCount = 0u;
    float minimum2 = uMinDistance * uMinDistance;
    float maximum2 = uMaxDistance * uMaxDistance;
    for (int z = -1; z <= 1; z++) {
        for (int y = -1; y <= 1; y++) {
            for (int x = -1; x <= 1; x++) {
                uint bucket = hash_cell(cell + ivec3(x, y, z), uBucketMask);
                bool seen = false;
                for (uint index = 0u; index < uniqueCount; index++) {
                    seen = seen || unique[index] == bucket;
                }
                if (seen) {
                    continue;
                }
                unique[uniqueCount++] = bucket;

                uint candidate = buckets[bucket].x;
                while (candidate != 0xffffffffu) {
                    ivec3 candidateCell = records[candidate].xyz;
                    bool nearby = all(greaterThanEqual(candidateCell, cell - ivec3(1))) &&
                        all(lessThanEqual(candidateCell, cell + ivec3(1)));
                    if (candidate != slot && nearby) {
                        RenderPoint other = load_final_render_point(candidate);
                        vec3 delta = other.position_size.xyz - point.position_size.xyz;
                        float distance2 = dot(delta, delta);
                        if (distance2 >= minimum2 && distance2 <= maximum2) {
                            float distance = distance2;
                            uint serial = other.serial;
                            uint candidateSlot = candidate;
                            for (uint rank = 0u; rank < uMaxNeighbors; rank++) {
                                if (better(
                                    distance,
                                    serial,
                                    bestDistance[rank],
                                    bestSerial[rank]
                                )) {
                                    float displacedDistance = bestDistance[rank];
                                    uint displacedSerial = bestSerial[rank];
                                    uint displacedSlot = bestSlot[rank];
                                    bestDistance[rank] = distance;
                                    bestSerial[rank] = serial;
                                    bestSlot[rank] = candidateSlot;
                                    distance = displacedDistance;
                                    serial = displacedSerial;
                                    candidateSlot = displacedSlot;
                                }
                            }
                        }
                    }
                    candidate = uint(records[candidate].w);
                }
            }
        }
    }
    for (uint rank = 0u; rank < uMaxNeighbors; rank++) {
        neighbors[slot * uMaxNeighbors + rank] = bestSlot[rank];
    }
}
"#;

const MUTUAL_SHADER: &str = r#"#version 430 core
layout(local_size_x = 64) in;
// POINT_ACCESS
layout(std430, binding = 1) readonly buffer Neighbors {
    uint neighbors[];
};
layout(std430, binding = 2) writeonly buffer Candidates {
    uint candidates[];
};
layout(std430, binding = 3) writeonly buffer EdgeCounts {
    uint edgeCounts[];
};
uniform uint uCapacity;
uniform uint uMaxNeighbors;

void main() {
    uint slot = gl_GlobalInvocationID.x;
    if (slot >= uCapacity) {
        return;
    }
    uint serial = load_final_render_point(slot).serial;
    uint count = 0u;
    for (uint rank = 0u; rank < uMaxNeighbors; rank++) {
        uint index = slot * uMaxNeighbors + rank;
        uint other = neighbors[index];
        uint outputValue = 0xffffffffu;
        if (other != 0xffffffffu) {
            uint otherSerial = load_final_render_point(other).serial;
            bool reciprocal = false;
            for (uint otherRank = 0u; otherRank < uMaxNeighbors; otherRank++) {
                reciprocal = reciprocal ||
                    neighbors[other * uMaxNeighbors + otherRank] == slot;
            }
            if (reciprocal && serial < otherSerial) {
                outputValue = other;
                count++;
            }
        }
        candidates[index] = outputValue;
    }
    edgeCounts[slot] = count;
}
"#;
