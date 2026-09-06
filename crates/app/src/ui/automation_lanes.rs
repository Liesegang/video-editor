//! Shared discovery and mutation boundary for Timeline automation.
//!
//! The Curve Editor and Timeline Dope Sheet consume these exact lanes. UI
//! panels never rediscover Module interfaces or reinterpret authored
//! keyframes independently.

use library::animation::EasingFunction;
use library::editor::{
    AuthoringKeyframeUpdate, AuthoringPropertyOwner, ModuleAutomationOwner, ModuleParameterOwner,
    TimelineEditorService, TransitionAutomationOwner,
};
use library::model::authoring::{
    AttachmentOwner, AttachmentProcessor, AuthoringProject, InstancePath, MediaTime, SourceRef,
    TimelineId, TimelineInterval, TimelineItemId, TimelineTrackId, TransitionId,
};
use library::model::property::{KeyframeId, Property, PropertyMap, PropertyValue};

use crate::state::authoring::{
    AutomationLaneId, AutomationOwner, AutomationTarget, CurveValueComponent,
};

mod module;
pub(crate) use module::module_parameter_owner;
use module::{push_module_parameter_lanes, ModuleLaneTarget};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AutomationPoint {
    pub id: KeyframeId,
    /// Local time owned by the Item or Transition represented by the lane.
    pub time: MediaTime,
    pub value: PropertyValue,
    pub easing: EasingFunction,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AutomationLane {
    pub id: AutomationLaneId,
    pub label: String,
    /// Type-defining authored/default value, even before a first key exists.
    pub base_value: Option<PropertyValue>,
    pub points: Vec<AutomationPoint>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AutomationChannelPoint {
    pub id: KeyframeId,
    pub time: MediaTime,
    pub value: f64,
    pub full_value: PropertyValue,
    pub easing: EasingFunction,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AutomationChannel {
    pub id: AutomationLaneId,
    pub component: CurveValueComponent,
    pub label: String,
    pub points: Vec<AutomationChannelPoint>,
}

/// Discover every editable item-owned automation lane in stable UI order.
///
/// Constant authored properties remain visible with no diamonds, and every
/// keyframe-capable Published Module parameter is exposed before it receives
/// automation. Constant-only runtime inputs stay Inspector/Node properties
/// and never masquerade as editable Curve lanes.
pub(crate) fn collect_item_lanes(
    project: &AuthoringProject,
    item_id: TimelineItemId,
) -> Vec<AutomationLane> {
    let Some(item) = project.items.get(&item_id) else {
        return Vec::new();
    };
    let mut lanes = Vec::new();
    push_authored_property_lanes(
        &mut lanes,
        AutomationOwner::Item(item_id),
        AuthoringPropertyOwner::Item(item_id),
        None,
        &item.authored_properties,
    );

    match &item.source {
        SourceRef::Text {
            appearance_operations,
            ensemble_operations,
            ..
        } => {
            for operation in ensemble_operations {
                push_authored_property_lanes(
                    &mut lanes,
                    AutomationOwner::Item(item_id),
                    AuthoringPropertyOwner::TextEnsemble {
                        item_id,
                        operation_id: operation.id,
                    },
                    Some(humanize_label(&operation.operation.component_id)),
                    &operation.properties,
                );
            }
            for operation in appearance_operations {
                push_authored_property_lanes(
                    &mut lanes,
                    AutomationOwner::Item(item_id),
                    AuthoringPropertyOwner::Appearance {
                        item_id,
                        operation_id: operation.id,
                    },
                    Some(humanize_label(&operation.operation.component_id)),
                    &operation.properties,
                );
            }
        }
        SourceRef::Shape { shape } => {
            for operation in &shape.appearance_operations {
                push_authored_property_lanes(
                    &mut lanes,
                    AutomationOwner::Item(item_id),
                    AuthoringPropertyOwner::Appearance {
                        item_id,
                        operation_id: operation.id,
                    },
                    Some(humanize_label(&operation.operation.component_id)),
                    &operation.properties,
                );
            }
        }
        _ => {}
    }

    if let SourceRef::Module(invocation) = &item.source {
        push_module_parameter_lanes(
            &mut lanes,
            project,
            AutomationOwner::Item(item_id),
            ModuleLaneTarget::Item,
            invocation,
        );
    }

    push_attachment_lanes(
        &mut lanes,
        project,
        AutomationOwner::Item(item_id),
        &AttachmentOwner::Item { item_id },
    );
    lanes
}

fn collect_track_lanes(
    project: &AuthoringProject,
    track_id: TimelineTrackId,
) -> Vec<AutomationLane> {
    let Some(track) = project.tracks.get(&track_id) else {
        return Vec::new();
    };
    let mut lanes = Vec::new();
    push_authored_property_lanes(
        &mut lanes,
        AutomationOwner::Track(track_id),
        AuthoringPropertyOwner::Track(track_id),
        None,
        &track.authored_properties,
    );
    push_attachment_lanes(
        &mut lanes,
        project,
        AutomationOwner::Track(track_id),
        &AttachmentOwner::Track { track_id },
    );
    lanes
}

fn collect_timeline_lanes(
    project: &AuthoringProject,
    timeline_id: TimelineId,
) -> Vec<AutomationLane> {
    let Some(timeline) = project.timelines.get(&timeline_id) else {
        return Vec::new();
    };
    let mut lanes = Vec::new();
    push_authored_property_lanes(
        &mut lanes,
        AutomationOwner::Timeline(timeline_id),
        AuthoringPropertyOwner::Timeline(timeline_id),
        None,
        &timeline.authored_properties,
    );
    push_attachment_lanes(
        &mut lanes,
        project,
        AutomationOwner::Timeline(timeline_id),
        &AttachmentOwner::Timeline { timeline_id },
    );
    lanes
}

fn push_attachment_lanes(
    lanes: &mut Vec<AutomationLane>,
    project: &AuthoringProject,
    lane_owner: AutomationOwner,
    attachment_owner: &AttachmentOwner,
) {
    let mut attachments = project
        .attachments
        .values()
        .filter(|attachment| &attachment.owner == attachment_owner)
        .collect::<Vec<_>>();
    attachments.sort_by_key(|attachment| (attachment.stage, attachment.order, attachment.id));
    for attachment in attachments {
        match &attachment.processor {
            AttachmentProcessor::BuiltinEffect(effect) => {
                for contract in &effect.contract.parameters {
                    let Some(parameter) = effect.parameters.get(&contract.key) else {
                        continue;
                    };
                    let points = parameter
                        .automation
                        .as_ref()
                        .map(|track| automation_points(&track.keyframes))
                        .unwrap_or_default();
                    lanes.push(AutomationLane {
                        id: AutomationLaneId {
                            owner: lane_owner.clone(),
                            target: AutomationTarget::AttachmentParameter {
                                attachment_id: attachment.id,
                                key: contract.key.clone(),
                            },
                        },
                        label: format!(
                            "{} \u{b7} {}",
                            humanize_label(&effect.operation.component_id),
                            humanize_label(&contract.key)
                        ),
                        base_value: Some(parameter.value.clone()),
                        points,
                    });
                }
            }
            AttachmentProcessor::Module(invocation) => push_module_parameter_lanes(
                lanes,
                project,
                lane_owner.clone(),
                ModuleLaneTarget::Attachment(attachment.id),
                invocation,
            ),
        }
    }
}

fn push_authored_property_lanes(
    lanes: &mut Vec<AutomationLane>,
    lane_owner: AutomationOwner,
    property_owner: AuthoringPropertyOwner,
    label_prefix: Option<String>,
    properties: &PropertyMap,
) {
    let mut properties = properties.iter().collect::<Vec<_>>();
    properties.sort_by(|left, right| left.0.cmp(right.0));
    for (key, property) in properties {
        let mut points = authored_property_points(property);
        points.sort_by_key(|point| point.time);
        lanes.push(AutomationLane {
            id: AutomationLaneId {
                owner: lane_owner.clone(),
                target: AutomationTarget::AuthoredProperty {
                    owner: property_owner,
                    key: key.clone(),
                },
            },
            label: label_prefix
                .as_ref()
                .map(|prefix| format!("{prefix} \u{b7} {}", humanize_label(key)))
                .unwrap_or_else(|| humanize_label(key)),
            base_value: property.value().cloned(),
            points,
        });
    }
}

fn authored_property_points(property: &Property) -> Vec<AutomationPoint> {
    property
        .keyframes()
        .into_iter()
        .filter_map(|keyframe| {
            MediaTime::from_seconds_f64(keyframe.time.into_inner(), 1_000_000)
                .ok()
                .map(|time| AutomationPoint {
                    id: keyframe.id,
                    time,
                    value: keyframe.value,
                    easing: keyframe.easing,
                })
        })
        .collect()
}

pub(crate) fn transition_owner(
    transition_id: TransitionId,
    instance_path: Option<&InstancePath>,
) -> AutomationOwner {
    match instance_path {
        Some(path) if !path.composition_items.is_empty() => AutomationOwner::TransitionInstance {
            transition_id,
            instance_path: path.clone(),
        },
        _ => AutomationOwner::TransitionDefinition(transition_id),
    }
}

pub(crate) fn collect_lanes(
    project: &AuthoringProject,
    owner: &AutomationOwner,
) -> Vec<AutomationLane> {
    match owner {
        AutomationOwner::Item(item_id) => collect_item_lanes(project, *item_id),
        AutomationOwner::Track(track_id) => collect_track_lanes(project, *track_id),
        AutomationOwner::Timeline(timeline_id) => collect_timeline_lanes(project, *timeline_id),
        AutomationOwner::TransitionDefinition(transition_id)
        | AutomationOwner::TransitionInstance { transition_id, .. } => {
            collect_transition_lanes(project, owner, *transition_id)
        }
    }
}

fn collect_transition_lanes(
    project: &AuthoringProject,
    owner: &AutomationOwner,
    transition_id: TransitionId,
) -> Vec<AutomationLane> {
    let Some(transition) = project.transitions.get(&transition_id) else {
        return Vec::new();
    };
    let Some(module) = transition.processor.module_processor() else {
        return Vec::new();
    };
    let Some(instance) = project.module_instances.get(&module.instance_id) else {
        return Vec::new();
    };
    let Some(definition) = project.module_definitions.get(&instance.definition_id) else {
        return Vec::new();
    };
    let Some(contract) = definition.host_contract.transition() else {
        return Vec::new();
    };
    let (values, tracks) = match owner {
        AutomationOwner::TransitionDefinition(_) => (
            instance.parameter_overrides.clone(),
            module.automation_tracks.clone(),
        ),
        AutomationOwner::TransitionInstance { instance_path, .. } => {
            let Ok(target) =
                project.resolve_transition_module_instance_target(instance_path, transition_id)
            else {
                return Vec::new();
            };
            let Ok(controls) = project.effective_transition_module_controls(&target) else {
                return Vec::new();
            };
            (controls.parameter_overrides, controls.automation_tracks)
        }
        AutomationOwner::Item(_) | AutomationOwner::Track(_) | AutomationOwner::Timeline(_) => {
            return Vec::new();
        }
    };
    definition
        .interface
        .parameters
        .iter()
        .filter(|parameter| parameter.id != contract.progress_parameter_id)
        .filter(|parameter| {
            matches!(
                definition.parameter_automation_capability(parameter.id),
                Ok(library::model::authoring::PublishedParameterAutomationCapability::FrameSampled)
            )
        })
        .map(|parameter| AutomationLane {
            id: AutomationLaneId {
                owner: owner.clone(),
                target: AutomationTarget::ModuleParameter(parameter.id),
            },
            label: parameter.name.clone(),
            base_value: values
                .get(&parameter.id)
                .cloned()
                .or_else(|| Some(parameter.default_value.clone())),
            points: tracks
                .get(&parameter.id)
                .map(|track| automation_points(&track.keyframes))
                .unwrap_or_default(),
        })
        .collect()
}

/// Timeline rows remain anchored beneath the Transition's B clip, while the
/// lane identity retains Transition ownership for edits and Undo.
pub(crate) fn collect_dope_lanes(
    project: &AuthoringProject,
    anchor_item_id: TimelineItemId,
    instance_path: Option<&InstancePath>,
) -> Vec<AutomationLane> {
    let mut lanes = collect_item_keyframed_lanes(project, anchor_item_id);
    let mut transitions = project
        .transitions
        .values()
        .filter(|transition| transition.to_item_id == anchor_item_id)
        .collect::<Vec<_>>();
    transitions.sort_by_key(|transition| transition.id);
    for transition in transitions {
        let owner = transition_owner(transition.id, instance_path);
        lanes.extend(
            collect_lanes(project, &owner)
                .into_iter()
                .filter(|lane| !lane.points.is_empty())
                .map(|mut lane| {
                    lane.label = format!("Transition \u{b7} {}", lane.label);
                    lane
                }),
        );
    }
    lanes
}

/// Timeline/Dope Sheet policy: only real keyframe evaluators produce rows.
/// Constant values and merely published parameters remain available to the
/// Inspector/Curve channel discovery, but never masquerade as keyframes.
pub(crate) fn collect_item_keyframed_lanes(
    project: &AuthoringProject,
    item_id: TimelineItemId,
) -> Vec<AutomationLane> {
    collect_item_lanes(project, item_id)
        .into_iter()
        .filter(|lane| !lane.points.is_empty())
        .collect()
}

fn automation_points(
    keyframes: &[library::model::authoring::AutomationKeyframe],
) -> Vec<AutomationPoint> {
    keyframes
        .iter()
        .map(|keyframe| AutomationPoint {
            id: keyframe.id,
            time: keyframe.time,
            value: keyframe.value.clone(),
            easing: keyframe.easing.clone(),
        })
        .collect()
}

/// Split lanes into numeric/vector Curve Editor channels without changing
/// lane identity or keyframe ownership.
pub(crate) fn numeric_channels(lanes: &[AutomationLane]) -> Vec<AutomationChannel> {
    let mut output = Vec::new();
    for lane in lanes {
        let type_value = lane
            .points
            .first()
            .map(|point| &point.value)
            .or(lane.base_value.as_ref());
        for component in components_for(type_value) {
            output.push(AutomationChannel {
                id: lane.id.clone(),
                component: *component,
                label: if *component == CurveValueComponent::Scalar {
                    lane.label.clone()
                } else {
                    format!("{}.{}", lane.label, component_name(*component))
                },
                points: lane
                    .points
                    .iter()
                    .filter_map(|point| {
                        component_value(&point.value, *component).map(|value| {
                            AutomationChannelPoint {
                                id: point.id,
                                time: point.time,
                                value,
                                full_value: point.value.clone(),
                                easing: point.easing.clone(),
                            }
                        })
                    })
                    .collect(),
            });
        }
    }
    output
}

pub(crate) fn update_keyframe(
    service: &TimelineEditorService,
    lane: &AutomationLaneId,
    keyframe_id: KeyframeId,
    update: AuthoringKeyframeUpdate,
) -> Result<(), library::LibraryError> {
    service
        .update_keyframe(&keyframe_target(lane)?, keyframe_id, update)
        .map(|_| ())
}

pub(crate) fn keyframe_target(
    lane: &AutomationLaneId,
) -> Result<library::editor::AuthoringKeyframeTarget, library::LibraryError> {
    use library::editor::AuthoringKeyframeTarget;
    match (&lane.owner, &lane.target) {
        (lane_owner, AutomationTarget::AuthoredProperty { owner, key }) => {
            let property_lane_owner = match owner {
                AuthoringPropertyOwner::Item(item_id)
                | AuthoringPropertyOwner::TextEnsemble { item_id, .. }
                | AuthoringPropertyOwner::Appearance { item_id, .. } => {
                    AutomationOwner::Item(*item_id)
                }
                AuthoringPropertyOwner::Track(track_id) => AutomationOwner::Track(*track_id),
                AuthoringPropertyOwner::Timeline(timeline_id) => {
                    AutomationOwner::Timeline(*timeline_id)
                }
            };
            if property_lane_owner != *lane_owner {
                return Err(library::LibraryError::Validation(format!(
                    "Automation owner {lane_owner:?} cannot edit {owner:?}"
                )));
            }
            Ok(AuthoringKeyframeTarget::AuthoredProperty {
                owner: *owner,
                key: key.clone(),
            })
        }
        (AutomationOwner::Item(item_id), AutomationTarget::ModuleParameter(parameter_id)) => {
            Ok(AuthoringKeyframeTarget::ModuleParameter {
                owner: ModuleParameterOwner::Invocation(ModuleAutomationOwner::Item(*item_id)),
                parameter_id: *parameter_id,
            })
        }
        (
            AutomationOwner::Item(_) | AutomationOwner::Track(_) | AutomationOwner::Timeline(_),
            AutomationTarget::AttachmentModuleParameter {
                attachment_id,
                parameter_id,
            },
        ) => Ok(AuthoringKeyframeTarget::ModuleParameter {
            owner: ModuleParameterOwner::Invocation(ModuleAutomationOwner::Attachment(
                *attachment_id,
            )),
            parameter_id: *parameter_id,
        }),
        (
            AutomationOwner::Item(_) | AutomationOwner::Track(_) | AutomationOwner::Timeline(_),
            AutomationTarget::AttachmentParameter { attachment_id, key },
        ) => Ok(AuthoringKeyframeTarget::BuiltinEffectParameter {
            attachment_id: *attachment_id,
            key: key.clone(),
        }),
        (
            AutomationOwner::TransitionDefinition(transition_id),
            AutomationTarget::ModuleParameter(parameter_id),
        ) => Ok(AuthoringKeyframeTarget::ModuleParameter {
            owner: ModuleParameterOwner::Transition(TransitionAutomationOwner::Definition(
                *transition_id,
            )),
            parameter_id: *parameter_id,
        }),
        (
            AutomationOwner::TransitionInstance {
                transition_id,
                instance_path,
            },
            AutomationTarget::ModuleParameter(parameter_id),
        ) => Ok(AuthoringKeyframeTarget::ModuleParameter {
            owner: ModuleParameterOwner::Transition(TransitionAutomationOwner::Instance {
                transition_id: *transition_id,
                instance_path: instance_path.clone(),
            }),
            parameter_id: *parameter_id,
        }),
        (AutomationOwner::TransitionDefinition(transition_id), target)
        | (AutomationOwner::TransitionInstance { transition_id, .. }, target) => {
            Err(library::LibraryError::Validation(format!(
                "Transition {transition_id} automation does not support {target:?}"
            )))
        }
        (AutomationOwner::Track(track_id), target) => Err(library::LibraryError::Validation(
            format!("Track {track_id} automation does not support {target:?}"),
        )),
        (AutomationOwner::Timeline(timeline_id), target) => Err(library::LibraryError::Validation(
            format!("Timeline {timeline_id} automation does not support {target:?}"),
        )),
    }
}

pub(crate) fn transition_service_owner(
    owner: &AutomationOwner,
) -> Option<TransitionAutomationOwner> {
    match owner {
        AutomationOwner::TransitionDefinition(transition_id) => {
            Some(TransitionAutomationOwner::Definition(*transition_id))
        }
        AutomationOwner::TransitionInstance {
            transition_id,
            instance_path,
        } => Some(TransitionAutomationOwner::Instance {
            transition_id: *transition_id,
            instance_path: instance_path.clone(),
        }),
        AutomationOwner::Item(_) | AutomationOwner::Track(_) | AutomationOwner::Timeline(_) => None,
    }
}

/// Convert one local automation time into the host Timeline time.
pub(crate) fn timeline_time_for_local(
    project: &AuthoringProject,
    owner: &AutomationOwner,
    local_time: MediaTime,
) -> Option<MediaTime> {
    match owner {
        AutomationOwner::Item(item_id) => {
            let item = project.items.get(item_id)?;
            let rate = item.time_map.playback_rate.to_f64();
            if !rate.is_finite() || rate.abs() <= f64::EPSILON {
                return None;
            }
            let seconds = item.interval.start.to_seconds_f64()
                + (local_time.to_seconds_f64() - item.time_map.source_start.to_seconds_f64())
                    / rate;
            MediaTime::from_seconds_f64(seconds, 1_000_000).ok()
        }
        AutomationOwner::Track(_) | AutomationOwner::Timeline(_) => Some(local_time),
        AutomationOwner::TransitionDefinition(transition_id)
        | AutomationOwner::TransitionInstance { transition_id, .. } => project
            .transitions
            .get(transition_id)?
            .interval()
            .ok()?
            .start
            .checked_add(local_time)
            .ok(),
    }
}

/// Convert frame-snapped host Timeline time back to item-local automation
/// time, clamped to the visible placement.
pub(crate) fn local_time_for_timeline(
    project: &AuthoringProject,
    owner: &AutomationOwner,
    timeline_time: MediaTime,
) -> Option<MediaTime> {
    match owner {
        AutomationOwner::Item(item_id) => {
            let item = project.items.get(item_id)?;
            let end = item.interval.end().ok()?;
            let timeline_time = timeline_time.clamp(item.interval.start, end);
            item.time_map.local_time(item.interval, timeline_time).ok()
        }
        AutomationOwner::Track(_) | AutomationOwner::Timeline(_) => Some(timeline_time),
        AutomationOwner::TransitionDefinition(transition_id)
        | AutomationOwner::TransitionInstance { transition_id, .. } => {
            let interval = project.transitions.get(transition_id)?.interval().ok()?;
            let end = interval.end().ok()?;
            timeline_time
                .clamp(interval.start, end)
                .checked_sub(interval.start)
                .ok()
        }
    }
}

pub(crate) fn owner_interval(
    project: &AuthoringProject,
    owner: &AutomationOwner,
) -> Option<library::model::authoring::TimelineInterval> {
    match owner {
        AutomationOwner::Item(item_id) => project.items.get(item_id).map(|item| item.interval),
        AutomationOwner::Track(track_id) => {
            let timeline_id = project.tracks.get(track_id)?.timeline_id;
            let timeline = project.timelines.get(&timeline_id)?;
            TimelineInterval::new(MediaTime::zero(), timeline.duration).ok()
        }
        AutomationOwner::Timeline(timeline_id) => {
            let timeline = project.timelines.get(timeline_id)?;
            TimelineInterval::new(MediaTime::zero(), timeline.duration).ok()
        }
        AutomationOwner::TransitionDefinition(transition_id)
        | AutomationOwner::TransitionInstance { transition_id, .. } => project
            .transitions
            .get(transition_id)
            .and_then(|transition| transition.interval().ok()),
    }
}

pub(crate) fn component_value(
    value: &PropertyValue,
    component: CurveValueComponent,
) -> Option<f64> {
    match (value, component) {
        (PropertyValue::Number(value), CurveValueComponent::Scalar) => Some(value.into_inner()),
        (PropertyValue::Integer(value), CurveValueComponent::Scalar) => Some(*value as f64),
        (PropertyValue::Vec2(value), CurveValueComponent::X) => Some(value.x.into_inner()),
        (PropertyValue::Vec2(value), CurveValueComponent::Y) => Some(value.y.into_inner()),
        (PropertyValue::Vec3(value), CurveValueComponent::X) => Some(value.x.into_inner()),
        (PropertyValue::Vec3(value), CurveValueComponent::Y) => Some(value.y.into_inner()),
        (PropertyValue::Vec3(value), CurveValueComponent::Z) => Some(value.z.into_inner()),
        (PropertyValue::Vec4(value), CurveValueComponent::X) => Some(value.x.into_inner()),
        (PropertyValue::Vec4(value), CurveValueComponent::Y) => Some(value.y.into_inner()),
        (PropertyValue::Vec4(value), CurveValueComponent::Z) => Some(value.z.into_inner()),
        (PropertyValue::Vec4(value), CurveValueComponent::W) => Some(value.w.into_inner()),
        _ => None,
    }
}

pub(crate) fn with_component(
    mut value: PropertyValue,
    component: CurveValueComponent,
    replacement: f64,
) -> PropertyValue {
    let replacement = ordered_float::OrderedFloat(replacement);
    match (&mut value, component) {
        (PropertyValue::Number(number), CurveValueComponent::Scalar) => *number = replacement,
        (PropertyValue::Integer(integer), CurveValueComponent::Scalar) => {
            *integer = replacement.into_inner().round() as i64;
        }
        (PropertyValue::Vec2(vector), CurveValueComponent::X) => vector.x = replacement,
        (PropertyValue::Vec2(vector), CurveValueComponent::Y) => vector.y = replacement,
        (PropertyValue::Vec3(vector), CurveValueComponent::X) => vector.x = replacement,
        (PropertyValue::Vec3(vector), CurveValueComponent::Y) => vector.y = replacement,
        (PropertyValue::Vec3(vector), CurveValueComponent::Z) => vector.z = replacement,
        (PropertyValue::Vec4(vector), CurveValueComponent::X) => vector.x = replacement,
        (PropertyValue::Vec4(vector), CurveValueComponent::Y) => vector.y = replacement,
        (PropertyValue::Vec4(vector), CurveValueComponent::Z) => vector.z = replacement,
        (PropertyValue::Vec4(vector), CurveValueComponent::W) => vector.w = replacement,
        _ => {}
    }
    value
}

pub(crate) fn component_name(component: CurveValueComponent) -> &'static str {
    match component {
        CurveValueComponent::Scalar => "value",
        CurveValueComponent::X => "x",
        CurveValueComponent::Y => "y",
        CurveValueComponent::Z => "z",
        CurveValueComponent::W => "w",
    }
}

pub(crate) fn target_metadata(target: &AutomationTarget) -> serde_json::Value {
    match target {
        AutomationTarget::AuthoredProperty { owner, key } => serde_json::json!({
            "kind": "authored_property",
            "owner": authored_property_owner_metadata(*owner),
            "key": key,
        }),
        AutomationTarget::ModuleParameter(id) => {
            serde_json::json!({"kind": "module_parameter", "id": id})
        }
        AutomationTarget::AttachmentModuleParameter {
            attachment_id,
            parameter_id,
        } => serde_json::json!({
            "kind": "attachment_module_parameter",
            "attachment_id": attachment_id,
            "id": parameter_id,
        }),
        AutomationTarget::AttachmentParameter { attachment_id, key } => serde_json::json!({
            "kind": "attachment_parameter",
            "attachment_id": attachment_id,
            "key": key,
        }),
    }
}

fn authored_property_owner_metadata(owner: AuthoringPropertyOwner) -> serde_json::Value {
    match owner {
        AuthoringPropertyOwner::Timeline(timeline_id) => serde_json::json!({
            "kind": "timeline",
            "timeline_id": timeline_id,
        }),
        AuthoringPropertyOwner::Track(track_id) => serde_json::json!({
            "kind": "track",
            "track_id": track_id,
        }),
        AuthoringPropertyOwner::Item(item_id) => serde_json::json!({
            "kind": "item",
            "item_id": item_id,
        }),
        AuthoringPropertyOwner::TextEnsemble {
            item_id,
            operation_id,
        } => serde_json::json!({
            "kind": "text_ensemble",
            "item_id": item_id,
            "operation_id": operation_id,
        }),
        AuthoringPropertyOwner::Appearance {
            item_id,
            operation_id,
        } => serde_json::json!({
            "kind": "appearance",
            "item_id": item_id,
            "operation_id": operation_id,
        }),
    }
}

pub(crate) fn lane_metadata(lane: &AutomationLaneId) -> serde_json::Value {
    serde_json::json!({
        "owner": owner_metadata(&lane.owner),
        "target": target_metadata(&lane.target),
    })
}

pub(crate) fn owner_metadata(owner: &AutomationOwner) -> serde_json::Value {
    match owner {
        AutomationOwner::Item(item_id) => serde_json::json!({
            "kind": "item",
            "item_id": item_id,
        }),
        AutomationOwner::Track(track_id) => serde_json::json!({
            "kind": "track",
            "track_id": track_id,
        }),
        AutomationOwner::Timeline(timeline_id) => serde_json::json!({
            "kind": "timeline",
            "timeline_id": timeline_id,
        }),
        AutomationOwner::TransitionDefinition(transition_id) => serde_json::json!({
            "kind": "transition_definition",
            "transition_id": transition_id,
        }),
        AutomationOwner::TransitionInstance {
            transition_id,
            instance_path,
        } => serde_json::json!({
            "kind": "transition_instance",
            "transition_id": transition_id,
            "instance_path": instance_path,
        }),
    }
}

fn components_for(value: Option<&PropertyValue>) -> &'static [CurveValueComponent] {
    match value {
        Some(PropertyValue::Number(_) | PropertyValue::Integer(_)) => {
            &[CurveValueComponent::Scalar]
        }
        Some(PropertyValue::Vec2(_)) => &[CurveValueComponent::X, CurveValueComponent::Y],
        Some(PropertyValue::Vec3(_)) => &[
            CurveValueComponent::X,
            CurveValueComponent::Y,
            CurveValueComponent::Z,
        ],
        Some(PropertyValue::Vec4(_)) => &[
            CurveValueComponent::X,
            CurveValueComponent::Y,
            CurveValueComponent::Z,
            CurveValueComponent::W,
        ],
        _ => &[],
    }
}

fn humanize_label(key: &str) -> String {
    let mut output = String::with_capacity(key.len());
    let mut uppercase_next = true;
    for character in key.chars() {
        if matches!(character, '_' | '-') {
            output.push(' ');
            uppercase_next = true;
        } else if uppercase_next {
            output.extend(character.to_uppercase());
            uppercase_next = false;
        } else {
            output.push(character);
        }
    }
    output
}

#[cfg(test)]
mod tests;
