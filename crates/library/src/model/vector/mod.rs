use serde::{Deserialize, Serialize};

mod edit;
mod path_value;

pub use edit::{insert_vertex, move_handle, move_vertices, set_point_type};
pub use path_value::{PathProjectionError, project_path_contour, replace_path_contour};

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
pub enum HandleType {
    In,
    Out,
    Vertex,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum PointType {
    Corner,
    /// Handles share one tangent, while retaining independent lengths.
    Smooth,
    /// Handles are mirrored in both angle and length.
    Symmetric,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct ControlPoint {
    pub position: [f32; 2],
    pub handle_in: [f32; 2],  // Relative to position
    pub handle_out: [f32; 2], // Relative to position
    pub point_type: PointType,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct VectorPath {
    pub points: Vec<ControlPoint>,
    pub is_closed: bool,
}
