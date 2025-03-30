use crate::model::frame::entity::FrameEntity::{Image, Video};
use crate::model::{frame::frame::FrameInfo, project::{Project, TrackEntity}};

pub fn get_frame_from_project(project: &Project, composition_index: usize, frame_index: f64) -> FrameInfo {
  let composition = &project.compositions[composition_index];
  let mut frame = FrameInfo {
    width: composition.width,
    height: composition.height,
    background_color: composition.background_color.clone(),
    color_profile: composition.color_profile.clone(),
    objects: Vec::new(),
  };

  for track in composition.tracks.iter() {
    for entity in track.entities.iter() {
      match entity {
        TrackEntity::Video { file_path, time_range, transform, zero } => {
          if time_range.start <= frame_index && time_range.end >= frame_index {
            let video = Video {
              file_path: file_path.clone(),
              frame_number: (zero + frame_index - time_range.start) as u64,
              transform: transform.get_value(frame_index),
            };
            frame.objects.push(video);
          }
        }
        TrackEntity::Image { file_path, time_range, transform } => {
          if time_range.start <= frame_index && time_range.end >= frame_index {
            let image = Image {
              file_path: file_path.clone(),
              transform: transform.get_value(frame_index),
            };
            frame.objects.push(image);
          }
        }
      }
    }
  }
  frame
}

