use library::model::frame::frame::Region;
use library::model::project::{Composition, Project};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::context_types::{
    GraphEditorState, InteractionState, KeyframeDialogState, NodeEditorState, SelectionState,
    SelectionTarget, TimelineState, ViewState,
};

#[derive(Serialize, Deserialize)]
pub struct EditorContext {
    pub timeline: TimelineState,
    pub view: ViewState,
    /// Composition currently shown by Timeline/Node Editor/Preview.
    /// This navigation context is independent from the typed entity selection.
    pub active_composition_id: Option<Uuid>,
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
    /// Authoritative evaluation that produced the currently displayed
    /// preview. Interaction is derived from this exact frame, never by
    /// resolving the Project graph a second time in the UI.
    #[serde(skip)]
    pub preview_frame_info: Option<library::model::frame::frame::FrameInfo>,

    #[serde(skip)]
    pub available_fonts: Vec<String>,
}

// use crate::state::context_types::GizmoState; // Re-export for compatibility if needed, though better to import from context_types

impl EditorContext {
    pub fn new(default_comp_id: Uuid) -> Self {
        Self {
            timeline: TimelineState::default(),
            view: ViewState::default(),
            active_composition_id: Some(default_comp_id),
            selection: SelectionState::default(),
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
            preview_frame_info: None,
            available_fonts: Vec::new(),
        }
    }

    pub fn get_current_composition<'a>(&self, project: &'a Project) -> Option<&'a Composition> {
        self.active_composition_id
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
            .active_composition_id
            .filter(|id| project.get_composition(*id).is_some())
            .or_else(|| {
                project
                    .compositions
                    .first()
                    .map(|composition| composition.id)
            });
        self.active_composition_id = composition_id;

        self.reconcile_selection(project);

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
        self.invalidate_composition_scoped_transients();
        // A replacement Project invalidates the edit baseline itself. Normal
        // composition navigation keeps this pending so Node Editor can flush
        // its already-applied edit into history on the next frame.
        self.node_editor_state.pending_continuous_edit = None;
    }

    pub fn activate_composition(&mut self, composition_id: Option<Uuid>) -> bool {
        if self.active_composition_id == composition_id {
            return false;
        }
        self.active_composition_id = composition_id;
        self.clear_selection();
        self.invalidate_composition_scoped_transients();
        true
    }

    fn invalidate_composition_scoped_transients(&mut self) {
        self.timeline.is_playing = false;
        self.timeline.playback_accumulator = 0.0;
        self.graph_editor.clear_target();
        self.keyframe_dialog = KeyframeDialogState::default();

        self.interaction.dragged_item = None;
        self.interaction.dragged_entity_original_track_id = None;
        self.interaction.dragged_entity_hovered_track_id = None;
        self.interaction.dragged_entity_has_moved = false;
        self.interaction.timeline_track_reorder = None;
        self.interaction.is_resizing_entity = false;
        self.interaction.is_moving_selected_entity = false;
        self.interaction.gizmo_state = None;
        self.interaction.vector_editor_state = None;
        self.interaction.body_drag_state = None;
        self.interaction.timeline_selection_drag_start = None;
        self.interaction.preview_selection_drag_start = None;
        self.interaction.preview_edit_target = None;
        self.interaction.handled_hand_tool_drag = false;
        self.interaction.preview_viewport.request_fit();
        self.interaction.editing_text_entity_id = None;
        self.interaction.text_edit_buffer.clear();
        self.interaction.bounds_cache.bounds.clear();
        self.interaction.selected_keyframe = None;
        self.interaction.editing_keyframe = None;
        self.interaction.current_time_text_input.clear();
        self.interaction.is_editing_current_time = false;
        self.interaction.context_menu_open_pos = None;
        self.interaction.renaming_track_id = None;
        self.interaction.rename_buffer.clear();

        self.node_editor_context_menu = None;
        self.node_editor_state.pending_navigation = None;
        self.node_editor_state.layout_changed_during_drag = false;
        self.node_editor_state.node_reparent = None;
        self.node_editor_state.moved_node_ids.clear();
        self.node_editor_state.container_resize = None;
        self.node_editor_state.canvas_marquee = None;
        self.node_editor_state.active_drag_selection = None;
        self.node_editor_state.node_editor_canvas_transform = None;
        self.node_editor_state.selected_connection_id = None;
        self.node_editor_state.wire_gesture = None;
        self.node_editor_state.normal_wire_drag_active = false;
        self.node_editor_state.normal_connect_gesture = None;
        self.node_editor_state.normal_connect_cancel_pending_release = false;
        self.node_editor_state.wire_knife = None;
        self.node_editor_state.wire_context_menu = None;
        self.node_editor_state.merge_layer_reorder = None;

        self.preview_texture = None;
        self.preview_texture_id = None;
        self.preview_texture_width = 0;
        self.preview_texture_height = 0;
        self.preview_nontransparent_pixels = None;
        self.preview_pixel_hash = None;
        self.preview_region = None;
        self.preview_frame_info = None;
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
        self.interaction.preview_edit_target = None;
    }

    pub fn select_target(&mut self, target: SelectionTarget) {
        self.selection.replace([target], Some(target));
        self.interaction.preview_edit_target = None;
    }

    pub fn replace_selection(
        &mut self,
        targets: impl IntoIterator<Item = SelectionTarget>,
        primary: Option<SelectionTarget>,
    ) {
        self.selection.replace(targets, primary);
        self.interaction.preview_edit_target = None;
    }

    pub fn add_selection(&mut self, target: SelectionTarget) {
        self.selection.push_primary(target);
        self.interaction.preview_edit_target = None;
    }

    pub fn set_primary_selection(&mut self, target: SelectionTarget) -> bool {
        let changed = self.selection.make_primary(target);
        if changed {
            self.interaction.preview_edit_target = None;
        }
        changed
    }

    pub fn remove_selection(&mut self, target: SelectionTarget) -> bool {
        let changed = self.selection.remove(target);
        if changed {
            self.interaction.preview_edit_target = None;
        }
        changed
    }

    pub fn toggle_selection(&mut self, target: SelectionTarget) {
        if !self.remove_selection(target) {
            self.add_selection(target);
        }
    }

    pub fn is_selected(&self, target: SelectionTarget) -> bool {
        self.selection.contains(target)
    }

    /// Remove selection targets that no longer resolve to an exact Project
    /// entity in the active Composition.
    ///
    /// Unlike [`Self::reconcile_project_replacement`], this is intentionally a
    /// light-weight post-mutation check: normal delete operations must not
    /// reset view state or invalidate unrelated editor gestures. Target kind
    /// is part of identity, so a surviving Clip never keeps a deleted Node
    /// with the same UUID alive (or vice versa).
    pub fn reconcile_selection(&mut self, project: &Project) -> bool {
        let before = self.selection.targets().to_vec();
        let composition_id = self.active_composition_id;
        self.selection.retain(|target| {
            composition_id
                .is_some_and(|active| selection_target_composition(project, target) == Some(active))
        });
        let changed = self.selection.targets() != before;
        if changed {
            self.interaction.preview_edit_target = None;
        }
        changed
    }
}

fn selection_target_composition(project: &Project, target: SelectionTarget) -> Option<Uuid> {
    match target {
        SelectionTarget::Composition(id) => project.get_composition(id).map(|_| id),
        SelectionTarget::Track(id) => project
            .get_track(id)
            .and_then(|_| project.find_composition_for_track(id)),
        SelectionTarget::Clip(id) => project.get_clip(id).and_then(|_| {
            project
                .find_track_for_clip(id)
                .and_then(|track_id| project.find_composition_for_track(track_id))
        }),
        SelectionTarget::Node(id) => project.get_node(id).and_then(|_| {
            project
                .find_node_container(id)
                .and_then(|container| match container {
                    library::model::NodeContainer::Composition(id) => Some(id),
                    library::model::NodeContainer::Track(id) => {
                        project.find_composition_for_track(id)
                    }
                    library::model::NodeContainer::Clip(id) => project
                        .find_track_for_clip(id)
                        .and_then(|track_id| project.find_composition_for_track(track_id)),
                })
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::EditorContext;
    use crate::state::context_types::{NodeEditorPendingEdit, PreviewEditTarget, SelectionTarget};
    use library::model::frame::color::Color;
    use library::model::frame::frame::{FrameInfo, Region};
    use library::model::project::{Composition, NodeContainer, PortOwner, Project};
    use library::model::{Clip, Node, Track};
    use ordered_float::OrderedFloat;

    #[test]
    fn project_replacement_removes_stale_selection_and_edit_state() {
        let mut old_project = Project::new("old");
        let (old_composition, old_track) = Composition::new("old", 1920, 1080, 30.0, 10.0);
        let old_composition_id = old_composition.id;
        let old_track_id = old_track.id;
        let old_clip = Clip::new("old clip", 0.0, 5.0);
        let old_clip_id = old_clip.id;
        assert!(
            old_project.add_track(old_track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        old_project.add_clip(old_clip);
        assert!(
            old_project.add_composition(old_composition).is_ok(),
            "container structural Merge insertion must succeed"
        );
        old_project
            .attach_clip_to_track(old_track_id, old_clip_id)
            .unwrap();

        let mut context = EditorContext::new(old_composition_id);
        context.select_target(SelectionTarget::Clip(old_clip_id));
        context.timeline.current_time = 9.0;
        context.interaction.editing_text_entity_id = Some(old_clip_id);
        context.interaction.text_edit_buffer = "stale".to_string();
        context.interaction.preview_edit_target = Some(PreviewEditTarget {
            owner: SelectionTarget::Clip(old_clip_id),
            content_node_id: old_clip_id,
            spatial_node_id: Some(old_clip_id),
            instance_path: vec![old_composition_id, old_track_id, old_clip_id],
        });
        context.preview_render_revision = 37;
        context.preview_pixel_hash = Some(99);

        let mut replacement = Project::new("replacement");
        let (new_composition, new_track) = Composition::new("new", 1920, 1080, 30.0, 2.0);
        let new_composition_id = new_composition.id;
        assert!(
            replacement.add_track(new_track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            replacement.add_composition(new_composition).is_ok(),
            "container structural Merge insertion must succeed"
        );

        context.reconcile_project_replacement(&replacement);

        assert_eq!(context.active_composition_id, Some(new_composition_id));
        assert!(context.selection.targets().is_empty());
        assert_eq!(context.timeline.current_time, 2.0);
        assert_eq!(context.interaction.editing_text_entity_id, None);
        assert!(context.interaction.text_edit_buffer.is_empty());
        assert!(context.interaction.preview_edit_target.is_none());
        assert_eq!(context.preview_render_revision, 37);
        assert_eq!(context.preview_pixel_hash, None);
    }

    #[test]
    fn same_uuid_node_and_clip_are_independent_selection_targets() {
        let composition_id = uuid::Uuid::new_v4();
        let shared_id = uuid::Uuid::new_v4();
        let mut context = EditorContext::new(composition_id);

        context.add_selection(SelectionTarget::Clip(shared_id));
        context.add_selection(SelectionTarget::Node(shared_id));

        assert_eq!(context.selection.len(), 2);
        assert!(context.is_selected(SelectionTarget::Clip(shared_id)));
        assert!(context.is_selected(SelectionTarget::Node(shared_id)));
        assert_eq!(
            context.selection.primary(),
            Some(SelectionTarget::Node(shared_id))
        );

        assert!(context.remove_selection(SelectionTarget::Node(shared_id)));
        assert!(!context.is_selected(SelectionTarget::Node(shared_id)));
        assert!(context.is_selected(SelectionTarget::Clip(shared_id)));
        assert_eq!(
            context.selection.primary(),
            Some(SelectionTarget::Clip(shared_id))
        );

        context.toggle_selection(SelectionTarget::Node(shared_id));
        assert!(context.is_selected(SelectionTarget::Node(shared_id)));
        assert!(context.is_selected(SelectionTarget::Clip(shared_id)));
    }

    #[test]
    fn selection_reconciliation_removes_cascade_deleted_track_children() {
        let mut project = Project::new("cascade selection");
        let (composition, deleted_track) = Composition::new("composition", 1920, 1080, 30.0, 10.0);
        let composition_id = composition.id;
        let deleted_track_id = deleted_track.id;
        let surviving_track = Track::new("surviving track");
        let surviving_track_id = surviving_track.id;
        let deleted_clip = Clip::new("deleted clip", 0.0, 5.0);
        let deleted_clip_id = deleted_clip.id;
        let surviving_clip = Clip::new("surviving clip", 0.0, 5.0);
        let surviving_clip_id = surviving_clip.id;
        let deleted_node = Node::new_merge("deleted node");
        let deleted_node_id = deleted_node.id;
        let surviving_node = Node::new_merge("surviving node");
        let surviving_node_id = surviving_node.id;

        assert!(
            project.add_track(deleted_track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_track(surviving_track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        project.add_clip(deleted_clip);
        project.add_clip(surviving_clip);
        project.add_node(deleted_node);
        project.add_node(surviving_node);
        assert!(
            project.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );
        project
            .attach_track_to_composition(composition_id, surviving_track_id)
            .unwrap();
        project
            .attach_clip_to_track(deleted_track_id, deleted_clip_id)
            .unwrap();
        project
            .attach_clip_to_track(surviving_track_id, surviving_clip_id)
            .unwrap();
        project
            .attach_node_to_container(NodeContainer::Clip(deleted_clip_id), deleted_node_id)
            .unwrap();
        project
            .attach_node_to_container(NodeContainer::Clip(surviving_clip_id), surviving_node_id)
            .unwrap();

        let mut context = EditorContext::new(composition_id);
        context.replace_selection(
            [
                SelectionTarget::Clip(surviving_clip_id),
                SelectionTarget::Node(surviving_node_id),
                SelectionTarget::Track(deleted_track_id),
                SelectionTarget::Clip(deleted_clip_id),
                SelectionTarget::Node(deleted_node_id),
            ],
            Some(SelectionTarget::Node(deleted_node_id)),
        );

        assert!(project.remove_track(deleted_track_id).is_some());
        assert!(context.reconcile_selection(&project));
        assert_eq!(
            context.selection.targets(),
            &[
                SelectionTarget::Clip(surviving_clip_id),
                SelectionTarget::Node(surviving_node_id),
            ]
        );
        assert_eq!(
            context.selection.primary(),
            Some(SelectionTarget::Node(surviving_node_id))
        );
    }

    #[test]
    fn selection_reconciliation_uses_exact_kind_for_same_uuid() {
        let mut project = Project::new("same UUID selection");
        let (composition, track) = Composition::new("composition", 1920, 1080, 30.0, 10.0);
        let composition_id = composition.id;
        let track_id = track.id;
        let shared_id = uuid::Uuid::new_v4();
        let mut clip = Clip::new("surviving clip", 0.0, 5.0);
        clip.id = shared_id;
        let mut node = Node::new_merge("deleted node");
        node.id = shared_id;

        assert!(
            project.add_track(track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        project.add_clip(clip);
        project.add_node(node);
        assert!(
            project.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );
        project.attach_clip_to_track(track_id, shared_id).unwrap();
        project
            .attach_node_to_container(NodeContainer::Clip(shared_id), shared_id)
            .unwrap();

        let mut context = EditorContext::new(composition_id);
        context.replace_selection(
            [
                SelectionTarget::Clip(shared_id),
                SelectionTarget::Node(shared_id),
            ],
            Some(SelectionTarget::Node(shared_id)),
        );

        assert!(project.remove_node(shared_id).unwrap().is_some());
        assert!(context.reconcile_selection(&project));
        assert_eq!(
            context.selection.targets(),
            &[SelectionTarget::Clip(shared_id)]
        );
        assert_eq!(
            context.selection.primary(),
            Some(SelectionTarget::Clip(shared_id))
        );
    }

    #[test]
    fn selection_reconciliation_removes_cascade_deleted_clip_children() {
        let mut project = Project::new("clip cascade selection");
        let (composition, track) = Composition::new("composition", 1920, 1080, 30.0, 10.0);
        let composition_id = composition.id;
        let track_id = track.id;
        let deleted_clip = Clip::new("deleted clip", 0.0, 5.0);
        let deleted_clip_id = deleted_clip.id;
        let surviving_clip = Clip::new("surviving clip", 0.0, 5.0);
        let surviving_clip_id = surviving_clip.id;
        let deleted_node = Node::new_merge("deleted node");
        let deleted_node_id = deleted_node.id;
        let surviving_node = Node::new_merge("surviving node");
        let surviving_node_id = surviving_node.id;
        assert!(
            project.add_track(track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        project.add_clip(deleted_clip);
        project.add_clip(surviving_clip);
        project.add_node(deleted_node);
        project.add_node(surviving_node);
        assert!(
            project.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );
        project
            .attach_clip_to_track(track_id, deleted_clip_id)
            .unwrap();
        project
            .attach_clip_to_track(track_id, surviving_clip_id)
            .unwrap();
        project
            .attach_node_to_container(NodeContainer::Clip(deleted_clip_id), deleted_node_id)
            .unwrap();
        project
            .attach_node_to_container(NodeContainer::Clip(surviving_clip_id), surviving_node_id)
            .unwrap();

        let mut context = EditorContext::new(composition_id);
        context.replace_selection(
            [
                SelectionTarget::Node(surviving_node_id),
                SelectionTarget::Clip(deleted_clip_id),
                SelectionTarget::Node(deleted_node_id),
            ],
            Some(SelectionTarget::Node(deleted_node_id)),
        );

        assert!(project.remove_clip(deleted_clip_id).is_some());
        assert!(context.reconcile_selection(&project));
        assert_eq!(
            context.selection.targets(),
            &[SelectionTarget::Node(surviving_node_id)]
        );
        assert_eq!(
            context.selection.primary(),
            Some(SelectionTarget::Node(surviving_node_id))
        );
    }

    #[test]
    fn selection_reconciliation_removes_deleted_active_composition_tree() {
        let mut project = Project::new("composition cascade selection");
        let (composition, track) = Composition::new("composition", 1920, 1080, 30.0, 10.0);
        let composition_id = composition.id;
        let track_id = track.id;
        let clip = Clip::new("clip", 0.0, 5.0);
        let clip_id = clip.id;
        let node = Node::new_merge("node");
        let node_id = node.id;
        assert!(
            project.add_track(track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        project.add_clip(clip);
        project.add_node(node);
        assert!(
            project.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );
        project.attach_clip_to_track(track_id, clip_id).unwrap();
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
            .unwrap();

        let mut context = EditorContext::new(composition_id);
        context.replace_selection(
            [
                SelectionTarget::Composition(composition_id),
                SelectionTarget::Track(track_id),
                SelectionTarget::Clip(clip_id),
                SelectionTarget::Node(node_id),
            ],
            Some(SelectionTarget::Node(node_id)),
        );

        assert!(project.remove_composition(composition_id).is_some());
        assert!(context.reconcile_selection(&project));
        assert!(context.selection.targets().is_empty());
        assert_eq!(context.selection.primary(), None);
    }

    #[test]
    fn selection_reconciliation_with_no_active_composition_clears_missing_targets() {
        let project = Project::new("empty");
        let missing = uuid::Uuid::new_v4();
        let mut context = EditorContext::new(missing);
        context.replace_selection(
            [
                SelectionTarget::Node(missing),
                SelectionTarget::Clip(missing),
                SelectionTarget::Track(missing),
                SelectionTarget::Composition(missing),
            ],
            Some(SelectionTarget::Node(missing)),
        );
        context.active_composition_id = None;

        assert!(context.reconcile_selection(&project));
        assert!(context.selection.targets().is_empty());
        assert_eq!(context.selection.primary(), None);
    }

    #[test]
    fn activating_composition_is_navigation_not_composition_selection() {
        let first = uuid::Uuid::new_v4();
        let second = uuid::Uuid::new_v4();
        let node_id = uuid::Uuid::new_v4();
        let mut context = EditorContext::new(first);
        context.select_target(SelectionTarget::Node(node_id));

        assert!(context.activate_composition(Some(second)));

        assert_eq!(context.active_composition_id, Some(second));
        assert!(context.selection.targets().is_empty());
        assert!(!context.is_selected(SelectionTarget::Composition(second)));
    }

    #[test]
    fn activating_composition_invalidates_render_and_hit_test_caches() {
        let first = uuid::Uuid::new_v4();
        let second = uuid::Uuid::new_v4();
        let node_id = uuid::Uuid::new_v4();
        let mut context = EditorContext::new(first);
        context.preview_texture_id = Some(17);
        context.preview_texture_width = 320;
        context.preview_texture_height = 180;
        context.preview_render_revision = 9;
        context.preview_nontransparent_pixels = Some(100);
        context.preview_pixel_hash = Some(200);
        context.preview_region = Some(Region::default());
        context.preview_frame_info = Some(FrameInfo {
            width: 320,
            height: 180,
            background_color: Color::default(),
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items: Vec::new(),
        });
        context
            .interaction
            .bounds_cache
            .bounds
            .insert(node_id, (1, (0.0, 0.0, 10.0, 10.0)));
        context.interaction.timeline_selection_drag_start = Some(egui::Pos2::ZERO);
        context.interaction.preview_selection_drag_start = Some(egui::Pos2::ZERO);
        context.interaction.preview_edit_target = Some(PreviewEditTarget {
            owner: SelectionTarget::Composition(first),
            content_node_id: node_id,
            spatial_node_id: Some(node_id),
            instance_path: vec![node_id],
        });
        context.interaction.preview_viewport.auto_fit = false;
        context.interaction.preview_viewport.fitted_composition_id = Some(first);
        context.graph_editor.active_target = Some(SelectionTarget::Node(node_id));
        context
            .graph_editor
            .visible_properties
            .insert("node:opacity".to_string());
        context.node_editor_state.selected_connection_id = Some(uuid::Uuid::new_v4());
        context.node_editor_state.pending_continuous_edit = Some(NodeEditorPendingEdit {
            owner: PortOwner::Node(node_id),
            key: "opacity".to_string(),
        });

        assert!(context.activate_composition(Some(second)));

        assert_eq!(context.preview_texture_id, None);
        assert_eq!(context.preview_texture_width, 0);
        assert_eq!(context.preview_texture_height, 0);
        assert_eq!(context.preview_render_revision, 9);
        assert_eq!(context.preview_nontransparent_pixels, None);
        assert_eq!(context.preview_pixel_hash, None);
        assert_eq!(context.preview_region, None);
        assert!(context.preview_frame_info.is_none());
        assert!(context.interaction.bounds_cache.bounds.is_empty());
        assert!(context.interaction.timeline_selection_drag_start.is_none());
        assert!(context.interaction.preview_selection_drag_start.is_none());
        assert!(context.interaction.preview_edit_target.is_none());
        assert!(context.interaction.preview_viewport.auto_fit);
        assert!(context
            .interaction
            .preview_viewport
            .fitted_composition_id
            .is_none());
        assert_eq!(context.graph_editor.active_target, None);
        assert!(context.graph_editor.visible_properties.is_empty());
        assert_eq!(context.node_editor_state.selected_connection_id, None);
        assert!(context.node_editor_state.pending_continuous_edit.is_some());
    }
}
