use library::model::project::Project;
use library::model::property::{PropertyTarget, PropertyValue};
use library::EditorService as ProjectService;
use library::PropertyOwner;
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
        let clip = project.get_clip(entity_id).or_else(|| {
            project
                .find_parent_clip(entity_id)
                .and_then(|clip_id| project.get_clip(clip_id))
        });
        if let Some(clip) = clip {
            let relative_time = global_time - clip.start_time.into_inner();
            let source_time =
                relative_time * clip.time_stretch.into_inner() + clip.trim_in.into_inner();
            return source_time;
        }
    }
    global_time
}

/// Resolve a Timeline Clip selection or a direct Node selection to the leaf
/// Node whose visual properties are edited by Inspector/Preview/Graph views.
pub fn visual_node_id(project: &Project, entity_id: Uuid) -> Option<Uuid> {
    if project.get_node(entity_id).is_some() {
        return Some(entity_id);
    }
    let clip = project.get_clip(entity_id)?;
    clip.output_node_id
        .filter(|node_id| project.get_node(*node_id).is_some())
        .or_else(|| {
            clip.node_ids
                .iter()
                .copied()
                .find(|node_id| project.get_node(*node_id).is_some())
        })
}

pub fn visual_property_owner(service: &ProjectService, entity_id: Uuid) -> Option<PropertyOwner> {
    let project = service.get_project();
    let project = project.read().ok()?;
    visual_node_id(&project, entity_id).map(PropertyOwner::Node)
}

pub fn update_number_property(
    service: &ProjectService,
    comp_id: Uuid,
    track_id: Uuid,
    entity_id: Uuid,
    prop_name: &str,
    time: f64,
    value: f64,
) -> Result<(), library::LibraryError> {
    let local_time = get_local_time(service, comp_id, track_id, entity_id, time);
    let owner = visual_property_owner(service, entity_id).ok_or_else(|| {
        library::LibraryError::Validation(format!("No leaf Node for entity {entity_id}"))
    })?;
    service.update_property_or_keyframe(
        owner,
        PropertyTarget::Direct,
        prop_name,
        local_time,
        PropertyValue::Number(OrderedFloat(value)),
        None,
    )
}

pub fn update_string_property(
    service: &ProjectService,
    comp_id: Uuid,
    track_id: Uuid,
    entity_id: Uuid,
    prop_name: &str,
    time: f64,
    value: String,
) -> Result<(), library::LibraryError> {
    let local_time = get_local_time(service, comp_id, track_id, entity_id, time);
    let owner = visual_property_owner(service, entity_id).ok_or_else(|| {
        library::LibraryError::Validation(format!("No leaf Node for entity {entity_id}"))
    })?;
    service.update_property_or_keyframe(
        owner,
        PropertyTarget::Direct,
        prop_name,
        local_time,
        PropertyValue::String(value),
        None,
    )
}

pub fn update_property(
    service: &ProjectService,
    comp_id: Uuid,
    track_id: Uuid,
    entity_id: Uuid,
    prop_name: &str,
    time: f64,
    value: PropertyValue,
) -> Result<(), library::LibraryError> {
    let local_time = get_local_time(service, comp_id, track_id, entity_id, time);
    let owner = visual_property_owner(service, entity_id).ok_or_else(|| {
        library::LibraryError::Validation(format!("No leaf Node for entity {entity_id}"))
    })?;
    service.update_property_or_keyframe(
        owner,
        PropertyTarget::Direct,
        prop_name,
        local_time,
        value,
        None,
    )
}
