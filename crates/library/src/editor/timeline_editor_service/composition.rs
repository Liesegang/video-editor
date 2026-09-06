use super::*;
use crate::model::authoring::property_value_type;

impl TimelineEditorService {
    /// Publishes one definition-internal control through a stable Timeline
    /// interface ID. Placement callers never receive the internal target.
    pub fn publish_composition_parameter(
        &self,
        timeline_id: TimelineId,
        name: String,
        target: CompositionParameterTarget,
        default_value: PropertyValue,
    ) -> Result<(CompositionParameterId, ChangeSet), LibraryError> {
        let parameter_id = CompositionParameterId::new();
        let data_type = property_value_type(&default_value);
        let mut session = self.write_session()?;
        session
            .transact(
                vec![ProjectInvalidation::TimelineStructure { timeline_id }],
                |project| {
                    validate_publish_target(project, timeline_id, &target, &default_value)?;
                    let timeline = project
                        .timelines
                        .get_mut(&timeline_id)
                        .ok_or_else(|| format!("Missing Timeline {timeline_id}"))?;
                    let normalized_name = name.trim();
                    if normalized_name.is_empty() {
                        return Err("Composition parameter name must not be empty".to_string());
                    }
                    if timeline
                        .published_parameters
                        .iter()
                        .any(|parameter| parameter.name.eq_ignore_ascii_case(normalized_name))
                    {
                        return Err(format!(
                            "Timeline {timeline_id} already has a Composition parameter named '{normalized_name}'"
                        ));
                    }
                    if timeline
                        .published_parameters
                        .iter()
                        .any(|parameter| parameter.target == target)
                    {
                        return Err(
                            "This Timeline control is already published as an instance parameter"
                                .to_string(),
                        );
                    }
                    timeline.published_parameters.push(CompositionParameter {
                        id: parameter_id,
                        name: normalized_name.to_string(),
                        data_type,
                        default_value,
                        target,
                    });
                    Ok(parameter_id)
                },
            )
            .map_err(LibraryError::Validation)
    }

    /// Removes an interface entry and every now-invalid placement override in
    /// the same atomic undo step.
    pub fn unpublish_composition_parameter(
        &self,
        timeline_id: TimelineId,
        parameter_id: CompositionParameterId,
    ) -> Result<(usize, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        session
            .transact(
                vec![ProjectInvalidation::TimelineStructure { timeline_id }],
                |project| {
                    let timeline = project
                        .timelines
                        .get_mut(&timeline_id)
                        .ok_or_else(|| format!("Missing Timeline {timeline_id}"))?;
                    let before = timeline.published_parameters.len();
                    timeline
                        .published_parameters
                        .retain(|parameter| parameter.id != parameter_id);
                    if timeline.published_parameters.len() == before {
                        return Err(format!(
                            "Missing Composition parameter {parameter_id} on Timeline {timeline_id}"
                        ));
                    }
                    let mut cleared = 0;
                    for item in project.items.values_mut() {
                        let SourceRef::Composition(instance) = &mut item.source else {
                            continue;
                        };
                        if instance.timeline_id == timeline_id
                            && instance.parameter_overrides.remove(&parameter_id).is_some()
                        {
                            cleared += 1;
                        }
                    }
                    Ok(cleared)
                },
            )
            .map_err(LibraryError::Validation)
    }

    pub fn set_composition_parameter_override(
        &self,
        composition_item_id: TimelineItemId,
        parameter_id: CompositionParameterId,
        value: PropertyValue,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let host_timeline_id = timeline_for_item(session.project(), composition_item_id)?;
        session
            .transact(
                vec![ProjectInvalidation::Item {
                    timeline_id: host_timeline_id,
                    item_id: composition_item_id,
                }],
                |project| {
                    let nested_timeline_id = composition_timeline_id(project, composition_item_id)?;
                    let parameter = project
                        .timelines
                        .get(&nested_timeline_id)
                        .and_then(|timeline| {
                            timeline
                                .published_parameters
                                .iter()
                                .find(|parameter| parameter.id == parameter_id)
                        })
                        .ok_or_else(|| {
                            format!(
                                "Composition item {composition_item_id} has no published parameter {parameter_id}"
                            )
                        })?;
                    if !parameter.data_type.accepts(property_value_type(&value)) {
                        return Err(format!(
                            "Composition parameter {parameter_id} has an incompatible value"
                        ));
                    }
                    let item = project
                        .items
                        .get_mut(&composition_item_id)
                        .ok_or_else(|| format!("Missing Timeline item {composition_item_id}"))?;
                    let SourceRef::Composition(instance) = &mut item.source else {
                        return Err(format!(
                            "Timeline item {composition_item_id} is not a Composition"
                        ));
                    };
                    instance.parameter_overrides.insert(parameter_id, value);
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn clear_composition_parameter_override(
        &self,
        composition_item_id: TimelineItemId,
        parameter_id: CompositionParameterId,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let host_timeline_id = timeline_for_item(session.project(), composition_item_id)?;
        session
            .transact(
                vec![ProjectInvalidation::Item {
                    timeline_id: host_timeline_id,
                    item_id: composition_item_id,
                }],
                |project| {
                    let nested_timeline_id = composition_timeline_id(project, composition_item_id)?;
                    let exists = project
                        .timelines
                        .get(&nested_timeline_id)
                        .is_some_and(|timeline| {
                            timeline
                                .published_parameters
                                .iter()
                                .any(|parameter| parameter.id == parameter_id)
                        });
                    if !exists {
                        return Err(format!(
                            "Composition item {composition_item_id} has no published parameter {parameter_id}"
                        ));
                    }
                    let item = project
                        .items
                        .get_mut(&composition_item_id)
                        .ok_or_else(|| format!("Missing Timeline item {composition_item_id}"))?;
                    let SourceRef::Composition(instance) = &mut item.source else {
                        return Err(format!(
                            "Timeline item {composition_item_id} is not a Composition"
                        ));
                    };
                    instance
                        .parameter_overrides
                        .remove(&parameter_id)
                        .map(|_| ())
                        .ok_or_else(|| {
                            format!("Composition parameter {parameter_id} has no instance override")
                        })
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    /// Replaces one ordinary placement with a reusable nested Timeline while
    /// retaining the placement identity and its external compositing role.
    pub fn extract_item_to_composition(
        &self,
        item_id: TimelineItemId,
        name: String,
    ) -> Result<(TimelineId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        session
            .transact(vec![ProjectInvalidation::ProjectStructure], |project| {
                extract_item_to_composition(project, item_id, &name)
            })
            .map_err(LibraryError::Validation)
    }

    /// Gives one Composition placement a private copy of its referenced
    /// Timeline definition. Nested Composition references remain shared.
    pub fn make_composition_unique(
        &self,
        item_id: TimelineItemId,
    ) -> Result<(TimelineId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        session
            .transact(vec![ProjectInvalidation::ProjectStructure], |project| {
                make_composition_unique(project, item_id)
            })
            .map_err(LibraryError::Validation)
    }
}

fn extract_item_to_composition(
    project: &mut AuthoringProject,
    item_id: TimelineItemId,
    name: &str,
) -> Result<TimelineId, String> {
    let normalized_name = name.trim();
    if normalized_name.is_empty() {
        return Err("Composition name must not be empty".to_string());
    }
    validate_extraction_boundary(project, item_id)?;
    let original = project
        .items
        .get(&item_id)
        .cloned()
        .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
    let host_track = project
        .tracks
        .get(&original.track_id)
        .cloned()
        .ok_or_else(|| format!("Timeline item {item_id} has no Track"))?;
    let host_timeline = project
        .timelines
        .get(&host_track.timeline_id)
        .cloned()
        .ok_or_else(|| format!("Timeline item {item_id} has no Timeline"))?;

    let timeline_id = TimelineId::new();
    let track_id = TimelineTrackId::new();
    let inner_item_id = TimelineItemId::new();
    let published_text = match &original.source {
        SourceRef::Text { text, .. } => Some((CompositionParameterId::new(), text.clone())),
        _ => None,
    };
    let published_parameters = published_text
        .as_ref()
        .map(|(parameter_id, text)| CompositionParameter {
            id: *parameter_id,
            name: "Text".to_string(),
            data_type: crate::model::project::PortDataType::String,
            default_value: PropertyValue::String(text.clone()),
            target: CompositionParameterTarget::TextContent {
                item_id: inner_item_id,
            },
        })
        .into_iter()
        .collect();
    project.timelines.insert(
        timeline_id,
        Timeline {
            id: timeline_id,
            name: normalized_name.to_string(),
            width: host_timeline.width,
            height: host_timeline.height,
            fps: host_timeline.fps,
            duration: original.interval.duration,
            background_color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
            color_profile: host_timeline.color_profile,
            track_order: vec![track_id],
            authored_properties: PropertyMap::new(),
            published_parameters,
        },
    );
    project.tracks.insert(
        track_id,
        TimelineTrack {
            id: track_id,
            timeline_id,
            name: host_track.name,
            kind: host_track.kind,
            authored_properties: PropertyMap::new(),
        },
    );
    project.items.insert(
        inner_item_id,
        TimelineItem {
            id: inner_item_id,
            track_id,
            name: original.name.clone(),
            source: original.source,
            interval: TimelineInterval::new(MediaTime::zero(), original.interval.duration)?,
            time_map: original.time_map,
            layer: 0,
            parent: None,
            blend_mode: BlendMode::Normal,
            authored_properties: original.authored_properties,
        },
    );
    for attachment in project.attachments.values_mut() {
        if attachment.owner == (AttachmentOwner::Item { item_id }) {
            attachment.owner = AttachmentOwner::Item {
                item_id: inner_item_id,
            };
        }
    }
    let outer = project
        .items
        .get_mut(&item_id)
        .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
    outer.source = SourceRef::Composition(CompositionInstance {
        timeline_id,
        duration_policy: DurationPolicy::Fixed,
        parameter_overrides: HashMap::new(),
        transition_module_overrides: Vec::new(),
    });
    outer.time_map = TimeMap::default();
    outer.authored_properties = PropertyMap::new();
    Ok(timeline_id)
}

fn validate_extraction_boundary(
    project: &AuthoringProject,
    item_id: TimelineItemId,
) -> Result<(), String> {
    let item = project
        .items
        .get(&item_id)
        .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
    if item.parent.is_some()
        || project
            .items
            .values()
            .any(|child| child.parent == Some(item_id))
    {
        return Err(
            "Extract to Composition does not yet support parented items or items with children"
                .to_string(),
        );
    }
    let dependencies = super::item::item_input_dependencies(project, item_id);
    if !dependencies.is_empty() {
        return Err(format!(
            "Extract to Composition cannot move an item referenced by {}",
            dependencies
                .iter()
                .map(|dependency| format!("{dependency:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if project.timelines.values().any(|timeline| {
        timeline
            .published_parameters
            .iter()
            .any(|parameter| parameter.target.item_id() == item_id)
    }) {
        return Err(
            "Extract to Composition cannot move an item targeted by a published Composition parameter"
                .to_string(),
        );
    }
    validate_source_bindings_for_extraction(&item.source)?;
    for attachment in project
        .attachments
        .values()
        .filter(|attachment| attachment.owner == (AttachmentOwner::Item { item_id }))
    {
        if let AttachmentProcessor::Module(invocation) = &attachment.processor
            && !invocation.input_bindings.is_empty()
        {
            return Err(format!(
                "Extract to Composition cannot move Attachment {} with external Module inputs",
                attachment.id
            ));
        }
    }
    Ok(())
}

fn validate_source_bindings_for_extraction(source: &SourceRef) -> Result<(), String> {
    match source {
        SourceRef::Module(invocation) if !invocation.input_bindings.is_empty() => Err(
            "Extract to Composition cannot move a Node Clip with external Module inputs"
                .to_string(),
        ),
        SourceRef::Composition(instance) if !instance.transition_module_overrides.is_empty() => {
            Err(
                "Extract to Composition cannot move placement-local Transition overrides"
                    .to_string(),
            )
        }
        _ => Ok(()),
    }
}

fn make_composition_unique(
    project: &mut AuthoringProject,
    item_id: TimelineItemId,
) -> Result<TimelineId, String> {
    let source_instance = project
        .items
        .get(&item_id)
        .ok_or_else(|| format!("Missing Timeline item {item_id}"))?
        .source
        .clone();
    let SourceRef::Composition(source_instance) = source_instance else {
        return Err(format!("Timeline item {item_id} is not a Composition"));
    };
    if !source_instance.transition_module_overrides.is_empty() {
        return Err(
            "Make Composition Unique does not yet support placement-local Transition overrides"
                .to_string(),
        );
    }
    let source_timeline = project
        .timelines
        .get(&source_instance.timeline_id)
        .cloned()
        .ok_or_else(|| {
            format!(
                "Composition item {item_id} refers to missing Timeline {}",
                source_instance.timeline_id
            )
        })?;
    let track_ids = source_timeline.track_order.to_vec();
    let source_items = project
        .items
        .values()
        .filter(|item| track_ids.contains(&item.track_id))
        .cloned()
        .collect::<Vec<_>>();
    let copied_layers = track_ids
        .iter()
        .flat_map(|track_id| {
            ordered_track_item_ids(project, *track_id, None)
                .into_iter()
                .enumerate()
                .map(|(layer, item_id)| {
                    i64::try_from(layer)
                        .map(|layer| (item_id, layer))
                        .map_err(|_| "Timeline layer overflow".to_string())
                })
        })
        .collect::<Result<HashMap<_, _>, _>>()?;
    for item in &source_items {
        if let SourceRef::Composition(instance) = &item.source
            && !instance.transition_module_overrides.is_empty()
        {
            return Err(format!(
                "Make Composition Unique cannot remap placement-local Transition overrides on nested item {}",
                item.id
            ));
        }
        if let Some(invocation) = item_invocation(&item.source) {
            validate_input_bindings_for_copy(&invocation.input_bindings)?;
        }
    }

    let source_item_ids = source_items
        .iter()
        .map(|item| item.id)
        .collect::<std::collections::HashSet<_>>();
    let source_attachments = project
        .attachments
        .values()
        .filter(|attachment| match attachment.owner {
            AttachmentOwner::Timeline { timeline_id } => timeline_id == source_timeline.id,
            AttachmentOwner::Track { track_id } => track_ids.contains(&track_id),
            AttachmentOwner::Item { item_id } => source_item_ids.contains(&item_id),
        })
        .cloned()
        .collect::<Vec<_>>();
    for attachment in &source_attachments {
        let invocation = match &attachment.processor {
            AttachmentProcessor::Module(invocation) => Some(invocation),
            AttachmentProcessor::BuiltinEffect(_) => None,
        };
        if let Some(invocation) = invocation {
            validate_input_bindings_for_copy(&invocation.input_bindings)?;
        }
    }
    let source_transitions = project
        .transitions
        .values()
        .filter(|transition| transition.timeline_id == source_timeline.id)
        .cloned()
        .collect::<Vec<_>>();
    for transition in &source_transitions {
        if let Some(processor) = transition.processor.module_processor() {
            validate_input_bindings_for_copy(&processor.input_bindings)?;
        }
    }

    let timeline_id = TimelineId::new();
    let track_map = track_ids
        .iter()
        .map(|id| (*id, TimelineTrackId::new()))
        .collect::<HashMap<_, _>>();
    let item_map = source_items
        .iter()
        .map(|item| (item.id, TimelineItemId::new()))
        .collect::<HashMap<_, _>>();
    let parameter_map = source_timeline
        .published_parameters
        .iter()
        .map(|parameter| (parameter.id, CompositionParameterId::new()))
        .collect::<HashMap<_, _>>();
    let transition_map = source_transitions
        .iter()
        .map(|transition| (transition.id, TransitionId::new()))
        .collect::<HashMap<_, _>>();
    let mut module_instance_map = HashMap::new();

    for old_track_id in &track_ids {
        let mut track = project
            .tracks
            .get(old_track_id)
            .cloned()
            .ok_or_else(|| format!("Timeline {} lists a missing Track", source_timeline.id))?;
        track.id = track_map[old_track_id];
        track.timeline_id = timeline_id;
        project.tracks.insert(track.id, track);
    }
    for mut item in source_items {
        let old_id = item.id;
        item.id = item_map[&old_id];
        item.track_id = track_map[&item.track_id];
        item.layer = copied_layers[&old_id];
        item.parent = item.parent.map(|parent| item_map[&parent]);
        remap_source_for_timeline_copy(
            project,
            &mut item.source,
            &item_map,
            &mut module_instance_map,
        )?;
        project.items.insert(item.id, item);
    }
    for mut attachment in source_attachments {
        attachment.id = AttachmentId::new();
        attachment.owner = match attachment.owner {
            AttachmentOwner::Timeline { .. } => AttachmentOwner::Timeline { timeline_id },
            AttachmentOwner::Track { track_id } => AttachmentOwner::Track {
                track_id: track_map[&track_id],
            },
            AttachmentOwner::Item { item_id } => AttachmentOwner::Item {
                item_id: item_map[&item_id],
            },
        };
        if let AttachmentProcessor::Module(invocation) = &mut attachment.processor {
            remap_invocation_for_timeline_copy(
                project,
                invocation,
                &item_map,
                &mut module_instance_map,
            )?;
        }
        project.attachments.insert(attachment.id, attachment);
    }
    for mut transition in source_transitions {
        let old_id = transition.id;
        transition.id = transition_map[&old_id];
        transition.timeline_id = timeline_id;
        transition.from_item_id = item_map[&transition.from_item_id];
        transition.to_item_id = item_map[&transition.to_item_id];
        if let Some(processor) = transition.processor.module_processor_mut() {
            processor.instance_id = clone_module_instance_for_timeline_copy(
                project,
                processor.instance_id,
                &mut module_instance_map,
            )?;
            remap_input_bindings(&mut processor.input_bindings, &item_map)?;
        }
        project.transitions.insert(transition.id, transition);
    }
    let mut timeline = source_timeline;
    timeline.id = timeline_id;
    timeline.name = format!("{} Copy", timeline.name);
    timeline.track_order = timeline
        .track_order
        .iter()
        .map(|id| track_map[id])
        .collect();
    for parameter in &mut timeline.published_parameters {
        parameter.id = parameter_map[&parameter.id];
        parameter.target = remap_composition_parameter_target(&parameter.target, &item_map);
    }
    project.timelines.insert(timeline_id, timeline);

    let outer = project
        .items
        .get_mut(&item_id)
        .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
    let SourceRef::Composition(instance) = &mut outer.source else {
        return Err(format!("Timeline item {item_id} is not a Composition"));
    };
    instance.timeline_id = timeline_id;
    instance.parameter_overrides = instance
        .parameter_overrides
        .drain()
        .map(|(parameter_id, value)| (parameter_map[&parameter_id], value))
        .collect();
    Ok(timeline_id)
}

fn item_invocation(source: &SourceRef) -> Option<&ModuleInvocation> {
    match source {
        SourceRef::Module(invocation) => Some(invocation),
        _ => None,
    }
}

fn validate_input_bindings_for_copy(
    bindings: &HashMap<PublishedMediaInputId, MediaInputBinding>,
) -> Result<(), String> {
    if bindings.values().any(|binding| {
        matches!(
            binding,
            MediaInputBinding::TimelineItemOutput {
                locator: InstanceLocator::Exact(_),
                ..
            }
        )
    }) {
        return Err(
            "Make Composition Unique cannot remap an exact InstancePath Module input".to_string(),
        );
    }
    Ok(())
}

fn remap_source_for_timeline_copy(
    project: &mut AuthoringProject,
    source: &mut SourceRef,
    item_map: &HashMap<TimelineItemId, TimelineItemId>,
    module_instance_map: &mut HashMap<ModuleInstanceId, ModuleInstanceId>,
) -> Result<(), String> {
    if let SourceRef::Module(invocation) = source {
        remap_invocation_for_timeline_copy(project, invocation, item_map, module_instance_map)?;
    }
    Ok(())
}

fn remap_invocation_for_timeline_copy(
    project: &mut AuthoringProject,
    invocation: &mut ModuleInvocation,
    item_map: &HashMap<TimelineItemId, TimelineItemId>,
    module_instance_map: &mut HashMap<ModuleInstanceId, ModuleInstanceId>,
) -> Result<(), String> {
    invocation.instance_id = clone_module_instance_for_timeline_copy(
        project,
        invocation.instance_id,
        module_instance_map,
    )?;
    remap_input_bindings(&mut invocation.input_bindings, item_map)
}

fn clone_module_instance_for_timeline_copy(
    project: &mut AuthoringProject,
    source_instance_id: ModuleInstanceId,
    instance_map: &mut HashMap<ModuleInstanceId, ModuleInstanceId>,
) -> Result<ModuleInstanceId, String> {
    if let Some(instance_id) = instance_map.get(&source_instance_id) {
        return Ok(*instance_id);
    }
    let instance_id = super::module::copy_module_instance(
        project,
        source_instance_id,
        super::module::ModuleInstanceCopyPolicy::Independent,
    )?;
    instance_map.insert(source_instance_id, instance_id);
    Ok(instance_id)
}

fn remap_input_bindings(
    bindings: &mut HashMap<PublishedMediaInputId, MediaInputBinding>,
    item_map: &HashMap<TimelineItemId, TimelineItemId>,
) -> Result<(), String> {
    for binding in bindings.values_mut() {
        let MediaInputBinding::TimelineItemOutput {
            locator, item_id, ..
        } = binding;
        match locator {
            InstanceLocator::SameTimeline => {
                *item_id = item_map.get(item_id).copied().ok_or_else(|| {
                    "Make Composition Unique found a SameTimeline input outside the copied Timeline"
                        .to_string()
                })?;
            }
            InstanceLocator::Exact(_) => {
                return Err(
                    "Make Composition Unique cannot remap an exact InstancePath Module input"
                        .to_string(),
                );
            }
        }
    }
    Ok(())
}

fn remap_composition_parameter_target(
    target: &CompositionParameterTarget,
    item_map: &HashMap<TimelineItemId, TimelineItemId>,
) -> CompositionParameterTarget {
    match target {
        CompositionParameterTarget::TextContent { item_id } => {
            CompositionParameterTarget::TextContent {
                item_id: item_map[item_id],
            }
        }
        CompositionParameterTarget::ItemProperty {
            item_id,
            property_key,
        } => CompositionParameterTarget::ItemProperty {
            item_id: item_map[item_id],
            property_key: property_key.clone(),
        },
    }
}

fn composition_timeline_id(
    project: &AuthoringProject,
    composition_item_id: TimelineItemId,
) -> Result<TimelineId, String> {
    let item = project
        .items
        .get(&composition_item_id)
        .ok_or_else(|| format!("Missing Timeline item {composition_item_id}"))?;
    let SourceRef::Composition(instance) = &item.source else {
        return Err(format!(
            "Timeline item {composition_item_id} is not a Composition"
        ));
    };
    Ok(instance.timeline_id)
}

fn validate_publish_target(
    project: &AuthoringProject,
    timeline_id: TimelineId,
    target: &CompositionParameterTarget,
    default_value: &PropertyValue,
) -> Result<(), String> {
    if !project.timelines.contains_key(&timeline_id) {
        return Err(format!("Missing Timeline {timeline_id}"));
    }
    let item = project
        .items
        .get(&target.item_id())
        .ok_or_else(|| format!("Missing Timeline item {}", target.item_id()))?;
    let item_timeline_id = project
        .tracks
        .get(&item.track_id)
        .ok_or_else(|| format!("Timeline item {} has no Track", item.id))?
        .timeline_id;
    if item_timeline_id != timeline_id {
        return Err("Composition parameter target must belong to its Timeline".to_string());
    }
    match target {
        CompositionParameterTarget::TextContent { .. } => {
            if !matches!(item.source, SourceRef::Text { .. })
                || !matches!(default_value, PropertyValue::String(_))
            {
                return Err("Text parameter target and default must both be Text".to_string());
            }
        }
        CompositionParameterTarget::ItemProperty { property_key, .. } => {
            if property_key.trim().is_empty() {
                return Err("Composition Property key must not be empty".to_string());
            }
            let authored = item
                .authored_properties
                .get(property_key)
                .and_then(|property| property.value());
            if authored.is_some_and(|authored| {
                !property_value_type(default_value).accepts(property_value_type(authored))
            }) {
                return Err(
                    "Composition parameter default does not match its authored Property"
                        .to_string(),
                );
            }
        }
    }
    Ok(())
}
