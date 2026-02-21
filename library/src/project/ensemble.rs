use crate::project::property::PropertyMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EffectorInstance {
    pub id: Uuid,
    pub effector_type: String,
    #[serde(default)]
    pub properties: PropertyMap,
}

impl EffectorInstance {
    pub fn new(effector_type: &str, properties: PropertyMap) -> Self {
        Self {
            id: Uuid::new_v4(),
            effector_type: effector_type.to_string(),
            properties,
        }
    }

    pub fn new_with_id(id: Uuid, effector_type: &str, properties: PropertyMap) -> Self {
        Self {
            id,
            effector_type: effector_type.to_string(),
            properties,
        }
    }
}

impl PartialEq for EffectorInstance {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.properties == other.properties
    }
}
impl Eq for EffectorInstance {}

impl std::hash::Hash for EffectorInstance {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.properties.hash(state);
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DecoratorInstance {
    pub id: Uuid,
    pub decorator_type: String, // e.g. "backplate"
    #[serde(default)]
    pub properties: PropertyMap,
}

impl DecoratorInstance {
    pub fn new(decorator_type: &str, properties: PropertyMap) -> Self {
        Self {
            id: Uuid::new_v4(),
            decorator_type: decorator_type.to_string(),
            properties,
        }
    }

    pub fn new_with_id(id: Uuid, decorator_type: &str, properties: PropertyMap) -> Self {
        Self {
            id,
            decorator_type: decorator_type.to_string(),
            properties,
        }
    }
}

impl PartialEq for DecoratorInstance {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.properties == other.properties
    }
}
impl Eq for DecoratorInstance {}

impl std::hash::Hash for DecoratorInstance {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.properties.hash(state);
    }
}
