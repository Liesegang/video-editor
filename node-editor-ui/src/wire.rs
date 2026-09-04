//! Stateless geometry for cubic Bezier node wires.

use egui::{Pos2, Vec2};

const DISTANCE_SEGMENTS: u16 = 32;
const INTERSECTION_SEGMENTS: u16 = 48;
const SEGMENT_EPSILON: f32 = 1.0e-4;
const BODY_SELECTION_PARAMETER: f32 = 0.5;
const BODY_HIT_SCREEN_RADIUS: f32 = 8.0;
const RECONNECT_HANDLE_SCREEN_OFFSET: f32 = 11.0;
const RECONNECT_HANDLE_HIT_SCREEN_RADIUS: f32 = 7.0;

/// A rendered cubic Bezier wire in canvas coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicBezier {
    start: Pos2,
    control_a: Pos2,
    control_b: Pos2,
    end: Pos2,
}

/// Which part of a rendered wire owns a direct manipulation gesture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitRegion {
    Start,
    Body,
    End,
}

/// Endpoint of an existing wire being moved by a reconnect gesture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconnectEndpoint {
    Source,
    Target,
}

/// Canonical direct-manipulation geometry for one wire.
///
/// All positions and radii are in graph space. Painting, hit testing, and
/// host-side accessibility/QA projection must derive their targets from this
/// value so zoomed or curved wires do not acquire parallel interaction math.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WireInteractionGeometry {
    body_selection_anchor: Pos2,
    body_hit_radius: f32,
    source_reconnect_handle: Pos2,
    target_reconnect_handle: Pos2,
    reconnect_handle_hit_radius: f32,
}

impl WireInteractionGeometry {
    pub const fn body_selection_anchor(self) -> Pos2 {
        self.body_selection_anchor
    }

    pub const fn body_hit_radius(self) -> f32 {
        self.body_hit_radius
    }

    pub const fn reconnect_handle(self, endpoint: ReconnectEndpoint) -> Pos2 {
        match endpoint {
            ReconnectEndpoint::Source => self.source_reconnect_handle,
            ReconnectEndpoint::Target => self.target_reconnect_handle,
        }
    }

    pub const fn reconnect_handle_hit_radius(self) -> f32 {
        self.reconnect_handle_hit_radius
    }
}

impl CubicBezier {
    /// Creates a curve from its two endpoints and two cubic control points.
    pub const fn new(start: Pos2, control_a: Pos2, control_b: Pos2, end: Pos2) -> Self {
        Self {
            start,
            control_a,
            control_b,
            end,
        }
    }

    /// Returns the point at normalized curve parameter `t`.
    pub fn point(self, t: f32) -> Pos2 {
        let one_minus_t = 1.0 - t;
        let weights = [
            one_minus_t.powi(3),
            3.0 * one_minus_t.powi(2) * t,
            3.0 * one_minus_t * t.powi(2),
            t.powi(3),
        ];
        Pos2::new(
            self.start.x * weights[0]
                + self.control_a.x * weights[1]
                + self.control_b.x * weights[2]
                + self.end.x * weights[3],
            self.start.y * weights[0]
                + self.control_a.y * weights[1]
                + self.control_b.y * weights[2]
                + self.end.y * weights[3],
        )
    }

    /// Approximates the shortest canvas distance from `point` to the wire.
    pub fn distance_to(self, point: Pos2) -> f32 {
        let mut previous = self.start;
        let mut distance = f32::INFINITY;
        for sample in 1..=DISTANCE_SEGMENTS {
            let current = self.point(f32::from(sample) / f32::from(DISTANCE_SEGMENTS));
            distance = distance.min(distance_to_segment(point, previous, current));
            previous = current;
        }
        distance
    }

    /// Tests whether a straight knife gesture touches this wire.
    pub fn intersects_segment(self, start: Pos2, end: Pos2, tolerance: f32) -> bool {
        let tolerance = tolerance.max(0.0);
        let mut previous = self.start;
        for sample in 1..=INTERSECTION_SEGMENTS {
            let current = self.point(f32::from(sample) / f32::from(INTERSECTION_SEGMENTS));
            let within_tolerance = segments_intersect(start, end, previous, current)
                || distance_to_segment(start, previous, current) <= tolerance
                || distance_to_segment(end, previous, current) <= tolerance
                || distance_to_segment(previous, start, end) <= tolerance
                || distance_to_segment(current, start, end) <= tolerance;
            if within_tolerance {
                return true;
            }
            previous = current;
        }
        false
    }

    /// Classifies an interaction as an endpoint reconnect or body disconnect.
    ///
    /// At overview scale a curve may be shorter than both endpoint radii. Each
    /// endpoint is therefore capped to the outer quarter of the endpoint span,
    /// leaving a body target in the middle.
    pub fn hit_region(self, position: Pos2, maximum_endpoint_radius: f32) -> HitRegion {
        let endpoint_radius = maximum_endpoint_radius
            .max(0.0)
            .min(self.start.distance(self.end) * 0.25);
        if position.distance(self.start) <= endpoint_radius {
            HitRegion::Start
        } else if position.distance(self.end) <= endpoint_radius {
            HitRegion::End
        } else {
            HitRegion::Body
        }
    }

    /// Resolves the wire's selection and reconnect targets for one canvas
    /// scale. Screen-space interaction sizes stay visually constant while the
    /// returned geometry remains suitable for the frame's graph-space tests.
    pub fn interaction_geometry(self, scale: f32) -> WireInteractionGeometry {
        let scale = scale.abs().max(f32::EPSILON);
        WireInteractionGeometry {
            body_selection_anchor: self.point(BODY_SELECTION_PARAMETER),
            body_hit_radius: BODY_HIT_SCREEN_RADIUS / scale,
            source_reconnect_handle: self.reconnect_handle(
                ReconnectEndpoint::Source,
                RECONNECT_HANDLE_SCREEN_OFFSET,
                scale,
            ),
            target_reconnect_handle: self.reconnect_handle(
                ReconnectEndpoint::Target,
                RECONNECT_HANDLE_SCREEN_OFFSET,
                scale,
            ),
            reconnect_handle_hit_radius: RECONNECT_HANDLE_HIT_SCREEN_RADIUS / scale,
        }
    }

    /// Places the reconnect handle a constant visual distance inside the wire.
    /// The returned coordinate remains in graph space.
    fn reconnect_handle(self, endpoint: ReconnectEndpoint, screen_offset: f32, scale: f32) -> Pos2 {
        let (edge, inward) = match endpoint {
            ReconnectEndpoint::Source => (self.start, self.control_a),
            ReconnectEndpoint::Target => (self.end, self.control_b),
        };
        let direction = inward - edge;
        let normalized = if direction.is_finite() && direction.length_sq() > f32::EPSILON {
            direction.normalized()
        } else {
            match endpoint {
                ReconnectEndpoint::Source => Vec2::X,
                ReconnectEndpoint::Target => -Vec2::X,
            }
        };
        edge + normalized * (screen_offset / scale.abs().max(f32::EPSILON))
    }

    pub(crate) const fn endpoint(self, endpoint: ReconnectEndpoint) -> Pos2 {
        match endpoint {
            ReconnectEndpoint::Source => self.start,
            ReconnectEndpoint::Target => self.end,
        }
    }
}

fn distance_to_segment(point: Pos2, start: Pos2, end: Pos2) -> f32 {
    let segment = end - start;
    let length_squared = segment.length_sq();
    if length_squared <= f32::EPSILON {
        return point.distance(start);
    }
    let t = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    point.distance(start + segment * t)
}

fn segment_orientation(start: Pos2, end: Pos2, point: Pos2) -> f32 {
    let segment = end - start;
    let offset = point - start;
    cross(segment, offset)
}

const fn cross(left: Vec2, right: Vec2) -> f32 {
    left.x * right.y - left.y * right.x
}

fn point_on_segment(point: Pos2, start: Pos2, end: Pos2) -> bool {
    segment_orientation(start, end, point).abs() <= SEGMENT_EPSILON
        && point.x >= start.x.min(end.x) - SEGMENT_EPSILON
        && point.x <= start.x.max(end.x) + SEGMENT_EPSILON
        && point.y >= start.y.min(end.y) - SEGMENT_EPSILON
        && point.y <= start.y.max(end.y) + SEGMENT_EPSILON
}

fn segments_intersect(
    left_start: Pos2,
    left_end: Pos2,
    right_start: Pos2,
    right_end: Pos2,
) -> bool {
    let left_to_right_start = segment_orientation(left_start, left_end, right_start);
    let left_to_right_end = segment_orientation(left_start, left_end, right_end);
    let right_to_left_start = segment_orientation(right_start, right_end, left_start);
    let right_to_left_end = segment_orientation(right_start, right_end, left_end);
    let crosses_both_lines = left_to_right_start * left_to_right_end < 0.0
        && right_to_left_start * right_to_left_end < 0.0;
    if crosses_both_lines {
        return true;
    }

    (left_to_right_start.abs() <= SEGMENT_EPSILON
        && point_on_segment(right_start, left_start, left_end))
        || (left_to_right_end.abs() <= SEGMENT_EPSILON
            && point_on_segment(right_end, left_start, left_end))
        || (right_to_left_start.abs() <= SEGMENT_EPSILON
            && point_on_segment(left_start, right_start, right_end))
        || (right_to_left_end.abs() <= SEGMENT_EPSILON
            && point_on_segment(left_end, right_start, right_end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_point_preserves_endpoints_and_midpoint() {
        let curve = CubicBezier::new(
            Pos2::new(0.0, 0.0),
            Pos2::new(10.0, 0.0),
            Pos2::new(20.0, 0.0),
            Pos2::new(30.0, 0.0),
        );

        assert_eq!(curve.point(0.0), Pos2::new(0.0, 0.0));
        assert_eq!(curve.point(0.5), Pos2::new(15.0, 0.0));
        assert_eq!(curve.point(1.0), Pos2::new(30.0, 0.0));
    }

    #[test]
    fn knife_detects_midspan_intersection_of_long_segments() {
        let curve = CubicBezier::new(
            Pos2::new(-1_000.0, 0.0),
            Pos2::new(-333.333_34, 0.0),
            Pos2::new(333.333_34, 0.0),
            Pos2::new(1_000.0, 0.0),
        );

        assert!(curve.intersects_segment(Pos2::new(10.0, -1_000.0), Pos2::new(10.0, 1_000.0), 3.0,));
        assert!(!curve.intersects_segment(Pos2::new(10.0, 20.0), Pos2::new(30.0, 20.0), 3.0,));
    }

    #[test]
    fn short_wire_keeps_a_body_gesture_region() {
        let curve = CubicBezier::new(
            Pos2::new(0.0, 0.0),
            Pos2::new(1.0, 0.0),
            Pos2::new(3.0, 0.0),
            Pos2::new(4.0, 0.0),
        );

        assert_eq!(curve.hit_region(Pos2::new(0.5, 0.0), 8.0), HitRegion::Start);
        assert_eq!(curve.hit_region(Pos2::new(2.0, 0.0), 8.0), HitRegion::Body);
        assert_eq!(curve.hit_region(Pos2::new(3.5, 0.0), 8.0), HitRegion::End);
    }

    #[test]
    fn interaction_geometry_uses_curve_and_scale_authoritatively() {
        let curve = CubicBezier::new(
            Pos2::new(0.0, 0.0),
            Pos2::new(0.0, 100.0),
            Pos2::new(100.0, 100.0),
            Pos2::new(100.0, 0.0),
        );

        let geometry = curve.interaction_geometry(2.0);
        assert_eq!(geometry.body_selection_anchor(), curve.point(0.5));
        assert_eq!(geometry.body_hit_radius(), 4.0);
        assert_eq!(geometry.reconnect_handle_hit_radius(), 3.5);
        assert_eq!(
            geometry.reconnect_handle(ReconnectEndpoint::Source),
            Pos2::new(0.0, 5.5)
        );
        assert_eq!(
            geometry.reconnect_handle(ReconnectEndpoint::Target),
            Pos2::new(100.0, 5.5)
        );
    }
}
