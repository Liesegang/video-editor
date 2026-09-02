//! Timeline-first rendering boundary used by the editor application.
//!
//! This deliberately accepts only [`AuthoringProject`].  The older graph
//! project evaluator remains an internal rendering concern while the hard
//! cutover is completed; application code must not select between the two
//! authoring models.

use std::sync::Arc;

use crate::core::cache::SharedCacheManager;
use crate::core::rendering::renderer::{RenderOutput, Renderer};
use crate::editor::render_service::{RenderDestination, RenderService};
use crate::error::LibraryError;
use crate::model::authoring::AuthoringProject;
use crate::model::frame::frame::FrameInfo;
use crate::plugin::{ExportFrame, PluginManager};

pub struct AuthoringRenderService<T: Renderer> {
    engine: RenderService<T>,
}

impl<T: Renderer> AuthoringRenderService<T> {
    pub fn new(
        renderer: T,
        plugin_manager: Arc<PluginManager>,
        cache_manager: SharedCacheManager,
    ) -> Self {
        Self {
            engine: RenderService::new(renderer, plugin_manager, cache_manager),
        }
    }

    pub fn renderer_mut(&mut self) -> &mut T {
        &mut self.engine.renderer
    }

    pub fn render_frame(
        &mut self,
        project: &AuthoringProject,
        frame_info: &FrameInfo,
        destination: RenderDestination,
    ) -> Result<RenderOutput, LibraryError> {
        self.engine
            .render_authoring_frame(project, frame_info, destination)
    }

    pub fn render_export_frame(
        &mut self,
        project: &AuthoringProject,
        frame_info: &FrameInfo,
    ) -> Result<ExportFrame, LibraryError> {
        self.engine
            .render_authoring_export_frame(project, frame_info)
    }
}
