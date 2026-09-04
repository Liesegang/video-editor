use std::collections::HashMap;

use crate::error::LibraryError;
use crate::model::authoring::{
    CompositionParameterId, CompositionParameterTarget, InstancePath, MediaTime, SourceRef,
    Timeline, TimelineId, TimelineItem, TimelineItemId,
};
use crate::model::project::property::PropertyValue;

use super::{AuthoringFrameEvaluator, frame_values};

impl AuthoringFrameEvaluator<'_> {
    pub(super) fn effective_text(
        &self,
        timeline: &Timeline,
        item_id: TimelineItemId,
        authored: &str,
        instance_path: &InstancePath,
    ) -> Result<String, LibraryError> {
        let Some(parameter) = timeline.published_parameters.iter().find(|parameter| {
            parameter.target == CompositionParameterTarget::TextContent { item_id }
        }) else {
            return Ok(authored.to_string());
        };
        let Some(value) =
            self.composition_parameter_override(timeline.id, instance_path, parameter.id)?
        else {
            return Ok(authored.to_string());
        };
        match value {
            PropertyValue::String(text) => Ok(text),
            _ => Err(LibraryError::Validation(format!(
                "Composition parameter {} must be Text",
                parameter.id
            ))),
        }
    }

    pub(super) fn effective_item_property_values(
        &self,
        timeline: &Timeline,
        item: &TimelineItem,
        local_time: MediaTime,
        instance_path: &InstancePath,
    ) -> Result<HashMap<String, PropertyValue>, LibraryError> {
        let mut values = frame_values::evaluate_property_map(
            &item.authored_properties,
            local_time.to_seconds_f64(),
            &format!("Timeline item {}", item.id),
        )?;
        for parameter in &timeline.published_parameters {
            let CompositionParameterTarget::ItemProperty {
                item_id,
                property_key,
            } = &parameter.target
            else {
                continue;
            };
            if *item_id != item.id {
                continue;
            }
            if let Some(value) =
                self.composition_parameter_override(timeline.id, instance_path, parameter.id)?
            {
                values.insert(property_key.clone(), value);
            }
        }
        Ok(values)
    }

    fn composition_parameter_override(
        &self,
        timeline_id: TimelineId,
        instance_path: &InstancePath,
        parameter_id: CompositionParameterId,
    ) -> Result<Option<PropertyValue>, LibraryError> {
        let Some(placement_id) = instance_path.composition_items.last() else {
            return Ok(None);
        };
        let placement = self.project.items.get(placement_id).ok_or_else(|| {
            LibraryError::Validation(format!(
                "InstancePath Composition item {placement_id} is missing"
            ))
        })?;
        let SourceRef::Composition(instance) = &placement.source else {
            return Err(LibraryError::Validation(format!(
                "InstancePath item {placement_id} is not a Composition"
            )));
        };
        if instance.timeline_id != timeline_id {
            return Err(LibraryError::Validation(format!(
                "InstancePath placement {placement_id} does not instantiate Timeline {timeline_id}"
            )));
        }
        Ok(instance.parameter_overrides.get(&parameter_id).cloned())
    }
}
