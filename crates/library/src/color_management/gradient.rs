//! Managed-color sampling for the canonical authored Gradient value.

use std::fmt;

use crate::model::property::{ColorValue, GradientGeometry, GradientSpread, GradientValue, Vec2};

use super::{ColorTransformError, to_working_linear_srgb};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GradientSampleError {
    NonFinitePosition,
    NonFiniteFactor,
    UnrepresentableGeometryParameter,
    UnrepresentableColor,
    ColorTransform(ColorTransformError),
}

impl fmt::Display for GradientSampleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinitePosition => {
                formatter.write_str("gradient sample position must be finite")
            }
            Self::NonFiniteFactor => formatter.write_str("gradient sample factor must be finite"),
            Self::UnrepresentableGeometryParameter => formatter.write_str(
                "gradient geometry and sample position exceed the numeric sampling range",
            ),
            Self::UnrepresentableColor => formatter.write_str(
                "gradient stop interpolation cannot be represented in the working color space",
            ),
            Self::ColorTransform(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for GradientSampleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ColorTransform(error) => Some(error),
            Self::NonFinitePosition
            | Self::NonFiniteFactor
            | Self::UnrepresentableGeometryParameter
            | Self::UnrepresentableColor => None,
        }
    }
}

impl From<ColorTransformError> for GradientSampleError {
    fn from(error: ColorTransformError) -> Self {
        Self::ColorTransform(error)
    }
}

/// Samples an authored Gradient in its normalized geometry domain.
///
/// Stops are transformed to straight-alpha linear sRGB before interpolation,
/// retaining extended-range RGB. At duplicate offsets the last stop at that
/// offset wins, making hard stops right-continuous.
pub fn sample_gradient(
    gradient: &GradientValue,
    position: Vec2,
) -> Result<ColorValue, GradientSampleError> {
    let position = [position.x.into_inner(), position.y.into_inner()];
    if position.iter().any(|component| !component.is_finite()) {
        return Err(GradientSampleError::NonFinitePosition);
    }
    let parameter = geometry_parameter(gradient.geometry(), position)?;
    sample_gradient_at(gradient, parameter)
}

/// Samples the one-dimensional color ramp at a normalized factor.
///
/// This ignores spatial Gradient geometry while retaining the authored spread
/// and stop semantics, making it the canonical evaluator for Color Ramp Nodes.
pub fn sample_gradient_at(
    gradient: &GradientValue,
    factor: f64,
) -> Result<ColorValue, GradientSampleError> {
    if !factor.is_finite() {
        return Err(GradientSampleError::NonFiniteFactor);
    }
    let parameter = apply_spread(factor, gradient.spread())?;
    sample_stops(gradient, parameter)
}

fn geometry_parameter(
    geometry: GradientGeometry,
    position: [f64; 2],
) -> Result<f64, GradientSampleError> {
    let coordinates = match geometry {
        GradientGeometry::Linear { start, end } => {
            let start = [start.x.into_inner(), start.y.into_inner()];
            let end = [end.x.into_inner(), end.y.into_inner()];
            let scale = coordinate_scale(&[start, end, position]);
            let start = scale_point(start, scale);
            let end = scale_point(end, scale);
            let position = scale_point(position, scale);
            let direction = [end[0] - start[0], end[1] - start[1]];
            let denominator = direction[0].mul_add(direction[0], direction[1] * direction[1]);
            let relative = [position[0] - start[0], position[1] - start[1]];
            relative[0].mul_add(direction[0], relative[1] * direction[1]) / denominator
        }
        GradientGeometry::Radial { center, radius } => {
            let center = [center.x.into_inner(), center.y.into_inner()];
            let radius = radius.into_inner();
            let scale = coordinate_scale(&[center, position, [radius, 0.0]]);
            let center = scale_point(center, scale);
            let position = scale_point(position, scale);
            let scaled_radius = radius / scale;
            (position[0] - center[0]).hypot(position[1] - center[1]) / scaled_radius
        }
    };
    if !coordinates.is_finite() {
        return Err(GradientSampleError::UnrepresentableGeometryParameter);
    }
    Ok(coordinates)
}

fn coordinate_scale(points: &[[f64; 2]]) -> f64 {
    points
        .iter()
        .flatten()
        .map(|value| value.abs())
        .fold(0.0, f64::max)
        .max(f64::MIN_POSITIVE)
}

fn scale_point(point: [f64; 2], scale: f64) -> [f64; 2] {
    [point[0] / scale, point[1] / scale]
}

fn apply_spread(parameter: f64, spread: GradientSpread) -> Result<f64, GradientSampleError> {
    let parameter = match spread {
        GradientSpread::Pad => parameter.clamp(0.0, 1.0),
        GradientSpread::Repeat => parameter.rem_euclid(1.0),
        GradientSpread::Reflect => {
            let reflected = parameter.rem_euclid(2.0);
            if reflected <= 1.0 {
                reflected
            } else {
                2.0 - reflected
            }
        }
    };
    if !parameter.is_finite() {
        return Err(GradientSampleError::UnrepresentableGeometryParameter);
    }
    Ok(parameter)
}

fn sample_stops(
    gradient: &GradientValue,
    parameter: f64,
) -> Result<ColorValue, GradientSampleError> {
    let stops = gradient.stops();
    let upper = stops.partition_point(|stop| stop.offset() <= parameter);
    let (left, right, amount) = if upper == 0 {
        (&stops[0], &stops[0], 0.0)
    } else if upper == stops.len() {
        (&stops[upper - 1], &stops[upper - 1], 0.0)
    } else {
        let left = &stops[upper - 1];
        let right = &stops[upper];
        let amount = (parameter - left.offset()) / (right.offset() - left.offset());
        (left, right, amount)
    };
    let left = to_working_linear_srgb(left.color())?;
    if amount == 0.0 {
        return Ok(left);
    }
    let right = to_working_linear_srgb(right.color())?;
    left.interpolate_same_space(&right, amount)
        .ok_or(GradientSampleError::UnrepresentableColor)
}

#[cfg(test)]
mod tests {
    use ordered_float::OrderedFloat;

    use super::*;
    use crate::model::property::{ColorSpaceRef, GradientStop, PaintValueError};

    fn point(x: f64, y: f64) -> Vec2 {
        Vec2 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
        }
    }

    fn linear_color(rgba: [f64; 4]) -> ColorValue {
        ColorValue::new(ColorSpaceRef::linear_srgb(), rgba).expect("valid test color")
    }

    fn gradient(
        geometry: GradientGeometry,
        spread: GradientSpread,
        stops: Vec<(f64, ColorValue)>,
    ) -> Result<GradientValue, PaintValueError> {
        GradientValue::new(
            geometry,
            spread,
            stops
                .into_iter()
                .map(|(offset, color)| GradientStop::new(offset, color).unwrap())
                .collect(),
        )
    }

    fn assert_rgba_near(actual: &ColorValue, expected: [f64; 4]) {
        assert_eq!(actual.color_space(), &ColorSpaceRef::linear_srgb());
        for (actual, expected) in actual.rgba().into_iter().zip(expected) {
            assert!(
                (actual - expected).abs() <= 1.0e-11,
                "{actual} != {expected}"
            );
        }
    }

    #[test]
    fn linear_and_radial_geometry_sample_in_normalized_space() {
        let stops = vec![
            (0.0, linear_color([0.0, -1.0, 2.0, 0.25])),
            (1.0, linear_color([2.0, 1.0, 4.0, 0.75])),
        ];
        let linear = gradient(
            GradientGeometry::Linear {
                start: point(1.0, 2.0),
                end: point(5.0, 2.0),
            },
            GradientSpread::Pad,
            stops.clone(),
        )
        .unwrap();
        assert_rgba_near(
            &sample_gradient(&linear, point(3.0, 7.0)).unwrap(),
            [1.0, 0.0, 3.0, 0.5],
        );

        let radial = gradient(
            GradientGeometry::Radial {
                center: point(2.0, 3.0),
                radius: OrderedFloat(4.0),
            },
            GradientSpread::Pad,
            stops,
        )
        .unwrap();
        assert_rgba_near(
            &sample_gradient(&radial, point(2.0, 5.0)).unwrap(),
            [1.0, 0.0, 3.0, 0.5],
        );
    }

    #[test]
    fn repeat_and_reflect_define_negative_coordinates() {
        let stops = vec![
            (0.0, linear_color([0.0, 0.0, 0.0, 1.0])),
            (1.0, linear_color([1.0, 1.0, 1.0, 1.0])),
        ];
        for (spread, expected) in [
            (GradientSpread::Pad, 0.0),
            (GradientSpread::Repeat, 0.75),
            (GradientSpread::Reflect, 0.25),
        ] {
            let value = gradient(
                GradientGeometry::Linear {
                    start: point(0.0, 0.0),
                    end: point(1.0, 0.0),
                },
                spread,
                stops.clone(),
            )
            .unwrap();
            assert_rgba_near(
                &sample_gradient(&value, point(-0.25, 0.0)).unwrap(),
                [expected, expected, expected, 1.0],
            );
        }
    }

    #[test]
    fn scalar_color_ramp_has_exact_endpoints_and_ignores_spatial_geometry() {
        let start = linear_color([-0.5, 0.25, 2.0, 0.2]);
        let end = linear_color([3.0, -1.0, 0.5, 0.8]);
        let stops = vec![(0.0, start.clone()), (1.0, end.clone())];
        let linear = gradient(
            GradientGeometry::Linear {
                start: point(-2.0, 8.0),
                end: point(7.0, 3.0),
            },
            GradientSpread::Pad,
            stops.clone(),
        )
        .unwrap();
        let radial = gradient(
            GradientGeometry::Radial {
                center: point(200.0, -500.0),
                radius: OrderedFloat(0.125),
            },
            GradientSpread::Pad,
            stops,
        )
        .unwrap();

        assert_eq!(sample_gradient_at(&linear, 0.0).unwrap(), start);
        assert_eq!(sample_gradient_at(&linear, 1.0).unwrap(), end);
        assert_eq!(
            sample_gradient_at(&linear, 0.375).unwrap(),
            sample_gradient_at(&radial, 0.375).unwrap()
        );
    }

    #[test]
    fn duplicate_offsets_are_right_continuous_hard_stops() {
        let value = gradient(
            GradientGeometry::Linear {
                start: point(0.0, 0.0),
                end: point(1.0, 0.0),
            },
            GradientSpread::Pad,
            vec![
                (0.0, linear_color([0.0, 0.0, 0.0, 1.0])),
                (0.5, linear_color([1.0, 0.0, 0.0, 1.0])),
                (0.5, linear_color([0.0, 0.0, 1.0, 0.5])),
                (1.0, linear_color([1.0, 1.0, 1.0, 1.0])),
            ],
        )
        .unwrap();
        assert_rgba_near(
            &sample_gradient(&value, point(0.5, 0.0)).unwrap(),
            [0.0, 0.0, 1.0, 0.5],
        );
        let before = sample_gradient(&value, point(0.5 - 1.0e-6, 0.0)).unwrap();
        assert!(before.rgba()[0] > 0.999 && before.rgba()[2] < 1.0e-8);
    }

    #[test]
    fn mixed_authored_spaces_transform_before_straight_alpha_interpolation() {
        let encoded = ColorValue::new(ColorSpaceRef::srgb(), [0.5, 0.25, 0.75, 0.2]).unwrap();
        let p3 = ColorValue::new(
            ColorSpaceRef::new("display-p3").unwrap(),
            [0.75, 0.5, 0.25, 0.8],
        )
        .unwrap();
        let left = to_working_linear_srgb(&encoded).unwrap();
        let right = to_working_linear_srgb(&p3).unwrap();
        let value = gradient(
            GradientGeometry::Linear {
                start: point(0.0, 0.0),
                end: point(1.0, 0.0),
            },
            GradientSpread::Pad,
            vec![(0.0, encoded), (1.0, p3)],
        )
        .unwrap();
        let expected =
            std::array::from_fn(|index| (left.rgba()[index] + right.rgba()[index]) * 0.5);
        assert_rgba_near(&sample_gradient(&value, point(0.5, 0.0)).unwrap(), expected);
    }

    #[test]
    fn finite_tiny_and_large_geometry_avoid_squared_length_overflow() {
        for scale in [1.0e-200, 1.0e200] {
            let value = gradient(
                GradientGeometry::Linear {
                    start: point(scale, 0.0),
                    end: point(2.0 * scale, 0.0),
                },
                GradientSpread::Pad,
                vec![
                    (0.0, linear_color([0.0, 0.0, 0.0, 1.0])),
                    (1.0, linear_color([1.0, 1.0, 1.0, 1.0])),
                ],
            )
            .unwrap();
            assert_rgba_near(
                &sample_gradient(&value, point(1.5 * scale, 0.0)).unwrap(),
                [0.5, 0.5, 0.5, 1.0],
            );
        }
    }

    #[test]
    fn nonfinite_sample_position_is_rejected() {
        assert_eq!(
            sample_gradient(&GradientValue::default(), point(f64::NAN, 0.0)),
            Err(GradientSampleError::NonFinitePosition)
        );
        assert_eq!(
            sample_gradient_at(&GradientValue::default(), f64::INFINITY),
            Err(GradientSampleError::NonFiniteFactor)
        );
    }
}
