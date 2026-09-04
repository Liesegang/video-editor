use std::collections::VecDeque;
use std::sync::Arc;

use crate::core::rendering::managed_color_backend::{
    ManagedRenderDestination, ProjectColorPipeline, ProjectColorPipelineCacheKey,
};
use crate::error::LibraryError;
use crate::model::authoring::AuthoringProject;
use crate::model::project::Project;

// A RenderService normally needs only Preview and Export for one active color
// config. Keeping two previous identities avoids rebuild churn while a config
// edit is undone/redone without allowing project history to grow this cache.
const MAX_PIPELINES: usize = 4;

pub(super) struct ProjectColorPipelineCache {
    entries: VecDeque<(ProjectColorPipelineCacheKey, Arc<ProjectColorPipeline>)>,
    #[cfg(test)]
    hits: u64,
    #[cfg(test)]
    misses: u64,
}

impl ProjectColorPipelineCache {
    pub(super) fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_PIPELINES),
            #[cfg(test)]
            hits: 0,
            #[cfg(test)]
            misses: 0,
        }
    }

    pub(super) fn for_project(
        &mut self,
        project: &Project,
        destination: ManagedRenderDestination,
    ) -> Result<Arc<ProjectColorPipeline>, LibraryError> {
        let key = ProjectColorPipeline::cache_key_for_project(project, destination)?;
        if let Some(pipeline) = self.take(&key) {
            return Ok(pipeline);
        }
        Ok(self.insert(
            key,
            Arc::new(ProjectColorPipeline::for_project(project, destination)?),
        ))
    }

    pub(super) fn for_authoring_project(
        &mut self,
        project: &AuthoringProject,
        destination: ManagedRenderDestination,
    ) -> Result<Arc<ProjectColorPipeline>, LibraryError> {
        let key = ProjectColorPipeline::cache_key_for_authoring_project(project, destination)?;
        if let Some(pipeline) = self.take(&key) {
            return Ok(pipeline);
        }
        Ok(self.insert(
            key,
            Arc::new(ProjectColorPipeline::for_authoring_project(
                project,
                destination,
            )?),
        ))
    }

    fn take(&mut self, key: &ProjectColorPipelineCacheKey) -> Option<Arc<ProjectColorPipeline>> {
        let index = self.entries.iter().position(|(entry, _)| entry == key)?;
        let entry = self.entries.remove(index)?;
        let pipeline = Arc::clone(&entry.1);
        self.entries.push_back(entry);
        #[cfg(test)]
        {
            self.hits += 1;
        }
        Some(pipeline)
    }

    fn insert(
        &mut self,
        key: ProjectColorPipelineCacheKey,
        pipeline: Arc<ProjectColorPipeline>,
    ) -> Arc<ProjectColorPipeline> {
        #[cfg(test)]
        {
            self.misses += 1;
        }
        if self.entries.len() == MAX_PIPELINES {
            self.entries.pop_front();
        }
        self.entries.push_back((key, Arc::clone(&pipeline)));
        pipeline
    }

    #[cfg(test)]
    fn stats(&self) -> (u64, u64, usize) {
        (self.hits, self.misses, self.entries.len())
    }
}

#[cfg(test)]
mod tests {
    use super::ProjectColorPipelineCache;
    use crate::core::rendering::managed_color_backend::ManagedRenderDestination;
    use crate::model::authoring::{AuthoringProject, MediaTime, RationalRate};
    use crate::model::project::Project;
    use std::sync::Arc;

    #[test]
    fn same_project_config_and_destination_reuse_compiled_pipeline() {
        let project = Project::new("pipeline cache");
        let mut cache = ProjectColorPipelineCache::new();
        let first = cache
            .for_project(&project, ManagedRenderDestination::Preview)
            .unwrap();
        let second = cache
            .for_project(&project, ManagedRenderDestination::Preview)
            .unwrap();

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(cache.stats(), (1, 1, 1));
    }

    #[test]
    fn authoring_edits_do_not_recompile_an_unchanged_color_config() {
        let mut project = AuthoringProject::new(
            "authoring cache",
            16,
            9,
            RationalRate::new(60, 1).unwrap(),
            MediaTime::new(1, 1).unwrap(),
        )
        .unwrap();
        let mut cache = ProjectColorPipelineCache::new();
        let first = cache
            .for_authoring_project(&project, ManagedRenderDestination::Preview)
            .unwrap();
        project.name = "an unrelated Timeline edit".to_string();
        let second = cache
            .for_authoring_project(&project, ManagedRenderDestination::Preview)
            .unwrap();

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(cache.stats(), (1, 1, 1));
    }

    #[test]
    fn preview_and_export_keep_distinct_terminal_processors() {
        let project = Project::new("destination cache split");
        let mut cache = ProjectColorPipelineCache::new();
        let preview = cache
            .for_project(&project, ManagedRenderDestination::Preview)
            .unwrap();
        let export = cache
            .for_project(&project, ManagedRenderDestination::Export)
            .unwrap();

        assert!(!Arc::ptr_eq(&preview, &export));
        assert_eq!(cache.stats(), (0, 2, 2));
    }
}
