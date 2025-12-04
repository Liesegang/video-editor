use std::collections::HashMap;
use std::sync::Arc;
use log::warn;

use crate::model::project::entity::Entity;
use crate::model::frame::entity::FrameObject;
use crate::model::project::project::Composition;
use super::property::PropertyEvaluatorRegistry;

/// Trait for converting an Entity into a FrameObject.
pub trait EntityConverter: Send + Sync {
    fn convert_entity(
        &self,
        entity: &Entity,
        time: f64,
        composition: &Composition,
        property_evaluators: &Arc<PropertyEvaluatorRegistry>,
    ) -> Option<FrameObject>;
}

/// Registry for EntityConverter implementations.
pub struct EntityConverterRegistry {
    converters: HashMap<String, Box<dyn EntityConverter>>,
}

impl EntityConverterRegistry {
    pub fn new() -> Self {
        Self {
            converters: HashMap::new(),
        }
    }

    pub fn register(&mut self, entity_type: String, converter: Box<dyn EntityConverter>) {
        self.converters.insert(entity_type, converter);
    }

    pub fn convert_entity(
        &self,
        entity: &Entity,
        time: f64,
        composition: &Composition,
        property_evaluators: &Arc<PropertyEvaluatorRegistry>,
    ) -> Option<FrameObject> {
        match self.converters.get(&entity.entity_type) {
            Some(converter) => converter.convert_entity(entity, time, composition, property_evaluators),
            None => {
                warn!("No converter registered for entity type '{}'", entity.entity_type);
                None
            }
        }
    }
}
