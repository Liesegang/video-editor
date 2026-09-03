use std::collections::HashMap;

use crate::model::authoring::{
    AttachmentOwner, AuthoringProject, ModuleDefinitionId, SourceRef, TimelineId,
};

use super::compiler::{compile_module, compile_timeline, definition_fingerprint};
use super::{CompiledModuleDefinition, CompiledTimeline, RenderPlan, RenderPlanCompiler};

#[derive(Clone)]
struct CachedDefinition {
    compiled: CompiledModuleDefinition,
}

#[derive(Clone)]
struct CachedTimeline {
    fingerprint: [u8; 32],
    compiled: CompiledTimeline,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RenderPlanCacheStats {
    pub compiled_definitions: usize,
    pub reused_definitions: usize,
    pub compiled_timelines: usize,
    pub reused_timelines: usize,
}

#[derive(Default)]
pub struct RenderPlanCache {
    definitions: HashMap<ModuleDefinitionId, CachedDefinition>,
    timelines: HashMap<TimelineId, CachedTimeline>,
}

impl RenderPlanCache {
    pub fn compile(
        &mut self,
        project: &AuthoringProject,
    ) -> Result<(RenderPlan, RenderPlanCacheStats), String> {
        let mut stats = RenderPlanCacheStats::default();
        let mut compiled = HashMap::new();
        self.definitions
            .retain(|id, _| project.module_definitions.contains_key(id));

        for (id, authored) in &project.module_definitions {
            let fingerprint = definition_fingerprint(authored)?;
            let cached = self
                .definitions
                .get(id)
                .filter(|cached| cached.compiled.fingerprint == fingerprint);
            let definition = if let Some(cached) = cached {
                stats.reused_definitions += 1;
                cached.compiled.clone()
            } else {
                stats.compiled_definitions += 1;
                let definition = compile_module(*id, authored)?;
                self.definitions.insert(
                    *id,
                    CachedDefinition {
                        compiled: definition.clone(),
                    },
                );
                definition
            };
            compiled.insert(*id, definition);
        }

        self.timelines
            .retain(|id, _| project.timelines.contains_key(id));
        let mut compiled_timelines = HashMap::new();
        for timeline_id in project.timelines.keys().copied() {
            let fingerprint = timeline_schedule_fingerprint(project, timeline_id)?;
            let timeline = if let Some(cached) = self
                .timelines
                .get(&timeline_id)
                .filter(|cached| cached.fingerprint == fingerprint)
            {
                stats.reused_timelines += 1;
                cached.compiled.clone()
            } else {
                stats.compiled_timelines += 1;
                let timeline = compile_timeline(project, timeline_id)?;
                self.timelines.insert(
                    timeline_id,
                    CachedTimeline {
                        fingerprint,
                        compiled: timeline.clone(),
                    },
                );
                timeline
            };
            compiled_timelines.insert(timeline_id, timeline);
        }

        RenderPlanCompiler::compile_with_parts(project, compiled, Some(compiled_timelines))
            .map(|plan| (plan, stats))
    }
}

fn timeline_schedule_fingerprint(
    project: &AuthoringProject,
    timeline_id: TimelineId,
) -> Result<[u8; 32], String> {
    use sha2::{Digest, Sha256};

    let timeline = project
        .timelines
        .get(&timeline_id)
        .ok_or_else(|| format!("Missing Timeline {timeline_id}"))?;
    let mut hasher = Sha256::new();
    hasher.update(timeline.id.as_uuid().as_bytes());
    for track_id in &timeline.track_order {
        hasher.update(track_id.as_uuid().as_bytes());
    }
    let mut items = project
        .items
        .values()
        .filter(|item| {
            project
                .tracks
                .get(&item.track_id)
                .is_some_and(|track| track.timeline_id == timeline_id)
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|item| item.id);
    for item in items {
        hasher.update(item.id.as_uuid().as_bytes());
        hasher.update(item.track_id.as_uuid().as_bytes());
        hasher.update(item.layer.to_le_bytes());
        hasher.update(item.interval.start.into_inner().to_bits().to_le_bytes());
        hasher.update(item.interval.duration.into_inner().to_bits().to_le_bytes());
        if let Some(matte) = item.matte {
            hasher.update([1]);
            hasher.update(matte.item_id.as_uuid().as_bytes());
        } else {
            hasher.update([0]);
        }
        match &item.source {
            SourceRef::Asset { .. } => hasher.update([0]),
            SourceRef::Text { .. } => hasher.update([1]),
            SourceRef::Shape { .. } => hasher.update([2]),
            SourceRef::Solid { .. } => hasher.update([3]),
            SourceRef::Composition(instance) => {
                hasher.update([4]);
                hasher.update(instance.timeline_id.as_uuid().as_bytes());
            }
            SourceRef::Module { module_instance_id } => {
                hasher.update([5]);
                hasher.update(module_instance_id.as_uuid().as_bytes());
            }
        }
    }
    let mut attachments = project
        .attachments
        .values()
        .filter_map(|attachment| match &attachment.owner {
            AttachmentOwner::Timeline { timeline_id: owner } if *owner == timeline_id => {
                Some(attachment.id)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    attachments.sort();
    for attachment_id in attachments {
        hasher.update(attachment_id.as_uuid().as_bytes());
    }
    Ok(hasher.finalize().into())
}
