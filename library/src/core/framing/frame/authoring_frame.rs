//! Frame assembly from the Timeline-first authoring model.
//!
//! This evaluator consumes only [`AuthoringProject`] and its derived
//! [`RenderPlan`]. It intentionally has no conversion path from the former
//! Composition/Track/Clip graph model.

use std::collections::HashSet;

use ordered_float::OrderedFloat;

use crate::core::binding_runtime::{SignalRuntimeValues, resolve_published_numeric_value};
use crate::core::render_plan::{
    CompiledModuleOperation, ModuleInvocationOwner, PlannedSource, RenderPlan,
};
use crate::core::timeline_runtime::map_composition_time;
use crate::error::LibraryError;
use crate::model::BlendMode;
use crate::model::authoring::{
    AttachmentOwner, AttachmentStage, AuthoringProject, ConstraintKind, InstancePath, ShapeKind,
    SourceRef, Timeline, TimelineId, TimelineItem, TransitionKind,
};
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::effect::ImageEffect;
use crate::model::frame::entity::{
    FrameBounds, FrameContent, FrameGroup, FrameGroupKind, FrameItem, FrameMask, FrameObject,
    ImageSurface, StyleConfig,
};
use crate::model::frame::frame::{FrameInfo, Region};
use crate::model::frame::transform::Transform;
use crate::model::path::{PathPoint, PathSegment, PathValue};
use crate::model::project::asset::AssetKind;
use crate::model::project::property::{PropertyMap, PropertyValue};

pub fn evaluate_authoring_frame(
    project: &AuthoringProject,
    plan: &RenderPlan,
    frame_number: u64,
    render_scale: f64,
    region: Option<Region>,
) -> Result<FrameInfo, LibraryError> {
    evaluate_authoring_timeline_frame_with_signals(
        project,
        plan,
        project.root_timeline_id,
        frame_number,
        render_scale,
        region,
        &InstancePath::root(project.root_timeline_id),
        &SignalRuntimeValues::default(),
    )
}

pub fn evaluate_authoring_timeline_frame(
    project: &AuthoringProject,
    plan: &RenderPlan,
    timeline_id: TimelineId,
    frame_number: u64,
    render_scale: f64,
    region: Option<Region>,
) -> Result<FrameInfo, LibraryError> {
    evaluate_authoring_timeline_frame_with_signals(
        project,
        plan,
        timeline_id,
        frame_number,
        render_scale,
        region,
        &InstancePath::root(timeline_id),
        &SignalRuntimeValues::default(),
    )
}

pub fn evaluate_authoring_timeline_frame_with_signals(
    project: &AuthoringProject,
    plan: &RenderPlan,
    timeline_id: TimelineId,
    frame_number: u64,
    render_scale: f64,
    region: Option<Region>,
    instance_path: &InstancePath,
    runtime_signals: &SignalRuntimeValues,
) -> Result<FrameInfo, LibraryError> {
    project.validate().map_err(LibraryError::Validation)?;
    if plan.root_timeline_id != project.root_timeline_id {
        return Err(LibraryError::Validation(
            "RenderPlan root does not match the Project root Timeline".to_string(),
        ));
    }
    let root = project
        .timelines
        .get(&timeline_id)
        .ok_or_else(|| LibraryError::Validation(format!("Timeline {timeline_id} is missing")))?;
    let time = frame_number as f64 / root.fps.into_inner();
    let mut items = collect_timeline_items(
        project,
        plan,
        root,
        time,
        instance_path,
        runtime_signals,
        &mut HashSet::new(),
    )?;
    let root_effects = attachment_effects(
        project,
        plan,
        &AttachmentOwner::Timeline {
            timeline_id: root.id,
        },
        AttachmentStage::TimelinePostComposite,
        time,
        instance_path,
        runtime_signals,
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
            masks: Vec::new(),
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
    instance_path: &InstancePath,
    runtime_signals: &SignalRuntimeValues,
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
    let matte_sources: HashSet<_> = project
        .items
        .values()
        .filter(|item| {
            project
                .tracks
                .get(&item.track_id)
                .is_some_and(|track| track.timeline_id == timeline.id)
        })
        .filter_map(|item| item.matte.map(|matte| matte.item_id))
        .collect();
    let mut output = Vec::new();
    for track_id in &timeline.track_order {
        let track = project.tracks.get(track_id).ok_or_else(|| {
            LibraryError::Validation(format!("Timeline {} has a missing Track", timeline.id))
        })?;
        let mut children = Vec::new();
        for scheduled in compiled.schedule.iter().filter(|scheduled| {
            scheduled.track_id == *track_id && scheduled.interval.contains(timeline_time)
        }) {
            if matte_sources.contains(&scheduled.item_id) {
                continue;
            }
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
                instance_path,
                runtime_signals,
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
                instance_path,
                runtime_signals,
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
                masks: Vec::new(),
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
    instance_path: &InstancePath,
    runtime_signals: &SignalRuntimeValues,
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
            let nested_path = instance_path.nested(item.id);
            let effects = attachment_effects(
                project,
                plan,
                &AttachmentOwner::Timeline {
                    timeline_id: nested.id,
                },
                AttachmentStage::TimelinePostComposite,
                nested_time,
                &nested_path,
                runtime_signals,
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
                masks: Vec::new(),
                items: collect_timeline_items(
                    project,
                    plan,
                    nested,
                    nested_time,
                    &nested_path,
                    runtime_signals,
                    active,
                )?,
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
        instance_path,
        runtime_signals,
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
            masks: Vec::new(),
            items: vec![child],
        });
    }
    let post_effects = attachment_effects(
        project,
        plan,
        &AttachmentOwner::Item { item_id: item.id },
        AttachmentStage::ItemPostTransform,
        local_time,
        instance_path,
        runtime_signals,
    )?;
    let inherited_transforms = inherited_transforms(project, item, timeline_time)?;
    let mut transform = transform_at(&item.authored_properties, local_time)?;
    apply_constraints(project, item, timeline_time, &mut transform)?;
    transform.opacity *= transition_opacity(project, item, local_time);
    transform.opacity *= inherited_transforms
        .iter()
        .map(|transform| transform.opacity)
        .product::<f64>();
    let content = FrameItem::Group(FrameGroup {
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
        masks: evaluate_masks(project, item, local_time)?,
        items: vec![child],
    });
    let Some(matte_ref) = item.matte else {
        return Ok(content);
    };
    let matte_item = project.items.get(&matte_ref.item_id).ok_or_else(|| {
        LibraryError::Validation(format!("Timeline item {} has a missing Matte", item.id))
    })?;
    let matte = if matte_item.interval.contains(timeline_time) {
        let compiled = plan.timelines.get(&owner_timeline.id).ok_or_else(|| {
            LibraryError::Validation(format!(
                "RenderPlan is missing Timeline {}",
                owner_timeline.id
            ))
        })?;
        let scheduled = compiled
            .schedule
            .iter()
            .find(|scheduled| scheduled.item_id == matte_item.id)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "RenderPlan is missing Matte item {}",
                    matte_item.id
                ))
            })?;
        collect_item(
            project,
            plan,
            owner_timeline,
            timeline_time,
            matte_item,
            &scheduled.source,
            instance_path,
            runtime_signals,
            active,
        )?
    } else {
        FrameItem::Group(FrameGroup {
            source_id: matte_item.id.as_uuid(),
            kind: FrameGroupKind::TimelineItem,
            width: owner_timeline.width,
            height: owner_timeline.height,
            background_color: transparent(),
            inherited_transforms: Vec::new(),
            transform: Transform::default(),
            blend_mode: BlendMode::Normal,
            effect_time: OrderedFloat(local_time),
            effects: Vec::new(),
            masks: Vec::new(),
            items: Vec::new(),
        })
    };
    Ok(FrameItem::Matte {
        content: Box::new(content),
        matte: Box::new(matte),
        mode: matte_ref.mode,
    })
}

fn apply_constraints(
    project: &AuthoringProject,
    item: &TimelineItem,
    timeline_time: f64,
    transform: &mut Transform,
) -> Result<(), LibraryError> {
    let item_local_time = timeline_time - item.interval.start.into_inner();
    for constraint in &item.constraints {
        let target = project
            .items
            .get(&constraint.target_item_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Constraint {} has a missing target",
                    constraint.id
                ))
            })?;
        let target_local_time = timeline_time - target.interval.start.into_inner();
        let target_transform = transform_at(&target.authored_properties, target_local_time)?;
        let influence = constraint
            .influence
            .evaluate_at(timeline_time)
            .ok()
            .and_then(|value| match value {
                PropertyValue::Number(value) => Some(value.into_inner()),
                PropertyValue::Integer(value) => Some(value as f64),
                _ => None,
            })
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        match constraint.kind {
            ConstraintKind::CopyPosition => {
                transform.position.x +=
                    (target_transform.position.x - transform.position.x) * influence;
                transform.position.y +=
                    (target_transform.position.y - transform.position.y) * influence;
            }
            ConstraintKind::CopyRotation => {
                transform.rotation += (target_transform.rotation - transform.rotation) * influence;
            }
            ConstraintKind::CopyScale => {
                transform.scale.x += (target_transform.scale.x - transform.scale.x) * influence;
                transform.scale.y += (target_transform.scale.y - transform.scale.y) * influence;
            }
            ConstraintKind::LookAt => {
                let angle = (target_transform.position.y - transform.position.y)
                    .atan2(target_transform.position.x - transform.position.x)
                    .to_degrees();
                transform.rotation += (angle - transform.rotation) * influence;
            }
            ConstraintKind::FollowPath => {
                let SourceRef::Shape { shape } = &target.source else {
                    return Err(LibraryError::Validation(format!(
                        "Follow Path constraint {} targets a non-Shape item",
                        constraint.id
                    )));
                };
                let Some(PropertyValue::Path(path)) = shape.parameters.get("path") else {
                    return Err(LibraryError::Validation(format!(
                        "Follow Path constraint {} requires a Path Shape target",
                        constraint.id
                    )));
                };
                let progress = constraint
                    .parameters
                    .get("progress")
                    .and_then(|property| property.evaluate_at(item_local_time).ok())
                    .and_then(|value| match value {
                        PropertyValue::Number(value) => Some(value.into_inner()),
                        PropertyValue::Integer(value) => Some(value as f64),
                        _ => None,
                    })
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
                let Some((point, tangent)) = sample_path(path, progress) else {
                    continue;
                };
                let desired_x = target_transform.position.x + point.x();
                let desired_y = target_transform.position.y + point.y();
                transform.position.x += (desired_x - transform.position.x) * influence;
                transform.position.y += (desired_y - transform.position.y) * influence;
                let auto_orient = constraint
                    .parameters
                    .get("auto_orient")
                    .and_then(|property| property.evaluate_at(item_local_time).ok())
                    .is_some_and(|value| matches!(value, PropertyValue::Boolean(true)));
                if auto_orient {
                    let angle = tangent.1.atan2(tangent.0).to_degrees();
                    transform.rotation += (angle - transform.rotation) * influence;
                }
            }
        }
    }
    Ok(())
}

fn evaluate_masks(
    project: &AuthoringProject,
    item: &TimelineItem,
    local_time: f64,
) -> Result<Vec<FrameMask>, LibraryError> {
    item.mask_ids
        .iter()
        .map(|mask_id| {
            let mask = project.masks.get(mask_id).ok_or_else(|| {
                LibraryError::Validation(format!("Timeline item {} has a missing Mask", item.id))
            })?;
            let number =
                |property: &crate::model::project::property::Property, fallback: f64| -> f64 {
                    property
                        .evaluate_at(local_time)
                        .ok()
                        .and_then(|value| match value {
                            PropertyValue::Number(value) => Some(value.into_inner()),
                            PropertyValue::Integer(value) => Some(value as f64),
                            _ => None,
                        })
                        .unwrap_or(fallback)
                };
            Ok(FrameMask {
                path: mask.path.clone(),
                mode: mask.mode,
                inverted: mask.inverted,
                feather: OrderedFloat(number(&mask.feather, 0.0).max(0.0)),
                opacity: OrderedFloat(number(&mask.opacity, 1.0).clamp(0.0, 1.0)),
            })
        })
        .collect()
}

fn sample_path(path: &PathValue, progress: f64) -> Option<(PathPoint, (f64, f64))> {
    let segments: Vec<_> = path
        .contours()
        .iter()
        .flat_map(|contour| {
            let mut from = contour.start();
            let mut result = Vec::new();
            for segment in contour.segments() {
                result.push((from, segment.clone()));
                from = segment_end(segment);
            }
            if contour.is_closed() && from != contour.start() {
                result.push((
                    from,
                    PathSegment::Line {
                        to: contour.start(),
                    },
                ));
            }
            result
        })
        .collect();
    if segments.is_empty() {
        return path
            .contours()
            .first()
            .map(|contour| (contour.start(), (1.0, 0.0)));
    }
    let scaled = progress.clamp(0.0, 1.0) * segments.len() as f64;
    let index = (scaled.floor() as usize).min(segments.len() - 1);
    let t = if progress >= 1.0 {
        1.0
    } else {
        scaled - index as f64
    };
    Some(sample_segment(segments[index].0, &segments[index].1, t))
}

fn segment_end(segment: &PathSegment) -> PathPoint {
    match segment {
        PathSegment::Line { to }
        | PathSegment::Quadratic { to, .. }
        | PathSegment::Conic { to, .. }
        | PathSegment::Cubic { to, .. } => *to,
    }
}

fn sample_segment(from: PathPoint, segment: &PathSegment, t: f64) -> (PathPoint, (f64, f64)) {
    let point = |x: f64, y: f64| PathPoint::new(x, y);
    let (x0, y0) = (from.x(), from.y());
    match segment {
        PathSegment::Line { to } => (
            point(x0 + (to.x() - x0) * t, y0 + (to.y() - y0) * t),
            (to.x() - x0, to.y() - y0),
        ),
        PathSegment::Quadratic { control, to } => {
            let u = 1.0 - t;
            let x = u * u * x0 + 2.0 * u * t * control.x() + t * t * to.x();
            let y = u * u * y0 + 2.0 * u * t * control.y() + t * t * to.y();
            let dx = 2.0 * (u * (control.x() - x0) + t * (to.x() - control.x()));
            let dy = 2.0 * (u * (control.y() - y0) + t * (to.y() - control.y()));
            (point(x, y), (dx, dy))
        }
        PathSegment::Conic {
            control,
            to,
            weight,
        } => {
            let u = 1.0 - t;
            let w = weight.into_inner();
            let denominator = u * u + 2.0 * w * u * t + t * t;
            if denominator.abs() <= f64::EPSILON {
                return (
                    point(x0 + (to.x() - x0) * t, y0 + (to.y() - y0) * t),
                    (to.x() - x0, to.y() - y0),
                );
            }
            let x = (u * u * x0 + 2.0 * w * u * t * control.x() + t * t * to.x()) / denominator;
            let y = (u * u * y0 + 2.0 * w * u * t * control.y() + t * t * to.y()) / denominator;
            let adjacent_t = if t < 1.0 {
                (t + 1.0e-5).min(1.0)
            } else {
                (t - 1.0e-5).max(0.0)
            };
            let adjacent_u = 1.0 - adjacent_t;
            let adjacent_denominator = adjacent_u * adjacent_u
                + 2.0 * w * adjacent_u * adjacent_t
                + adjacent_t * adjacent_t;
            if adjacent_denominator.abs() <= f64::EPSILON {
                return (point(x, y), (to.x() - x0, to.y() - y0));
            }
            let adjacent_x = (adjacent_u * adjacent_u * x0
                + 2.0 * w * adjacent_u * adjacent_t * control.x()
                + adjacent_t * adjacent_t * to.x())
                / adjacent_denominator;
            let adjacent_y = (adjacent_u * adjacent_u * y0
                + 2.0 * w * adjacent_u * adjacent_t * control.y()
                + adjacent_t * adjacent_t * to.y())
                / adjacent_denominator;
            let direction = if t < 1.0 { 1.0 } else { -1.0 };
            (
                point(x, y),
                ((adjacent_x - x) * direction, (adjacent_y - y) * direction),
            )
        }
        PathSegment::Cubic {
            control1,
            control2,
            to,
        } => {
            let u = 1.0 - t;
            let x = u.powi(3) * x0
                + 3.0 * u * u * t * control1.x()
                + 3.0 * u * t * t * control2.x()
                + t.powi(3) * to.x();
            let y = u.powi(3) * y0
                + 3.0 * u * u * t * control1.y()
                + 3.0 * u * t * t * control2.y()
                + t.powi(3) * to.y();
            let dx = 3.0
                * (u * u * (control1.x() - x0)
                    + 2.0 * u * t * (control2.x() - control1.x())
                    + t * t * (to.x() - control2.x()));
            let dy = 3.0
                * (u * u * (control1.y() - y0)
                    + 2.0 * u * t * (control2.y() - control1.y())
                    + t * t * (to.y() - control2.y()));
            (point(x, y), (dx, dy))
        }
    }
}

fn transition_opacity(project: &AuthoringProject, item: &TimelineItem, local_time: f64) -> f64 {
    let mut opacity: f64 = 1.0;
    if let Some(transition) = item
        .transition_in
        .and_then(|id| project.transitions.get(&id))
        && matches!(transition.kind, TransitionKind::CrossDissolve)
    {
        let duration = transition.duration.into_inner();
        if duration > 0.0 {
            opacity *= (local_time / duration).clamp(0.0, 1.0);
        }
    }
    if let Some(transition) = item
        .transition_out
        .and_then(|id| project.transitions.get(&id))
        && matches!(transition.kind, TransitionKind::CrossDissolve)
    {
        let duration = transition.duration.into_inner();
        if duration > 0.0 {
            let remaining = item.interval.duration.into_inner() - local_time;
            opacity *= (remaining / duration).clamp(0.0, 1.0);
        }
    }
    opacity
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
                masks: Vec::new(),
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
    instance_path: &InstancePath,
    runtime_signals: &SignalRuntimeValues,
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
                let value = resolve_published_numeric_value(
                    authored.id,
                    instance,
                    instance_path,
                    published,
                    project.signal_bindings.values(),
                    runtime_signals,
                )
                .map(|effective| effective.value)
                .unwrap_or_else(|| {
                    instance
                        .parameter_overrides
                        .get(&published.id)
                        .unwrap_or(&published.default_value)
                        .clone()
                });
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
        BindingOperator, BindingScope, ModuleDefinition, ModuleDefinitionId, ModuleGraph,
        ModuleInstance, ModuleInstanceId, ModulePortAddress, ModuleRole, PublishedParameter,
        PublishedParameterId, SignalBinding, SignalBindingId, SignalMapping, SignalSource,
        SourceRef, TimelineInterval, Transition, TransitionId, TransitionKind,
    };
    use crate::model::frame::entity::{FrameContent, FrameItem};
    use crate::model::project::property::{Keyframe, Property, PropertyValue, Vec2};

    use super::*;

    #[test]
    fn path_sampling_preserves_curve_endpoints_and_direction() {
        let path = PathValue::new(
            crate::model::path::FillRule::NonZero,
            vec![crate::model::path::PathContour::new(
                PathPoint::new(10.0, 20.0),
                vec![PathSegment::cubic(
                    PathPoint::new(20.0, 20.0),
                    PathPoint::new(30.0, 40.0),
                    PathPoint::new(40.0, 40.0),
                )],
                false,
            )],
        )
        .expect("path");
        let (start, start_tangent) = sample_path(&path, 0.0).expect("start");
        let (end, end_tangent) = sample_path(&path, 1.0).expect("end");
        assert_eq!(start, PathPoint::new(10.0, 20.0));
        assert_eq!(end, PathPoint::new(40.0, 40.0));
        assert!(start_tangent.0 > 0.0);
        assert!(end_tangent.0 > 0.0);
    }

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
    fn cross_dissolve_is_evaluated_as_timeline_owned_opacity() {
        let project =
            AuthoringProject::new("Transition", 640, 360, 10.0, 3.0).expect("valid Project");
        let track_id = *project.tracks.keys().next().expect("default Track");
        let mut session = AuthoringSession::new(project).expect("session");
        let (from_id, _) = session
            .add_item(
                track_id,
                "From".to_string(),
                SourceRef::Solid {
                    color: crate::model::frame::color::Color::black(),
                },
                TimelineInterval::new(0.0, 2.0).expect("interval"),
                0,
            )
            .expect("from item");
        let (to_id, _) = session
            .add_item(
                track_id,
                "To".to_string(),
                SourceRef::Solid {
                    color: crate::model::frame::color::Color::white(),
                },
                TimelineInterval::new(0.0, 2.0).expect("interval"),
                1,
            )
            .expect("to item");
        let mut project = session.into_project();
        let transition_id = TransitionId::new();
        project.transitions.insert(
            transition_id,
            Transition {
                id: transition_id,
                from_item_id: from_id,
                to_item_id: to_id,
                duration: OrderedFloat(1.0),
                kind: TransitionKind::CrossDissolve,
                authored_properties: PropertyMap::new(),
            },
        );
        project.items.get_mut(&from_id).unwrap().transition_out = Some(transition_id);
        project.items.get_mut(&to_id).unwrap().transition_in = Some(transition_id);
        let plan = RenderPlanCompiler::compile(&project).expect("compile");
        let frame = evaluate_authoring_frame(&project, &plan, 5, 1.0, None).expect("frame");
        let FrameItem::Group(track) = &frame.items[0] else {
            panic!("Track group expected");
        };
        let to = track
            .items
            .iter()
            .find_map(|item| match item {
                FrameItem::Group(group) if group.source_id == to_id.as_uuid() => Some(group),
                _ => None,
            })
            .expect("incoming item");
        assert_eq!(to.transform.opacity, 0.5);
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
                output_node_id: Some(node_id),
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

        let binding_id = SignalBindingId::new();
        project.signal_bindings.insert(
            binding_id,
            SignalBinding {
                id: binding_id,
                source: SignalSource::AudioEnvelope {
                    channel: "music".to_string(),
                },
                scope: BindingScope::Instance {
                    instance_path: InstancePath::root(project.root_timeline_id),
                    module_instance_id: instance_id,
                },
                target_parameter_id: parameter_id,
                mapping: SignalMapping {
                    input_min: OrderedFloat(0.0),
                    input_max: OrderedFloat(1.0),
                    output_min: OrderedFloat(0.0),
                    output_max: OrderedFloat(1.0),
                    clamp: true,
                },
                operator: BindingOperator::Multiply,
                smoothing_seconds: OrderedFloat(0.0),
                priority: 0,
            },
        );
        let plan = RenderPlanCompiler::compile(&project).expect("compile Binding");
        let mut signals = SignalRuntimeValues::default();
        signals.set(binding_id, 0.5).expect("finite Signal");
        let frame = evaluate_authoring_timeline_frame_with_signals(
            &project,
            &plan,
            project.root_timeline_id,
            5,
            1.0,
            None,
            &InstancePath::root(project.root_timeline_id),
            &signals,
        )
        .expect("bound frame");
        let FrameItem::Group(track) = &frame.items[0] else {
            panic!("Track group expected");
        };
        let FrameItem::Group(item) = &track.items[0] else {
            panic!("Item group expected");
        };
        assert_eq!(
            item.effects[0].properties["sigma_x"],
            PropertyValue::Number(OrderedFloat(6.0))
        );
    }
}
