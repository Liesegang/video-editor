use crate::model::project::entity::Entity;

use super::{END_TIME_KEY, START_TIME_KEY, TrackEntity};

impl From<&TrackEntity> for Entity {
  fn from(track_entity: &TrackEntity) -> Self {
    let mut entity = Entity::new(track_entity.entity_type.as_str());
    entity.start_time = track_entity
      .properties
      .get_constant_number(START_TIME_KEY, 0.0);
    entity.end_time = track_entity
      .properties
      .get_constant_number(END_TIME_KEY, 0.0);
    entity.properties = track_entity.properties.clone();
    entity
  }
}
