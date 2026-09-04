//! Transient state for direct Text editing in Preview.
//!
//! The buffer is a visual projection only. The authoritative Timeline source
//! changes once, when the edit is accepted, so one typing session is one Undo
//! step and never creates a parallel persisted model.

use library::model::authoring::{ProjectRevision, TimelineItemId};

#[derive(Clone, Debug, Default)]
pub(crate) struct TextEditorState {
    pub target_item: Option<TimelineItemId>,
    pub target_revision: Option<ProjectRevision>,
    pub original: String,
    pub buffer: String,
    pub editing: bool,
    pub request_focus: bool,
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
    }
}
