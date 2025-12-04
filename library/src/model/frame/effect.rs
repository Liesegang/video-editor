use crate::model::project::property::PropertyValue;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImageEffect {
    #[serde(rename = "type")]
    pub effect_type: String,
    #[serde(default)]
    pub properties: HashMap<String, PropertyValue>,
}
