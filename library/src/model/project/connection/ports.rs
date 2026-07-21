use crate::model::{GeneratorContent, NodeContent};

use super::super::Project;
use super::{
    AUDIO_OUTPUT_PORT, BACKGROUND_SHAPE_INPUT_PORT, DURATION_PORT, FMOD_X_INPUT_PORT, FPS_PORT,
    FRAME_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, MERGE_SOUNDS_PORT,
    NUMERIC_A_INPUT_PORT, PortAddress, PortDataType, PortDefinition, PortDirection, PortExposure,
    PortOwner, PortSide, RESOLUTION_PORT, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
};

fn metadata_catalog(direction: PortDirection, exposure: PortExposure) -> Vec<PortDefinition> {
    let ports: &[(&str, &str, PortDataType)] = match direction {
        // Time, Duration, and Resolution remain the authored container
        // overrides. FPS and Frame are derived, read-only context values.
        PortDirection::Input => &[
            (TIME_PORT, "Time", PortDataType::Number),
            (DURATION_PORT, "Duration", PortDataType::Number),
            (RESOLUTION_PORT, "Resolution", PortDataType::Vec2),
        ],
        PortDirection::Output => &[
            (TIME_PORT, "Time", PortDataType::Number),
            (FRAME_PORT, "Frame", PortDataType::Integer),
            (FPS_PORT, "FPS", PortDataType::Number),
            (DURATION_PORT, "Duration", PortDataType::Number),
            (RESOLUTION_PORT, "Resolution", PortDataType::Vec2),
        ],
    };
    ports
        .iter()
        .cloned()
        .map(|(key, label, data_type)| match direction {
            PortDirection::Input => PortDefinition {
                exposure,
                ..PortDefinition::input(key, label, data_type)
            },
            PortDirection::Output => {
                PortDefinition::output(key, label, data_type, PortSide::Left, exposure)
            }
        })
        .collect()
}

fn container_ports() -> Vec<PortDefinition> {
    let mut ports = metadata_catalog(PortDirection::Input, PortExposure::External);
    ports.extend(metadata_catalog(
        PortDirection::Output,
        PortExposure::Internal,
    ));
    ports.push(PortDefinition::output(
        IMAGE_OUTPUT_PORT,
        "Image",
        PortDataType::Image,
        PortSide::Right,
        PortExposure::External,
    ));
    ports.push(PortDefinition::output(
        AUDIO_OUTPUT_PORT,
        "Audio",
        PortDataType::Audio,
        PortSide::Right,
        PortExposure::External,
    ));
    ports
}

fn node_ports(
    node: &crate::model::Node,
    media_kind: Option<&crate::model::asset::AssetKind>,
) -> Vec<PortDefinition> {
    let mut ports = Vec::new();
    let time_input = || PortDefinition::input(TIME_PORT, "Time", PortDataType::Number);
    let image_output = || {
        PortDefinition::output(
            IMAGE_OUTPUT_PORT,
            "Image",
            PortDataType::Image,
            PortSide::Right,
            PortExposure::Graph,
        )
    };
    let audio_output = || {
        PortDefinition::output(
            AUDIO_OUTPUT_PORT,
            "Audio",
            PortDataType::Audio,
            PortSide::Right,
            PortExposure::Graph,
        )
    };
    let mut include_property_inputs = true;
    match node.content() {
        NodeContent::Generator(GeneratorContent::Text) => {
            ports.extend([
                time_input(),
                PortDefinition::input("text", "Text", PortDataType::String),
                PortDefinition::input("font_family", "Font", PortDataType::String),
                PortDefinition::input("size", "Size", PortDataType::Number),
            ]);
            ports.push(PortDefinition::output(
                SHAPE_OUTPUT_PORT,
                "Shape",
                PortDataType::Shape,
                PortSide::Right,
                PortExposure::Graph,
            ));
        }
        NodeContent::Generator(GeneratorContent::Solid) => {
            ports.push(time_input());
            ports.push(PortDefinition::input("color", "Color", PortDataType::Color));
            ports.push(image_output());
        }
        NodeContent::Generator(GeneratorContent::Shape) => {
            ports.extend([
                time_input(),
                PortDefinition::input("path", "Path", PortDataType::Path),
            ]);
            ports.push(PortDefinition::output(
                SHAPE_OUTPUT_PORT,
                "Shape",
                PortDataType::Shape,
                PortSide::Right,
                PortExposure::Graph,
            ));
        }
        NodeContent::Generator(GeneratorContent::SkSL) => {
            ports.push(time_input());
            ports.push(PortDefinition::input(
                "shader",
                "Shader",
                PortDataType::String,
            ));
            ports.push(image_output());
        }
        NodeContent::Media(_) => {
            ports.push(time_input());
            match media_kind {
                Some(crate::model::asset::AssetKind::Video) => {
                    ports.push(image_output());
                    ports.push(audio_output());
                }
                Some(crate::model::asset::AssetKind::Image) => ports.push(image_output()),
                Some(crate::model::asset::AssetKind::Audio) => ports.push(audio_output()),
                _ => {}
            }
        }
        NodeContent::CompositionInstance(_) => {
            ports.push(time_input());
            ports.extend(metadata_catalog(PortDirection::Output, PortExposure::Graph));
            ports.push(image_output());
            ports.push(audio_output());
        }
        NodeContent::PluginOperation(operation) => {
            include_property_inputs = false;
            ports.extend(operation.declared_ports.iter().cloned());
        }
        NodeContent::Value(value) => {
            include_property_inputs = false;
            ports.extend(value.port_definitions().iter().cloned());
        }
        NodeContent::Merge => {
            ports.push(time_input());
            ports.push(
                PortDefinition::input(MERGE_IMAGES_PORT, "Images", PortDataType::Image).variadic(),
            );
            ports.push(image_output());
        }
        NodeContent::SoundMerge => {
            ports.push(time_input());
            ports.push(
                PortDefinition::input(MERGE_SOUNDS_PORT, "Sounds", PortDataType::Audio).variadic(),
            );
            ports.push(audio_output());
        }
    }
    if include_property_inputs {
        let mut properties = node.properties().iter().collect::<Vec<_>>();
        properties.sort_by(|(left, _), (right, _)| {
            canonical_common_property_rank(left)
                .cmp(&canonical_common_property_rank(right))
                .then_with(|| left.cmp(right))
        });
        for (key, property) in properties {
            if ports
                .iter()
                .any(|port| port.key == *key && port.direction == PortDirection::Input)
            {
                continue;
            }
            let data_type = property
                .value()
                .map(property_value_data_type)
                .unwrap_or(PortDataType::Any);
            ports.push(PortDefinition::input(
                key,
                &humanize_port_key(key),
                data_type,
            ));
        }
    }
    canonicalize_node_ports(node, ports)
}

fn canonicalize_node_ports(
    node: &crate::model::Node,
    ports: Vec<PortDefinition>,
) -> Vec<PortDefinition> {
    let mut indexed = ports.into_iter().enumerate().collect::<Vec<_>>();
    indexed.sort_by(|(left_index, left), (right_index, right)| {
        let left_rank = canonical_node_port_rank(node, left);
        let right_rank = canonical_node_port_rank(node, right);
        left_rank
            .cmp(&right_rank)
            .then_with(|| left_index.cmp(right_index))
    });
    indexed.into_iter().map(|(_, port)| port).collect()
}

fn canonical_common_property_rank(name: &str) -> u8 {
    match name {
        "position" => 0,
        "rotation" => 1,
        "scale" => 2,
        "anchor" => 3,
        _ => 4,
    }
}

/// One model-side ordering contract consumed by every Node view. Port order
/// is presentation metadata only; addresses and graph evaluation are keyed.
fn canonical_node_port_rank(node: &crate::model::Node, port: &PortDefinition) -> u8 {
    if port.direction == PortDirection::Output {
        return 4;
    }
    if port.key == TIME_PORT {
        return 0;
    }
    if matches!(
        port.key.as_str(),
        IMAGE_INPUT_PORT
            | SHAPE_INPUT_PORT
            | BACKGROUND_SHAPE_INPUT_PORT
            | MERGE_IMAGES_PORT
            | MERGE_SOUNDS_PORT
            | FMOD_X_INPUT_PORT
            | NUMERIC_A_INPUT_PORT
    ) {
        return 1;
    }
    let property_name = port.key.strip_prefix("property:").unwrap_or(&port.key);
    if node.properties().get(property_name).is_some() {
        return 3;
    }
    2
}

fn property_value_data_type(value: &crate::model::property::PropertyValue) -> PortDataType {
    use crate::model::property::PropertyValue;
    match value {
        PropertyValue::Number(_) => PortDataType::Number,
        PropertyValue::Integer(_) => PortDataType::Integer,
        PropertyValue::String(_) => PortDataType::String,
        PropertyValue::Boolean(_) => PortDataType::Boolean,
        PropertyValue::Vec2(_) => PortDataType::Vec2,
        PropertyValue::Color(_) => PortDataType::Color,
        PropertyValue::Vec3(_) => PortDataType::Vec3,
        PropertyValue::Vec4(_) => PortDataType::Vec4,
        PropertyValue::Array(_) | PropertyValue::Map(_) => PortDataType::Any,
    }
}

fn humanize_port_key(key: &str) -> String {
    let mut result = String::new();
    let mut uppercase = true;
    for character in key.chars() {
        if matches!(character, '_' | '-') {
            result.push(' ');
            uppercase = true;
        } else if uppercase {
            result.extend(character.to_uppercase());
            uppercase = false;
        } else {
            result.push(character);
        }
    }
    result
}

impl Project {
    pub fn port_definitions(&self, owner: PortOwner) -> Vec<PortDefinition> {
        match owner {
            PortOwner::Composition(id) if self.get_composition(id).is_some() => container_ports(),
            PortOwner::Track(id) if self.get_track(id).is_some() => container_ports(),
            PortOwner::Clip(id) if self.get_clip(id).is_some() => container_ports(),
            PortOwner::Node(id) => self
                .get_node(id)
                .map(|node| {
                    let media_kind = match node.content() {
                        NodeContent::Media(media) => {
                            self.get_asset(media.asset_id).map(|asset| &asset.kind)
                        }
                        _ => None,
                    };
                    node_ports(node, media_kind)
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    pub fn port_definition(
        &self,
        address: &PortAddress,
        direction: PortDirection,
    ) -> Option<PortDefinition> {
        self.port_definitions(address.owner)
            .into_iter()
            .find(|port| port.key == address.port && port.direction == direction)
    }
}

pub(super) fn is_graph_connectable_type(data_type: PortDataType) -> bool {
    data_type != PortDataType::Any
}
