use library::model::project::project::{Composition, Project};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::context_types::{
    GraphEditorState, InteractionState, SelectionState, TimelineState, ViewState,
};

#[derive(Serialize, Deserialize)]
pub struct EditorContext {
    pub timeline: TimelineState,
    pub view: ViewState,
    pub selection: SelectionState,
    // Added graph_editor state
    pub graph_editor: GraphEditorState,

    #[serde(skip)]
    pub interaction: InteractionState,

    #[serde(skip)]
    pub preview_texture: Option<egui::TextureHandle>,
    #[serde(skip)]
    pub preview_texture_id: Option<u32>, // Raw GL texture ID
}

pub use crate::state::context_types::GizmoState; // Re-export for compatibility if needed, though better to import from context_types

impl EditorContext {
    pub fn new(default_comp_id: Uuid) -> Self {
        let mut selection = SelectionState::default();
        selection.composition_id = Some(default_comp_id);

        Self {
            timeline: TimelineState::default(),
            view: ViewState::default(),
            selection,
            graph_editor: GraphEditorState::default(),
            interaction: InteractionState::default(),
            preview_texture: None,
            preview_texture_id: None,
        }
    }

    pub fn get_current_composition<'a>(&self, project: &'a Project) -> Option<&'a Composition> {
        self.selection
            .composition_id
            .and_then(|id| project.compositions.iter().find(|&c| c.id == id))
    }

    pub fn select_clip(&mut self, entity_id: Uuid, track_id: Uuid) {
        self.selection.entity_id = Some(entity_id);
        self.selection.track_id = Some(track_id);
    }
}
