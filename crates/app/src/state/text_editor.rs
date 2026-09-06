//! Transient state for direct Text editing in Preview.
//!
//! The buffer is a visual projection only. The authoritative Timeline source
//! changes once, when the edit is accepted, so one typing session is one Undo
//! step and never creates a parallel persisted model.

use library::model::authoring::{InstancePath, ProjectRevision, TimelineId, TimelineItemId};

use super::authoring::AuthoringSelection;

/// A Text-tool intent waiting for the matching Preview geometry. Keeping the
/// Composition point preserves the click even if the camera moves meanwhile.
#[derive(Clone, Debug)]
pub(crate) struct TextToolClick {
    pub position: egui::Pos2,
    pub revision: ProjectRevision,
    pub timeline_id: TimelineId,
    pub instance_path: Option<InstancePath>,
    pub frame_number: i64,
    pub selection: Option<AuthoringSelection>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TextEditorState {
    pub target_item: Option<TimelineItemId>,
    pub target_revision: Option<ProjectRevision>,
    pub original: String,
    pub buffer: String,
    pub editing: bool,
    pub request_focus: bool,
    /// Last evaluated edit region in Composition coordinates. This belongs
    /// only to the typing session, not the object's visual bounds or picking.
    pub layout_bounds: Option<egui::Rect>,
    pub pending_click: Option<TextToolClick>,
}

impl TextEditorState {
    pub fn begin(&mut self, item_id: TimelineItemId, revision: ProjectRevision, text: &str) {
        self.target_item = Some(item_id);
        self.target_revision = Some(revision);
        self.original.clear();
        self.original.push_str(text);
        self.buffer.clear();
        self.buffer.push_str(text);
        self.editing = true;
        self.request_focus = true;
        self.layout_bounds = None;
        self.pending_click = None;
    }

    pub fn changed(&self) -> bool {
        self.editing && self.buffer != self.original
    }

    pub fn finish(&mut self) {
        self.target_item = None;
        self.target_revision = None;
        self.original.clear();
        self.buffer.clear();
        self.editing = false;
        self.request_focus = false;
        self.layout_bounds = None;
        self.pending_click = None;
    }
}
