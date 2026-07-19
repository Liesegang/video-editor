use crate::model::property::PropertyMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
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
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
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
}
