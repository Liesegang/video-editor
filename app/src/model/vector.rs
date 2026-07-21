use library::model::vector::{HandleType, VectorPath};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub struct VectorDragGesture {
    pub target: (usize, HandleType),
    pub original_path: VectorPath,
    pub selected_indices: Vec<usize>,
    pub pointer_start_screen: [f32; 2],
    pub pointer_start_local: [f32; 2],
    pub world_to_local: library::rendering::renderer::Affine2D,
    pub break_coupling: bool,
    pub create_handles: bool,
    pub changed: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct VectorEditorState {
    #[serde(skip)]
    pub selected_point_indices: HashSet<usize>,
    #[serde(skip)]
    /// Handle which currently owns a pointer gesture.
    pub selected_handle: Option<(usize, HandleType)>,
    #[serde(skip)]
    /// Last directly selected handle, retained after release for clear paint.
    pub focused_handle: Option<(usize, HandleType)>,
    #[serde(skip)]
    pub drag: Option<VectorDragGesture>,
}

impl VectorEditorState {
    pub fn cancel_drag(&mut self) {
        self.selected_handle = None;
        self.drag = None;
    }

    pub fn has_changed_drag(&self) -> bool {
        self.drag.as_ref().is_some_and(|drag| drag.changed)
    }
}
