//! Effect configuration for clips.

use crate::model::project::property::PropertyMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EffectConfig {
    pub id: Uuid,
    pub effect_type: String,
    pub properties: PropertyMap,
}

impl std::hash::Hash for EffectConfig {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl PartialEq for EffectConfig {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for EffectConfig {}
