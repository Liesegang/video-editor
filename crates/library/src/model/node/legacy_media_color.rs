//! Detection of retired config-less Media Node color properties.
//!
//! Source interpretation now belongs to `Asset::source_color` and is bound to
//! the exact Project color configuration. Persisted pre-v1 Nodes remain
//! loadable, but a non-empty retired field must stop that Media leaf instead
//! of being ignored or reinterpreted.

use super::{Node, NodeContent};
use crate::model::property::{Property, PropertyValue};

pub const LEGACY_MEDIA_COLOR_PROPERTY_KEYS: [&str; 2] = ["input_color_space", "output_color_space"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyMediaColorProperty {
    key: &'static str,
    authored_state: String,
}

impl LegacyMediaColorProperty {
    pub const fn key(&self) -> &'static str {
        self.key
    }

    pub fn authored_state(&self) -> &str {
        &self.authored_state
    }
}

pub fn is_legacy_media_color_property(key: &str) -> bool {
    LEGACY_MEDIA_COLOR_PROPERTY_KEYS.contains(&key)
}

pub fn active_legacy_media_color_properties(node: &Node) -> Vec<LegacyMediaColorProperty> {
    if !matches!(node.content(), NodeContent::Media(_)) {
        return Vec::new();
    }
    LEGACY_MEDIA_COLOR_PROPERTY_KEYS
        .into_iter()
        .filter_map(|key| {
            let property = node.properties().get(key)?;
            (!is_explicit_blank_default(property)).then(|| LegacyMediaColorProperty {
                key,
                authored_state: authored_state(property),
            })
        })
        .collect()
}

fn is_explicit_blank_default(property: &Property) -> bool {
    property.evaluator == "constant"
        && property.properties.len() == 1
        && matches!(
            property.properties.get("value"),
            Some(PropertyValue::String(value)) if value.trim().is_empty()
        )
}

fn authored_state(property: &Property) -> String {
    match property.value() {
        Some(PropertyValue::String(value)) if !value.trim().is_empty() => {
            format!("{} value {:?}", property.evaluator, value)
        }
        Some(PropertyValue::String(_)) => format!("{} non-default authoring", property.evaluator),
        Some(_) => format!(
            "{} value with an invalid non-string type",
            property.evaluator
        ),
        None => format!("{} authoring without a fallback value", property.evaluator),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MediaContent, Node};
    use uuid::Uuid;

    fn persisted_media_node(properties: serde_json::Value) -> Node {
        let media = MediaContent {
            asset_id: Uuid::new_v4(),
            stream_index: None,
            audio_stream_index: None,
        };
        let mut node = serde_json::to_value(
            Node::from_media_converter("legacy", media, &[], "fixture.mp4".to_string())
                .expect("empty converter contract is valid for a Media test Node"),
        )
        .expect("serialize Media Node");
        node["properties"] = properties;
        serde_json::from_value(node).expect("deserialize persisted Media Node")
    }

    #[test]
    fn only_the_exact_old_blank_defaults_are_inert() {
        let properties = serde_json::json!({
            "file_path": {"type":"constant", "properties":{"value":"fixture.mp4"}},
            "input_color_space": {"type":"constant", "properties":{"value":""}},
            "output_color_space": {"type":"constant", "properties":{"value":"   "}}
        });
        let node = persisted_media_node(properties);
        assert!(active_legacy_media_color_properties(&node).is_empty());
    }

    #[test]
    fn authored_and_malformed_old_fields_are_active() {
        let properties = serde_json::json!({
            "file_path": {"type":"constant", "properties":{"value":"fixture.mp4"}},
            "input_color_space": {"type":"constant", "properties":{"value":"ACEScg"}},
            "output_color_space": {"type":"expression", "properties":{
                "value":"", "expression":"'sRGB'"
            }}
        });
        let node = persisted_media_node(properties);
        let active = active_legacy_media_color_properties(&node);
        assert_eq!(active.len(), 2);
        assert_eq!(active[0].key(), "input_color_space");
        assert_eq!(active[1].key(), "output_color_space");
    }

    #[test]
    fn non_media_nodes_never_acquire_legacy_media_semantics() {
        let node = Node::new_merge("merge");
        assert!(active_legacy_media_color_properties(&node).is_empty());
    }
}
