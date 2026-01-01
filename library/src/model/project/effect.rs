//! Effect configuration for clips.

use crate::model::project::property::PropertyMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq)]
pub struct EffectConfig {
    pub id: Uuid,
    pub effect_type: String,
    pub properties: PropertyMap,
}
