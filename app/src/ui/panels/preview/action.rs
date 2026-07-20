use crate::state::context_types::PreviewEditTarget;
use library::model::property::PropertyValue;
use uuid::Uuid;

pub enum PreviewAction {
    UpdateProperty {
        /// Exact evaluated branch that authorized this write. A UUID alone is
        /// insufficient because one Node may fan out through multiple paths.
        edit_target: PreviewEditTarget,
        node_id: Uuid,
        prop_name: String,
        time: f64,
        value: PropertyValue,
    },
    /// Commit all Project mutations queued before this action as one history
    /// state. Keeping this deferred guarantees the snapshot is taken after the
    /// authoritative Project has been updated.
    CommitHistory,
}
