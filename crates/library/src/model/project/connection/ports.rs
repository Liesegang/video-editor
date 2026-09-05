use crate::model::{ListContent, NodeContent, native_node_descriptor_for_node};

use super::super::Project;
use super::{
    AUDIO_OUTPUT_PORT, BACKGROUND_SHAPE_INPUT_PORT, DURATION_PORT, FMOD_X_INPUT_PORT, FPS_PORT,
    FRAME_PORT, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, MERGE_SOUNDS_PORT,
    NUMERIC_A_INPUT_PORT, PortAddress, PortDataType, PortDefinition, PortDirection, PortExposure,
    PortOwner, PortSide, RESOLUTION_PORT, SHAPE_INPUT_PORT, SOUND_INPUT_PORT, SPECTRUM_INPUT_PORT,
    TIME_PORT,
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

fn node_ports(node: &crate::model::Node, media_asset_exists: bool) -> Vec<PortDefinition> {
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
        NodeContent::ModuleOutput(_) => {
            include_property_inputs = false;
        }
        NodeContent::Generator(_)
        | NodeContent::Value(_)
        | NodeContent::Color(_)
        | NodeContent::Data(_)
        | NodeContent::List(_)
        | NodeContent::Path(_)
        | NodeContent::NativeOperation(_)
        | NodeContent::Merge
        | NodeContent::SoundMerge
        | NodeContent::SoundAnalysis(_) => {
            include_property_inputs = false;
            if let Some(descriptor) = native_node_descriptor_for_node(node) {
                ports.extend(descriptor.ports().iter().cloned());
            } else {
                log::error!(
                    "Native Node {} has no catalog descriptor; exposing no graph ports",
                    node.id
                );
            }
        }
        NodeContent::Media(media) => {
            ports.push(time_input());
            if media_asset_exists && media.has_image_output() {
                ports.push(image_output());
            }
            if media_asset_exists && media.has_audio_output() {
                ports.push(audio_output());
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
            | SOUND_INPUT_PORT
            | SPECTRUM_INPUT_PORT
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
        PropertyValue::ColorValue(_) | PropertyValue::Color(_) => PortDataType::Color,
        PropertyValue::Vec3(_) => PortDataType::Vec3,
        PropertyValue::Vec4(_) => PortDataType::Vec4,
        PropertyValue::Path(_) => PortDataType::Path,
        PropertyValue::Gradient(_) => PortDataType::Gradient,
        PropertyValue::Pattern(_) => PortDataType::Pattern,
        PropertyValue::Array(_) | PropertyValue::Map(_) | PropertyValue::OpaqueJson(_) => {
            PortDataType::Any
        }
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
                    let media_asset_exists = match node.content() {
                        NodeContent::Media(media) => {
                            self.assets.iter().any(|asset| asset.id == media.asset_id)
                        }
                        _ => true,
                    };
                    node_ports(node, media_asset_exists)
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

fn is_graph_connectable_type(data_type: PortDataType) -> bool {
    data_type != PortDataType::Any
}

/// Whether one concrete output address can author a graph connection.
///
/// `Any` remains denied by default because most catalog placeholders and
/// plugin contracts do not provide a safe heterogeneous runtime payload. The
/// native Get List Item output is the narrow exception: its evaluator always
/// returns an existing serializable `PropertyValue` or `NoOutput`.
pub(super) fn is_graph_connectable_output(
    project: &Project,
    address: &PortAddress,
    data_type: PortDataType,
) -> bool {
    if is_graph_connectable_type(data_type) {
        return true;
    }
    data_type == PortDataType::Any
        && address.port == super::LIST_ITEM_OUTPUT_PORT
        && matches!(
            address.owner,
            PortOwner::Node(node_id)
                if project
                    .get_node(node_id)
                    .is_some_and(|node| matches!(node.content(), NodeContent::List(ListContent::GetItem)))
        )
}

/// Make List is sequence construction rather than set membership: the same
/// source address may intentionally occupy several independently ordered
/// slots (for example `[x, x]`). Other variadic operations retain their
/// existing duplicate-source policy until they explicitly adopt this
/// contract.
pub(super) fn variadic_target_allows_duplicate_sources(
    project: &Project,
    target: &PortAddress,
) -> bool {
    target.port == super::LIST_ITEMS_INPUT_PORT
        && matches!(
            target.owner,
            PortOwner::Node(node_id)
                if project
                    .get_node(node_id)
                    .is_some_and(|node| matches!(node.content(), NodeContent::List(ListContent::Make)))
        )
}

#[cfg(test)]
mod structured_property_value_tests {
    use super::*;
    use crate::model::path::{FillRule, PathValue};
    use crate::model::property::{ColorSpaceRef, ColorValue, PropertyValue};

    #[test]
    fn tagged_graph_color_uses_the_color_port_contract() -> Result<(), Box<dyn std::error::Error>> {
        let color = ColorValue::new(ColorSpaceRef::srgb(), [1.0, 0.0, 0.0, 1.0])?;
        assert_eq!(
            property_value_data_type(&PropertyValue::ColorValue(color)),
            PortDataType::Color
        );
        Ok(())
    }

    #[test]
    fn canonical_path_property_uses_path_ports() {
        let value = PropertyValue::Path(PathValue::empty(FillRule::NonZero));
        assert_eq!(property_value_data_type(&value), PortDataType::Path);
    }
}
