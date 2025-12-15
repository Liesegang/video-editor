use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct VectorEditorState {
    pub points: Vec<ControlPoint>,
    pub is_closed: bool,
    #[serde(skip)]
    pub selected_point_indices: std::collections::HashSet<usize>,
    #[serde(skip)]
    pub selected_handle: Option<(usize, HandleType)>, // Point Index, Handle Type
}

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

impl Default for VectorEditorState {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            is_closed: false,
            selected_point_indices: Default::default(),
            selected_handle: None,
        }
    }
}
