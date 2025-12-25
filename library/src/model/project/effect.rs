//! Effect configuration for clips.

use crate::model::project::property::{Property, PropertyMap, PropertyValue};
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
            && self.effect_type == other.effect_type
            && self.properties == other.properties
    }
}

impl Eq for EffectConfig {}

impl EffectConfig {
    /// Update or upsert an effect property value/keyframe.
    pub fn update_property_or_keyframe(
        &mut self,
        key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) {
        if let Some(prop) = self.properties.get_mut(key) {
            if prop.evaluator == "keyframe" {
                prop.upsert_keyframe(time, value, easing);
            } else {
                self.properties
                    .set(key.to_string(), Property::constant(value));
            }
        } else {
            self.properties
                .set(key.to_string(), Property::constant(value));
        }
    }
}
