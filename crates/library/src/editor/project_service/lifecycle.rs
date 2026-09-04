//! Authoritative Project adoption, persistence, and shared service state.

use crate::error::LibraryError;
use crate::model::project::{Composition, Project};
use crate::plugin::PluginManager;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// History-free commands over the single authoritative Project instance.
///
/// Higher-level editor services own undo history; this type owns only shared
/// Project access and plugin-backed model mutations.
pub struct ProjectManager {
    pub(super) project: Arc<RwLock<Project>>,
    pub(super) plugin_manager: Arc<PluginManager>,
}

impl ProjectManager {
    pub fn new(project: Arc<RwLock<Project>>, plugin_manager: Arc<PluginManager>) -> Self {
        Self {
            project,
            plugin_manager,
        }
    }

    pub fn get_project(&self) -> Arc<RwLock<Project>> {
        Arc::clone(&self.project)
    }

    pub fn get_plugin_manager(&self) -> Arc<PluginManager> {
        Arc::clone(&self.plugin_manager)
    }

    pub fn set_project(&self, new_project: Project) -> Result<(), LibraryError> {
        Self::validate_project_for_adoption(&new_project)?;
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        *project_write = new_project;
        Ok(())
    }

    pub fn load_project(&self, json_str: &str) -> Result<Project, LibraryError> {
        // Project adoption is deliberately free of document-controlled media
        // I/O. Missing source authority remains inspectable and fail-closed;
        // the user can explicitly re-probe a linked local regular file from
        // the Timeline/Preview Inspector.
        let new_project = Project::load(json_str)?;
        Self::validate_project_for_adoption(&new_project)?;
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        *project_write = new_project.clone();
        Ok(new_project)
    }

    fn validate_project_for_adoption(project: &Project) -> Result<(), LibraryError> {
        let errors = project.validation_issues();
        if errors.is_empty() {
            return Ok(());
        }
        Err(LibraryError::Validation(
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        ))
    }

    pub fn create_new_project(&self) -> Result<(Uuid, Project), LibraryError> {
        let mut new_project = Project::new("New Project");
        let (default_comp, root_track) =
            Composition::new("Main Composition", 1920, 1080, 30.0, 60.0);
        let new_comp_id = default_comp.id;
        new_project
            .add_track(root_track)
            .and_then(|()| new_project.add_composition(default_comp))
            .map_err(|error| LibraryError::Project(error.to_string()))?;

        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        *project_write = new_project.clone();

        Ok((new_comp_id, new_project))
    }

    pub fn save_project(&self) -> Result<String, LibraryError> {
        let project_read = self.project.read().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project read lock: {}", e))
        })?;
        Ok(project_read.save()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CacheManager;
    use crate::model::asset::{Asset, AssetKind, SourceColorDescription};
    use crate::plugin::Plugin;
    use crate::plugin::loaders::{
        AssetMetadata, LoadPlugin, LoadPluginError, LoadPluginResult, LoadRequest, LoadResponse,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingLoader {
        opens: Arc<AtomicUsize>,
    }

    impl Plugin for CountingLoader {
        fn id(&self) -> &'static str {
            "load-purity-probe"
        }

        fn name(&self) -> String {
            "Load purity probe".to_string()
        }

        fn category(&self) -> String {
            "Tests".to_string()
        }

        fn version(&self) -> (u32, u32, u32) {
            (0, 1, 0)
        }
    }

    impl LoadPlugin for CountingLoader {
        fn open(&self, _path: &str) -> LoadPluginResult<Vec<AssetMetadata>> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            Err(LoadPluginError::Unsupported)
        }

        fn load(
            &self,
            _request: &LoadRequest,
            _cache: &CacheManager,
        ) -> LoadPluginResult<LoadResponse> {
            Err(LoadPluginError::Unsupported)
        }
    }

    #[test]
    fn project_load_never_opens_document_controlled_asset_paths() {
        let opens = Arc::new(AtomicUsize::new(0));
        let plugins = Arc::new(PluginManager::new());
        plugins.register_load_plugin(Arc::new(CountingLoader {
            opens: Arc::clone(&opens),
        }));
        let shared = Arc::new(RwLock::new(Project::new("target")));
        let manager = ProjectManager::new(Arc::clone(&shared), plugins);

        let mut source = Project::new("untrusted document");
        let mut asset = Asset::new(
            "unused network source",
            "http://127.0.0.1:65535/probe.mp4",
            AssetKind::Video,
        );
        asset.source_color.replace_detected(SourceColorDescription {
            bit_depth: Some(8),
            ..SourceColorDescription::default()
        });
        let asset_id = asset.id;
        source.assets.push(asset);

        manager.load_project(&source.save().unwrap()).unwrap();
        assert_eq!(
            opens.load(Ordering::SeqCst),
            0,
            "deserialize/adopt must perform no loader, network, FIFO, device, or media I/O"
        );
        let loaded = shared.read().unwrap();
        let detected = loaded.get_asset(asset_id).unwrap().source_color.detected();
        assert_eq!(detected.bit_depth, Some(8));
        assert_eq!(detected.assumption, None);
    }
}
