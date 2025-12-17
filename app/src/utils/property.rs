use library::model::project::property::PropertyValue;
use library::service::project_service::ProjectService;
use ordered_float::OrderedFloat;
use uuid::Uuid;

pub fn update_number_property(
    service: &ProjectService,
    comp_id: Uuid,
    track_id: Uuid,
    entity_id: Uuid,
    prop_name: &str,
    time: f64,
    value: f64,
) {
    let _ = service.update_property_or_keyframe(
        comp_id,
        track_id,
        entity_id,
        prop_name,
        time,
        PropertyValue::Number(OrderedFloat(value)),
        None,
    );
}

pub fn update_string_property(
    service: &ProjectService,
    comp_id: Uuid,
    track_id: Uuid,
    entity_id: Uuid,
    prop_name: &str,
    time: f64,
    value: String,
) {
    let _ = service.update_property_or_keyframe(
        comp_id,
        track_id,
        entity_id,
        prop_name,
        time,
        PropertyValue::String(value),
        None,
    );
}

pub fn update_property(
    service: &ProjectService,
    comp_id: Uuid,
    track_id: Uuid,
    entity_id: Uuid,
    prop_name: &str,
    time: f64,
    value: PropertyValue,
) {
    let _ = service
        .update_property_or_keyframe(comp_id, track_id, entity_id, prop_name, time, value, None);
}
