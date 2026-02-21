use crate::audio::engine::AudioEngine;
use crate::plugin::PluginManager;
use crate::project::project::Project;
use crate::rendering::cache::CacheManager;
use crate::service::audio_service::AudioService;
use crate::service::project_service::ProjectManager;
use std::sync::{Arc, RwLock};

pub struct EditorService {
    pub(crate) project_manager: Arc<ProjectManager>,
    pub(crate) audio_service: Arc<AudioService>,
}

impl Clone for EditorService {
    fn clone(&self) -> Self {
        Self {
            project_manager: self.project_manager.clone(),
            audio_service: self.audio_service.clone(),
        }
    }
}

impl EditorService {
    pub fn new(
        project: Arc<RwLock<Project>>,
        plugin_manager: Arc<PluginManager>,
        cache_manager: Arc<CacheManager>,
    ) -> Self {
        let audio_engine = Arc::new(AudioEngine::new().expect("Failed to initialize Audio Engine"));

        let project_manager = Arc::new(ProjectManager::new(project.clone(), plugin_manager));
        let audio_service = Arc::new(AudioService::new(project, audio_engine, cache_manager));

        Self {
            project_manager,
            audio_service,
        }
    }

    /// Access the project immutably via a closure.
    pub fn with_project<R>(&self, f: impl FnOnce(&Project) -> R) -> R {
        let project = self.project_manager.get_project();
        let guard = project.read().expect("Failed to acquire project read lock");
        f(&guard)
    }

    /// Access the project mutably via a closure.
    pub fn with_project_mut<R>(&self, f: impl FnOnce(&mut Project) -> R) -> R {
        let project = self.project_manager.get_project();
        let mut guard = project
            .write()
            .expect("Failed to acquire project write lock");
        f(&mut guard)
    }

    #[deprecated(note = "Use with_project() or with_project_mut() instead")]
    pub fn get_project(&self) -> Arc<RwLock<Project>> {
        self.project_manager.get_project()
    }

    pub fn set_project(&self, project: Project) {
        let _ = self.project_manager.set_project(project);
    }

    pub fn get_audio_service(&self) -> Arc<AudioService> {
        self.audio_service.clone()
    }

    pub fn get_cache_manager(&self) -> Arc<crate::cache::CacheManager> {
        self.audio_service.get_cache_manager()
    }

    pub fn get_plugin_manager(&self) -> Arc<PluginManager> {
        self.project_manager.get_plugin_manager()
    }

    pub fn get_audio_engine(&self) -> Arc<AudioEngine> {
        self.audio_service.get_audio_engine()
    }

    pub fn audio_engine(&self) -> Arc<AudioEngine> {
        self.audio_service.get_audio_engine()
    }

    // --- Audio Operations ---

    pub fn reset_audio_pump(&self, time: f64) {
        self.audio_service.reset_audio_pump(time);
    }

    pub fn pump_audio(&self) {
        self.audio_service.pump_audio();
    }

    pub fn render_audio(&self, start_time: f64, duration: f64) -> Vec<f32> {
        self.audio_service.render_audio(start_time, duration)
    }
}
