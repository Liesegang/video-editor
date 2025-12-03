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
  let frame = composition.render_frame(frame_index);

  debug!(
    "Frame {} summary: objects={}",
    frame_index,
    frame.objects.len()
  );
  frame
}
