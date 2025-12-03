use crate::model::entity::Entity;
use crate::model::{frame::frame::FrameInfo, project::project::Project};

pub fn get_frame_from_project(
  project: &Project,
  composition_index: usize,
  frame_index: f64,
) -> FrameInfo {
  let composition = &project.compositions[composition_index];
  let mut frame = FrameInfo {
    width: composition.width,
    height: composition.height,
    background_color: composition.background_color.clone(),
    color_profile: composition.color_profile.clone(),
    objects: Vec::new(),
  };

  for track in composition.tracks.iter() {
    for track_entity in track.entities.iter() {
      let entity: Entity = track_entity.into(); // conversion using From trait

      if entity.start_time <= frame_index && entity.end_time >= frame_index {
        if let Some(frame_entity) = entity.to_frame_entity(frame_index) {
          frame.objects.push(frame_entity);
        }
      }
    }
  }
  frame
}
