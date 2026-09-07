//! Point proximity topology and shared line-renderer Node contracts.

use ordered_float::OrderedFloat;

use super::descriptor::{DescriptorIdentity, DescriptorSpec, PortSpec};
use super::particle::POINT_COLOR_INPUT_PORT;
use super::point::POINT_SOURCE_PORT;
use crate::model::frame::color::Color;
use crate::model::frame::point::{
    POINT_CONNECTION_MAX_DISTANCE, POINT_CONNECTION_MAX_NEIGHBORS, POINT_LINE_MAX_WIDTH,
};
use crate::model::project::{IMAGE_OUTPUT_PORT, PortDataType};
use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};

pub(crate) const CONNECT_POINTS_CATALOG_ID: &str = "native.point.connect-points";
pub(crate) const POINT_LINE_RENDERER_CATALOG_ID: &str = "native.point.line-renderer";
pub(crate) const POINT_CONNECTIONS_PORT: &str = "connections";
pub(crate) const POINT_MIN_DISTANCE_INPUT_PORT: &str = "min_distance";
pub(crate) const POINT_MAX_DISTANCE_INPUT_PORT: &str = "max_distance";
pub(crate) const POINT_MAX_NEIGHBORS_INPUT_PORT: &str = "max_neighbors";
pub(crate) const POINT_LINE_WIDTH_INPUT_PORT: &str = "width";
pub(crate) const POINT_LINE_FADE_INPUT_PORT: &str = "fade";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointConnectionNodeRole {
    ConnectPoints,
    LineRenderer,
}

impl PointConnectionNodeRole {
    pub(crate) const fn catalog_id(self) -> &'static str {
        match self {
            Self::ConnectPoints => CONNECT_POINTS_CATALOG_ID,
            Self::LineRenderer => POINT_LINE_RENDERER_CATALOG_ID,
        }
    }

    pub(crate) fn from_catalog_id(catalog_id: &str) -> Option<Self> {
        [Self::ConnectPoints, Self::LineRenderer]
            .into_iter()
            .find(|role| role.catalog_id() == catalog_id)
    }
}

const CONNECT_POINTS_INPUTS: &[PortSpec] = &[
    PortSpec::single(POINT_SOURCE_PORT, "Points", PortDataType::PointSource),
    PortSpec::single(
        POINT_MIN_DISTANCE_INPUT_PORT,
        "Min Distance",
        PortDataType::Number,
    ),
    PortSpec::single(
        POINT_MAX_DISTANCE_INPUT_PORT,
        "Max Distance",
        PortDataType::Number,
    ),
    PortSpec::single(
        POINT_MAX_NEIGHBORS_INPUT_PORT,
        "Max Neighbors",
        PortDataType::Integer,
    ),
];
const CONNECT_POINTS_OUTPUTS: &[PortSpec] = &[PortSpec::single(
    POINT_CONNECTIONS_PORT,
    "Connections",
    PortDataType::PointConnections,
)];
const LINE_RENDERER_INPUTS: &[PortSpec] = &[
    PortSpec::single(
        POINT_CONNECTIONS_PORT,
        "Connections",
        PortDataType::PointConnections,
    ),
    PortSpec::single(POINT_COLOR_INPUT_PORT, "Color", PortDataType::Color),
    PortSpec::single(POINT_LINE_WIDTH_INPUT_PORT, "Width", PortDataType::Number),
    PortSpec::single(POINT_LINE_FADE_INPUT_PORT, "Fade", PortDataType::Number),
];
const LINE_RENDERER_OUTPUTS: &[PortSpec] = &[PortSpec::single(
    IMAGE_OUTPUT_PORT,
    "Image",
    PortDataType::Image,
)];

const SPECS: &[DescriptorSpec] = &[
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            CONNECT_POINTS_CATALOG_ID,
            "Connect Points",
            "Points",
            "node_editor.menu.create.point_connect_points",
            &["point", "connect", "proximity", "nearest", "plexus"],
        ),
        CONNECT_POINTS_INPUTS,
        CONNECT_POINTS_OUTPUTS,
        connect_points_properties,
    ),
    DescriptorSpec::implemented_native(
        DescriptorIdentity::new(
            POINT_LINE_RENDERER_CATALOG_ID,
            "Line Renderer",
            "Points",
            "node_editor.menu.create.point_line_renderer",
            &["point", "line", "render", "connection", "plexus"],
        ),
        LINE_RENDERER_INPUTS,
        LINE_RENDERER_OUTPUTS,
        line_renderer_properties,
    ),
];

pub(super) const fn specs() -> &'static [DescriptorSpec] {
    SPECS
}

fn connect_points_properties() -> Vec<PropertyDefinition> {
    vec![
        number_property(
            POINT_MIN_DISTANCE_INPUT_PORT,
            "Min Distance",
            0.0,
            f64::from(POINT_CONNECTION_MAX_DISTANCE),
            0.0,
            1.0,
            " px",
        ),
        number_property(
            POINT_MAX_DISTANCE_INPUT_PORT,
            "Max Distance",
            0.0,
            f64::from(POINT_CONNECTION_MAX_DISTANCE),
            80.0,
            1.0,
            " px",
        ),
        PropertyDefinition::new(
            POINT_MAX_NEIGHBORS_INPUT_PORT,
            PropertyUiType::Integer {
                min: 1,
                max: i64::from(POINT_CONNECTION_MAX_NEIGHBORS),
                suffix: String::new(),
                min_hard_limit: true,
                max_hard_limit: true,
            },
            "Max Neighbors",
            PropertyValue::Integer(6),
        ),
    ]
}

fn line_renderer_properties() -> Vec<PropertyDefinition> {
    vec![
        PropertyDefinition::new(
            POINT_COLOR_INPUT_PORT,
            PropertyUiType::Color,
            "Color",
            PropertyValue::Color(Color::white()),
        ),
        number_property(
            POINT_LINE_WIDTH_INPUT_PORT,
            "Width",
            0.0,
            f64::from(POINT_LINE_MAX_WIDTH),
            2.0,
            0.1,
            " px",
        ),
        number_property(POINT_LINE_FADE_INPUT_PORT, "Fade", 0.0, 1.0, 0.0, 0.01, ""),
    ]
}

fn number_property(
    name: &str,
    label: &str,
    min: f64,
    max: f64,
    default: f64,
    step: f64,
    suffix: &str,
) -> PropertyDefinition {
    PropertyDefinition::new(
        name,
        PropertyUiType::Float {
            min,
            max,
            step,
            suffix: suffix.to_string(),
            min_hard_limit: true,
            max_hard_limit: true,
        },
        label,
        PropertyValue::Number(OrderedFloat(default)),
    )
}
