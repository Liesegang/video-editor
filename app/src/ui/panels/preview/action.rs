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
}
