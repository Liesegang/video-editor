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
            if let Some(authored) = item
                .authored_properties
                .get(property_key)
                .and_then(|property| property.value())
                && !property_value_type(default_value).accepts(property_value_type(authored))
            {
                return Err(
                    "Composition parameter default does not match its authored Property"
                        .to_string(),
                );
            }
        }
    }
    Ok(())
}
