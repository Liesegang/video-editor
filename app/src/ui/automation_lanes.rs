//! Shared discovery and mutation boundary for Timeline automation.
//!
//! The Curve Editor and Timeline Dope Sheet consume these exact lanes. UI
//! panels never rediscover Module interfaces or reinterpret authored
//! keyframes independently.

use library::animation::EasingFunction;
use library::editor::{AuthoringKeyframeUpdate, AuthoringPropertyOwner, TimelineEditorService};
use library::model::authoring::{
    AttachmentOwner, AttachmentProcessor, AuthoringProject, MediaTime, SourceRef, TimelineItemId,
};
use library::model::property::{KeyframeId, PropertyValue};

use crate::state::authoring::{AutomationTarget, CurveValueComponent};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AutomationPoint {
    pub id: KeyframeId,
    /// Local source time owned by the Timeline item.
    pub time: MediaTime,
    pub value: PropertyValue,
    pub easing: EasingFunction,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AutomationLane {
    pub target: AutomationTarget,
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
    pub target: AutomationTarget,
    pub component: CurveValueComponent,
    pub label: String,
    pub points: Vec<AutomationChannelPoint>,
}

/// Discover every editable item-owned automation lane in stable UI order.
///
/// Constant authored properties remain visible with no diamonds, and every
/// published Module parameter is exposed even before it receives automation.
pub(crate) fn collect_item_lanes(
    project: &AuthoringProject,
    item_id: TimelineItemId,
) -> Vec<AutomationLane> {
    let Some(item) = project.items.get(&item_id) else {
        return Vec::new();
    };
    let mut lanes = Vec::new();
    let mut properties = item.authored_properties.iter().collect::<Vec<_>>();
    properties.sort_by(|left, right| left.0.cmp(right.0));
    for (key, property) in properties {
        let mut points = property
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
            .collect::<Vec<_>>();
        points.sort_by_key(|point| point.time);
        lanes.push(AutomationLane {
            target: AutomationTarget::AuthoredProperty(key.clone()),
            label: humanize_label(key),
            base_value: property.value().cloned(),
            points,
        });
    }

    if let SourceRef::Module(invocation) = &item.source {
        if let Some((instance, definition)) = project
            .module_instances
            .get(&invocation.instance_id)
            .and_then(|instance| {
                project
                    .module_definitions
                    .get(&instance.definition_id)
                    .map(|definition| (instance, definition))
            })
        {
            for parameter in &definition.interface.parameters {
                let mut points = invocation
                    .automation_tracks
                    .get(&parameter.id)
                    .map(|track| automation_points(&track.keyframes))
                    .unwrap_or_default();
                points.sort_by_key(|point| point.time);
                lanes.push(AutomationLane {
                    target: AutomationTarget::ModuleParameter(parameter.id),
                    label: parameter.name.clone(),
                    base_value: instance
                        .parameter_overrides
                        .get(&parameter.id)
                        .cloned()
                        .or_else(|| Some(parameter.default_value.clone())),
                    points,
                });
            }
        }
    }

    let mut attachments = project
        .attachments
        .values()
        .filter(|attachment| {
            matches!(attachment.owner, AttachmentOwner::Item { item_id: id } if id == item_id)
        })
        .collect::<Vec<_>>();
    attachments.sort_by_key(|attachment| (attachment.stage, attachment.order, attachment.id));
    for attachment in attachments {
        let AttachmentProcessor::BuiltinEffect(effect) = &attachment.processor else {
            continue;
        };
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
                target: AutomationTarget::AttachmentParameter {
                    attachment_id: attachment.id,
                    key: contract.key.clone(),
                },
                label: format!(
                    "{} · {}",
                    humanize_label(&effect.operation.component_id),
                    humanize_label(&contract.key)
                ),
                base_value: Some(parameter.value.clone()),
                points,
            });
        }
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
                target: lane.target.clone(),
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
    item_id: TimelineItemId,
    target: &AutomationTarget,
    keyframe_id: KeyframeId,
    update: AuthoringKeyframeUpdate,
) -> Result<(), library::LibraryError> {
    match target {
        AutomationTarget::AuthoredProperty(key) => service
            .update_authored_property_keyframe(
                AuthoringPropertyOwner::Item(item_id),
                key,
                keyframe_id,
                update,
            )
            .map(|_| ()),
        AutomationTarget::ModuleParameter(parameter_id) => service
            .update_module_parameter_keyframe(item_id, *parameter_id, keyframe_id, update)
            .map(|_| ()),
        AutomationTarget::AttachmentParameter { attachment_id, key } => service
            .update_builtin_effect_parameter_keyframe(*attachment_id, key, keyframe_id, update)
            .map(|_| ()),
    }
}

/// Convert one local automation time into the host Timeline time.
pub(crate) fn timeline_time_for_local(
    project: &AuthoringProject,
    item_id: TimelineItemId,
    local_time: MediaTime,
) -> Option<MediaTime> {
    let item = project.items.get(&item_id)?;
    let rate = item.time_map.playback_rate.to_f64();
    if !rate.is_finite() || rate.abs() <= f64::EPSILON {
        return None;
    }
    let seconds = item.interval.start.to_seconds_f64()
        + (local_time.to_seconds_f64() - item.time_map.source_start.to_seconds_f64()) / rate;
    MediaTime::from_seconds_f64(seconds, 1_000_000).ok()
}

/// Convert frame-snapped host Timeline time back to item-local automation
/// time, clamped to the visible placement.
pub(crate) fn local_time_for_timeline(
    project: &AuthoringProject,
    item_id: TimelineItemId,
    timeline_time: MediaTime,
) -> Option<MediaTime> {
    let item = project.items.get(&item_id)?;
    let end = item.interval.end().ok()?;
    let timeline_time = timeline_time.clamp(item.interval.start, end);
    item.time_map.local_time(item.interval, timeline_time).ok()
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
        AutomationTarget::AuthoredProperty(key) => {
            serde_json::json!({"kind": "authored_property", "key": key})
        }
        AutomationTarget::ModuleParameter(id) => {
            serde_json::json!({"kind": "module_parameter", "id": id})
        }
        AutomationTarget::AttachmentParameter { attachment_id, key } => serde_json::json!({
            "kind": "attachment_parameter",
            "attachment_id": attachment_id,
            "key": key,
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
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use library::editor::TimelineEditorService;
    use library::model::authoring::{
        MediaTime, ModuleDefinition, ModuleDefinitionSharing, ModuleInstance, ModuleInstanceId,
        ModuleInvocation, PublishedParameterId, SourceRef, TimelineInterval, TimelineItem,
        TimelineItemId,
    };
    use library::model::frame::color::Color;
    use library::model::property::PropertyValue;
    use library::plugin::PluginManager;

    use super::*;

    #[test]
    fn authored_and_empty_published_lanes_share_one_discovery_contract() {
        let service = TimelineEditorService::create_default("lanes").expect("service");
        let project = service.snapshot().expect("project");
        let track_id = project.timelines[&project.root_timeline_id].track_order[0];
        let (item_id, _) = service
            .add_item(
                track_id,
                "Solid".to_string(),
                SourceRef::Solid {
                    color: Color::black(),
                },
                TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).unwrap()).unwrap(),
                0,
            )
            .unwrap();
        service
            .set_authored_property_constant(
                AuthoringPropertyOwner::Item(item_id),
                "position".to_string(),
                PropertyValue::from(2.0),
            )
            .unwrap();
        let project = service.snapshot().unwrap();
        let authored = collect_item_lanes(&project, item_id);
        assert_eq!(authored.len(), 1);
        assert_eq!(authored[0].label, "Position");
        assert!(authored[0].points.is_empty());
        assert!(collect_item_keyframed_lanes(&project, item_id).is_empty());

        let (mut definition, output_id) =
            ModuleDefinition::new_image("Module", ModuleDefinitionSharing::Private);
        let parameter_id = PublishedParameterId::new();
        definition
            .interface
            .parameters
            .push(library::model::authoring::PublishedParameter {
                id: parameter_id,
                name: "Amount".to_string(),
                data_type: library::model::project::PortDataType::Number,
                default_value: PropertyValue::from(1.0),
                target: library::model::authoring::ModulePortAddress {
                    node_id: uuid::Uuid::new_v4(),
                    port: "amount".to_string(),
                },
            });
        let definition_id = definition.id;
        let instance_id = ModuleInstanceId::new();
        let module_item = TimelineItemId::new();
        let mut project = (*service.snapshot().unwrap()).clone();
        project.module_definitions.insert(definition_id, definition);
        project.module_instances.insert(
            instance_id,
            ModuleInstance {
                id: instance_id,
                definition_id,
                parameter_overrides: HashMap::new(),
            },
        );
        project.items.insert(
            module_item,
            TimelineItem {
                id: module_item,
                track_id,
                name: "Module".to_string(),
                source: SourceRef::Module(ModuleInvocation {
                    instance_id,
                    output_id,
                    input_bindings: HashMap::new(),
                    automation_tracks: HashMap::new(),
                }),
                interval: TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).unwrap())
                    .unwrap(),
                time_map: Default::default(),
                layer: 1,
                parent: None,
                blend_mode: library::model::BlendMode::Normal,
                authored_properties: Default::default(),
            },
        );
        let module = collect_item_lanes(&project, module_item);
        assert_eq!(module.len(), 1);
        assert_eq!(
            module[0].target,
            AutomationTarget::ModuleParameter(parameter_id)
        );
        assert!(module[0].points.is_empty());
        assert!(collect_item_keyframed_lanes(&project, module_item).is_empty());
    }

    #[test]
    fn local_and_timeline_time_round_trip_through_item_time_map() {
        let service = TimelineEditorService::create_default("time").unwrap();
        let project = service.snapshot().unwrap();
        let track_id = project.timelines[&project.root_timeline_id].track_order[0];
        let (item_id, _) = service
            .add_item(
                track_id,
                "Solid".to_string(),
                SourceRef::Solid {
                    color: Color::black(),
                },
                TimelineInterval::new(MediaTime::new(3, 1).unwrap(), MediaTime::new(5, 1).unwrap())
                    .unwrap(),
                0,
            )
            .unwrap();
        let project = service.snapshot().unwrap();
        let local = MediaTime::new(2, 1).unwrap();
        let timeline = timeline_time_for_local(&project, item_id, local).unwrap();
        assert_eq!(timeline, MediaTime::new(5, 1).unwrap());
        assert_eq!(
            local_time_for_timeline(&project, item_id, timeline),
            Some(local)
        );
    }

    #[test]
    fn builtin_effect_keyframes_keep_one_id_across_inspector_timeline_and_curve() {
        let media = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("test_data")
            .join("e2e_media");
        let fixture =
            library::editor::build_authoring_e2e_fixture(&media, &PluginManager::default())
                .expect("fixture");
        let attachment_id = fixture.info.effect_attachment_ids[0];
        let local_time = MediaTime::new(1, 1).unwrap();
        let (keyframe_id, _) = fixture
            .service
            .upsert_builtin_effect_parameter_keyframe(
                attachment_id,
                "sigma_x",
                local_time,
                PropertyValue::from(4.0),
                None,
            )
            .expect("Inspector keyframe");
        let project = fixture.service.snapshot().unwrap();
        let lanes = collect_item_lanes(&project, fixture.info.text_item_id);
        let target = AutomationTarget::AttachmentParameter {
            attachment_id,
            key: "sigma_x".to_string(),
        };
        let lane = lanes
            .iter()
            .find(|lane| lane.target == target)
            .expect("Timeline effect lane");
        assert_eq!(lane.points[0].id, keyframe_id);
        let curve = numeric_channels(&lanes)
            .into_iter()
            .find(|channel| channel.target == target)
            .expect("Curve effect channel");
        assert_eq!(curve.points[0].id, keyframe_id);

        update_keyframe(
            &fixture.service,
            fixture.info.text_item_id,
            &target,
            keyframe_id,
            AuthoringKeyframeUpdate {
                time: Some(MediaTime::new(3, 2).unwrap()),
                value: None,
                easing: None,
            },
        )
        .expect("shared update");
        let project = fixture.service.snapshot().unwrap();
        let lane = collect_item_lanes(&project, fixture.info.text_item_id)
            .into_iter()
            .find(|lane| lane.target == target)
            .expect("updated lane");
        assert_eq!(lane.points[0].id, keyframe_id);
        assert_eq!(lane.points[0].time, MediaTime::new(3, 2).unwrap());
    }
}
