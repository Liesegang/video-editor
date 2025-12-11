use crate::model::project::entity::Entity;

use super::TrackEntity;

impl From<&TrackEntity> for Entity {
    fn from(track_entity: &TrackEntity) -> Self {
        let mut entity = Entity::new(track_entity.entity_type.as_str());
        entity.id = track_entity.id; // Preserve ID
        entity.in_frame = track_entity.in_frame;
        entity.out_frame = track_entity.out_frame;
        entity.source_begin_frame = track_entity.source_begin_frame;
        entity.duration_frame = track_entity.duration_frame;
        entity.fps = track_entity.fps;
        entity.properties = track_entity.properties.clone();
        entity.effects = track_entity.effects.clone();
        entity
    }
}
