use std::collections::HashMap;

use crate::model::authoring::{AuthoringProject, ModuleDefinition, ModuleDefinitionId};

use super::compiler::{compile_module, definition_fingerprint};
use super::{CompiledModuleDefinition, RenderPlan, RenderPlanCompiler};

#[derive(Clone)]
struct CachedDefinition {
    authored: ModuleDefinition,
    compiled: CompiledModuleDefinition,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RenderPlanCacheStats {
    pub compiled_definitions: usize,
    pub reused_definitions: usize,
}

#[derive(Default)]
pub struct RenderPlanCache {
    definitions: HashMap<ModuleDefinitionId, CachedDefinition>,
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
            let cached = self.definitions.get(id).filter(|cached| {
                cached.compiled.fingerprint == fingerprint && &cached.authored == authored
            });
            let definition = if let Some(cached) = cached {
                stats.reused_definitions += 1;
                cached.compiled.clone()
            } else {
                stats.compiled_definitions += 1;
                let definition = compile_module(*id, authored)?;
                self.definitions.insert(
                    *id,
                    CachedDefinition {
                        authored: authored.clone(),
                        compiled: definition.clone(),
                    },
                );
                definition
            };
            compiled.insert(*id, definition);
        }

        RenderPlanCompiler::compile_with_definitions(project, compiled).map(|plan| (plan, stats))
    }
}
