use crate::model::frame::entity::FrameObject;
use crate::model::{
  frame::frame::FrameInfo,
  project::entity::Entity,
  project::project::{Composition, Project},
};
use crate::util::timing::ScopedTimer;
use log::debug;

pub struct FrameEvaluator<'a> {
  composition: &'a Composition,
}

impl<'a> FrameEvaluator<'a> {
  pub fn new(composition: &'a Composition) -> Self {
    Self { composition }
  }

  pub fn evaluate(&self, time: f64) -> FrameInfo {
    let mut frame = self.initialize_frame();
    for entity in self.active_entities(time) {
      if let Some(object) = self.convert_entity(entity, time) {
        frame.objects.push(object);
      }
    }
    frame
  }

  fn initialize_frame(&self) -> FrameInfo {
    FrameInfo {
      width: self.composition.width,
      height: self.composition.height,
      background_color: self.composition.background_color.clone(),
      color_profile: self.composition.color_profile.clone(),
      objects: Vec::new(),
    }
  }

  fn active_entities(&self, time: f64) -> impl Iterator<Item = &Entity> {
    self
      .composition
      .cached_entities()
      .iter()
      .filter(move |entity| entity.start_time <= time && entity.end_time >= time)
  }

  fn convert_entity(&self, entity: &Entity, time: f64) -> Option<FrameObject> {
    entity.to_frame_object(time)
  }
}

pub fn evaluate_composition_frame(composition: &Composition, time: f64) -> FrameInfo {
  FrameEvaluator::new(composition).evaluate(time)
}

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
  let frame = evaluate_composition_frame(composition, frame_index);

  debug!(
    "Frame {} summary: objects={}",
    frame_index,
    frame.objects.len()
  );
  frame
}
