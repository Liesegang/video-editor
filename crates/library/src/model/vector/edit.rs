use super::{ControlPoint, HandleType, PointType, VectorPath};

const EPSILON: f32 = 1.0e-4;
const DEFAULT_HANDLE_FRACTION: f32 = 1.0 / 3.0;

/// Move only the requested vertices from an immutable gesture snapshot.
///
/// Handles are relative to their vertex and therefore retain their authored
/// curve when the vertex moves. Invalid indices are ignored so a stale UI
/// selection cannot corrupt a path after an external edit.
pub fn move_vertices(path: &mut VectorPath, indices: &[usize], delta: [f32; 2]) {
    for &index in indices {
        let Some(point) = path.points.get_mut(index) else {
            continue;
        };
        point.position[0] += delta[0];
        point.position[1] += delta[1];
    }
}

/// Split one authored segment without changing its geometry.
///
/// `segment_index` identifies the segment leaving that point. The last point
/// owns a segment only for a closed path. The returned index is the inserted
/// point, which lets editor surfaces move selection without re-identifying it.
pub fn insert_vertex(path: &mut VectorPath, segment_index: usize, t: f32) -> Option<usize> {
    if path.points.len() < 2 || !t.is_finite() || !(0.0..=1.0).contains(&t) {
        return None;
    }
    let next_index = if segment_index + 1 < path.points.len() {
        segment_index + 1
    } else if path.is_closed && segment_index + 1 == path.points.len() {
        0
    } else {
        return None;
    };
    let start = path.points[segment_index].clone();
    let end = path.points[next_index].clone();
    let inserted_index = segment_index + 1;

    if length(start.handle_out) <= EPSILON && length(end.handle_in) <= EPSILON {
        path.points.insert(
            inserted_index,
            ControlPoint {
                position: lerp(start.position, end.position, t),
                handle_in: [0.0, 0.0],
                handle_out: [0.0, 0.0],
                point_type: PointType::Corner,
            },
        );
        return Some(inserted_index);
    }

    let a = start.position;
    let b = add(a, start.handle_out);
    let d = end.position;
    let c = add(d, end.handle_in);
    let ab = lerp(a, b, t);
    let bc = lerp(b, c, t);
    let cd = lerp(c, d, t);
    let abc = lerp(ab, bc, t);
    let bcd = lerp(bc, cd, t);
    let split = lerp(abc, bcd, t);

    path.points[segment_index].handle_out = subtract(ab, a);
    path.points[next_index].handle_in = subtract(cd, d);
    path.points.insert(
        inserted_index,
        ControlPoint {
            position: split,
            handle_in: subtract(abc, split),
            handle_out: subtract(bcd, split),
            point_type: PointType::Smooth,
        },
    );
    Some(inserted_index)
}

/// Move one Bezier handle to a relative vector and enforce its node mode.
///
/// `break_coupling` is the direct-selection Alt/Option gesture: it converts
/// the point to a cusp while preserving the untouched opposite handle.
pub fn move_handle(
    point: &mut ControlPoint,
    handle: HandleType,
    relative: [f32; 2],
    break_coupling: bool,
) {
    if handle == HandleType::Vertex {
        return;
    }

    if break_coupling {
        point.point_type = PointType::Corner;
    }

    let opposite = match handle {
        HandleType::In => {
            point.handle_in = relative;
            point.handle_out
        }
        HandleType::Out => {
            point.handle_out = relative;
            point.handle_in
        }
        HandleType::Vertex => return,
    };

    let coupled = match point.point_type {
        PointType::Corner => None,
        PointType::Smooth => {
            let opposite_length = length(opposite);
            let active_length = length(relative);
            let wanted_length = if opposite_length > EPSILON {
                opposite_length
            } else {
                active_length
            };
            opposite_direction(relative, wanted_length)
        }
        PointType::Symmetric => Some([-relative[0], -relative[1]]),
    };

    if let Some(coupled) = coupled {
        match handle {
            HandleType::In => point.handle_out = coupled,
            HandleType::Out => point.handle_in = coupled,
            HandleType::Vertex => {}
        }
    }
}

/// Change selected point modes without moving vertices or unrelated points.
///
/// Degenerate line nodes receive useful tangent handles when becoming Smooth
/// or Symmetric. Existing curves keep their authored handle lengths whenever
/// the requested mode permits it.
pub fn set_point_type(path: &mut VectorPath, indices: &[usize], point_type: PointType) {
    for &index in indices {
        if index >= path.points.len() {
            continue;
        }
        match point_type {
            PointType::Corner => make_corner(&mut path.points[index]),
            PointType::Smooth => make_smooth(path, index),
            PointType::Symmetric => make_symmetric(path, index),
        }
    }
}

fn make_corner(point: &mut ControlPoint) {
    // The SVG path string is the authoritative persisted representation and
    // has no separate handle-link flag. Converting a linked node to a sharp
    // corner therefore collapses its handles so the mode survives a
    // serialize/parse round trip. An existing cusp already encodes Corner via
    // non-collinear handles and is left intact.
    if point.point_type != PointType::Corner {
        point.handle_in = [0.0, 0.0];
        point.handle_out = [0.0, 0.0];
    }
    point.point_type = PointType::Corner;
}

fn make_smooth(path: &mut VectorPath, index: usize) {
    let fallback = fallback_handles(path, index);
    let point = &mut path.points[index];
    let in_length = length(point.handle_in);
    let out_length = length(point.handle_out);

    let direction = authored_tangent(point)
        .or_else(|| fallback.map(|handles| normalize(handles.1)))
        .unwrap_or([1.0, 0.0]);
    let fallback = fallback.unwrap_or(([-32.0, 0.0], [32.0, 0.0]));
    let wanted_in = if in_length > EPSILON {
        in_length
    } else {
        length(fallback.0)
    };
    let wanted_out = if out_length > EPSILON {
        out_length
    } else {
        length(fallback.1)
    };

    point.handle_in = [-direction[0] * wanted_in, -direction[1] * wanted_in];
    point.handle_out = [direction[0] * wanted_out, direction[1] * wanted_out];
    point.point_type = PointType::Smooth;
}

fn make_symmetric(path: &mut VectorPath, index: usize) {
    let fallback = fallback_handles(path, index);
    let point = &mut path.points[index];
    let direction = authored_tangent(point)
        .or_else(|| fallback.map(|handles| normalize(handles.1)))
        .unwrap_or([1.0, 0.0]);
    let authored_length = length(point.handle_in).max(length(point.handle_out));
    let fallback_length = fallback
        .map(|handles| length(handles.0).max(length(handles.1)))
        .unwrap_or(32.0);
    let wanted_length = if authored_length > EPSILON {
        authored_length
    } else {
        fallback_length
    };

    point.handle_out = [direction[0] * wanted_length, direction[1] * wanted_length];
    point.handle_in = [-point.handle_out[0], -point.handle_out[1]];
    point.point_type = PointType::Symmetric;
}

fn authored_tangent(point: &ControlPoint) -> Option<[f32; 2]> {
    if length(point.handle_out) > EPSILON {
        Some(normalize(point.handle_out))
    } else if length(point.handle_in) > EPSILON {
        let incoming = normalize(point.handle_in);
        Some([-incoming[0], -incoming[1]])
    } else {
        None
    }
}

fn fallback_handles(path: &VectorPath, index: usize) -> Option<([f32; 2], [f32; 2])> {
    let point = path.points.get(index)?;
    let previous = previous_point(path, index);
    let next = next_point(path, index);
    let tangent = match (previous, next) {
        (Some(previous), Some(next)) => normalize(subtract(next.position, previous.position)),
        (Some(previous), None) => normalize(subtract(point.position, previous.position)),
        (None, Some(next)) => normalize(subtract(next.position, point.position)),
        (None, None) => return None,
    };
    if length(tangent) <= EPSILON {
        return None;
    }

    let incoming_length = previous
        .map(|previous| distance(previous.position, point.position) * DEFAULT_HANDLE_FRACTION)
        .or_else(|| {
            next.map(|next| distance(next.position, point.position) * DEFAULT_HANDLE_FRACTION)
        })?;
    let outgoing_length = next
        .map(|next| distance(next.position, point.position) * DEFAULT_HANDLE_FRACTION)
        .unwrap_or(incoming_length);
    Some((
        [-tangent[0] * incoming_length, -tangent[1] * incoming_length],
        [tangent[0] * outgoing_length, tangent[1] * outgoing_length],
    ))
}

fn previous_point(path: &VectorPath, index: usize) -> Option<&ControlPoint> {
    if index > 0 {
        path.points.get(index - 1)
    } else if path.is_closed {
        path.points.last()
    } else {
        None
    }
}

fn next_point(path: &VectorPath, index: usize) -> Option<&ControlPoint> {
    path.points.get(index + 1).or_else(|| {
        if path.is_closed {
            path.points.first()
        } else {
            None
        }
    })
}

fn opposite_direction(vector: [f32; 2], wanted_length: f32) -> Option<[f32; 2]> {
    let vector_length = length(vector);
    (vector_length > EPSILON).then(|| {
        [
            -vector[0] / vector_length * wanted_length,
            -vector[1] / vector_length * wanted_length,
        ]
    })
}

fn subtract(left: [f32; 2], right: [f32; 2]) -> [f32; 2] {
    [left[0] - right[0], left[1] - right[1]]
}

fn add(left: [f32; 2], right: [f32; 2]) -> [f32; 2] {
    [left[0] + right[0], left[1] + right[1]]
}

fn lerp(start: [f32; 2], end: [f32; 2], t: f32) -> [f32; 2] {
    [
        start[0] + (end[0] - start[0]) * t,
        start[1] + (end[1] - start[1]) * t,
    ]
}

fn distance(left: [f32; 2], right: [f32; 2]) -> f32 {
    length(subtract(left, right))
}

fn length(vector: [f32; 2]) -> f32 {
    vector[0].hypot(vector[1])
}

fn normalize(vector: [f32; 2]) -> [f32; 2] {
    let vector_length = length(vector);
    if vector_length <= EPSILON {
        [0.0, 0.0]
    } else {
        [vector[0] / vector_length, vector[1] / vector_length]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f32, y: f32) -> ControlPoint {
        ControlPoint {
            position: [x, y],
            handle_in: [0.0, 0.0],
            handle_out: [0.0, 0.0],
            point_type: PointType::Corner,
        }
    }

    #[test]
    fn vertex_move_changes_only_selected_positions_and_preserves_handles() {
        let mut path = VectorPath {
            points: vec![point(0.0, 0.0), point(10.0, 10.0), point(20.0, 20.0)],
            is_closed: false,
        };
        path.points[1].handle_in = [-3.0, 1.0];
        path.points[1].handle_out = [4.0, -2.0];
        let original = path.clone();

        move_vertices(&mut path, &[1], [7.0, -5.0]);

        assert_eq!(path.points[0].position, original.points[0].position);
        assert_eq!(path.points[2].position, original.points[2].position);
        assert_eq!(path.points[1].position, [17.0, 5.0]);
        assert_eq!(path.points[1].handle_in, original.points[1].handle_in);
        assert_eq!(path.points[1].handle_out, original.points[1].handle_out);
    }

    #[test]
    fn smooth_handle_keeps_opposite_length_but_links_tangent() {
        let mut point = point(0.0, 0.0);
        point.point_type = PointType::Smooth;
        point.handle_in = [-5.0, 0.0];
        point.handle_out = [12.0, 0.0];

        move_handle(&mut point, HandleType::Out, [0.0, 20.0], false);

        assert_eq!(point.handle_out, [0.0, 20.0]);
        assert!((point.handle_in[0] - 0.0).abs() < EPSILON);
        assert!((point.handle_in[1] + 5.0).abs() < EPSILON);
    }

    #[test]
    fn symmetric_handle_mirrors_angle_and_length() {
        let mut point = point(0.0, 0.0);
        point.point_type = PointType::Symmetric;

        move_handle(&mut point, HandleType::In, [-8.0, 3.0], false);

        assert_eq!(point.handle_in, [-8.0, 3.0]);
        assert_eq!(point.handle_out, [8.0, -3.0]);
    }

    #[test]
    fn break_coupling_converts_to_corner_without_touching_opposite_handle() {
        let mut point = point(0.0, 0.0);
        point.point_type = PointType::Symmetric;
        point.handle_in = [-5.0, 0.0];
        point.handle_out = [5.0, 0.0];

        move_handle(&mut point, HandleType::Out, [3.0, 9.0], true);

        assert_eq!(point.point_type, PointType::Corner);
        assert_eq!(point.handle_in, [-5.0, 0.0]);
        assert_eq!(point.handle_out, [3.0, 9.0]);
    }

    #[test]
    fn mode_switch_initializes_degenerate_handles_without_moving_vertices() {
        let mut path = VectorPath {
            points: vec![point(0.0, 0.0), point(30.0, 20.0), point(90.0, 0.0)],
            is_closed: false,
        };
        let positions = path
            .points
            .iter()
            .map(|point| point.position)
            .collect::<Vec<_>>();

        set_point_type(&mut path, &[1], PointType::Symmetric);

        assert_eq!(
            path.points
                .iter()
                .map(|point| point.position)
                .collect::<Vec<_>>(),
            positions
        );
        assert_eq!(path.points[1].point_type, PointType::Symmetric);
        assert_eq!(
            path.points[1].handle_in,
            [-path.points[1].handle_out[0], -path.points[1].handle_out[1]]
        );
        assert!(length(path.points[1].handle_out) > EPSILON);
    }

    #[test]
    fn insert_vertex_splits_line_and_closed_last_segment() {
        let mut open = VectorPath {
            points: vec![point(0.0, 0.0), point(40.0, 20.0)],
            is_closed: false,
        };
        assert_eq!(insert_vertex(&mut open, 0, 0.25), Some(1));
        assert_eq!(open.points[1].position, [10.0, 5.0]);
        assert_eq!(insert_vertex(&mut open, 2, 0.5), None);

        let mut closed = VectorPath {
            points: vec![point(0.0, 0.0), point(40.0, 0.0), point(40.0, 40.0)],
            is_closed: true,
        };
        assert_eq!(insert_vertex(&mut closed, 2, 0.5), Some(3));
        assert_eq!(closed.points[3].position, [20.0, 20.0]);
    }

    #[test]
    fn insert_vertex_preserves_cubic_de_casteljau_controls() {
        let mut path = VectorPath {
            points: vec![point(0.0, 0.0), point(90.0, 0.0)],
            is_closed: false,
        };
        path.points[0].handle_out = [30.0, 60.0];
        path.points[1].handle_in = [-30.0, 60.0];

        assert_eq!(insert_vertex(&mut path, 0, 0.5), Some(1));
        assert_eq!(path.points[0].handle_out, [15.0, 30.0]);
        assert_eq!(path.points[1].position, [45.0, 45.0]);
        assert_eq!(path.points[1].handle_in, [-15.0, 0.0]);
        assert_eq!(path.points[1].handle_out, [15.0, 0.0]);
        assert_eq!(path.points[2].handle_in, [-15.0, 30.0]);
    }
}
