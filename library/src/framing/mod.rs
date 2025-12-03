use crate::model::entity::Entity;
use crate::model::{frame::frame::FrameInfo, project::project::Project};
use crate::util::timing::ScopedTimer;
use log::debug;

pub fn get_frame_from_project(
  project: &Project,
  composition_index: usize,
  frame_index: f64,
) -> FrameInfo {
  let _timer = ScopedTimer::debug(format!(
    "Frame assembly comp={} frame={}",
    composition_index, frame_index
  ));

  let composition = &project.compositions[composition_index];
  let mut frame = FrameInfo {
    width: composition.width,
    height: composition.height,
    background_color: composition.background_color.clone(),
    color_profile: composition.color_profile.clone(),
    objects: Vec::new(),
  };

  let mut considered_entities = 0usize;
  let mut active_entities = 0usize;

  for track in composition.tracks.iter() {
    for track_entity in track.entities.iter() {
      considered_entities += 1;
      let entity: Entity = track_entity.into(); // conversion using From trait

      if entity.start_time <= frame_index && entity.end_time >= frame_index {
        active_entities += 1;
        if let Some(frame_entity) = entity.to_frame_entity(frame_index) {
          frame.objects.push(frame_entity);
        }
      }
    }
  }

  debug!(
    "Frame {} summary: considered={}, active={}, objects={}",
    frame_index,
    considered_entities,
    active_entities,
    frame.objects.len()
  );
  frame
}
