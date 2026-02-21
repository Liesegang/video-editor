use library::model::project::property::PropertyValue;
use uuid::Uuid;

pub(super) enum PreviewAction {
    UpdateProperty {
        comp_id: Uuid,
        track_id: Uuid,
        entity_id: Uuid,
        prop_name: String,
        time: f64,
        value: PropertyValue,
    },
    /// Update a property on a graph node (e.g. compositing.transform).
    UpdateGraphNodeProperty {
        node_id: Uuid,
        prop_name: String,
        time: f64,
        value: PropertyValue,
    },
}
