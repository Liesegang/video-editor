pub mod model;
mod loader;
pub mod rendering;
pub mod framing;

use model::frame::parse_frame_info;
use crate::loader::image::Image;
use crate::rendering::render_frame;
use crate::model::project::Project;
use crate::rendering::skia_renderer::SkiaRenderer;

pub fn render_frame_from_json(json_str: &str) -> Result<Image, Box<dyn std::error::Error>> {
    let frame_info = parse_frame_info(json_str)?;
    let mut renderer = SkiaRenderer::new(
        frame_info.width as u32,
        frame_info.height as u32,
        frame_info.background_color.clone(),
    );
    render_frame(frame_info, &mut renderer)
}

pub fn load_project(project_path: &str) -> Result<Project, Box<dyn std::error::Error>> {
    let project = Project::load(project_path)?;
    Ok(project)
}
