pub mod model;
mod loader;
mod rendering;

use model::frame::parse_frame_info;
use crate::loader::image::Image;
use crate::rendering::render_frame;
use crate::model::project::Project;

pub fn render_frame_from_json(json_str: &str) -> Result<Image, Box<dyn std::error::Error>> {
    let frame_info = parse_frame_info(json_str)?;
    render_frame(frame_info)
}

pub fn load_project(project_path: &str) -> Result<Project, Box<dyn std::error::Error>> {
    let project = Project::load(project_path)?;
    Ok(project)
}
