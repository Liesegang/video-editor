use library::EditorService as ProjectService;
use library::PropertyOwner;
use library::model::property::{PropertyTarget, PropertyValue};
use uuid::Uuid;

fn node_local_time(service: &ProjectService, node_id: Uuid, global_time: f64) -> f64 {
    if let Ok(project) = service.get_project().read() {
        if let Some(clip) = project
            .find_parent_clip(node_id)
            .and_then(|clip_id| project.get_clip(clip_id))
        {
            let relative_time = global_time - clip.start_time.into_inner();
            return relative_time * clip.time_stretch.into_inner() + clip.trim_in.into_inner();
        }
    }
    global_time
}

/// Update an explicitly identified visual Node.
///
/// Callers must obtain `node_id` from authoritative evaluation/selection.
/// This intentionally does not guess a Clip's sink from `output_node_id` or
/// containment order: those Nodes may be Style/Effect/Merge operations rather
/// than the visual source the user interacted with.
pub fn update_node_property(
    service: &ProjectService,
    node_id: Uuid,
    prop_name: &str,
    time: f64,
    value: PropertyValue,
) -> Result<(), library::LibraryError> {
    let project = service.get_project();
    let project = project
        .read()
        .map_err(|_| library::LibraryError::Validation("Project lock is poisoned".to_string()))?;
    if project.get_node(node_id).is_none() {
        return Err(library::LibraryError::Validation(format!(
            "Preview source Node {node_id} does not exist"
        )));
    }
    drop(project);

    service.update_property_or_keyframe(
        PropertyOwner::Node(node_id),
        PropertyTarget::Direct,
        prop_name,
        node_local_time(service, node_id, time),
        value,
        None,
    )
}
