//! Lightweight data types for the node editor UI.

use uuid::Uuid;

/// Data type of a pin, used for type-based coloring and connection validation.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum PinDataType {
    Image,
    Scalar,
    Integer,
    Boolean,
    Vec2,
    Vec3,
    Color,
    String,
    Style,
    Shape,
    Path,
    Enum,
    List,
    Audio,
    #[default]
    Any,
}

/// Check whether two pin data types are compatible for connection.
pub fn are_types_compatible(from: &PinDataType, to: &PinDataType) -> bool {
    if *from == PinDataType::Any || *to == PinDataType::Any {
        return true;
    }
    from == to
}

/// Information about a pin for rendering.
#[derive(Clone, Debug)]
pub struct PinInfo {
    pub name: String,
    pub display_name: String,
    pub is_output: bool,
    pub data_type: PinDataType,
}

impl PinInfo {
    pub fn input(name: &str, display_name: &str, data_type: PinDataType) -> Self {
        Self {
            name: name.to_string(),
            display_name: display_name.to_string(),
            is_output: false,
            data_type,
        }
    }

    pub fn output(name: &str, display_name: &str, data_type: PinDataType) -> Self {
        Self {
            name: name.to_string(),
            display_name: display_name.to_string(),
            is_output: true,
            data_type,
        }
    }
}

/// A connection between two pins (view data).
#[derive(Clone, Debug)]
pub struct ConnectionView {
    pub id: Uuid,
    pub from_node: Uuid,
    pub from_pin: String,
    pub to_node: Uuid,
    pub to_pin: String,
}

/// Kind of container node for distinct rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContainerKind {
    Composition,
    Track,
    Layer,
}

/// How a node should be displayed.
#[derive(Clone, Debug)]
pub enum NodeDisplay {
    /// A data-flow graph node with typed pins.
    Graph {
        type_id: String,
        display_name: String,
        pins: Vec<PinInfo>,
    },
    /// A container (composition/track/layer) that holds child nodes.
    Container {
        kind: ContainerKind,
        name: String,
        child_ids: Vec<Uuid>,
        pins: Vec<PinInfo>,
    },
    /// A leaf node (source) with fixed pins.
    Leaf {
        kind_label: String,
        pins: Vec<PinInfo>,
    },
}

/// Node type information for the context menu.
#[derive(Clone, Debug)]
pub struct NodeTypeInfo {
    pub type_id: String,
    pub display_name: String,
    pub category: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_compatible_same_type() {
        assert!(are_types_compatible(
            &PinDataType::Image,
            &PinDataType::Image
        ));
        assert!(are_types_compatible(
            &PinDataType::Scalar,
            &PinDataType::Scalar
        ));
    }

    #[test]
    fn test_type_compatible_any_matches_all() {
        assert!(are_types_compatible(&PinDataType::Any, &PinDataType::Image));
        assert!(are_types_compatible(
            &PinDataType::Scalar,
            &PinDataType::Any
        ));
        assert!(are_types_compatible(&PinDataType::Any, &PinDataType::Any));
    }

    #[test]
    fn test_type_incompatible() {
        assert!(!are_types_compatible(
            &PinDataType::Image,
            &PinDataType::Scalar
        ));
        assert!(!are_types_compatible(
            &PinDataType::Vec2,
            &PinDataType::Color
        ));
        assert!(!are_types_compatible(
            &PinDataType::Boolean,
            &PinDataType::String
        ));
    }

    #[test]
    fn test_pin_info_requires_data_type() {
        let pin = PinInfo::input("test", "Test", PinDataType::Scalar);
        assert_eq!(pin.data_type, PinDataType::Scalar);
        assert!(!pin.is_output);

        let pin = PinInfo::output("out", "Out", PinDataType::Image);
        assert_eq!(pin.data_type, PinDataType::Image);
        assert!(pin.is_output);
    }
}
