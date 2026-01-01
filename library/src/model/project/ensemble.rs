use crate::model::project::property::{Property, PropertyMap, PropertyValue};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EffectorInstance {
    pub id: Uuid,
    pub effector_type: String, // e.g. "step_delay", "randomize"
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

    pub fn default_of_type(type_name: &str) -> Self {
        let mut props = PropertyMap::default();
        match type_name {
            "transform" => {
                props.set(
                    "tx".into(),
                    Property::constant(PropertyValue::Number(ordered_float::OrderedFloat(0.0))),
                );
                props.set(
                    "ty".into(),
                    Property::constant(PropertyValue::Number(ordered_float::OrderedFloat(0.0))),
                );
                props.set(
                    "scale_x".into(),
                    Property::constant(PropertyValue::Number(ordered_float::OrderedFloat(1.0))),
                );
                props.set(
                    "scale_y".into(),
                    Property::constant(PropertyValue::Number(ordered_float::OrderedFloat(1.0))),
                );
                props.set(
                    "rotation".into(),
                    Property::constant(PropertyValue::Number(ordered_float::OrderedFloat(0.0))),
                );
            }
            "step_delay" => {
                props.set(
                    "delay".into(),
                    Property::constant(PropertyValue::Number(ordered_float::OrderedFloat(0.1))),
                );
                props.set(
                    "duration".into(),
                    Property::constant(PropertyValue::Number(ordered_float::OrderedFloat(1.0))),
                );
            }
            "randomize" => {
                props.set(
                    "seed".into(),
                    Property::constant(PropertyValue::Number(ordered_float::OrderedFloat(0.0))),
                );
                props.set(
                    "amount".into(),
                    Property::constant(PropertyValue::Number(ordered_float::OrderedFloat(1.0))),
                );
            }
            _ => {}
        }
        Self::new(type_name, props)
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

    pub fn default_of_type(type_name: &str) -> Self {
        let mut props = PropertyMap::default();
        match type_name {
            "backplate" => {
                props.set(
                    "color".into(),
                    Property::constant(PropertyValue::Color(crate::model::frame::color::Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 128,
                    })),
                );
                props.set(
                    "padding".into(),
                    Property::constant(PropertyValue::Number(ordered_float::OrderedFloat(10.0))),
                );
                props.set(
                    "radius".into(),
                    Property::constant(PropertyValue::Number(ordered_float::OrderedFloat(4.0))),
                );
            }
            _ => {}
        }
        Self::new(type_name, props)
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
