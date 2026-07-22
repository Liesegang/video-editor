use library::model::project::{EvalOutput, EvalResult, PortAddress, PortOwner};
use library::model::property::PropertyValue;
use library::model::Project;
use library::plugin::PluginManager;
use library::EditorService as ProjectService;
use library::PropertyOwner;
use uuid::Uuid;

/// Resolve the property authoring time for a Node.
///
/// With runtime services available this follows explicit Time wires as well
/// as container inheritance. The structural fallback exists only for
/// dependency-light UI tests and inactive owners that deliberately have no
/// render-time output at the current timeline position.
pub(crate) fn node_local_time(
    project: &Project,
    plugin_manager: Option<&PluginManager>,
    node_id: Uuid,
    global_time: f64,
) -> f64 {
    if let Some(plugin_manager) = plugin_manager {
        if let Some(composition) = project
            .find_containing_composition(node_id)
            .and_then(|composition_id| project.get_composition(composition_id))
        {
            let evaluator = library::framing::FrameEvaluator::new(
                project,
                composition,
                plugin_manager.get_property_evaluators(),
                plugin_manager,
            );
            match evaluator.evaluate_owner_time(PortOwner::Node(node_id), global_time) {
                Ok(EvalOutput::Produced(time)) => return time,
                Ok(EvalOutput::NoOutput) => {}
                Err(error) => {
                    log::warn!("Cannot resolve local Time for Node {node_id}: {error}");
                }
            }
        }
    }

    project
        .find_parent_clip(node_id)
        .and_then(|clip_id| project.get_clip(clip_id))
        .map_or(global_time, |clip| clip.local_time(global_time))
}

pub(crate) fn linked_node_inputs(
    project: &Project,
    node_id: Uuid,
    ports: &[&str],
) -> Vec<(String, PortAddress)> {
    let mut linked = project
        .connections
        .iter()
        .filter(|connection| {
            connection.to.owner == PortOwner::Node(node_id)
                && ports.contains(&connection.to.port.as_str())
        })
        .map(|connection| (connection.to.port.clone(), connection.from.clone()))
        .collect::<Vec<_>>();
    linked.sort_by(|left, right| left.0.cmp(&right.0));
    linked
}

pub(crate) fn evaluate_node_metadata_output(
    project: &Project,
    plugin_manager: &PluginManager,
    node_id: Uuid,
    output_port: &str,
    global_time: f64,
) -> EvalResult<PropertyValue> {
    let composition = project
        .find_containing_composition(node_id)
        .and_then(|composition_id| project.get_composition(composition_id))
        .ok_or_else(|| {
            library::LibraryError::Project(format!(
                "Node {node_id} has no owning Composition evaluation context"
            ))
        })?;
    library::framing::FrameEvaluator::new(
        project,
        composition,
        plugin_manager.get_property_evaluators(),
        plugin_manager,
    )
    .evaluate_metadata_output(
        &PortAddress::new(PortOwner::Node(node_id), output_port),
        global_time,
    )
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

    let local_time = {
        let project = service.get_project();
        let project = project.read().map_err(|_| {
            library::LibraryError::Validation("Project lock is poisoned".to_string())
        })?;
        let plugin_manager = service.get_plugin_manager();
        node_local_time(&project, Some(plugin_manager.as_ref()), node_id, time)
    };

    service.update_property_or_keyframe(
        PropertyOwner::Node(node_id),
        prop_name,
        local_time,
        value,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::project::{
        PortAddress, FMOD_X_INPUT_PORT, NUMBER_RESULT_OUTPUT_PORT, TIME_PORT,
    };
    use library::model::{
        Clip, ColorContent, Composition, Node, NodeContainer, Project, COLOR_RED_PORT,
        COLOR_VALUE_PORT,
    };

    #[test]
    fn inspector_and_node_editor_time_match_runtime_remap_and_linked_value() {
        let mut project = Project::new("local Time parity");
        let (composition, track) = Composition::new("Main", 640, 360, 30.0, 4.0);
        let track_id = track.id;
        project.add_track(track).unwrap();
        project.add_composition(composition).unwrap();
        let clip = Clip::new("Clip", 0.0, 4.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        project.attach_clip_to_track(track_id, clip_id).unwrap();

        let fmod = Node::new_fmod("Loop Time");
        let fmod_id = fmod.id;
        let compose = Node::new_color("Compose", ColorContent::Compose);
        let compose_id = compose.id;
        project.add_node(fmod);
        project.add_node(compose);
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), fmod_id)
            .unwrap();
        project
            .attach_node_to_container(NodeContainer::Clip(clip_id), compose_id)
            .unwrap();
        project
            .connect_ports(
                PortAddress::new(PortOwner::Clip(clip_id), TIME_PORT),
                PortAddress::new(PortOwner::Node(fmod_id), FMOD_X_INPUT_PORT),
            )
            .unwrap();
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(fmod_id), NUMBER_RESULT_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(compose_id), TIME_PORT),
            )
            .unwrap();
        project
            .connect_ports(
                PortAddress::new(PortOwner::Node(fmod_id), NUMBER_RESULT_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(compose_id), COLOR_RED_PORT),
            )
            .unwrap();

        let plugins = PluginManager::default();
        let node_editor_time = node_local_time(&project, Some(&plugins), compose_id, 1.25);
        let inspector_time = node_local_time(&project, Some(&plugins), compose_id, 1.25);
        assert_eq!(node_editor_time, inspector_time);
        assert!((node_editor_time - 0.25).abs() <= f64::EPSILON);
        assert_ne!(
            node_editor_time,
            project.get_clip(clip_id).unwrap().local_time(1.25)
        );
        let linked = linked_node_inputs(&project, compose_id, &[COLOR_RED_PORT]);
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].0, COLOR_RED_PORT);
        assert_eq!(linked[0].1.owner, PortOwner::Node(fmod_id));

        let output =
            evaluate_node_metadata_output(&project, &plugins, compose_id, COLOR_VALUE_PORT, 1.25)
                .unwrap();
        let EvalOutput::Produced(PropertyValue::ColorValue(color)) = output else {
            panic!("linked Compose output should resolve to its effective runtime Color");
        };
        assert!((color.rgba()[0] - 0.25).abs() <= f64::EPSILON);
    }
}
