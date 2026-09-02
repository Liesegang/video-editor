//! Frame assembly from the Timeline-first authoring model.
//!
//! This evaluator consumes only [`AuthoringProject`] and its derived
//! [`RenderPlan`]. It intentionally has no conversion path from the former
//! Composition/Track/Clip graph model.

use std::collections::HashSet;

use ordered_float::OrderedFloat;

use crate::core::render_plan::{
    CompiledModuleOperation, ModuleInvocationOwner, PlannedSource, RenderPlan,
};
use crate::core::timeline_runtime::map_composition_time;
use crate::error::LibraryError;
use crate::model::BlendMode;
use crate::model::authoring::{
    AttachmentOwner, AttachmentStage, AuthoringProject, ShapeKind, SourceRef, Timeline, TimelineId,
    TimelineItem,
};
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::effect::ImageEffect;
use crate::model::frame::entity::{
    FrameBounds, FrameContent, FrameGroup, FrameGroupKind, FrameItem, FrameObject, ImageSurface,
    StyleConfig,
};
use crate::model::frame::frame::{FrameInfo, Region};
use crate::model::frame::transform::Transform;
use crate::model::project::asset::AssetKind;
use crate::model::project::property::{PropertyMap, PropertyValue};

pub fn evaluate_authoring_frame(
    project: &AuthoringProject,
    plan: &RenderPlan,
    frame_number: u64,
    render_scale: f64,
    region: Option<Region>,
) -> Result<FrameInfo, LibraryError> {
    project.validate().map_err(LibraryError::Validation)?;
    if plan.root_timeline_id != project.root_timeline_id {
        return Err(LibraryError::Validation(
            "RenderPlan root does not match the Project root Timeline".to_string(),
        ));
    }
    let root = project
        .timelines
        .get(&plan.root_timeline_id)
        .ok_or_else(|| LibraryError::Validation("Root Timeline is missing".to_string()))?;
    let time = frame_number as f64 / root.fps.into_inner();
    let mut items = collect_timeline_items(project, plan, root, time, &mut HashSet::new())?;
    let root_effects = attachment_effects(
        project,
        plan,
        &AttachmentOwner::Timeline {
            timeline_id: root.id,
        },
        AttachmentStage::TimelinePostComposite,
        time,
    )?;
    let background_color = if root_effects.is_empty() {
        root.background_color.clone()
    } else {
        items = vec![FrameItem::Group(FrameGroup {
            source_id: root.id.as_uuid(),
            kind: FrameGroupKind::Timeline,
            width: root.width,
            height: root.height,
            background_color: root.background_color.clone(),
            inherited_transforms: Vec::new(),
            transform: Transform::default(),
            blend_mode: BlendMode::Normal,
            effect_time: OrderedFloat(time),
            effects: root_effects,
            items,
        })];
        transparent()
    };
    Ok(FrameInfo {
        width: root.width,
        height: root.height,
        background_color,
        color_profile: root.color_profile.clone(),
        render_scale: OrderedFloat(render_scale),
        now_time: OrderedFloat(time),
        region,
        items,
    })
}

fn collect_timeline_items(
    project: &AuthoringProject,
    plan: &RenderPlan,
    timeline: &Timeline,
    timeline_time: f64,
    active: &mut HashSet<TimelineId>,
) -> Result<Vec<FrameItem>, LibraryError> {
    if !active.insert(timeline.id) {
        return Err(LibraryError::Validation(format!(
            "Nested Timeline cycle reaches {}",
            timeline.id
        )));
    }
    let compiled = plan.timelines.get(&timeline.id).ok_or_else(|| {
        LibraryError::Validation(format!("RenderPlan is missing Timeline {}", timeline.id))
    })?;
    let mut output = Vec::new();
    for track_id in &timeline.track_order {
        let track = project.tracks.get(track_id).ok_or_else(|| {
            LibraryError::Validation(format!("Timeline {} has a missing Track", timeline.id))
        })?;
        let mut children = Vec::new();
        for scheduled in compiled.schedule.iter().filter(|scheduled| {
            scheduled.track_id == *track_id && scheduled.interval.contains(timeline_time)
        }) {
            let item = project.items.get(&scheduled.item_id).ok_or_else(|| {
                LibraryError::Validation(format!(
                    "RenderPlan refers to a missing Timeline item {}",
                    scheduled.item_id
                ))
            })?;
            if item.track_id != scheduled.track_id || item.interval != scheduled.interval {
                return Err(LibraryError::Validation(format!(
                    "RenderPlan schedule is stale for Timeline item {}",
                    item.id
                )));
            }
            children.push(collect_item(
                project,
                plan,
                timeline,
                timeline_time,
                item,
                &scheduled.source,
                active,
            )?);
        }
        if !children.is_empty() {
            let effects = attachment_effects(
                project,
                plan,
                &AttachmentOwner::Track { track_id: track.id },
                AttachmentStage::TrackPostComposite,
                timeline_time,
            )?;
            output.push(FrameItem::Group(FrameGroup {
                source_id: track.id.as_uuid(),
                kind: FrameGroupKind::Track,
                width: timeline.width,
                height: timeline.height,
                background_color: transparent(),
                inherited_transforms: Vec::new(),
                transform: transform_at(&track.authored_properties, timeline_time)?,
                blend_mode: BlendMode::Normal,
                effect_time: OrderedFloat(timeline_time),
                effects,
                items: children,
            }));
        }
    }
    active.remove(&timeline.id);
    Ok(output)
}

fn collect_item(
    project: &AuthoringProject,
    plan: &RenderPlan,
    owner_timeline: &Timeline,
    timeline_time: f64,
    item: &TimelineItem,
    planned_source: &PlannedSource,
    active: &mut HashSet<TimelineId>,
) -> Result<FrameItem, LibraryError> {
    let local_time = timeline_time - item.interval.start.into_inner();
    let mut child = match (&item.source, planned_source) {
        (SourceRef::Asset { asset_id, time_map }, PlannedSource::Asset) => {
            let source_time = time_map.source_start.into_inner()
                + local_time * time_map.playback_rate.into_inner();
            asset_item(project, item, *asset_id, source_time)?
        }
        (SourceRef::Text { text }, PlannedSource::Text) => text_item(item, text, local_time)?,
        (SourceRef::Shape { shape }, PlannedSource::Shape) => {
            shape_item(item, shape.shape_kind, &shape.parameters)?
        }
        (SourceRef::Solid { color }, PlannedSource::Solid) => solid_item(
            item,
            owner_timeline.width,
            owner_timeline.height,
            color.clone(),
        ),
        (SourceRef::Composition(instance), PlannedSource::Composition { timeline_id })
            if instance.timeline_id == *timeline_id =>
        {
            let nested = project.timelines.get(timeline_id).ok_or_else(|| {
                LibraryError::Validation(format!("Nested Timeline {timeline_id} is missing"))
            })?;
            let nested_time = map_composition_time(
                instance,
                item.interval,
                nested.duration.into_inner(),
                timeline_time,
            )
            .map_err(LibraryError::Validation)?
            .ok_or_else(|| {
                LibraryError::Render(format!(
                    "Active item {} did not map to nested Timeline time",
                    item.id
                ))
            })?;
            let effects = attachment_effects(
                project,
                plan,
                &AttachmentOwner::Timeline {
                    timeline_id: nested.id,
                },
                AttachmentStage::TimelinePostComposite,
                nested_time,
            )?;
            FrameItem::Group(FrameGroup {
                source_id: nested.id.as_uuid(),
                kind: FrameGroupKind::Timeline,
                width: nested.width,
                height: nested.height,
                background_color: nested.background_color.clone(),
                inherited_transforms: Vec::new(),
                transform: Transform::default(),
                blend_mode: BlendMode::Normal,
                effect_time: OrderedFloat(nested_time),
                effects,
                items: collect_timeline_items(project, plan, nested, nested_time, active)?,
            })
        }
        (SourceRef::Module { .. }, PlannedSource::Module { .. }) => {
            return Err(LibraryError::Render(format!(
                "Module source on Timeline item {} cannot execute yet",
                item.id
            )));
        }
        _ => {
            return Err(LibraryError::Validation(format!(
                "RenderPlan source does not match Timeline item {}",
                item.id
            )));
        }
    };
    let pre_effects = attachment_effects(
        project,
        plan,
        &AttachmentOwner::Item { item_id: item.id },
        AttachmentStage::ItemPreTransform,
        local_time,
    )?;
    if !pre_effects.is_empty() {
        child = FrameItem::Group(FrameGroup {
            source_id: item.id.as_uuid(),
            kind: FrameGroupKind::Effect,
            width: owner_timeline.width,
            height: owner_timeline.height,
            background_color: transparent(),
            inherited_transforms: Vec::new(),
            transform: Transform::default(),
            blend_mode: BlendMode::Normal,
            effect_time: OrderedFloat(local_time),
            effects: pre_effects,
            items: vec![child],
        });
    }
    let post_effects = attachment_effects(
        project,
        plan,
        &AttachmentOwner::Item { item_id: item.id },
        AttachmentStage::ItemPostTransform,
        local_time,
    )?;
    let inherited_transforms = inherited_transforms(project, item, timeline_time)?;
    let mut transform = transform_at(&item.authored_properties, local_time)?;
    transform.opacity *= inherited_transforms
        .iter()
        .map(|transform| transform.opacity)
        .product::<f64>();
    Ok(FrameItem::Group(FrameGroup {
        source_id: item.id.as_uuid(),
        kind: FrameGroupKind::TimelineItem,
        width: owner_timeline.width,
        height: owner_timeline.height,
        background_color: transparent(),
        inherited_transforms,
        transform,
        blend_mode: BlendMode::Normal,
        effect_time: OrderedFloat(local_time),
        effects: post_effects,
        items: vec![child],
    }))
}

fn asset_item(
    project: &AuthoringProject,
    item: &TimelineItem,
    asset_id: uuid::Uuid,
    source_time: f64,
) -> Result<FrameItem, LibraryError> {
    let asset = project
        .assets
        .iter()
        .find(|asset| asset.id == asset_id)
        .ok_or_else(|| LibraryError::Validation(format!("Asset {asset_id} is missing")))?;
    let surface = ImageSurface {
        asset_id: Some(asset.id),
        file_path: asset.path.clone(),
        effects: Vec::new(),
        input_color_space: None,
        output_color_space: None,
        transform: Transform::default(),
    };
    let content = match asset.kind {
        AssetKind::Video => FrameContent::Video {
            surface,
            source_time,
            stream_index: asset.stream_index,
        },
        AssetKind::Image => FrameContent::Image { surface },
        AssetKind::Audio => {
            return Ok(FrameItem::Group(FrameGroup {
                source_id: item.id.as_uuid(),
                kind: FrameGroupKind::TimelineItem,
                width: 0,
                height: 0,
                background_color: transparent(),
                inherited_transforms: Vec::new(),
                transform: Transform::default(),
                blend_mode: BlendMode::Normal,
                effect_time: OrderedFloat(source_time),
                effects: Vec::new(),
                items: Vec::new(),
            }));
        }
        AssetKind::Model3D | AssetKind::Other => {
            return Err(LibraryError::Render(format!(
                "Asset {} has no visual Timeline renderer",
                asset.id
            )));
        }
    };
    Ok(FrameItem::Object(FrameObject {
        source_node_id: item.id.as_uuid(),
        spatial_transform_node_id: None,
        spatial_transform: Box::default(),
        content_bounds: match (asset.width, asset.height) {
            (Some(width), Some(height)) => {
                Some(FrameBounds::new(0.0, 0.0, width as f32, height as f32))
            }
            _ => None,
        },
        content,
    }))
}

fn text_item(item: &TimelineItem, text: &str, local_time: f64) -> Result<FrameItem, LibraryError> {
    let font = string_property(&item.authored_properties, "font", local_time)?
        .unwrap_or_else(|| "Arial".to_string());
    let size = number_property(&item.authored_properties, "font_size", local_time)?.unwrap_or(48.0);
    let color = color_property(&item.authored_properties, "color", local_time)?
        .unwrap_or_else(crate::model::frame::color::Color::white);
    Ok(FrameItem::Object(FrameObject {
        source_node_id: item.id.as_uuid(),
        spatial_transform_node_id: None,
        spatial_transform: Box::default(),
        content_bounds: None,
        content: FrameContent::Text {
            text: text.to_string(),
            font,
            size,
            styles: vec![StyleConfig {
                id: item.id.as_uuid(),
                style: DrawStyle::Fill { color, offset: 0.0 },
            }],
            effects: Vec::new(),
            ensemble: None,
            transform: Transform::default(),
        },
    }))
}

fn solid_item(
    item: &TimelineItem,
    width: u64,
    height: u64,
    color: crate::model::frame::color::Color,
) -> FrameItem {
    shape_object(
        item,
        format!("M 0 0 H {width} V {height} H 0 Z"),
        None,
        color,
        Some(FrameBounds::new(0.0, 0.0, width as f32, height as f32)),
    )
}

fn shape_item(
    item: &TimelineItem,
    kind: ShapeKind,
    parameters: &std::collections::HashMap<String, PropertyValue>,
) -> Result<FrameItem, LibraryError> {
    let width = direct_number(parameters, "width").unwrap_or(100.0);
    let height = direct_number(parameters, "height").unwrap_or(100.0);
    let color = match parameters.get("color") {
        Some(PropertyValue::Color(color)) => color.clone(),
        Some(_) => return Err(type_error("shape color", "Color")),
        None => crate::model::frame::color::Color::white(),
    };
    let (path, canonical) = match kind {
        ShapeKind::Rectangle => (format!("M 0 0 H {width} V {height} H 0 Z"), None),
        ShapeKind::Ellipse => (
            format!(
                "M {} 0 A {} {} 0 1 1 {} 0 A {} {} 0 1 1 {} 0 Z",
                width,
                width / 2.0,
                height / 2.0,
                0.0,
                width / 2.0,
                height / 2.0,
                width
            ),
            None,
        ),
        ShapeKind::Path => match parameters.get("path") {
            Some(PropertyValue::Path(path)) => (
                crate::model::path::write_legacy_svg_path_data(path)
                    .map_err(|error| LibraryError::Render(error.to_string()))?,
                Some(path.clone()),
            ),
            _ => return Err(type_error("shape path", "Path")),
        },
    };
    Ok(shape_object(
        item,
        path,
        canonical,
        color,
        Some(FrameBounds::new(0.0, 0.0, width as f32, height as f32)),
    ))
}

fn shape_object(
    item: &TimelineItem,
    path: String,
    canonical_path: Option<crate::model::path::PathValue>,
    color: crate::model::frame::color::Color,
    content_bounds: Option<FrameBounds>,
) -> FrameItem {
    FrameItem::Object(FrameObject {
        source_node_id: item.id.as_uuid(),
        spatial_transform_node_id: None,
        spatial_transform: Box::default(),
        content_bounds,
        content: FrameContent::Shape {
            path,
            canonical_path,
            styles: vec![StyleConfig {
                id: item.id.as_uuid(),
                style: DrawStyle::Fill { color, offset: 0.0 },
            }],
            path_effects: Vec::new(),
            effects: Vec::new(),
            ensemble: None,
            transform: Transform::default(),
        },
    })
}

fn inherited_transforms(
    project: &AuthoringProject,
    item: &TimelineItem,
    timeline_time: f64,
) -> Result<Vec<Transform>, LibraryError> {
    let mut transforms = Vec::new();
    let mut parent_id = item.parent;
    while let Some(id) = parent_id {
        let parent = project.items.get(&id).ok_or_else(|| {
            LibraryError::Validation(format!("Timeline item {} has a missing parent", item.id))
        })?;
        let local_time = timeline_time - parent.interval.start.into_inner();
        transforms.push(transform_at(&parent.authored_properties, local_time)?);
        parent_id = parent.parent;
    }
    transforms.reverse();
    Ok(transforms)
}

fn attachment_effects(
    project: &AuthoringProject,
    plan: &RenderPlan,
    owner: &AttachmentOwner,
    stage: AttachmentStage,
    time: f64,
) -> Result<Vec<ImageEffect>, LibraryError> {
    let mut invocations: Vec<_> = plan
        .module_invocations
        .iter()
        .filter_map(|invocation| match &invocation.owner {
            ModuleInvocationOwner::Attachment {
                attachment_id,
                owner: invocation_owner,
                stage: invocation_stage,
            } if invocation_owner == owner && *invocation_stage == stage => {
                let attachment = project.attachments.get(attachment_id)?;
                Some((attachment.order, *attachment_id, invocation))
            }
            _ => None,
        })
        .collect();
    invocations.sort_by_key(|(order, attachment_id, _)| (*order, *attachment_id));
    let mut effects = Vec::new();
    for (_, _, invocation) in invocations {
        let instance = project
            .module_instances
            .get(&invocation.module_instance_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "RenderPlan refers to missing Module instance {}",
                    invocation.module_instance_id
                ))
            })?;
        let authored = project
            .module_definitions
            .get(&invocation.definition_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "RenderPlan refers to missing Module definition {}",
                    invocation.definition_id
                ))
            })?;
        let compiled = plan
            .module_definitions
            .get(&invocation.definition_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "RenderPlan has no compiled Module definition {}",
                    invocation.definition_id
                ))
            })?;
        for operation in &compiled.operations {
            let CompiledModuleOperation::ImageEffect {
                node_id,
                effect_type,
                enabled,
                bypassed,
                properties,
            } = operation;
            if !enabled || *bypassed {
                continue;
            }
            let mut values = properties
                .iter()
                .map(|(name, property)| {
                    property
                        .evaluate_at(time)
                        .map(|value| (name.clone(), value))
                        .map_err(|error| {
                            LibraryError::Render(format!(
                                "Cannot evaluate Module property '{name}': {error}"
                            ))
                        })
                })
                .collect::<Result<std::collections::HashMap<_, _>, _>>()?;
            for published in authored
                .published_parameters
                .iter()
                .filter(|published| published.target.node_id == *node_id)
            {
                let key = published
                    .target
                    .port
                    .strip_prefix(crate::plugin::PROPERTY_PORT_PREFIX)
                    .unwrap_or(&published.target.port)
                    .to_string();
                let value = instance
                    .parameter_overrides
                    .get(&published.id)
                    .unwrap_or(&published.default_value)
                    .clone();
                values.insert(key, value);
            }
            effects.push(ImageEffect {
                effect_type: effect_type.clone(),
                properties: values,
            });
        }
    }
    Ok(effects)
}

fn transform_at(properties: &PropertyMap, time: f64) -> Result<Transform, LibraryError> {
    let mut transform = Transform::default();
    if let Some(value) = sample(properties, "position", time)? {
        let PropertyValue::Vec2(value) = value else {
            return Err(type_error("position", "Vec2"));
        };
        transform.position.x = value.x.into_inner();
        transform.position.y = value.y.into_inner();
    }
    if let Some(value) = sample(properties, "scale", time)? {
        let PropertyValue::Vec2(value) = value else {
            return Err(type_error("scale", "Vec2"));
        };
        transform.scale.x = value.x.into_inner();
        transform.scale.y = value.y.into_inner();
    }
    if let Some(value) = sample(properties, "anchor", time)? {
        let PropertyValue::Vec2(value) = value else {
            return Err(type_error("anchor", "Vec2"));
        };
        transform.anchor.x = value.x.into_inner();
        transform.anchor.y = value.y.into_inner();
    }
    transform.rotation = number_property(properties, "rotation", time)?.unwrap_or(0.0);
    transform.opacity = number_property(properties, "opacity", time)?.unwrap_or(1.0);
    Ok(transform)
}

fn sample(
    properties: &PropertyMap,
    key: &str,
    time: f64,
) -> Result<Option<PropertyValue>, LibraryError> {
    properties
        .get(key)
        .map(|property| {
            property.evaluate_at(time).map_err(|error| {
                LibraryError::Render(format!(
                    "Cannot evaluate Timeline property '{key}': {error}"
                ))
            })
        })
        .transpose()
}

fn number_property(
    properties: &PropertyMap,
    key: &str,
    time: f64,
) -> Result<Option<f64>, LibraryError> {
    match sample(properties, key, time)? {
        Some(PropertyValue::Number(value)) => Ok(Some(value.into_inner())),
        Some(PropertyValue::Integer(value)) => Ok(Some(value as f64)),
        Some(_) => Err(type_error(key, "Number")),
        None => Ok(None),
    }
}

fn string_property(
    properties: &PropertyMap,
    key: &str,
    time: f64,
) -> Result<Option<String>, LibraryError> {
    match sample(properties, key, time)? {
        Some(PropertyValue::String(value)) => Ok(Some(value)),
        Some(_) => Err(type_error(key, "String")),
        None => Ok(None),
    }
}

fn color_property(
    properties: &PropertyMap,
    key: &str,
    time: f64,
) -> Result<Option<crate::model::frame::color::Color>, LibraryError> {
    match sample(properties, key, time)? {
        Some(PropertyValue::Color(value)) => Ok(Some(value)),
        Some(_) => Err(type_error(key, "Color")),
        None => Ok(None),
    }
}

fn direct_number(
    values: &std::collections::HashMap<String, PropertyValue>,
    key: &str,
) -> Option<f64> {
    match values.get(key) {
        Some(PropertyValue::Number(value)) => Some(value.into_inner()),
        Some(PropertyValue::Integer(value)) => Some(*value as f64),
        _ => None,
    }
}

fn type_error(property: &str, expected: &str) -> LibraryError {
    LibraryError::Validation(format!("Timeline property '{property}' must be {expected}"))
}

fn transparent() -> crate::model::frame::color::Color {
    crate::model::frame::color::Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }
}

#[cfg(test)]
mod tests {
    use crate::animation::EasingFunction;
    use crate::core::render_plan::RenderPlanCompiler;
    use crate::model::authoring::{
        Attachment, AttachmentId, AttachmentOwner, AttachmentStage, AuthoringSession,
        ModuleDefinition, ModuleDefinitionId, ModuleGraph, ModuleInstance, ModuleInstanceId,
        ModulePortAddress, ModuleRole, PublishedParameter, PublishedParameterId, SourceRef,
        TimelineInterval,
    };
    use crate::model::frame::entity::{FrameContent, FrameItem};
    use crate::model::project::property::{Keyframe, Property, PropertyValue, Vec2};

    use super::*;

    #[test]
    fn text_item_is_assembled_from_timeline_without_creating_a_node() {
        let project = AuthoringProject::new("Frame", 1280, 720, 10.0, 3.0).expect("valid Project");
        let track_id = *project.tracks.keys().next().expect("default Track");
        let mut session = AuthoringSession::new(project).expect("session");
        let (item_id, _) = session
            .add_item(
                track_id,
                "Title".to_string(),
                SourceRef::Text {
                    text: "Timeline first".to_string(),
                },
                TimelineInterval::new(0.0, 2.0).expect("interval"),
                0,
            )
            .expect("add item");
        session
            .set_item_property(
                item_id,
                "position".to_string(),
                Property::keyframe(vec![
                    Keyframe::new(
                        0.0,
                        PropertyValue::Vec2(Vec2 {
                            x: OrderedFloat(0.0),
                            y: OrderedFloat(0.0),
                        }),
                        EasingFunction::Linear,
                    ),
                    Keyframe::new(
                        1.0,
                        PropertyValue::Vec2(Vec2 {
                            x: OrderedFloat(100.0),
                            y: OrderedFloat(50.0),
                        }),
                        EasingFunction::Linear,
                    ),
                ]),
            )
            .expect("keyframe position");
        let project = session.into_project();
        let plan = RenderPlanCompiler::compile(&project).expect("compile");
        let frame = evaluate_authoring_frame(&project, &plan, 5, 1.0, None).expect("frame");

        let FrameItem::Group(track) = &frame.items[0] else {
            panic!("Track group expected");
        };
        let FrameItem::Group(item) = &track.items[0] else {
            panic!("Timeline item group expected");
        };
        assert_eq!(item.kind, FrameGroupKind::TimelineItem);
        assert_eq!(item.transform.position.x, 50.0);
        let FrameItem::Object(object) = &item.items[0] else {
            panic!("Text object expected");
        };
        assert!(matches!(
            &object.content,
            FrameContent::Text { text, .. } if text == "Timeline first"
        ));
    }

    #[test]
    fn inactive_timeline_items_do_not_enter_the_frame() {
        let project = AuthoringProject::new("Frame", 640, 360, 10.0, 3.0).expect("valid Project");
        let track_id = *project.tracks.keys().next().expect("default Track");
        let mut session = AuthoringSession::new(project).expect("session");
        session
            .add_item(
                track_id,
                "Late".to_string(),
                SourceRef::Solid {
                    color: crate::model::frame::color::Color::white(),
                },
                TimelineInterval::new(2.0, 1.0).expect("interval"),
                0,
            )
            .expect("add item");
        let project = session.into_project();
        let plan = RenderPlanCompiler::compile(&project).expect("compile");
        let frame = evaluate_authoring_frame(&project, &plan, 10, 1.0, None).expect("frame");
        assert!(frame.items.is_empty());
    }

    #[test]
    fn stale_render_plan_is_rejected_instead_of_rendering_old_placement() {
        let project = AuthoringProject::new("Frame", 640, 360, 10.0, 3.0).expect("valid Project");
        let track_id = *project.tracks.keys().next().expect("default Track");
        let mut session = AuthoringSession::new(project).expect("session");
        let (item_id, _) = session
            .add_item(
                track_id,
                "Solid".to_string(),
                SourceRef::Solid {
                    color: crate::model::frame::color::Color::white(),
                },
                TimelineInterval::new(0.0, 2.0).expect("interval"),
                0,
            )
            .expect("add item");
        let mut project = session.into_project();
        let plan = RenderPlanCompiler::compile(&project).expect("compile");
        project.items.get_mut(&item_id).expect("item").interval =
            TimelineInterval::new(0.0, 1.0).expect("new interval");

        let error = evaluate_authoring_frame(&project, &plan, 5, 1.0, None)
            .expect_err("stale plan must fail");
        assert!(error.to_string().contains("stale"));
    }

    #[test]
    fn child_keeps_layer_order_while_inheriting_parent_transform() {
        let project = AuthoringProject::new("Parents", 640, 360, 10.0, 3.0).expect("valid Project");
        let track_id = *project.tracks.keys().next().expect("default Track");
        let mut session = AuthoringSession::new(project).expect("session");
        let (parent_id, _) = session
            .add_item(
                track_id,
                "Parent".to_string(),
                SourceRef::Text {
                    text: "P".to_string(),
                },
                TimelineInterval::new(0.0, 2.0).expect("interval"),
                0,
            )
            .expect("parent");
        let (child_id, _) = session
            .add_item(
                track_id,
                "Child".to_string(),
                SourceRef::Text {
                    text: "C".to_string(),
                },
                TimelineInterval::new(0.0, 2.0).expect("interval"),
                1,
            )
            .expect("child");
        session
            .set_item_property(
                parent_id,
                "position".to_string(),
                Property::constant(PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(25.0),
                    y: OrderedFloat(10.0),
                })),
            )
            .expect("parent position");
        session
            .set_item_property(
                parent_id,
                "opacity".to_string(),
                Property::constant(PropertyValue::Number(OrderedFloat(0.5))),
            )
            .expect("parent opacity");
        let mut project = session.into_project();
        project.items.get_mut(&child_id).expect("child").parent = Some(parent_id);
        let plan = RenderPlanCompiler::compile(&project).expect("compile");
        let frame = evaluate_authoring_frame(&project, &plan, 5, 1.0, None).expect("frame");

        let FrameItem::Group(track) = &frame.items[0] else {
            panic!("Track group expected");
        };
        let FrameItem::Group(child) = &track.items[1] else {
            panic!("Child item group expected");
        };
        assert_eq!(child.source_id, child_id.as_uuid());
        assert_eq!(child.inherited_transforms.len(), 1);
        assert_eq!(child.inherited_transforms[0].position.x, 25.0);
        assert_eq!(child.transform.opacity, 0.5);
    }

    #[test]
    fn published_blur_parameter_becomes_item_effect_without_graph_expansion() {
        let project = AuthoringProject::new("Effect", 640, 360, 10.0, 3.0).expect("valid Project");
        let track_id = *project.tracks.keys().next().expect("default Track");
        let mut session = AuthoringSession::new(project).expect("session");
        let (item_id, _) = session
            .add_item(
                track_id,
                "Title".to_string(),
                SourceRef::Text {
                    text: "Blur".to_string(),
                },
                TimelineInterval::new(0.0, 2.0).expect("interval"),
                0,
            )
            .expect("item");
        let mut project = session.into_project();
        let plugins = crate::plugin::PluginManager::default();
        let node = plugins
            .create_effect_operation_node("blur")
            .expect("Blur operation");
        let node_id = node.id;
        let definition_id = ModuleDefinitionId::new();
        let parameter_id = PublishedParameterId::new();
        project.module_definitions.insert(
            definition_id,
            ModuleDefinition {
                id: definition_id,
                name: "Blur".to_string(),
                role: ModuleRole::Effect,
                graph: ModuleGraph {
                    nodes: std::collections::HashMap::from([(node_id, node)]),
                    connections: Vec::new(),
                },
                published_parameters: vec![PublishedParameter {
                    id: parameter_id,
                    name: "Horizontal blur".to_string(),
                    data_type: crate::model::project::PortDataType::Number,
                    default_value: PropertyValue::Number(OrderedFloat(0.0)),
                    target: ModulePortAddress {
                        node_id,
                        port: format!("{}sigma_x", crate::plugin::PROPERTY_PORT_PREFIX),
                    },
                }],
                published_signals: Vec::new(),
                published_actions: Vec::new(),
                version: 1,
            },
        );
        let instance_id = ModuleInstanceId::new();
        project.module_instances.insert(
            instance_id,
            ModuleInstance {
                id: instance_id,
                definition_id,
                parameter_overrides: std::collections::HashMap::from([(
                    parameter_id,
                    PropertyValue::Number(OrderedFloat(12.0)),
                )]),
            },
        );
        let attachment_id = AttachmentId::new();
        project.attachments.insert(
            attachment_id,
            Attachment {
                id: attachment_id,
                owner: AttachmentOwner::Item { item_id },
                module_instance_id: instance_id,
                stage: AttachmentStage::ItemPostTransform,
                order: 0,
            },
        );
        let plan = RenderPlanCompiler::compile(&project).expect("compile");
        let frame = evaluate_authoring_frame(&project, &plan, 5, 1.0, None).expect("frame");

        assert_eq!(plan.module_definitions.len(), 1);
        assert_eq!(plan.module_invocations.len(), 1);
        let FrameItem::Group(track) = &frame.items[0] else {
            panic!("Track group expected");
        };
        let FrameItem::Group(item) = &track.items[0] else {
            panic!("Item group expected");
        };
        assert_eq!(item.effects.len(), 1);
        assert_eq!(item.effects[0].effect_type, "blur");
        assert_eq!(
            item.effects[0].properties["sigma_x"],
            PropertyValue::Number(OrderedFloat(12.0))
        );
    }
}
