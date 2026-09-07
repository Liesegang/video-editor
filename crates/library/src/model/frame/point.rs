//! Derived commands for shared Point fields and Sprite rendering.
//!
//! Authored topology stays in ModuleDefinition. Source simulation and procedural
//! generation share consumers, not mutable simulation state or fake lifetimes.

use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{color::Color, particle::ParticleSceneParameters};
use crate::model::authoring::{InstancePath, ModuleInstanceId, ModuleOutputId};
use crate::model::point::{POINT_MAX_CAPACITY, PointInstruction, PointRenderProgram};
use crate::model::property::Vec3;

mod connections;
pub use connections::{
    POINT_CONNECTION_MAX_DISTANCE, POINT_CONNECTION_MAX_NEIGHBORS, POINT_LINE_MAX_WIDTH,
    PointConnectionParameters, PointRenderStyle,
};

pub const POINT_GRID_AXIS_BITS: u32 = 10;
pub const POINT_GRID_MAX_AXIS: u32 = 1 << POINT_GRID_AXIS_BITS;
pub const POINT_GRID_POSITION_LIMIT: f64 = 1_000_000.0;
pub const POINT_GRID_MAX_SIZE: f32 = 512.0;

/// Placement identity and output state are independent from the producer ID.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(deny_unknown_fields)]
pub struct SceneInvocationKey {
    pub instance_path: InstancePath,
    pub module_instance_id: ModuleInstanceId,
    pub state_slot_id: Uuid,
    pub output_id: ModuleOutputId,
}

/// A centered 3D lattice. Dense evaluation slots are not Point identity.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(deny_unknown_fields)]
pub struct PointGridParameters {
    pub counts: [u32; 3],
    pub spacing: Vec3,
    pub center: Vec3,
    pub size: OrderedFloat<f32>,
    pub seed: u32,
}

impl PointGridParameters {
    /// Saturation is deliberate: invalid capacities can never wrap to an
    /// apparently valid allocation. validate() runs before any allocation.
    pub fn capacity(&self) -> u32 {
        self.counts.into_iter().fold(1_u32, u32::saturating_mul)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self
            .counts
            .iter()
            .any(|count| !(1..=POINT_GRID_MAX_AXIS).contains(count))
        {
            return Err(format!(
                "Point Grid axis counts must be in 1..={POINT_GRID_MAX_AXIS}"
            ));
        }
        if self.capacity() > POINT_MAX_CAPACITY {
            return Err(format!(
                "Point Grid supports at most {POINT_MAX_CAPACITY} points"
            ));
        }
        let spacing = [self.spacing.x.0, self.spacing.y.0, self.spacing.z.0];
        let center = [self.center.x.0, self.center.y.0, self.center.z.0];
        for axis in 0..3 {
            let extent = f64::from(self.counts[axis] - 1) * 0.5 * spacing[axis].abs();
            if !spacing[axis].is_finite() || !center[axis].is_finite() {
                return Err("Point Grid center and spacing must be finite".into());
            }
            if spacing[axis].abs() > POINT_GRID_POSITION_LIMIT
                || center[axis].abs() + extent > POINT_GRID_POSITION_LIMIT
            {
                return Err("Point Grid positions and spacing must stay within ±1000000px".into());
            }
        }
        if !self.size.0.is_finite() || self.size.0 <= 0.0 || self.size.0 > POINT_GRID_MAX_SIZE {
            return Err("Point Grid size must be positive and at most 512px".into());
        }
        Ok(())
    }

    /// Stable lattice identity, independent of row-major slot and axis counts.
    pub fn point_serial(&self, coordinate: [u32; 3]) -> Option<u32> {
        if coordinate
            .iter()
            .zip(self.counts)
            .any(|(value, count)| *value >= count || *value >= POINT_GRID_MAX_AXIS)
        {
            return None;
        }
        Some(
            coordinate[0]
                | (coordinate[1] << POINT_GRID_AXIS_BITS)
                | (coordinate[2] << (2 * POINT_GRID_AXIS_BITS)),
        )
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointSceneSource {
    Particle {
        target_step: u64,
        parameters: ParticleSceneParameters,
    },
    Grid(PointGridParameters),
}

impl PointSceneSource {
    pub fn capacity(&self) -> u32 {
        match self {
            Self::Particle { parameters, .. } => parameters.capacity,
            Self::Grid(parameters) => parameters.capacity(),
        }
    }

    pub fn seed(&self) -> u32 {
        match self {
            Self::Particle { parameters, .. } => parameters.seed,
            Self::Grid(parameters) => parameters.seed,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Particle { parameters, .. } => parameters.validate(),
            Self::Grid(parameters) => parameters.validate(),
        }
    }

    pub fn supports_age(&self) -> bool {
        matches!(self, Self::Particle { .. })
    }
}

/// Preview and export pass the same sampled command to the same renderer.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SpriteSelection {
    Random,
    /// Used when there is no per-point selection register. GPU selection
    /// clamps finite factors to [0,1]; a factor of 1 chooses the last image.
    Value(OrderedFloat<f64>),
}

/// Preview and export pass the same sampled command to the same renderer.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(deny_unknown_fields)]
pub struct PointSceneFrame {
    pub invocation: SceneInvocationKey,
    /// Producer identity scopes random streams; Sprite branches do not reseed it.
    pub source_node_id: Uuid,
    pub logical_width: u32,
    pub logical_height: u32,
    pub source: PointSceneSource,
    pub color: Color,
    pub render_style: PointRenderStyle,
    pub point_program: Option<PointRenderProgram>,
}

impl PointSceneFrame {
    pub fn validate(&self) -> Result<(), String> {
        if self.logical_width == 0 || self.logical_height == 0 {
            return Err("Point render dimensions must be positive".into());
        }
        self.source.validate()?;
        self.render_style.validate()?;
        if let Some(program) = &self.point_program {
            program.validate()?;
            if matches!(self.render_style, PointRenderStyle::Lines { .. })
                && program.sprite_selection_register.is_some()
            {
                return Err("Point line rendering cannot consume a Sprite selection field".into());
            }
            if !self.source.supports_age()
                && program.instructions.iter().any(|instruction| {
                    matches!(
                        instruction,
                        PointInstruction::Age | PointInstruction::NormalizedAge
                    )
                })
            {
                return Err("Point Age and Normalized Age require a Particle source".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
