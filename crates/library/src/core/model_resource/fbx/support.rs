use std::collections::HashMap;
use std::hash::Hash;
use std::mem::size_of;

use super::super::ModelResourceError;
use super::enforce_limit;

pub(super) struct OwnedBudget {
    used: usize,
    limit: usize,
}

impl OwnedBudget {
    pub(super) fn new(limit: usize) -> Self {
        Self { used: 0, limit }
    }

    pub(super) fn reserve(
        &mut self,
        resource: &'static str,
        bytes: usize,
    ) -> Result<(), ModelResourceError> {
        self.used = self.used.saturating_add(bytes);
        enforce_limit(resource, self.used, self.limit)
    }

    pub(super) fn vec_with_capacity<T>(
        &mut self,
        count: usize,
    ) -> Result<Vec<T>, ModelResourceError> {
        self.reserve(
            "owned scene bytes",
            count
                .checked_mul(size_of::<T>())
                .ok_or(ModelResourceError::BudgetExceeded {
                    resource: "owned scene bytes",
                    actual: usize::MAX,
                    limit: self.limit,
                })?,
        )?;
        try_vec_with_capacity(count, "owned scene bytes")
    }
}

/// Conservative cumulative budget for temporary Rust-side decode storage.
///
/// Cumulative accounting deliberately does not credit released allocations.
/// This is stricter than a peak-only budget, but means a malformed source can
/// never evade the configured limit by forcing many sequential work buffers.
pub(super) struct WorkingBudget {
    used: usize,
    limit: usize,
}

impl WorkingBudget {
    pub(super) fn new(limit: usize) -> Self {
        Self { used: 0, limit }
    }

    pub(super) fn vec_with_capacity<T>(
        &mut self,
        count: usize,
        resource: &'static str,
    ) -> Result<Vec<T>, ModelResourceError> {
        let bytes =
            count
                .checked_mul(size_of::<T>())
                .ok_or(ModelResourceError::BudgetExceeded {
                    resource,
                    actual: usize::MAX,
                    limit: self.limit,
                })?;
        self.reserve(resource, bytes)?;
        try_vec_with_capacity(count, resource)
    }

    pub(super) fn hash_map_with_capacity<K: Eq + Hash, V>(
        &mut self,
        count: usize,
    ) -> Result<HashMap<K, V>, ModelResourceError> {
        // Hash table capacity and control bytes are implementation details.
        // Charge a conservative two slots per requested entry, plus one byte
        // of control storage per slot, before asking the allocator.
        let slot_bytes = size_of::<(K, V)>().saturating_add(1);
        let charged = count
            .checked_mul(2)
            .and_then(|slots| slots.checked_mul(slot_bytes))
            .ok_or(ModelResourceError::BudgetExceeded {
                resource: "decode working bytes",
                actual: usize::MAX,
                limit: self.limit,
            })?;
        self.reserve("decode working bytes", charged)?;
        let mut values = HashMap::new();
        values
            .try_reserve(count)
            .map_err(|_| ModelResourceError::AllocationFailed {
                resource: "decode working bytes",
                requested: charged,
            })?;
        Ok(values)
    }

    fn reserve(&mut self, resource: &'static str, bytes: usize) -> Result<(), ModelResourceError> {
        self.used = self.used.saturating_add(bytes);
        enforce_limit(resource, self.used, self.limit)
    }
}

fn try_vec_with_capacity<T>(
    count: usize,
    resource: &'static str,
) -> Result<Vec<T>, ModelResourceError> {
    let requested = count.saturating_mul(size_of::<T>());
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| ModelResourceError::AllocationFailed {
            resource,
            requested,
        })?;
    Ok(values)
}

pub(super) fn validate_vec3_attribute(
    attribute: &ufbx::VertexVec3,
    vertex_count: usize,
    label: &'static str,
    required: bool,
    mesh_name: &str,
) -> Result<(), ModelResourceError> {
    if !attribute.exists {
        if required {
            return Err(ModelResourceError::InvalidData {
                detail: format!("mesh {mesh_name:?} has no vertex {label} data"),
            });
        }
        return Ok(());
    }
    validate_attribute_indices(
        &attribute.indices,
        attribute.values.len(),
        vertex_count,
        label,
        mesh_name,
    )
}

pub(super) fn validate_vec2_attribute(
    attribute: &ufbx::VertexVec2,
    vertex_count: usize,
    label: &'static str,
    required: bool,
    mesh_name: &str,
) -> Result<(), ModelResourceError> {
    if !attribute.exists {
        if required {
            return Err(ModelResourceError::InvalidData {
                detail: format!("mesh {mesh_name:?} has no vertex {label} data"),
            });
        }
        return Ok(());
    }
    validate_attribute_indices(
        &attribute.indices,
        attribute.values.len(),
        vertex_count,
        label,
        mesh_name,
    )
}

fn validate_attribute_indices(
    indices: &[u32],
    value_count: usize,
    vertex_count: usize,
    label: &'static str,
    mesh_name: &str,
) -> Result<(), ModelResourceError> {
    if indices.len() < vertex_count {
        return Err(ModelResourceError::InvalidData {
            detail: format!(
                "mesh {mesh_name:?} {label} index array has {} entries for {vertex_count} polygon vertices",
                indices.len()
            ),
        });
    }
    if let Some(index) = indices
        .iter()
        .take(vertex_count)
        .find(|index| **index as usize >= value_count)
    {
        return Err(ModelResourceError::InvalidData {
            detail: format!(
                "mesh {mesh_name:?} {label} index {index} exceeds {value_count} values"
            ),
        });
    }
    Ok(())
}

pub(super) fn checked_vec3(
    value: ufbx::Vec3,
    label: &'static str,
    element: &str,
) -> Result<[f32; 3], ModelResourceError> {
    Ok([
        checked_scalar(value.x, label, element)?,
        checked_scalar(value.y, label, element)?,
        checked_scalar(value.z, label, element)?,
    ])
}

pub(super) fn checked_vec2(
    value: ufbx::Vec2,
    label: &'static str,
    element: &str,
) -> Result<[f32; 2], ModelResourceError> {
    Ok([
        checked_scalar(value.x, label, element)?,
        checked_scalar(value.y, label, element)?,
    ])
}

pub(super) fn checked_color(
    value: ufbx::Vec4,
    element: &str,
) -> Result<[f32; 4], ModelResourceError> {
    Ok([
        checked_scalar(value.x, "material base color", element)?,
        checked_scalar(value.y, "material base color", element)?,
        checked_scalar(value.z, "material base color", element)?,
        checked_scalar(value.w, "material base color", element)?,
    ])
}

pub(super) fn checked_matrix(
    value: ufbx::Matrix,
    label: &'static str,
    element: &str,
) -> Result<[[f32; 4]; 4], ModelResourceError> {
    Ok([
        [
            checked_scalar(value.m00, label, element)?,
            checked_scalar(value.m10, label, element)?,
            checked_scalar(value.m20, label, element)?,
            0.0,
        ],
        [
            checked_scalar(value.m01, label, element)?,
            checked_scalar(value.m11, label, element)?,
            checked_scalar(value.m21, label, element)?,
            0.0,
        ],
        [
            checked_scalar(value.m02, label, element)?,
            checked_scalar(value.m12, label, element)?,
            checked_scalar(value.m22, label, element)?,
            0.0,
        ],
        [
            checked_scalar(value.m03, label, element)?,
            checked_scalar(value.m13, label, element)?,
            checked_scalar(value.m23, label, element)?,
            1.0,
        ],
    ])
}

pub(super) fn checked_scalar(
    value: f64,
    label: &'static str,
    element: &str,
) -> Result<f32, ModelResourceError> {
    if !value.is_finite() || value.abs() > f32::MAX as f64 {
        return Err(ModelResourceError::InvalidData {
            detail: format!("{element:?} has a non-finite or out-of-range {label}"),
        });
    }
    Ok(value as f32)
}

pub(super) fn checked_product(
    left: f32,
    right: f32,
    label: &'static str,
    element: &str,
) -> Result<f32, ModelResourceError> {
    let value = left * right;
    if !value.is_finite() {
        return Err(ModelResourceError::InvalidData {
            detail: format!("{element:?} has an out-of-range {label}"),
        });
    }
    Ok(value)
}

pub(super) fn generate_vertex_normals(
    vertices: &mut [super::super::MeshVertex],
    indices: &[u32],
    working: &mut WorkingBudget,
) -> Result<usize, ModelResourceError> {
    let mut sums = working.vec_with_capacity::<[f64; 3]>(vertices.len(), "decode working bytes")?;
    sums.resize(vertices.len(), [0.0_f64; 3]);
    for triangle in indices.chunks_exact(3) {
        let a = vertices[triangle[0] as usize].position.map(f64::from);
        let b = vertices[triangle[1] as usize].position.map(f64::from);
        let c = vertices[triangle[2] as usize].position.map(f64::from);
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let normal = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        for index in triangle {
            let sum = &mut sums[*index as usize];
            for channel in 0..3 {
                sum[channel] += normal[channel];
            }
        }
    }
    let mut degenerate = 0;
    for (vertex, normal) in vertices.iter_mut().zip(sums) {
        let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if length.is_finite() && length > f64::EPSILON {
            vertex.normal = [
                (normal[0] / length) as f32,
                (normal[1] / length) as f32,
                (normal[2] / length) as f32,
            ];
        } else {
            vertex.normal = [0.0, 1.0, 0.0];
            degenerate += 1;
        }
    }
    Ok(degenerate)
}

pub(super) fn copy_text(
    value: &ufbx::String,
    budget: &mut OwnedBudget,
) -> Result<String, ModelResourceError> {
    copy_owned_text(&bounded_text(value, 4_096), budget)
}

pub(super) fn copy_owned_text(
    value: &str,
    budget: &mut OwnedBudget,
) -> Result<String, ModelResourceError> {
    budget.reserve("owned scene bytes", value.len())?;
    let mut output = String::new();
    output
        .try_reserve_exact(value.len())
        .map_err(|_| ModelResourceError::AllocationFailed {
            resource: "owned scene bytes",
            requested: value.len(),
        })?;
    output.push_str(value);
    Ok(output)
}

pub(super) fn bounded_element_name(value: &ufbx::String) -> String {
    bounded_text(value, 512)
}

pub(super) fn bounded_text(value: &ufbx::String, max_bytes: usize) -> String {
    let value: &str = value.as_ref();
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut output = String::new();
    for character in value.chars() {
        if output.len().saturating_add(character.len_utf8()) > max_bytes {
            break;
        }
        output.push(character);
    }
    output.push('…');
    output
}
