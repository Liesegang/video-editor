use library::model::vector::HandleType;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct VectorEditorState {
    #[serde(skip)]
    pub selected_point_indices: HashSet<usize>,
    #[serde(skip)]
    pub selected_handle: Option<(usize, HandleType)>, // Point Index, Handle Type
}
