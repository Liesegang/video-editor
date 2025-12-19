use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
pub enum HandleType {
    In,
    Out,
    Vertex,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub enum PointType {
    Corner,
    Smooth, // Linked handles, mirror angle but length can differ? Or mirror both? Typically mirror angle.
    Symmetric, // Mirror angle and length
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ControlPoint {
    pub position: [f32; 2],
    pub handle_in: [f32; 2],  // Relative to position
    pub handle_out: [f32; 2], // Relative to position
    pub point_type: PointType,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct VectorPath {
    pub points: Vec<ControlPoint>,
    pub is_closed: bool,
}
