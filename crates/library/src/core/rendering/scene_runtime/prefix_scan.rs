//! Deterministic GPU exclusive scan used to compact derived Point topology.

use glow::HasContext;

use super::gl_backend::required_uniform;
use super::gl_operation_result;
use super::point_buffer_alloc::allocate_buffer;
use crate::error::LibraryError;
use crate::rendering::gl_resources::link_program;

const SCAN_BLOCK_SIZE: u32 = 256;
const INDIRECT_COMMAND_BYTES: u64 = 4 * size_of::<u32>() as u64;

pub(super) struct PrefixScanBuffers {
    offsets: Vec<glow::Buffer>,
    sums: Vec<glow::Buffer>,
    lengths: Vec<u32>,
    pub compact_edges: glow::Buffer,
    pub indirect: glow::Buffer,
}

impl PrefixScanBuffers {
    pub fn required_bytes(capacity: u32, max_neighbors: u32) -> Result<u64, LibraryError> {
        if capacity == 0 || max_neighbors == 0 {
            return Err(LibraryError::Validation(
                "Point edge compaction requires at least one slot".into(),
            ));
        }
        let edge_slots = capacity
            .checked_mul(max_neighbors)
            .ok_or_else(|| LibraryError::Render("Point edge count overflow".into()))?;
        let mut total = INDIRECT_COMMAND_BYTES
            .checked_add(u64::from(edge_slots.div_ceil(2)) * 8)
            .ok_or_else(|| LibraryError::Render("Point edge state size overflow".into()))?;
        let mut length = capacity;
        loop {
            total = total
                .checked_add(u64::from(length) * 4)
                .and_then(|bytes| {
                    bytes.checked_add(u64::from(length.div_ceil(SCAN_BLOCK_SIZE)) * 4)
                })
                .ok_or_else(|| LibraryError::Render("Point edge scan byte size overflow".into()))?;
            let blocks = length.div_ceil(SCAN_BLOCK_SIZE);
            if blocks == 1 {
                return Ok(total);
            }
            length = blocks;
        }
    }

    pub fn create(
        gl: &glow::Context,
        capacity: u32,
        max_neighbors: u32,
    ) -> Result<Self, LibraryError> {
        if capacity == 0 || max_neighbors == 0 {
            return Err(LibraryError::Validation(
                "Point edge compaction requires at least one slot".into(),
            ));
        }
        let mut lengths = Vec::new();
        let edge_slots = capacity
            .checked_mul(max_neighbors)
            .ok_or_else(|| LibraryError::Render("Point edge count overflow".into()))?;
        let mut length = capacity;
        loop {
            lengths.push(length);
            let blocks = length.div_ceil(SCAN_BLOCK_SIZE);
            if blocks == 1 {
                break;
            }
            length = blocks;
        }
        let mut offsets = Vec::with_capacity(lengths.len());
        let mut sums = Vec::with_capacity(lengths.len());
        let mut compact_edges = None;
        Self::required_bytes(capacity, max_neighbors)?;
        let result = (|| {
            for &length in &lengths {
                let offset_bytes = u64::from(length) * 4;
                let sum_bytes = u64::from(length.div_ceil(SCAN_BLOCK_SIZE)) * 4;
                offsets.push(allocate_buffer(
                    gl,
                    offset_bytes,
                    glow::DYNAMIC_COPY,
                    "Point edge scan offsets",
                )?);
                sums.push(allocate_buffer(
                    gl,
                    sum_bytes,
                    glow::DYNAMIC_COPY,
                    "Point edge scan sums",
                )?);
            }
            // Mutual directed entries appear twice; only the lower stable ID
            // survives, so at most ceil(slot_count / 2) compact edges exist.
            let compact_capacity = edge_slots.div_ceil(2);
            let compact_bytes = u64::from(compact_capacity) * 8;
            let compact_buffer =
                allocate_buffer(gl, compact_bytes, glow::DYNAMIC_COPY, "Point compact edges")?;
            compact_edges = Some(compact_buffer);
            let indirect = allocate_buffer(
                gl,
                INDIRECT_COMMAND_BYTES,
                glow::DYNAMIC_COPY,
                "Point indirect line command",
            )?;
            Ok(Self {
                offsets: std::mem::take(&mut offsets),
                sums: std::mem::take(&mut sums),
                lengths,
                compact_edges: compact_buffer,
                indirect,
            })
        })();
        if result.is_err() {
            // SAFETY: construction failed before any buffer escaped.
            unsafe {
                for buffer in offsets {
                    gl.delete_buffer(buffer);
                }
                for buffer in sums {
                    gl.delete_buffer(buffer);
                }
                if let Some(buffer) = compact_edges {
                    gl.delete_buffer(buffer);
                }
            }
        }
        result
    }

    pub fn destroy(self, gl: &glow::Context) {
        // SAFETY: these buffers are uniquely owned by this invocation.
        unsafe {
            for buffer in self.offsets {
                gl.delete_buffer(buffer);
            }
            for buffer in self.sums {
                gl.delete_buffer(buffer);
            }
            gl.delete_buffer(self.compact_edges);
            gl.delete_buffer(self.indirect);
        }
    }
}

#[derive(Clone)]
pub(super) struct PrefixScanPipeline {
    scan: glow::Program,
    scan_length: glow::UniformLocation,
    add: glow::Program,
    add_length: glow::UniformLocation,
    scatter: glow::Program,
    scatter_length: glow::UniformLocation,
    scatter_neighbors: glow::UniformLocation,
    command: glow::Program,
}

impl PrefixScanPipeline {
    pub fn create(gl: &glow::Context) -> Result<Self, LibraryError> {
        let scan = link_program(
            gl,
            &[(glow::COMPUTE_SHADER, SCAN_SHADER)],
            "Point edge scan",
        )?;
        let add = match link_program(
            gl,
            &[(glow::COMPUTE_SHADER, ADD_SHADER)],
            "Point edge scan add",
        ) {
            Ok(program) => program,
            Err(error) => {
                // SAFETY: scan is still uniquely owned on this error path.
                unsafe { gl.delete_program(scan) };
                return Err(error);
            }
        };
        let scatter = match link_program(
            gl,
            &[(glow::COMPUTE_SHADER, SCATTER_SHADER)],
            "Point edge scatter",
        ) {
            Ok(program) => program,
            Err(error) => {
                // SAFETY: both earlier programs are uniquely owned here.
                unsafe {
                    gl.delete_program(scan);
                    gl.delete_program(add);
                }
                return Err(error);
            }
        };
        let command = match link_program(
            gl,
            &[(glow::COMPUTE_SHADER, COMMAND_SHADER)],
            "Point indirect line command",
        ) {
            Ok(program) => program,
            Err(error) => {
                // SAFETY: all earlier programs are uniquely owned here.
                unsafe {
                    gl.delete_program(scan);
                    gl.delete_program(add);
                    gl.delete_program(scatter);
                }
                return Err(error);
            }
        };
        let uniforms = (|| {
            Ok(Self {
                scan,
                scan_length: required_uniform(gl, scan, "uLength")?,
                add,
                add_length: required_uniform(gl, add, "uLength")?,
                scatter,
                scatter_length: required_uniform(gl, scatter, "uLength")?,
                scatter_neighbors: required_uniform(gl, scatter, "uMaxNeighbors")?,
                command,
            })
        })();
        if uniforms.is_err() {
            // SAFETY: uniform discovery failed before any program escaped.
            unsafe {
                gl.delete_program(scan);
                gl.delete_program(add);
                gl.delete_program(scatter);
                gl.delete_program(command);
            }
        }
        uniforms
    }

    pub fn compact(
        &self,
        gl: &glow::Context,
        counts: glow::Buffer,
        candidates: glow::Buffer,
        max_neighbors: u32,
        buffers: &PrefixScanBuffers,
    ) -> Result<(), LibraryError> {
        let total = buffers.sums.last().copied().ok_or_else(|| {
            LibraryError::Validation("Point edge scan has no reduction level".into())
        })?;
        // SAFETY: all buffers/programs belong to this current context. Every
        // dispatch is bounded by the checked allocation lengths.
        unsafe {
            for (level, &length) in buffers.lengths.iter().enumerate() {
                let input = if level == 0 {
                    counts
                } else {
                    buffers.sums[level - 1]
                };
                gl.use_program(Some(self.scan));
                gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 0, Some(input));
                gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 1, Some(buffers.offsets[level]));
                gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 2, Some(buffers.sums[level]));
                gl.uniform_1_u32(Some(&self.scan_length), length);
                gl.dispatch_compute(length.div_ceil(SCAN_BLOCK_SIZE), 1, 1);
                gl.memory_barrier(glow::SHADER_STORAGE_BARRIER_BIT);
            }
            for level in (0..buffers.lengths.len().saturating_sub(1)).rev() {
                let length = buffers.lengths[level];
                gl.use_program(Some(self.add));
                gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 0, Some(buffers.offsets[level]));
                gl.bind_buffer_base(
                    glow::SHADER_STORAGE_BUFFER,
                    1,
                    Some(buffers.offsets[level + 1]),
                );
                gl.uniform_1_u32(Some(&self.add_length), length);
                gl.dispatch_compute(length.div_ceil(SCAN_BLOCK_SIZE), 1, 1);
                gl.memory_barrier(glow::SHADER_STORAGE_BARRIER_BIT);
            }
            let capacity = buffers.lengths[0];
            gl.use_program(Some(self.scatter));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 0, Some(candidates));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 1, Some(buffers.offsets[0]));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 2, Some(buffers.compact_edges));
            gl.uniform_1_u32(Some(&self.scatter_length), capacity);
            gl.uniform_1_u32(Some(&self.scatter_neighbors), max_neighbors);
            gl.dispatch_compute(capacity.div_ceil(SCAN_BLOCK_SIZE), 1, 1);
            gl.memory_barrier(glow::SHADER_STORAGE_BARRIER_BIT);

            gl.use_program(Some(self.command));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 0, Some(total));
            gl.bind_buffer_base(glow::SHADER_STORAGE_BUFFER, 1, Some(buffers.indirect));
            gl.dispatch_compute(1, 1, 1);
            gl.memory_barrier(glow::COMMAND_BARRIER_BIT | glow::SHADER_STORAGE_BARRIER_BIT);
        }
        gl_operation_result(gl, "edge compaction")
    }

    pub fn destroy(self, gl: &glow::Context) {
        // SAFETY: the pipeline cache uniquely owns these programs.
        unsafe {
            gl.delete_program(self.scan);
            gl.delete_program(self.add);
            gl.delete_program(self.scatter);
            gl.delete_program(self.command);
        }
    }
}

const SCAN_SHADER: &str = r#"#version 430 core
layout(local_size_x = 256) in;
layout(std430, binding = 0) readonly buffer InputValues {
    uint inputValues[];
};
layout(std430, binding = 1) writeonly buffer OutputOffsets {
    uint outputOffsets[];
};
layout(std430, binding = 2) writeonly buffer BlockSums {
    uint blockSums[];
};
uniform uint uLength;
shared uint values[256];

void main() {
    uint lane = gl_LocalInvocationID.x;
    uint index = gl_WorkGroupID.x * 256u + lane;
    uint value = index < uLength ? inputValues[index] : 0u;
    values[lane] = value;
    barrier();

    for (uint offset = 1u; offset < 256u; offset <<= 1u) {
        uint addend = lane >= offset ? values[lane - offset] : 0u;
        barrier();
        values[lane] += addend;
        barrier();
    }

    if (index < uLength) {
        outputOffsets[index] = values[lane] - value;
    }
    if (index < uLength && (lane == 255u || index + 1u == uLength)) {
        blockSums[gl_WorkGroupID.x] = values[lane];
    }
}
"#;

const ADD_SHADER: &str = r#"#version 430 core
layout(local_size_x = 256) in;
layout(std430, binding = 0) buffer Offsets {
    uint offsets[];
};
layout(std430, binding = 1) readonly buffer ParentOffsets {
    uint parentOffsets[];
};
uniform uint uLength;

void main() {
    uint index = gl_GlobalInvocationID.x;
    if (index < uLength) {
        offsets[index] += parentOffsets[index / 256u];
    }
}
"#;

const SCATTER_SHADER: &str = r#"#version 430 core
layout(local_size_x = 256) in;
layout(std430, binding = 0) readonly buffer Candidates {
    uint candidates[];
};
layout(std430, binding = 1) readonly buffer Offsets {
    uint offsets[];
};
layout(std430, binding = 2) writeonly buffer CompactEdges {
    uvec2 compactEdges[];
};
uniform uint uLength;
uniform uint uMaxNeighbors;

void main() {
    uint slot = gl_GlobalInvocationID.x;
    if (slot >= uLength) {
        return;
    }
    uint outputIndex = offsets[slot];
    for (uint rank = 0u; rank < uMaxNeighbors; rank++) {
        uint neighbor = candidates[slot * uMaxNeighbors + rank];
        if (neighbor != 0xffffffffu) {
            compactEdges[outputIndex++] = uvec2(slot, neighbor);
        }
    }
}
"#;

const COMMAND_SHADER: &str = r#"#version 430 core
layout(local_size_x = 1) in;
layout(std430, binding = 0) readonly buffer Total {
    uint total[];
};
layout(std430, binding = 1) writeonly buffer Command {
    uint command[];
};

void main() {
    command[0] = total[0] * 6u;
    command[1] = 1u;
    command[2] = 0u;
    command[3] = 0u;
}
"#;
