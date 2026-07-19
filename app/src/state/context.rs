use library::model::frame::frame::Region;
use library::model::project::{Composition, Project};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::context_types::{
    GraphEditorState, InteractionState, KeyframeDialogState, NodeEditorState, SelectionState,
    TimelineState, ViewState,
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

    // Node Editor view state. Project data itself is never stored here.
    #[serde(default)]
    pub node_editor_state: NodeEditorState,

    #[serde(skip)]
    pub node_editor_context_menu: Option<crate::state::context_types::ContextMenuState>,

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
    /// Increments whenever a successful preview render result is applied.
    #[serde(skip)]
    pub preview_render_revision: u64,
    /// QA-only probe of the most recently applied CPU image.
    #[serde(skip)]
    pub preview_nontransparent_pixels: Option<u64>,
    /// QA-only deterministic FNV-1a checksum of the most recent RGBA image.
    #[serde(skip)]
    pub preview_pixel_hash: Option<u64>,
    #[serde(skip)]
    pub preview_region: Option<Region>,

    #[serde(skip)]
    pub available_fonts: Vec<String>,
}

// use crate::state::context_types::GizmoState; // Re-export for compatibility if needed, though better to import from context_types

impl EditorContext {
    pub fn new(default_comp_id: Uuid) -> Self {
        let selection = SelectionState {
            composition_id: Some(default_comp_id),
            ..SelectionState::default()
        };

        Self {
            timeline: TimelineState::default(),
            view: ViewState::default(),
            selection,
            graph_editor: GraphEditorState::default(),
            keyframe_dialog: KeyframeDialogState::default(),
            node_editor_state: NodeEditorState::default(),
            node_editor_context_menu: None,
            interaction: InteractionState::default(),
            preview_texture: None,
            preview_texture_id: None,
            preview_texture_width: 0,
            preview_texture_height: 0,
            preview_render_revision: 0,
            preview_nontransparent_pixels: None,
            preview_pixel_hash: None,
            preview_region: None,
            available_fonts: Vec::new(),
        }
    }

    pub fn get_current_composition<'a>(&self, project: &'a Project) -> Option<&'a Composition> {
        self.selection
            .composition_id
            .and_then(|id| project.compositions.iter().find(|&c| c.id == id))
    }

    /// Reconcile transient editor state after the authoritative Project has
    /// been replaced by New, Load, Undo, or Redo.
    ///
    /// Project-backed data is never copied into this context. This method only
    /// removes UI references that no longer resolve and invalidates derived
    /// preview/editing caches.
    pub fn reconcile_project_replacement(&mut self, project: &Project) {
        let composition_id = self
            .selection
            .composition_id
            .filter(|id| project.get_composition(*id).is_some())
            .or_else(|| {
                project
                    .compositions
                    .first()
                    .map(|composition| composition.id)
            });
        self.selection.composition_id = composition_id;

        self.selection.selected_entities.retain(|entity_id| {
            (project.get_clip(*entity_id).is_some() || project.get_node(*entity_id).is_some())
                && project.find_containing_composition(*entity_id) == composition_id
        });
        if self
            .selection
            .last_selected_entity_id
            .is_none_or(|entity_id| !self.selection.selected_entities.contains(&entity_id))
        {
            self.selection.last_selected_entity_id =
                self.selection.selected_entities.iter().next().copied();
        }

        self.selection.last_selected_track_id = self
            .selection
            .last_selected_entity_id
            .and_then(|entity_id| project.find_parent_track(entity_id))
            .or_else(|| {
                self.selection.last_selected_track_id.filter(|track_id| {
                    project.find_composition_for_track(*track_id) == composition_id
                })
            });

        self.timeline
            .expanded_tracks
            .retain(|track_id| project.find_composition_for_track(*track_id) == composition_id);
        if let Some(composition) = composition_id.and_then(|id| project.get_composition(id)) {
            self.timeline.current_time = self
                .timeline
                .current_time
                .clamp(0.0, composition.duration.max(0.0) as f32);
        } else {
            self.timeline.current_time = 0.0;
        }
        self.timeline.is_playing = false;
        self.timeline.playback_accumulator = 0.0;

        self.interaction.dragged_entity_original_track_id = None;
        self.interaction.dragged_entity_hovered_track_id = None;
        self.interaction.dragged_entity_has_moved = false;
        self.interaction.is_resizing_entity = false;
        self.interaction.is_moving_selected_entity = false;
        self.interaction.gizmo_state = None;
        self.interaction.vector_editor_state = None;
        self.interaction.body_drag_state = None;
        self.interaction.timeline_selection_drag_start = None;
        self.interaction.preview_selection_drag_start = None;
        self.interaction.handled_hand_tool_drag = false;
        self.interaction.preview_viewport.primary_gesture =
            crate::state::context_types::PreviewPrimaryGesture::Idle;
        self.interaction.editing_text_entity_id = None;
        self.interaction.text_edit_buffer.clear();
        self.interaction.bounds_cache.bounds.clear();
        self.interaction.selected_keyframe = None;
        self.interaction.editing_keyframe = None;
        self.keyframe_dialog = KeyframeDialogState::default();
        self.node_editor_context_menu = None;
        self.node_editor_state.pending_navigation = None;
        self.node_editor_state.pending_continuous_edit = None;

        self.preview_texture = None;
        self.preview_texture_id = None;
        self.preview_texture_width = 0;
        self.preview_texture_height = 0;
        self.preview_render_revision = 0;
        self.preview_nontransparent_pixels = None;
        self.preview_pixel_hash = None;
        self.preview_region = None;
    }

    pub fn select_clip(&mut self, entity_id: Uuid, track_id: Uuid) {
        self.selection.selected_entities.clear();
        self.selection.selected_entities.insert(entity_id);
        self.selection.last_selected_entity_id = Some(entity_id);
        self.selection.last_selected_track_id = Some(track_id);
    }

    #[allow(
        dead_code,
        reason = "multi-select command path is retained for the node/timeline selection integration"
    )]
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

#[cfg(test)]
mod tests {
    use super::EditorContext;
    use library::model::project::{Composition, Project};
    use library::model::Clip;

    #[test]
    fn project_replacement_removes_stale_selection_and_edit_state() {
        let mut old_project = Project::new("old");
        let (old_composition, old_track) = Composition::new("old", 1920, 1080, 30.0, 10.0);
        let old_composition_id = old_composition.id;
        let old_track_id = old_track.id;
        let old_clip = Clip::new("old clip", 0.0, 5.0);
        let old_clip_id = old_clip.id;
        old_project.add_track(old_track);
        old_project.add_clip(old_clip);
        old_project.add_composition(old_composition);
        old_project
            .attach_clip_to_track(old_track_id, old_clip_id)
            .unwrap();

        let mut context = EditorContext::new(old_composition_id);
        context.select_clip(old_clip_id, old_track_id);
        context.timeline.current_time = 9.0;
        context.interaction.editing_text_entity_id = Some(old_clip_id);
        context.interaction.text_edit_buffer = "stale".to_string();

        let mut replacement = Project::new("replacement");
        let (new_composition, new_track) = Composition::new("new", 1920, 1080, 30.0, 2.0);
        let new_composition_id = new_composition.id;
        replacement.add_track(new_track);
        replacement.add_composition(new_composition);

        context.reconcile_project_replacement(&replacement);

        assert_eq!(context.selection.composition_id, Some(new_composition_id));
        assert!(context.selection.selected_entities.is_empty());
        assert_eq!(context.selection.last_selected_entity_id, None);
        assert_eq!(context.selection.last_selected_track_id, None);
        assert_eq!(context.timeline.current_time, 2.0);
        assert_eq!(context.interaction.editing_text_entity_id, None);
        assert!(context.interaction.text_edit_buffer.is_empty());
    }
}
