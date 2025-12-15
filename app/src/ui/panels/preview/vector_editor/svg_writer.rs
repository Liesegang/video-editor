use crate::model::vector::VectorEditorState;

pub fn to_svg_path(state: &VectorEditorState) -> String {
    if state.points.is_empty() {
        return String::new();
    }

    let mut path = String::new();

    let first = &state.points[0];
    path.push_str(&format!("M {},{} ", first.position[0], first.position[1]));

    for i in 0..state.points.len() {
        let current = &state.points[i];
        let next_idx = (i + 1) % state.points.len();

        if !state.is_closed && i == state.points.len() - 1 {
            break;
        }

        let next = &state.points[next_idx];

        if is_zero(current.handle_out) && is_zero(next.handle_in) {
            path.push_str(&format!("L {},{} ", next.position[0], next.position[1]));
        } else {
            let c1x = current.position[0] + current.handle_out[0];
            let c1y = current.position[1] + current.handle_out[1];

            let c2x = next.position[0] + next.handle_in[0];
            let c2y = next.position[1] + next.handle_in[1];

            path.push_str(&format!(
                "C {},{} {},{} {},{} ",
                c1x, c1y, c2x, c2y, next.position[0], next.position[1]
            ));
        }
    }

    if state.is_closed {
        path.push_str("Z");
    }

    path
}

fn is_zero(v: [f32; 2]) -> bool {
    v[0].abs() < 0.001 && v[1].abs() < 0.001
}
