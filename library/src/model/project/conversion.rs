use crate::model::project::entity::Entity;

use super::TrackEntity;

impl From<&TrackEntity> for Entity {
    fn from(track_entity: &TrackEntity) -> Self {
        let mut entity = Entity::new(track_entity.entity_type.as_str());
        entity.start_time = track_entity.start_time;
        entity.end_time = track_entity.end_time;
        entity.fps = track_entity.fps;
        entity.properties = track_entity.properties.clone();
        entity
    }
}
