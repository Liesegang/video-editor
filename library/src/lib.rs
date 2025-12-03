pub mod animation;
pub mod framing;
mod loader;
pub mod model;
pub mod plugin;
pub mod rendering;
pub mod util;

use crate::loader::image::Image;
use crate::model::project::project::Project;
use crate::rendering::RenderContext;
use crate::rendering::skia_renderer::SkiaRenderer;
use model::frame::parse_frame_info;
use std::error::Error;

pub fn render_frame_from_json(json_str: &str) -> Result<Image, Box<dyn std::error::Error>> {
  let frame_info = parse_frame_info(json_str)?;
  let renderer = SkiaRenderer::new(
    frame_info.width as u32,
    frame_info.height as u32,
    frame_info.background_color.clone(),
  );
  let mut context = RenderContext::new(renderer);
  context.render_frame(frame_info)
}

pub fn create_render_context(
  width: u32,
  height: u32,
  background_color: model::frame::color::Color,
) -> RenderContext<SkiaRenderer> {
  let renderer = SkiaRenderer::new(width, height, background_color);
  RenderContext::new(renderer)
}

pub fn create_render_context_from_json(
  json_str: &str,
) -> Result<RenderContext<SkiaRenderer>, Box<dyn Error>> {
  let frame_info = parse_frame_info(json_str)?;
  let renderer = SkiaRenderer::new(
    frame_info.width as u32,
    frame_info.height as u32,
    frame_info.background_color.clone(),
  );
  Ok(RenderContext::new(renderer))
}

pub fn render_frame_with_context(
  context: &mut RenderContext<SkiaRenderer>,
  json_str: &str,
) -> Result<Image, Box<dyn Error>> {
  let frame_info = parse_frame_info(json_str)?;
  context.render_frame(frame_info)
}

pub fn load_project(project_path: &str) -> Result<Project, Box<dyn std::error::Error>> {
  let project = Project::load(project_path)?;
  Ok(project)
}
