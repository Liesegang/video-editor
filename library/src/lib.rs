pub mod animation;
pub mod cache;
pub mod framing;
mod loader;
pub mod model;
pub mod plugin;
pub mod rendering;
pub mod service;
pub mod util;

pub use crate::loader::image::Image;
pub use crate::service::ProjectService;

use crate::model::project::project::Project;

pub fn load_project(project_path: &str) -> Result<Project, Box<dyn std::error::Error>> {
  let project = Project::load(project_path)?;
  Ok(project)
}
