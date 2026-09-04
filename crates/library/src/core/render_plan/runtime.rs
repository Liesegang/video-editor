use std::collections::{HashMap, HashSet};

use ordered_float::OrderedFloat;

use crate::error::LibraryError;
use crate::model::BlendMode;
use crate::model::authoring::{
    AttachmentOwner, AttachmentProcessor, AttachmentStage, AuthoringProject, DurationPolicy,
    InstanceLocator, InstancePath, ItemOutputStage, MediaInputBinding, MediaOutputKind, MediaTime,
    ModulePortAddress, SourceRef, Timeline, TimelineId, TimelineItem, TimelineItemId,
    TimelineTrackKind,
};
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::effect::ImageEffect;
use crate::model::frame::entity::{
    FrameBounds, FrameContent, FrameGroup, FrameGroupKind, FrameItem, FrameObject, ImageSurface,
    SkSLColorDomain, StyleConfig,
};
use crate::model::frame::frame::{FrameInfo, Region};
use crate::model::frame::transform::Transform;
use crate::model::node::{GeneratorContent, NodeContent};
use crate::model::project::asset::AssetKind;
use crate::model::project::connection::{
    DATA_VALUE_OUTPUT_PORT, DATA_VALUE_PROPERTY, IMAGE_INPUT_PORT, MERGE_IMAGES_PORT,
    NUMBER_RESULT_OUTPUT_PORT, PortDataType, TIME_PORT,
};
use crate::model::project::property::{PropertyMap, PropertyValue};
use crate::plugin::{
    EFFECT_APPLY_OPERATION, EFFECT_CATEGORY, IMAGE_OPACITY_STYLE_COMPONENT_ID,
    IMAGE_TRANSFORM_COMPONENT_ID, STYLE_APPLY_OPERATION, STYLE_CATEGORY, TRANSFORM_APPLY_OPERATION,
    TRANSFORM_CATEGORY, property_name_from_port,
};

use super::{
    CompiledModuleDefinition, CompiledModuleInvocation, CompiledNode, ModuleHost, PlannedSource,
    RenderPlan,
};

mod composition_parameters;
mod frame_values;
mod module_image;
mod module_shape;
mod particle;
mod text_ensemble;
pub(super) mod time_map;

#[cfg(test)]
mod instance_tests;

use frame_values::{
    planned_source_matches, shape_item, solid_item, stage_key, text_item_from_values, transform_at,
    transform_from_values, transparent,
};
use module_image::ModuleImageRuntime;
use text_ensemble::evaluate_text_ensemble;
use time_map::{map_composition_time, unmap_composition_time};

/// Evaluate the root Timeline at an exact frame boundary into the existing
/// renderer's `FrameInfo` IR. No legacy Project or compatibility graph is
/// constructed along this path.
pub fn evaluate_render_plan_frame(
    project: &AuthoringProject,
    plan: &RenderPlan,
    plugins: &crate::plugin::PluginManager,
    frame_number: u64,
    render_scale: f64,
    region: Option<Region>,
) -> Result<FrameInfo, LibraryError> {
    let frame_number = i64::try_from(frame_number).map_err(|_| {
        LibraryError::Render("Frame number exceeds the exact-time runtime range".to_string())
    })?;
    evaluate_timeline_render_plan_frame(
        project,
        plan,
        plugins,
        plan.root_timeline_id,
        frame_number,
        render_scale,
        region,
    )
}

/// Evaluate one Timeline definition. Nested calls retain their hierarchy in
/// `FrameGroupKind::Composition`; flattening remains a renderer optimization.
pub fn evaluate_timeline_render_plan_frame(
    project: &AuthoringProject,
    plan: &RenderPlan,
    plugins: &crate::plugin::PluginManager,
    timeline_id: TimelineId,
    frame_number: i64,
    render_scale: f64,
    region: Option<Region>,
) -> Result<FrameInfo, LibraryError> {
    evaluate_timeline_render_plan_frame_at_instance(
        project,
        plan,
        plugins,
        timeline_id,
        frame_number,
        render_scale,
        region,
        None,
    )
}

/// Evaluate a Timeline either as an isolated definition (`None`) or through a
/// concrete root-to-placement path. The concrete form preserves access to
/// Exact bindings and disambiguates repeated Composition instances.
#[expect(
    clippy::too_many_arguments,
    reason = "render boundary keeps timeline, exact frame, viewport, and instance context explicit"
)]
pub fn evaluate_timeline_render_plan_frame_at_instance(
    project: &AuthoringProject,
    plan: &RenderPlan,
    plugins: &crate::plugin::PluginManager,
    timeline_id: TimelineId,
    frame_number: i64,
    render_scale: f64,
    region: Option<Region>,
    instance_path: Option<&InstancePath>,
) -> Result<FrameInfo, LibraryError> {
    if frame_number < 0 {
        return Err(LibraryError::Render(format!(
            "Frame number must be non-negative, not {frame_number}"
        )));
    }
    if plan.root_timeline_id != project.root_timeline_id {
        return Err(LibraryError::Validation(
            "RenderPlan root does not match its authoring Project".to_string(),
        ));
    }
    let timeline = project.timelines.get(&timeline_id).ok_or_else(|| {
        LibraryError::Validation(format!("Timeline {timeline_id} does not exist"))
    })?;
    let time = MediaTime::from_frame_index(frame_number, timeline.fps)
        .map_err(LibraryError::Validation)?;
    let (evaluation_root_id, evaluation_root_time, path) = match instance_path {
        Some(path) => {
            let (resolved_timeline_id, root_time) =
                root_time_for_instance_local_time(project, path, time)?;
            if resolved_timeline_id != timeline_id {
                return Err(LibraryError::Validation(format!(
                    "InstancePath resolves Timeline {resolved_timeline_id}, not requested Timeline {timeline_id}"
                )));
            }
            (project.root_timeline_id, root_time, path.clone())
        }
        None => (timeline_id, time, InstancePath::root(timeline_id)),
    };
    let mut evaluator = AuthoringFrameEvaluator {
        project,
        plan,
        plugins,
        evaluation_root_id,
        evaluation_root_time,
        active_timelines: HashSet::new(),
        active_items: HashSet::new(),
    };
    let root = evaluator.evaluate_timeline_group(timeline_id, time, &path)?;
    Ok(FrameInfo {
        width: timeline.width,
        height: timeline.height,
        background_color: transparent(),
        color_profile: timeline.color_profile.clone(),
        render_scale: OrderedFloat(render_scale),
        now_time: OrderedFloat(time.to_seconds_f64()),
        region,
        items: vec![root],
    })
}

fn root_time_for_instance_local_time(
    project: &AuthoringProject,
    path: &InstancePath,
    local_time: MediaTime,
) -> Result<(TimelineId, MediaTime), LibraryError> {
    if path.root_timeline_id != project.root_timeline_id {
        return Err(LibraryError::Validation(
            "InstancePath must start at the Project root Timeline".to_string(),
        ));
    }
    let mut timeline_id = path.root_timeline_id;
    let mut placements = Vec::with_capacity(path.composition_items.len());
    for item_id in &path.composition_items {
        let item = project.items.get(item_id).ok_or_else(|| {
            LibraryError::Validation(format!("InstancePath item {item_id} is missing"))
        })?;
        let track = project.tracks.get(&item.track_id).ok_or_else(|| {
            LibraryError::Validation(format!("InstancePath item {item_id} has no Track"))
        })?;
        if track.timeline_id != timeline_id {
            return Err(LibraryError::Validation(
                "InstancePath does not follow the nested Timeline hierarchy".to_string(),
            ));
        }
        let SourceRef::Composition(instance) = &item.source else {
            return Err(LibraryError::Validation(format!(
                "InstancePath item {item_id} is not a Composition"
            )));
        };
        let nested = project
            .timelines
            .get(&instance.timeline_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "InstancePath reaches missing Timeline {}",
                    instance.timeline_id
                ))
            })?;
        placements.push((item, nested.duration, &instance.duration_policy));
        timeline_id = instance.timeline_id;
    }
    let mut parent_time = local_time;
    for (item, definition_duration, policy) in placements.into_iter().rev() {
        parent_time = unmap_composition_time(item, definition_duration, policy, parent_time)
            .map_err(LibraryError::Validation)?;
    }
    Ok((timeline_id, parent_time))
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct ItemEvaluationKey {
    instance_path: InstancePath,
    item_id: TimelineItemId,
    stage: u8,
}

struct AuthoringFrameEvaluator<'a> {
    project: &'a AuthoringProject,
    plan: &'a RenderPlan,
    plugins: &'a crate::plugin::PluginManager,
    evaluation_root_id: TimelineId,
    evaluation_root_time: MediaTime,
    active_timelines: HashSet<TimelineId>,
    active_items: HashSet<ItemEvaluationKey>,
}

impl AuthoringFrameEvaluator<'_> {
    fn evaluate_timeline_group(
        &mut self,
        timeline_id: TimelineId,
        timeline_time: MediaTime,
        instance_path: &InstancePath,
    ) -> Result<FrameItem, LibraryError> {
        if !self.active_timelines.insert(timeline_id) {
            return Err(LibraryError::Validation(format!(
                "Nested Timeline cycle reaches {timeline_id}"
            )));
        }
        let result = self.evaluate_timeline_group_inner(timeline_id, timeline_time, instance_path);
        self.active_timelines.remove(&timeline_id);
        result
    }

    fn evaluate_timeline_group_inner(
        &mut self,
        timeline_id: TimelineId,
        timeline_time: MediaTime,
        instance_path: &InstancePath,
    ) -> Result<FrameItem, LibraryError> {
        let timeline = self.project.timelines.get(&timeline_id).ok_or_else(|| {
            LibraryError::Validation(format!("Timeline {timeline_id} does not exist"))
        })?;
        let compiled = self.plan.timelines.get(&timeline_id).ok_or_else(|| {
            LibraryError::Validation(format!("RenderPlan has no Timeline {timeline_id}"))
        })?;
        let mut tracks = Vec::new();
        for track_id in &timeline.track_order {
            let track = self.project.tracks.get(track_id).ok_or_else(|| {
                LibraryError::Validation(format!("Timeline {timeline_id} has a missing Track"))
            })?;
            if track.kind == TimelineTrackKind::Audio {
                continue;
            }
            let mut children = Vec::new();
            for schedule_index in compiled.track_schedules.get(track_id).into_iter().flatten() {
                let scheduled = compiled.schedule.get(*schedule_index).ok_or_else(|| {
                    LibraryError::Validation(format!(
                        "Timeline {timeline_id} schedule index is invalid"
                    ))
                })?;
                if !scheduled
                    .is_active(timeline_time)
                    .map_err(LibraryError::Validation)?
                {
                    continue;
                }
                if let Some(item) = self.evaluate_item_stage(
                    timeline_id,
                    scheduled.item_id,
                    timeline_time,
                    instance_path,
                    ItemOutputStage::PostTransform,
                )? {
                    children.push(item);
                }
            }
            if children.is_empty() {
                continue;
            }
            let track_time = timeline_time.to_seconds_f64();
            let mut group = FrameItem::Group(FrameGroup {
                source_id: track.id.as_uuid(),
                kind: FrameGroupKind::Track,
                width: timeline.width,
                height: timeline.height,
                background_color: transparent(),
                transform: transform_at(&track.authored_properties, track_time)?,
                blend_mode: BlendMode::Normal,
                effect_time: OrderedFloat(track_time),
                effects: Vec::new(),
                items: children,
            });
            group = self.apply_attachments(
                group,
                &AttachmentOwner::Track { track_id: track.id },
                AttachmentStage::TrackPostComposite,
                timeline_id,
                timeline_time,
                timeline_time,
                instance_path,
            )?;
            tracks.push(group);
        }
        let seconds = timeline_time.to_seconds_f64();
        let mut group = FrameItem::Group(FrameGroup {
            source_id: timeline.id.as_uuid(),
            kind: FrameGroupKind::Composition,
            width: timeline.width,
            height: timeline.height,
            background_color: timeline.background_color.clone(),
            transform: transform_at(&timeline.authored_properties, seconds)?,
            blend_mode: BlendMode::Normal,
            effect_time: OrderedFloat(seconds),
            effects: Vec::new(),
            items: tracks,
        });
        group = self.apply_attachments(
            group,
            &AttachmentOwner::Timeline { timeline_id },
            AttachmentStage::TimelinePostComposite,
            timeline_id,
            timeline_time,
            timeline_time,
            instance_path,
        )?;
        Ok(group)
    }

    fn evaluate_item_stage(
        &mut self,
        timeline_id: TimelineId,
        item_id: TimelineItemId,
        timeline_time: MediaTime,
        instance_path: &InstancePath,
        stage: ItemOutputStage,
    ) -> Result<Option<FrameItem>, LibraryError> {
        let key = ItemEvaluationKey {
            instance_path: instance_path.clone(),
            item_id,
            stage: stage_key(stage),
        };
        if !self.active_items.insert(key.clone()) {
            return Err(LibraryError::Validation(format!(
                "Timeline media-input cycle reaches item {item_id}"
            )));
        }
        let result = self.evaluate_item_stage_inner(
            timeline_id,
            item_id,
            timeline_time,
            instance_path,
            stage,
        );
        self.active_items.remove(&key);
        result
    }

    fn evaluate_item_stage_inner(
        &mut self,
        timeline_id: TimelineId,
        item_id: TimelineItemId,
        timeline_time: MediaTime,
        instance_path: &InstancePath,
        stage: ItemOutputStage,
    ) -> Result<Option<FrameItem>, LibraryError> {
        let timeline = self.project.timelines.get(&timeline_id).ok_or_else(|| {
            LibraryError::Validation(format!("Timeline {timeline_id} does not exist"))
        })?;
        let item = self.project.items.get(&item_id).ok_or_else(|| {
            LibraryError::Validation(format!("Timeline item {item_id} does not exist"))
        })?;
        let track = self.project.tracks.get(&item.track_id).ok_or_else(|| {
            LibraryError::Validation(format!("Item {item_id} has a missing Track"))
        })?;
        if track.timeline_id != timeline_id {
            return Err(LibraryError::Validation(format!(
                "Item {item_id} does not belong to Timeline {timeline_id}"
            )));
        }
        if !item
            .interval
            .contains(timeline_time)
            .map_err(LibraryError::Validation)?
        {
            return Ok(None);
        }
        let scheduled = self
            .plan
            .timelines
            .get(&timeline_id)
            .and_then(|compiled| {
                compiled
                    .schedule
                    .iter()
                    .find(|scheduled| scheduled.item_id == item_id)
            })
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "RenderPlan has no schedule entry for item {item_id}"
                ))
            })?;
        if scheduled.track_id != item.track_id
            || scheduled.interval != item.interval
            || scheduled.time_map != item.time_map
            || !planned_source_matches(scheduled.source, &item.source)
        {
            return Err(LibraryError::Validation(format!(
                "RenderPlan schedule is stale for item {item_id}"
            )));
        }
        let local_time = scheduled
            .local_time(timeline_time)
            .map_err(LibraryError::Validation)?;
        let mut output =
            self.evaluate_item_source(timeline, item, timeline_time, local_time, instance_path)?;
        if stage == ItemOutputStage::Content || output.is_none() {
            return Ok(output);
        }
        if let Some(frame) = output.take() {
            output = Some(self.apply_attachments(
                frame,
                &AttachmentOwner::Item { item_id },
                AttachmentStage::ItemPreTransform,
                timeline_id,
                local_time,
                timeline_time,
                instance_path,
            )?);
        }
        if stage == ItemOutputStage::PostEffects || output.is_none() {
            return Ok(output);
        }
        let mut frame = output.ok_or_else(|| {
            LibraryError::Render(format!("Item {item_id} unexpectedly lost its Image output"))
        })?;
        let transform_values =
            self.effective_item_property_values(timeline, item, local_time, instance_path)?;
        frame = FrameItem::Group(FrameGroup {
            source_id: item.id.as_uuid(),
            kind: FrameGroupKind::Clip,
            width: timeline.width,
            height: timeline.height,
            background_color: transparent(),
            transform: transform_from_values(&transform_values)?,
            blend_mode: item.blend_mode,
            effect_time: OrderedFloat(local_time.to_seconds_f64()),
            effects: Vec::new(),
            items: vec![frame],
        });
        frame = self.wrap_parent_transforms(frame, timeline, item, timeline_time, instance_path)?;
        self.apply_attachments(
            frame,
            &AttachmentOwner::Item { item_id },
            AttachmentStage::ItemPostTransform,
            timeline_id,
            local_time,
            timeline_time,
            instance_path,
        )
        .map(Some)
    }

    fn evaluate_item_source(
        &mut self,
        timeline: &Timeline,
        item: &TimelineItem,
        timeline_time: MediaTime,
        local_time: MediaTime,
        instance_path: &InstancePath,
    ) -> Result<Option<FrameItem>, LibraryError> {
        match &item.source {
            SourceRef::Asset { asset_id } => self.asset_item(
                item.id.as_uuid(),
                *asset_id,
                local_time,
                timeline.fps.to_f64(),
            ),
            SourceRef::Text {
                text,
                ensemble_operations,
            } => {
                let text = self.effective_text(timeline, item.id, text, instance_path)?;
                let values =
                    self.effective_item_property_values(timeline, item, local_time, instance_path)?;
                let ensemble = match evaluate_text_ensemble(
                    self.plugins,
                    ensemble_operations,
                    local_time.to_seconds_f64(),
                    timeline.fps.to_f64(),
                    (timeline.width, timeline.height),
                )? {
                    crate::model::project::EvalOutput::Produced(ensemble) => ensemble,
                    crate::model::project::EvalOutput::NoOutput => return Ok(None),
                };
                text_item_from_values(item.id.as_uuid(), &text, &values, ensemble).map(Some)
            }
            SourceRef::Shape { shape } => shape_item(item.id.as_uuid(), shape).map(Some),
            SourceRef::Solid { color } => Ok(Some(solid_item(
                item.id.as_uuid(),
                timeline.width,
                timeline.height,
                color.clone(),
                BlendMode::Normal,
            ))),
            SourceRef::Composition(instance) => {
                let nested = self
                    .project
                    .timelines
                    .get(&instance.timeline_id)
                    .ok_or_else(|| {
                        LibraryError::Validation(format!(
                            "Item {} refers to missing nested Timeline {}",
                            item.id, instance.timeline_id
                        ))
                    })?;
                let Some(nested_time) = map_composition_time(
                    item,
                    nested.duration,
                    &instance.duration_policy,
                    timeline_time,
                )
                .map_err(LibraryError::Validation)?
                else {
                    return Ok(None);
                };
                let path = instance_path.nested(item.id);
                self.evaluate_timeline_group(instance.timeline_id, nested_time, &path)
                    .map(Some)
            }
            SourceRef::Module(_) => self.evaluate_module_host(
                ModuleHost::TimelineItem {
                    timeline_id: timeline.id,
                    item_id: item.id,
                },
                timeline.id,
                local_time,
                timeline_time,
                instance_path,
                None,
            ),
        }
    }

    fn wrap_parent_transforms(
        &self,
        mut frame: FrameItem,
        timeline: &Timeline,
        item: &TimelineItem,
        timeline_time: MediaTime,
        instance_path: &InstancePath,
    ) -> Result<FrameItem, LibraryError> {
        let mut parent_id = item.parent;
        let mut active = HashSet::new();
        while let Some(id) = parent_id {
            if !active.insert(id) {
                return Err(LibraryError::Validation(format!(
                    "Parent cycle reaches Timeline item {id}"
                )));
            }
            let parent = self.project.items.get(&id).ok_or_else(|| {
                LibraryError::Validation(format!("Item {} has missing parent {id}", item.id))
            })?;
            let local = parent
                .time_map
                .local_time(parent.interval, timeline_time)
                .map_err(LibraryError::Validation)?;
            let values =
                self.effective_item_property_values(timeline, parent, local, instance_path)?;
            frame = FrameItem::Group(FrameGroup {
                source_id: item.id.as_uuid(),
                kind: FrameGroupKind::Clip,
                width: 0,
                height: 0,
                background_color: transparent(),
                transform: transform_from_values(&values)?,
                blend_mode: BlendMode::Normal,
                effect_time: OrderedFloat(local.to_seconds_f64()),
                effects: Vec::new(),
                items: vec![frame],
            });
            parent_id = parent.parent;
        }
        Ok(frame)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "attachment evaluation keeps host Timeline, local time, and instance path explicit"
    )]
    fn apply_attachments(
        &mut self,
        mut frame: FrameItem,
        owner: &AttachmentOwner,
        stage: AttachmentStage,
        timeline_id: TimelineId,
        local_time: MediaTime,
        timeline_time: MediaTime,
        instance_path: &InstancePath,
    ) -> Result<FrameItem, LibraryError> {
        let mut attachments = self
            .project
            .attachments
            .values()
            .filter(|attachment| attachment.owner == *owner && attachment.stage == stage)
            .collect::<Vec<_>>();
        attachments.sort_by_key(|attachment| (attachment.order, attachment.id));
        for attachment in attachments {
            if !attachment.enabled || attachment.bypassed {
                continue;
            }
            frame = match &attachment.processor {
                AttachmentProcessor::BuiltinEffect(effect) => {
                    if effect.contract.input_type != PortDataType::Image
                        || effect.contract.output_type != PortDataType::Image
                    {
                        return Err(LibraryError::Render(format!(
                            "Visual attachment {} is not Image -> Image",
                            attachment.id
                        )));
                    }
                    let properties = effect
                        .parameters
                        .iter()
                        .map(|(key, parameter)| {
                            let value = match &parameter.automation {
                                Some(track) => track.evaluate_at(local_time)?,
                                None => parameter.value.clone(),
                            };
                            Ok((key.clone(), value))
                        })
                        .collect::<Result<HashMap<_, _>, LibraryError>>()?;
                    FrameItem::Group(FrameGroup {
                        source_id: attachment.id.as_uuid(),
                        kind: FrameGroupKind::Effect,
                        width: 0,
                        height: 0,
                        background_color: transparent(),
                        transform: Transform::default(),
                        blend_mode: effect.blend_mode,
                        effect_time: OrderedFloat(local_time.to_seconds_f64()),
                        effects: vec![ImageEffect {
                            effect_type: effect.operation.component_id.clone(),
                            properties,
                        }],
                        items: vec![frame],
                    })
                }
                AttachmentProcessor::Module(_) => self
                    .evaluate_module_host(
                        ModuleHost::Attachment(attachment.id),
                        timeline_id,
                        local_time,
                        timeline_time,
                        instance_path,
                        Some(frame),
                    )?
                    .ok_or_else(|| {
                        LibraryError::Render(format!(
                            "Module attachment {} produced no Image",
                            attachment.id
                        ))
                    })?,
            };
        }
        Ok(frame)
    }

    fn asset_item(
        &self,
        source_id: uuid::Uuid,
        asset_id: uuid::Uuid,
        source_time: MediaTime,
        evaluation_fps: f64,
    ) -> Result<Option<FrameItem>, LibraryError> {
        let asset = self
            .project
            .assets
            .iter()
            .find(|asset| asset.id == asset_id)
            .ok_or_else(|| LibraryError::Validation(format!("Asset {asset_id} is missing")))?;
        if asset.kind == AssetKind::Audio {
            return Ok(None);
        }
        if matches!(asset.kind, AssetKind::Model3D | AssetKind::Other) {
            return Err(LibraryError::Render(format!(
                "Asset {asset_id} has no visual Timeline renderer"
            )));
        }
        let seconds = source_time.to_seconds_f64();
        if let Some(frame) = asset.source_frame_number_at(seconds, evaluation_fps)
            && !asset.contains_source_frame(frame)
        {
            return Ok(None);
        }
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
                source_time: seconds,
                stream_index: asset.stream_index,
            },
            AssetKind::Image => FrameContent::Image { surface },
            AssetKind::Audio | AssetKind::Model3D | AssetKind::Other => {
                return Err(LibraryError::Render(format!(
                    "Asset {asset_id} changed type during frame evaluation"
                )));
            }
        };
        Ok(Some(FrameItem::Object(FrameObject {
            source_node_id: source_id,
            spatial_transform_node_id: None,
            spatial_transform: Box::default(),
            content_bounds: match (asset.width, asset.height) {
                (Some(width), Some(height)) => {
                    Some(FrameBounds::new(0.0, 0.0, width as f32, height as f32))
                }
                _ => None,
            },
            content,
        })))
    }

    fn evaluate_module_host(
        &mut self,
        host: ModuleHost,
        timeline_id: TimelineId,
        local_time: MediaTime,
        timeline_time: MediaTime,
        instance_path: &InstancePath,
        implicit_primary: Option<FrameItem>,
    ) -> Result<Option<FrameItem>, LibraryError> {
        let invocation = self.plan.invocation(host).ok_or_else(|| {
            LibraryError::Validation(format!("RenderPlan has no invocation for {host:?}"))
        })?;
        let definition = self
            .plan
            .module_definitions
            .get(&invocation.definition_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "RenderPlan has no Module definition {}",
                    invocation.definition_id
                ))
            })?;
        let output = definition
            .outputs
            .get(&invocation.output_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Module invocation selects missing output {}",
                    invocation.output_id
                ))
            })?;
        let mut external_images = HashMap::new();
        for (input_id, binding) in &invocation.input_bindings {
            let input = definition.media_inputs.get(input_id).ok_or_else(|| {
                LibraryError::Validation(format!("Module input {input_id} is no longer published"))
            })?;
            if input.data_type != PortDataType::Image {
                return Err(LibraryError::Render(format!(
                    "Audio Module input {input_id} is outside the stateless Image runtime"
                )));
            }
            if let Some(frame) =
                self.evaluate_media_binding(binding, timeline_id, timeline_time, instance_path)?
            {
                external_images.insert(input.target.clone(), frame);
            }
        }
        if let Some(primary) = implicit_primary {
            let published = definition
                .media_inputs
                .values()
                .find(|input| input.primary)
                .ok_or_else(|| {
                    LibraryError::Validation(format!(
                        "Attachment Module {} has no primary media input",
                        definition.id
                    ))
                })?;
            external_images
                .entry(published.target.clone())
                .or_insert(primary);
        }
        for input in definition
            .media_inputs
            .values()
            .filter(|input| input.required)
        {
            if !external_images.contains_key(&input.target) {
                return Ok(None);
            }
        }

        let mut runtime = ModuleImageRuntime {
            project: self.project,
            definition,
            invocation,
            instance_path,
            local_time,
            width: self
                .project
                .timelines
                .get(&timeline_id)
                .map(|timeline| timeline.width)
                .ok_or_else(|| {
                    LibraryError::Validation(format!("Timeline {timeline_id} does not exist"))
                })?,
            height: self
                .project
                .timelines
                .get(&timeline_id)
                .map(|timeline| timeline.height)
                .ok_or_else(|| {
                    LibraryError::Validation(format!("Timeline {timeline_id} does not exist"))
                })?,
            evaluation_fps: self
                .project
                .timelines
                .get(&timeline_id)
                .map(|timeline| timeline.fps.to_f64())
                .ok_or_else(|| {
                    LibraryError::Validation(format!("Timeline {timeline_id} does not exist"))
                })?,
            plugins: self.plugins,
            external_images,
            image_memo: HashMap::new(),
            image_path: HashSet::new(),
            shape_memo: HashMap::new(),
            shape_path: HashSet::new(),
            value_memo: HashMap::new(),
            value_path: HashSet::new(),
        };
        runtime.evaluate_terminal(output)
    }

    fn evaluate_media_binding(
        &mut self,
        binding: &MediaInputBinding,
        current_timeline_id: TimelineId,
        current_time: MediaTime,
        instance_path: &InstancePath,
    ) -> Result<Option<FrameItem>, LibraryError> {
        let MediaInputBinding::TimelineItemOutput {
            locator,
            item_id,
            output,
            stage,
        } = binding;
        if *output != MediaOutputKind::Image {
            return Err(LibraryError::Render(
                "Audio media bindings require the future audio RenderPlan runtime".to_string(),
            ));
        }
        match locator {
            InstanceLocator::SameTimeline => self.evaluate_item_stage(
                current_timeline_id,
                *item_id,
                current_time,
                instance_path,
                *stage,
            ),
            InstanceLocator::Exact(path) => {
                let (timeline_id, timeline_time) = self.time_for_instance_path(path)?;
                self.evaluate_item_stage(timeline_id, *item_id, timeline_time, path, *stage)
            }
        }
    }

    fn time_for_instance_path(
        &self,
        path: &InstancePath,
    ) -> Result<(TimelineId, MediaTime), LibraryError> {
        if path.root_timeline_id != self.evaluation_root_id {
            return Err(LibraryError::Render(format!(
                "InstancePath root {} is outside evaluation root {}",
                path.root_timeline_id, self.evaluation_root_id
            )));
        }
        let mut timeline_id = path.root_timeline_id;
        let mut time = self.evaluation_root_time;
        for item_id in &path.composition_items {
            let item = self.project.items.get(item_id).ok_or_else(|| {
                LibraryError::Validation(format!("InstancePath item {item_id} is missing"))
            })?;
            let track = self.project.tracks.get(&item.track_id).ok_or_else(|| {
                LibraryError::Validation(format!("InstancePath item {item_id} has no Track"))
            })?;
            if track.timeline_id != timeline_id {
                return Err(LibraryError::Validation(
                    "InstancePath does not follow the nested Timeline hierarchy".to_string(),
                ));
            }
            let SourceRef::Composition(instance) = &item.source else {
                return Err(LibraryError::Validation(format!(
                    "InstancePath item {item_id} is not a Composition"
                )));
            };
            let nested = self
                .project
                .timelines
                .get(&instance.timeline_id)
                .ok_or_else(|| {
                    LibraryError::Validation(format!(
                        "InstancePath reaches missing Timeline {}",
                        instance.timeline_id
                    ))
                })?;
            time = map_composition_time(item, nested.duration, &instance.duration_policy, time)
                .map_err(LibraryError::Validation)?
                .ok_or_else(|| {
                    LibraryError::Render(format!(
                        "InstancePath item {item_id} is inactive at the evaluation time"
                    ))
                })?;
            timeline_id = instance.timeline_id;
        }
        Ok((timeline_id, time))
    }
}
