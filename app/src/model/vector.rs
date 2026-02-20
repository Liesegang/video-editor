use library::model::vector::{HandleType, VectorPath};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub(crate) struct VectorEditorState {
    pub(crate) path: VectorPath, // Use the library model
    #[serde(skip)]
    pub(crate) selected_point_indices: HashSet<usize>,
    #[serde(skip)]
    pub(crate) selected_handle: Option<(usize, HandleType)>, // Point Index, Handle Type
}

impl Default for VectorEditorState {
    fn default() -> Self {
        Self {
            path: VectorPath::default(),
            selected_point_indices: Default::default(),
            selected_handle: None,
        }
    }
}
