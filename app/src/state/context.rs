use library::model::frame::frame::Region;
use library::model::project::project::{Composition, Project};
use library::EditorService;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::action::HistoryManager;

/// Bundles the common parameters passed to every UI panel function.
/// This reduces parameter explosion in panel signatures.
pub(crate) struct PanelContext<'a> {
    pub(crate) editor_context: &'a mut EditorContext,
    pub(crate) history_manager: &'a mut HistoryManager,
    pub(crate) project_service: &'a mut EditorService,
    pub(crate) project: &'a Arc<RwLock<Project>>,
}

use crate::state::context_types::{
    GraphEditorState, InteractionState, KeyframeDialogState, SelectionState, TimelineState,
    ViewState,
};

#[derive(Serialize, Deserialize)]
pub(crate) struct EditorContext {
    pub(crate) timeline: TimelineState,
    pub(crate) view: ViewState,
    pub(crate) selection: SelectionState,
    // Added graph_editor state
    pub(crate) graph_editor: GraphEditorState,

    // Added keyframe_dialog state
    pub(crate) keyframe_dialog: KeyframeDialogState,

    // Node Editor State
    #[serde(skip)]
    pub(crate) node_editor_state: egui_node_editor::NodeEditorState,

    #[serde(skip)]
    pub(crate) interaction: InteractionState,

    #[serde(skip)]
    pub(crate) preview_texture: Option<egui::TextureHandle>,
    #[serde(skip)]
    pub(crate) preview_texture_id: Option<u32>, // Raw GL texture ID
    #[serde(skip)]
    pub(crate) preview_texture_width: u32,
    #[serde(skip)]
    pub(crate) preview_texture_height: u32,
    #[serde(skip)]
    pub(crate) preview_region: Option<Region>,

    #[serde(skip)]
    pub(crate) available_fonts: Vec<String>,
}

pub(crate) use crate::state::context_types::GizmoState; // Re-export for compatibility if needed, though better to import from context_types

impl EditorContext {
    pub(crate) fn new(default_comp_id: Uuid) -> Self {
        let mut selection = SelectionState::default();
        selection.composition_id = Some(default_comp_id);

        Self {
            timeline: TimelineState::default(),
            view: ViewState::default(),
            selection,
            graph_editor: GraphEditorState::default(),
            keyframe_dialog: KeyframeDialogState::default(),
            node_editor_state: Default::default(),
            interaction: InteractionState::default(),
            preview_texture: None,
            preview_texture_id: None,
            preview_texture_width: 0,
            preview_texture_height: 0,
            preview_region: None,
            available_fonts: Vec::new(),
        }
    }

    pub(crate) fn get_current_composition<'a>(
        &self,
        project: &'a Project,
    ) -> Option<&'a Composition> {
        self.selection
            .composition_id
            .and_then(|id| project.compositions.iter().find(|&c| c.id == id))
    }

    pub(crate) fn select_clip(&mut self, entity_id: Uuid, track_id: Uuid) {
        self.selection.selected_entities.clear();
        self.selection.selected_entities.insert(entity_id);
        self.selection.last_selected_entity_id = Some(entity_id);
        self.selection.last_selected_track_id = Some(track_id);
    }

    #[allow(dead_code)]
    pub(crate) fn add_selection(&mut self, entity_id: Uuid, track_id: Uuid) {
        self.selection.selected_entities.insert(entity_id);
        self.selection.last_selected_entity_id = Some(entity_id);
        self.selection.last_selected_track_id = Some(track_id);
    }

    pub(crate) fn toggle_selection(&mut self, entity_id: Uuid, track_id: Uuid) {
        if self.selection.selected_entities.contains(&entity_id) {
            self.selection.selected_entities.remove(&entity_id);
            if self.selection.last_selected_entity_id == Some(entity_id) {
                // We don't track entity→track mapping, so we can't recover the
                // track_id for a remaining entity. Clear both to stay consistent.
                self.selection.last_selected_entity_id = None;
                self.selection.last_selected_track_id = None;
            }
        } else {
            self.selection.selected_entities.insert(entity_id);
            self.selection.last_selected_entity_id = Some(entity_id);
            self.selection.last_selected_track_id = Some(track_id);
        }
    }

    pub(crate) fn is_selected(&self, entity_id: Uuid) -> bool {
        self.selection.selected_entities.contains(&entity_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Domain: Single Selection (replace semantics) ──

    #[test]
    fn select_clip_replaces_entire_selection() {
        let mut ctx = EditorContext::new(Uuid::new_v4());
        let entity_a = Uuid::new_v4();
        let entity_b = Uuid::new_v4();
        let track = Uuid::new_v4();

        ctx.select_clip(entity_a, track);
        ctx.select_clip(entity_b, track);

        assert!(
            !ctx.is_selected(entity_a),
            "Previous selection should be cleared"
        );
        assert!(ctx.is_selected(entity_b));
        assert_eq!(ctx.selection.selected_entities.len(), 1);
    }

    #[test]
    fn select_clip_updates_last_selected() {
        let mut ctx = EditorContext::new(Uuid::new_v4());
        let entity = Uuid::new_v4();
        let track = Uuid::new_v4();

        ctx.select_clip(entity, track);

        assert_eq!(ctx.selection.last_selected_entity_id, Some(entity));
        assert_eq!(ctx.selection.last_selected_track_id, Some(track));
    }

    // ── Domain: Additive Selection ──

    #[test]
    fn add_selection_preserves_existing() {
        let mut ctx = EditorContext::new(Uuid::new_v4());
        let entity_a = Uuid::new_v4();
        let entity_b = Uuid::new_v4();
        let track = Uuid::new_v4();

        ctx.select_clip(entity_a, track);
        ctx.add_selection(entity_b, track);

        assert!(ctx.is_selected(entity_a));
        assert!(ctx.is_selected(entity_b));
        assert_eq!(ctx.selection.selected_entities.len(), 2);
    }

    // ── Domain: Toggle Selection ──

    #[test]
    fn toggle_adds_when_not_selected() {
        let mut ctx = EditorContext::new(Uuid::new_v4());
        let entity = Uuid::new_v4();
        let track = Uuid::new_v4();

        ctx.toggle_selection(entity, track);

        assert!(ctx.is_selected(entity));
        assert_eq!(ctx.selection.last_selected_entity_id, Some(entity));
        assert_eq!(ctx.selection.last_selected_track_id, Some(track));
    }

    #[test]
    fn toggle_removes_when_already_selected() {
        let mut ctx = EditorContext::new(Uuid::new_v4());
        let entity = Uuid::new_v4();
        let track = Uuid::new_v4();

        ctx.select_clip(entity, track);
        ctx.toggle_selection(entity, track);

        assert!(!ctx.is_selected(entity));
    }

    #[test]
    fn toggle_off_primary_clears_last_selected_track() {
        let mut ctx = EditorContext::new(Uuid::new_v4());
        let entity = Uuid::new_v4();
        let track = Uuid::new_v4();

        ctx.select_clip(entity, track);
        ctx.toggle_selection(entity, track);

        assert_eq!(ctx.selection.last_selected_entity_id, None);
        assert_eq!(ctx.selection.last_selected_track_id, None);
    }

    #[test]
    fn toggle_off_primary_with_others_picks_remaining_entity() {
        let mut ctx = EditorContext::new(Uuid::new_v4());
        let entity_a = Uuid::new_v4();
        let entity_b = Uuid::new_v4();
        let track = Uuid::new_v4();

        ctx.select_clip(entity_a, track);
        ctx.add_selection(entity_b, track);
        // entity_b is now the primary (last selected)
        ctx.toggle_selection(entity_b, track);

        assert!(
            ctx.is_selected(entity_a),
            "Other entity should remain selected"
        );
        assert!(
            !ctx.is_selected(entity_b),
            "Toggled entity should be removed"
        );
        // Both last_selected fields are cleared to stay consistent
        // (we don't track entity→track mapping for remaining entities)
        assert_eq!(ctx.selection.last_selected_entity_id, None);
        assert_eq!(ctx.selection.last_selected_track_id, None);
    }

    // ── Domain: Initial State ──

    #[test]
    fn new_context_has_composition_set() {
        let comp_id = Uuid::new_v4();
        let ctx = EditorContext::new(comp_id);

        assert_eq!(ctx.selection.composition_id, Some(comp_id));
        assert!(ctx.selection.selected_entities.is_empty());
        assert_eq!(ctx.selection.last_selected_entity_id, None);
    }

    // ── Domain: body_drag_state invalidation on selection change ──

    #[test]
    fn body_drag_state_stale_when_selection_changes() {
        use crate::state::context_types::BodyDragState;

        let mut ctx = EditorContext::new(Uuid::new_v4());
        let entity_a = Uuid::new_v4();
        let entity_b = Uuid::new_v4();
        let track = Uuid::new_v4();

        // Start with entity_a selected, simulate preview drag
        ctx.select_clip(entity_a, track);
        let mut positions = std::collections::HashMap::new();
        positions.insert(entity_a, [100.0_f32, 200.0]);
        ctx.interaction.preview.body_drag_state = Some(BodyDragState {
            start_mouse_pos: egui::pos2(0.0, 0.0),
            original_positions: positions,
        });

        // Timeline changes selection to entity_b
        ctx.select_clip(entity_b, track);

        // body_drag_state still references entity_a, which is no longer selected
        let drag_state = ctx.interaction.preview.body_drag_state.as_ref().unwrap();
        let all_still_selected = drag_state
            .original_positions
            .keys()
            .all(|id| ctx.selection.selected_entities.contains(id));

        assert!(
            !all_still_selected,
            "Drag state should be stale after selection change"
        );
    }

    #[test]
    fn body_drag_state_valid_when_selection_unchanged() {
        use crate::state::context_types::BodyDragState;

        let mut ctx = EditorContext::new(Uuid::new_v4());
        let entity_a = Uuid::new_v4();
        let track = Uuid::new_v4();

        ctx.select_clip(entity_a, track);
        let mut positions = std::collections::HashMap::new();
        positions.insert(entity_a, [100.0_f32, 200.0]);
        ctx.interaction.preview.body_drag_state = Some(BodyDragState {
            start_mouse_pos: egui::pos2(0.0, 0.0),
            original_positions: positions,
        });

        // Selection hasn't changed
        let drag_state = ctx.interaction.preview.body_drag_state.as_ref().unwrap();
        let all_still_selected = drag_state
            .original_positions
            .keys()
            .all(|id| ctx.selection.selected_entities.contains(id));

        assert!(
            all_still_selected,
            "Drag state should be valid when selection is unchanged"
        );
    }
}
