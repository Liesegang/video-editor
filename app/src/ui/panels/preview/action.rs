use library::model::property::PropertyValue;
use uuid::Uuid;

pub enum PreviewAction {
    UpdateProperty {
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
