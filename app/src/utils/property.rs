use library::model::project::property::PropertyValue;
use library::EditorService as ProjectService;
use ordered_float::OrderedFloat;
use uuid::Uuid;

fn get_local_time(
    service: &ProjectService,
    comp_id: Uuid,
    _track_id: Uuid,
    entity_id: Uuid,
    global_time: f64,
) -> f64 {
    if let Ok(project) = service.get_project().read() {
        if let Some(comp) = project.get_composition(comp_id) {
            // Use flat lookup from project.get_clip() instead of nested track traversal
            if let Some(clip) = project.get_clip(entity_id) {
                let fps = comp.fps;
                let current_frame = (global_time * fps).round() as i64;
                let delta_frames = current_frame - clip.in_frame as i64;
                let time_offset = (clip.source_begin_frame as f64) / clip.fps;
                let delta_seconds = delta_frames as f64 / fps;
                let local_time = time_offset + delta_seconds;
                return local_time;
            }
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
