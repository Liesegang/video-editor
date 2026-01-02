use library::model::property::PropertyValue;
use library::EditorService as ProjectService;
use ordered_float::OrderedFloat;
use uuid::Uuid;

fn get_local_time(
    service: &ProjectService,
    _comp_id: Uuid,
    _track_id: Uuid,
    entity_id: Uuid,
    global_time: f64,
) -> f64 {
    if let Ok(project) = service.get_project().read() {
        if let Some(layer) = project.get_layer(entity_id) {
            let relative_time = global_time - layer.start_time.into_inner();
            let source_time =
                relative_time * layer.time_stretch.into_inner() + layer.trim_in.into_inner();
            return source_time;
        }
    }
    global_time
}

pub fn update_number_property(
    service: &ProjectService,
    comp_id: Uuid,
    track_id: Uuid,
    entity_id: Uuid,
    prop_name: &str,
    time: f64,
    value: f64,
) {
    let local_time = get_local_time(service, comp_id, track_id, entity_id, time);
    let _ = service.update_property_or_keyframe(
        entity_id,
        prop_name,
        local_time,
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
    let local_time = get_local_time(service, comp_id, track_id, entity_id, time);
    let _ = service.update_property_or_keyframe(
        entity_id,
        prop_name,
        local_time,
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
    let local_time = get_local_time(service, comp_id, track_id, entity_id, time);
    let _ = service.update_property_or_keyframe(entity_id, prop_name, local_time, value, None);
}
