//! Published Module parameter lanes shared by Node Clips and Module Effects.

use library::editor::ModuleAutomationOwner;
use library::model::authoring::{
    AttachmentId, AttachmentOwner, AuthoringProject, ModuleInvocation,
};

use super::{
    automation_points, AutomationLane, AutomationLaneId, AutomationOwner, AutomationTarget,
};

#[derive(Clone, Copy)]
pub(super) enum ModuleLaneTarget {
    Item,
    Attachment(AttachmentId),
}

pub(super) fn push_module_parameter_lanes(
    lanes: &mut Vec<AutomationLane>,
    project: &AuthoringProject,
    lane_owner: AutomationOwner,
    lane_target: ModuleLaneTarget,
    invocation: &ModuleInvocation,
) {
    let Some((instance, definition)) = project
        .module_instances
        .get(&invocation.instance_id)
        .and_then(|instance| {
            project
                .module_definitions
                .get(&instance.definition_id)
                .map(|definition| (instance, definition))
        })
    else {
        return;
    };
    for parameter in definition.interface.parameters.iter().filter(|parameter| {
        matches!(
            definition.parameter_automation_capability(parameter.id),
            Ok(library::model::authoring::PublishedParameterAutomationCapability::FrameSampled)
        )
    }) {
        let mut points = invocation
            .automation_tracks
            .get(&parameter.id)
            .map(|track| automation_points(&track.keyframes))
            .unwrap_or_default();
        points.sort_by_key(|point| point.time);
        lanes.push(AutomationLane {
            id: AutomationLaneId {
                owner: lane_owner.clone(),
                target: match lane_target {
                    ModuleLaneTarget::Item => AutomationTarget::ModuleParameter(parameter.id),
                    ModuleLaneTarget::Attachment(attachment_id) => {
                        AutomationTarget::AttachmentModuleParameter {
                            attachment_id,
                            parameter_id: parameter.id,
                        }
                    }
                },
            },
            label: match lane_target {
                ModuleLaneTarget::Item => parameter.name.clone(),
                ModuleLaneTarget::Attachment(_) => {
                    format!("{} \u{b7} {}", definition.name, parameter.name)
                }
            },
            base_value: instance
                .parameter_overrides
                .get(&parameter.id)
                .cloned()
                .or_else(|| Some(parameter.default_value.clone())),
            points,
        });
    }
}

/// Maps the Timeline-owned Module automation target to the shared Curve/Dope
/// Sheet owner without rediscovering the host in each editor surface.
pub(crate) fn module_parameter_owner(
    project: &AuthoringProject,
    owner: &ModuleAutomationOwner,
) -> Option<AutomationOwner> {
    owner.invocation(project).ok()?;
    match *owner {
        ModuleAutomationOwner::Item(item_id) => Some(AutomationOwner::Item(item_id)),
        ModuleAutomationOwner::Attachment(attachment_id) => {
            match &project.attachments.get(&attachment_id)?.owner {
                AttachmentOwner::Item { item_id } => Some(AutomationOwner::Item(*item_id)),
                AttachmentOwner::Track { track_id } => Some(AutomationOwner::Track(*track_id)),
                AttachmentOwner::Timeline { timeline_id } => {
                    Some(AutomationOwner::Timeline(*timeline_id))
                }
            }
        }
    }
}
