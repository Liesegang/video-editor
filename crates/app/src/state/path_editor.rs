//! Transient state for direct canonical Path manipulation in Preview.

use std::collections::HashSet;

use library::model::authoring::{ProjectRevision, TimelineItemId};
use library::model::vector::{HandleType, VectorPath};
use library::rendering::renderer::Affine2D;

#[derive(Clone, Debug)]
pub(crate) struct PathDragGesture {
    pub target: (usize, HandleType),
    pub original_path: VectorPath,
    pub selected_indices: Vec<usize>,
    pub pointer_start_screen: [f32; 2],
    pub pointer_start_local: [f32; 2],
    pub world_to_local: Affine2D,
    pub break_coupling: bool,
    pub create_handles: bool,
    pub changed: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ShapeCreationGesture {
    pub start_world: [f32; 2],
    pub current_world: [f32; 2],
}

#[derive(Clone, Debug, Default)]
pub(crate) struct PathEditorState {
    pub target_item: Option<TimelineItemId>,
    pub target_revision: Option<ProjectRevision>,
    pub selected_point_indices: HashSet<usize>,
    /// Handle which currently owns a pointer gesture.
    pub selected_handle: Option<(usize, HandleType)>,
    /// Last directly selected handle, retained after release for clear paint.
    pub focused_handle: Option<(usize, HandleType)>,
    pub drag: Option<PathDragGesture>,
    pub shape_creation: Option<ShapeCreationGesture>,
    pub pen_points: Vec<[f32; 2]>,
}

impl PathEditorState {
    pub fn reconcile(
        &mut self,
        item_id: TimelineItemId,
        revision: ProjectRevision,
        point_count: usize,
    ) {
        if self.target_item != Some(item_id) {
            self.clear();
            self.target_item = Some(item_id);
        }
        if self.drag.is_some() && self.target_revision != Some(revision) {
            self.cancel_drag();
        }
        if self.drag.is_none() {
            self.target_revision = Some(revision);
        }
        self.selected_point_indices
            .retain(|index| *index < point_count);
        self.selected_handle = self
            .selected_handle
            .filter(|(index, _)| *index < point_count);
        self.focused_handle = self
            .focused_handle
            .filter(|(index, _)| *index < point_count);
    }

    pub fn cancel_drag(&mut self) {
        self.selected_handle = None;
        self.drag = None;
    }

    pub fn clear(&mut self) {
        self.target_item = None;
        self.target_revision = None;
        self.selected_point_indices.clear();
        self.selected_handle = None;
        self.focused_handle = None;
        self.drag = None;
        self.shape_creation = None;
        self.pen_points.clear();
    }
}
