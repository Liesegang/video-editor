use library::core::model::property::PropertyValue;
use uuid::Uuid;

pub enum PreviewAction {
    UpdateProperty {
        comp_id: Uuid,
        track_id: Uuid,
        entity_id: Uuid,
        prop_name: String,
        time: f64,
        value: PropertyValue,
    },
}
