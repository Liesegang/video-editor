use std::collections::HashMap;
use std::sync::Arc;

use crate::model::authoring::{AuthoringProject, ModuleDefinitionId, TimelineId};

use super::compiler::{
    compile_module, compile_timeline, definition_fingerprint, referenced_definitions,
    timeline_schedule_fingerprint,
};
use super::{CompiledModuleDefinition, CompiledTimeline, RenderPlan, RenderPlanCompiler};

#[derive(Clone)]
struct CachedDefinition {
    fingerprint: [u8; 32],
    compiled: Arc<CompiledModuleDefinition>,
}

#[derive(Clone)]
struct CachedTimeline {
    fingerprint: [u8; 32],
    compiled: Arc<CompiledTimeline>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RenderPlanCacheStats {
    pub compiled_definitions: usize,
    pub reused_definitions: usize,
    pub compiled_timelines: usize,
    pub reused_timelines: usize,
}

/// Incremental compiler cache. Definition and Timeline schedule entries are
/// independent so instance parameters never force Module topology recompiles.
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
        project.validate()?;
        let referenced = referenced_definitions(project)?;
        self.definitions.retain(|id, _| referenced.contains(id));
        self.timelines
            .retain(|id, _| project.timelines.contains_key(id));

        let mut stats = RenderPlanCacheStats::default();
        let mut definitions = HashMap::new();
        for id in referenced {
            let authored = project
                .module_definitions
                .get(&id)
                .ok_or_else(|| format!("Missing Module definition {id}"))?;
            let fingerprint = definition_fingerprint(authored)?;
            let compiled = match self
                .definitions
                .get(&id)
                .filter(|entry| entry.fingerprint == fingerprint)
            {
                Some(entry) => {
                    stats.reused_definitions += 1;
                    Arc::clone(&entry.compiled)
                }
                None => {
                    stats.compiled_definitions += 1;
                    let compiled = Arc::new(compile_module(authored)?);
                    self.definitions.insert(
                        id,
                        CachedDefinition {
                            fingerprint,
                            compiled: Arc::clone(&compiled),
                        },
                    );
                    compiled
                }
            };
            definitions.insert(id, compiled);
        }

        let mut timelines = HashMap::new();
        let mut timeline_ids = project.timelines.keys().copied().collect::<Vec<_>>();
        timeline_ids.sort();
        for id in timeline_ids {
            let fingerprint = timeline_schedule_fingerprint(project, id)?;
            let compiled = match self
                .timelines
                .get(&id)
                .filter(|entry| entry.fingerprint == fingerprint)
            {
                Some(entry) => {
                    stats.reused_timelines += 1;
                    Arc::clone(&entry.compiled)
                }
                None => {
                    stats.compiled_timelines += 1;
                    let compiled = Arc::new(compile_timeline(project, id)?);
                    self.timelines.insert(
                        id,
                        CachedTimeline {
                            fingerprint,
                            compiled: Arc::clone(&compiled),
                        },
                    );
                    compiled
                }
            };
            timelines.insert(id, compiled);
        }

        RenderPlanCompiler::assemble(project, timelines, definitions).map(|plan| (plan, stats))
    }
}
