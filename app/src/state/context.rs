use library::model::frame::frame::Region;
use library::model::project::project::{Composition, Project};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::context_types::{
    GraphEditorState, InteractionState, KeyframeDialogState, SelectionState, TimelineState,
    ViewState,
};

#[derive(Serialize, Deserialize)]
pub struct EditorContext {
    pub timeline: TimelineState,
    pub view: ViewState,
    pub selection: SelectionState,
    // Added graph_editor state
    pub graph_editor: GraphEditorState,

    // Added keyframe_dialog state
    pub keyframe_dialog: KeyframeDialogState,

    // Node Editor State
    #[serde(skip)]
    // Assuming we don't need to persist this for now or implement Serialize manually
    pub node_graph_state: egui_snarl::Snarl<crate::model::node_graph::MyNodeTemplate>,

    #[serde(skip)]
    pub interaction: InteractionState,

    #[serde(skip)]
    pub preview_texture: Option<egui::TextureHandle>,
    #[serde(skip)]
    pub preview_texture_id: Option<u32>, // Raw GL texture ID
    #[serde(skip)]
    pub preview_texture_width: u32,
    #[serde(skip)]
    pub preview_texture_height: u32,
    #[serde(skip)]
    pub preview_region: Option<Region>,

    #[serde(skip)]
    pub available_fonts: Vec<String>,
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
            keyframe_dialog: KeyframeDialogState::default(),
            node_graph_state: Default::default(),
            interaction: InteractionState::default(),
            preview_texture: None,
            preview_texture_id: None,
            preview_texture_width: 0,
            preview_texture_height: 0,
            preview_region: None,
            available_fonts: Vec::new(),
        }
    }

    pub fn get_current_composition<'a>(&self, project: &'a Project) -> Option<&'a Composition> {
        self.selection
            .composition_id
            .and_then(|id| project.compositions.iter().find(|&c| c.id == id))
    }

    pub fn select_clip(&mut self, entity_id: Uuid, track_id: Uuid) {
        self.selection.selected_entities.clear();
        self.selection.selected_entities.insert(entity_id);
        self.selection.last_selected_entity_id = Some(entity_id);
        self.selection.last_selected_track_id = Some(track_id);
    }

    #[allow(dead_code)]
    pub fn add_selection(&mut self, entity_id: Uuid, track_id: Uuid) {
        self.selection.selected_entities.insert(entity_id);
        self.selection.last_selected_entity_id = Some(entity_id);
        self.selection.last_selected_track_id = Some(track_id);
    }

    pub fn toggle_selection(&mut self, entity_id: Uuid, track_id: Uuid) {
        if self.selection.selected_entities.contains(&entity_id) {
            self.selection.selected_entities.remove(&entity_id);
            if self.selection.last_selected_entity_id == Some(entity_id) {
                // If we removed the last selected (primary), just pick another arbitrary one or None
                // For valid UX, ideally we pick the previous one but we don't track history.
                // Just set to None or a random one.
                self.selection.last_selected_entity_id =
                    self.selection.selected_entities.iter().next().cloned();
                // We lose track_id context if we pick random.
                // It's acceptable for "last selected" to be None if the primary was deselected.
                self.selection.last_selected_track_id = None;
            }
        } else {
            self.selection.selected_entities.insert(entity_id);
            self.selection.last_selected_entity_id = Some(entity_id);
            self.selection.last_selected_track_id = Some(track_id);
        }
    }

    pub fn is_selected(&self, entity_id: Uuid) -> bool {
        self.selection.selected_entities.contains(&entity_id)
    }
}
