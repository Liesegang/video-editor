//! Sampled Point topology and drawing parameters, not persisted edge arrays.

use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

use super::SpriteSelection;
use crate::model::property::ImageCollectionValue;

pub const POINT_CONNECTION_MAX_NEIGHBORS: u32 = 32;
pub const POINT_CONNECTION_MAX_DISTANCE: f32 = 1_000_000.0;
pub const POINT_LINE_MAX_WIDTH: f32 = 512.0;

/// Exact mutual-K-nearest relationships in the Point producer's 3D space.
/// Equal distances are ordered by stable Point identity, never buffer order.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(deny_unknown_fields)]
pub struct PointConnectionParameters {
    pub min_distance: OrderedFloat<f32>,
    pub max_distance: OrderedFloat<f32>,
    pub max_neighbors: u32,
}

impl PointConnectionParameters {
    pub fn validate(&self) -> Result<(), String> {
        let minimum = self.min_distance.0;
        let maximum = self.max_distance.0;
        if !minimum.is_finite()
            || !maximum.is_finite()
            || minimum < 0.0
            || minimum > maximum
            || maximum > POINT_CONNECTION_MAX_DISTANCE
        {
            return Err(format!(
                "Point connection distances require 0 <= minimum <= maximum <= {POINT_CONNECTION_MAX_DISTANCE}"
            ));
        }
        if !(1..=POINT_CONNECTION_MAX_NEIGHBORS).contains(&self.max_neighbors) {
            return Err(format!(
                "Point connection Max Neighbors must be in 1..={POINT_CONNECTION_MAX_NEIGHBORS}"
            ));
        }
        Ok(())
    }
}

/// Renderer-specific state has one owner and cannot mix Sprite resources with
/// Line topology. Both variants consume the same Point source and field SSA.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointRenderStyle {
    Sprites {
        images: ImageCollectionValue,
        selection: SpriteSelection,
    },
    Lines {
        connections: PointConnectionParameters,
        width: OrderedFloat<f32>,
        /// Zero preserves endpoint opacity; one fades line opacity linearly
        /// to zero as its 3D length reaches Maximum Distance.
        fade: OrderedFloat<f32>,
    },
}

impl Default for PointRenderStyle {
    fn default() -> Self {
        Self::Sprites {
            images: ImageCollectionValue::default(),
            selection: SpriteSelection::Random,
        }
    }
}

impl PointRenderStyle {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Sprites { images, selection } => {
                images.validate().map_err(|error| error.to_string())?;
                if let SpriteSelection::Value(value) = selection
                    && (!value.0.is_finite() || value.0.abs() > f64::from(f32::MAX))
                {
                    return Err(
                        "Sprite selection must be finite and representable on the GPU".into(),
                    );
                }
            }
            Self::Lines {
                connections,
                width,
                fade,
            } => {
                connections.validate()?;
                if !width.0.is_finite() || !(0.0..=POINT_LINE_MAX_WIDTH).contains(&width.0) {
                    return Err(format!(
                        "Point line width must be finite and in 0..={POINT_LINE_MAX_WIDTH}px"
                    ));
                }
                if !fade.0.is_finite() || !(0.0..=1.0).contains(&fade.0) {
                    return Err("Point line distance fade must be finite and in 0..=1".into());
                }
            }
        }
        Ok(())
    }

    pub fn sprite_images(&self) -> Option<&ImageCollectionValue> {
        match self {
            Self::Sprites { images, .. } => Some(images),
            Self::Lines { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connections() -> PointConnectionParameters {
        PointConnectionParameters {
            min_distance: OrderedFloat(0.0),
            max_distance: OrderedFloat(80.0),
            max_neighbors: 6,
        }
    }

    #[test]
    fn connection_ranges_and_degree_limit_are_checked_without_allocating() {
        let mut value = connections();
        value.validate().unwrap();
        value.max_distance = OrderedFloat(0.0);
        value.validate().unwrap();
        value.min_distance = OrderedFloat(1.0);
        assert!(value.validate().is_err());
        value.min_distance = OrderedFloat(0.0);
        for maximum in [
            -1.0,
            f32::NAN,
            f32::INFINITY,
            POINT_CONNECTION_MAX_DISTANCE * 2.0,
        ] {
            value.max_distance = OrderedFloat(maximum);
            assert!(value.validate().is_err());
        }
        value = connections();
        for count in [0, POINT_CONNECTION_MAX_NEIGHBORS + 1, u32::MAX] {
            value.max_neighbors = count;
            assert!(value.validate().is_err());
        }
    }

    #[test]
    fn line_and_sprite_resources_are_exclusive_and_round_trip() {
        for style in [
            PointRenderStyle::default(),
            PointRenderStyle::Lines {
                connections: connections(),
                width: OrderedFloat(2.0),
                fade: OrderedFloat(0.5),
            },
        ] {
            style.validate().unwrap();
            let mut json = serde_json::to_value(&style).unwrap();
            assert_eq!(
                serde_json::from_value::<PointRenderStyle>(json.clone()).unwrap(),
                style
            );
            if matches!(style, PointRenderStyle::Lines { .. }) {
                assert!(style.sprite_images().is_none());
                json["images"] = serde_json::to_value(ImageCollectionValue::default()).unwrap();
                assert!(serde_json::from_value::<PointRenderStyle>(json).is_err());
            }
        }
    }

    #[test]
    fn line_width_and_fade_reject_nonfinite_or_out_of_range_values() {
        for (width, fade) in [
            (f32::NAN, 0.0),
            (-1.0, 0.0),
            (513.0, 0.0),
            (2.0, f32::INFINITY),
            (2.0, -0.1),
            (2.0, 1.1),
        ] {
            assert!(
                PointRenderStyle::Lines {
                    connections: connections(),
                    width: OrderedFloat(width),
                    fade: OrderedFloat(fade),
                }
                .validate()
                .is_err()
            );
        }
    }
}
