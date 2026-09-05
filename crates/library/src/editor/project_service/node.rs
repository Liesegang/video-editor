//! Detached Node and explicit generator graph factories.

use super::lifecycle::ProjectManager;
use crate::error::LibraryError;
use crate::model::frame::color::Color;
use crate::model::project::{
    IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeGraphBundle, PortAddress, PortOwner,
    ProjectConnection, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT,
};
use crate::model::property::{Property, PropertyMap, PropertyValue};
use crate::model::{GeneratorContent, Node};
use crate::plugin::entity_converter::measure_text_size;
use uuid::Uuid;

pub const DEFAULT_TEXT_FONT: &str = "Arial";
pub const DEFAULT_SHAPE_PATH: &str =
    "M 50,30 A 20,20 0,0,1 90,30 C 90,55 50,85 50,85 C 50,85 10,55 10,30 A 20,20 0,0,1 50,30 Z";
pub const DEFAULT_SKSL_SHADER: &str = r#"
half4 main(float2 fragCoord) {
    float2 uv = fragCoord / iResolution.xy;
    float3 col = 0.5 + 0.5*cos(iTime+uv.xyx+float3(0,2,4));
    return half4(col,1.0);
}
"#;

/// Creation-only values for a generator Node. Authored values are materialized
/// into `Node::properties`; `GeneratorContent` stores only the generator kind.
#[derive(Clone, Debug, PartialEq)]
pub enum GeneratorNodeRequest {
    Text { text: String, font: String },
    Shape { path: String },
    Solid { color: Color },
    SkSL { shader: String },
}

/// Creation-only values for a Media Node. Every public authoring path enters
/// through `ProjectManager::create_media_node`; persisted pre-v1 Nodes still
/// deserialize directly without being repaired or rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum MediaNodeRequest {
    Audio {
        asset_id: Uuid,
        file_path: String,
        audio_stream_index: Option<usize>,
    },
    Video {
        asset_id: Uuid,
        file_path: String,
        stream_index: Option<usize>,
        audio_stream_index: Option<usize>,
        outputs: crate::model::MediaOutputSelection,
    },
    Image {
        asset_id: Uuid,
        file_path: String,
    },
}

#[cfg(test)]
pub(crate) fn test_generator_node(name: &str, request: GeneratorNodeRequest) -> Node {
    let manager = ProjectManager::new(
        std::sync::Arc::new(std::sync::RwLock::new(crate::model::project::Project::new(
            "generator test factory",
        ))),
        std::sync::Arc::new(crate::plugin::PluginManager::default()),
    );
    let result = manager.create_generator_node(request, 1920, 1080, 1920, 1080);
    assert!(
        result.is_ok(),
        "built-in Generator converter must create a complete test Node: {result:?}"
    );
    let mut node = result.unwrap_or_else(|_| Node::new_merge("invalid Generator test fallback"));
    node.name = name.to_string();
    node
}

impl ProjectManager {
    /// Builds a detached generator Node from the same converter definitions
    /// used by timeline Clip factories. Every definition is materialized in
    /// the authoritative property map before content-specific values replace
    /// their defaults.
    pub fn create_generator_node(
        &self,
        request: GeneratorNodeRequest,
        canvas_width: u64,
        canvas_height: u64,
        clip_width: u64,
        clip_height: u64,
    ) -> Result<Node, LibraryError> {
        let (name, converter_kind): (&str, &str) = match &request {
            GeneratorNodeRequest::Text { .. } => ("Text", "text"),
            GeneratorNodeRequest::Shape { .. } => ("Shape", "shape"),
            GeneratorNodeRequest::Solid { .. } => ("Solid", "solid"),
            GeneratorNodeRequest::SkSL { .. } => ("SkSL", "sksl"),
        };
        let converter = self
            .plugin_manager
            .get_entity_converter(converter_kind)
            .ok_or_else(|| {
                LibraryError::Plugin(format!(
                    "{name} converter plugin not found ({converter_kind})"
                ))
            })?;
        let definitions = converter.get_property_definitions(
            canvas_width,
            canvas_height,
            clip_width,
            clip_height,
        );
        let mut properties = PropertyMap::from_definitions(&definitions);

        let content = match &request {
            GeneratorNodeRequest::Text { text, font } => {
                properties.set(
                    "text".to_string(),
                    Property::constant(PropertyValue::String(text.clone())),
                );
                properties.set(
                    "font_family".to_string(),
                    Property::constant(PropertyValue::String(font.clone())),
                );
                GeneratorContent::Text
            }
            GeneratorNodeRequest::Shape { path } => {
                let path =
                    crate::model::path::parse_legacy_svg_path_data(path).map_err(|error| {
                        LibraryError::Validation(format!("Invalid Shape SVG path: {error}"))
                    })?;
                properties.set(
                    "path".to_string(),
                    Property::constant(PropertyValue::Path(path)),
                );
                GeneratorContent::Shape
            }
            GeneratorNodeRequest::Solid { color } => {
                properties.set(
                    "color".to_string(),
                    Property::constant(PropertyValue::ColorValue(
                        crate::model::property::ColorValue::from_straight_srgba8(color),
                    )),
                );
                GeneratorContent::Solid
            }
            GeneratorNodeRequest::SkSL { shader } => {
                properties.set(
                    "shader".to_string(),
                    Property::constant(PropertyValue::String(shader.clone())),
                );
                GeneratorContent::SkSL
            }
        };

        Node::new_generator(name, content, &definitions, properties)
            .map_err(LibraryError::Validation)
    }

    /// Builds a detached Media Node with the converter defaults and source
    /// identity materialized atomically. Callers cannot construct a half-
    /// initialized Media Node and fill its property map later.
    pub fn create_media_node(
        &self,
        name: &str,
        request: MediaNodeRequest,
        canvas_width: u64,
        canvas_height: u64,
        media_width: u64,
        media_height: u64,
    ) -> Result<Node, LibraryError> {
        crate::editor::AuthoringNodeFactory::create_media(
            self.plugin_manager.as_ref(),
            name,
            request,
            canvas_width,
            canvas_height,
            media_width,
            media_height,
        )
    }

    fn create_positioned_transform_node(
        &self,
        position: [f64; 2],
        anchor: [f64; 2],
    ) -> Result<Node, LibraryError> {
        let mut node = self
            .plugin_manager
            .create_shape_transform_operation_node()?;
        for (key, value) in [
            (
                "position",
                crate::plugin::transforms::vec2_value(position[0], position[1]),
            ),
            (
                "anchor",
                crate::plugin::transforms::vec2_value(anchor[0], anchor[1]),
            ),
        ] {
            node.set_property(key.to_string(), Property::constant(value))
                .map_err(LibraryError::Validation)?;
        }
        Ok(node)
    }

    /// Builds a detached Text -> Transform -> Fill graph. Text produces only
    /// grouped Shape metadata, Transform owns absolute placement, and Fill is
    /// the explicit Shape -> Image boundary and therefore the graph output.
    pub fn create_text_graph(
        &self,
        text: &str,
        font: &str,
        canvas_width: u64,
        canvas_height: u64,
    ) -> Result<NodeGraphBundle, LibraryError> {
        let (text_width, text_height) = measure_text_size(text, font, 100.0);
        let mut text_node = self.create_generator_node(
            GeneratorNodeRequest::Text {
                text: text.to_string(),
                font: font.to_string(),
            },
            canvas_width,
            canvas_height,
            text_width as u64,
            text_height as u64,
        )?;
        let mut transform_node = self.create_positioned_transform_node(
            [canvas_width as f64 / 2.0, canvas_height as f64 / 2.0],
            [
                f64::from(text_width.trunc()) / 2.0,
                f64::from(text_height.trunc()) / 2.0,
            ],
        )?;
        let mut fill_node = self.plugin_manager.create_style_operation_node("fill")?;
        text_node.ui_position = [0.0, 0.0];
        transform_node.ui_position = [320.0, 0.0];
        fill_node.ui_position = [640.0, 0.0];
        let output_node_id = fill_node.id;
        let connections = vec![
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(text_node.id), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(transform_node.id), SHAPE_INPUT_PORT),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(transform_node.id), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(fill_node.id), SHAPE_INPUT_PORT),
                0,
            ),
        ];
        Ok(NodeGraphBundle::new(
            vec![text_node, transform_node, fill_node],
            connections,
            Some(output_node_id),
        ))
    }

    /// Builds Shape -> Transform, fans the placed Shape into Fill and Stroke,
    /// then explicitly merges both Image branches. ProjectConnection order on
    /// Merge is the raster layer authority; storage/UI order is irrelevant.
    pub fn create_shape_graph(
        &self,
        path: &str,
        canvas_width: u64,
        canvas_height: u64,
        shape_width: u64,
        shape_height: u64,
    ) -> Result<NodeGraphBundle, LibraryError> {
        let mut shape_node = self.create_generator_node(
            GeneratorNodeRequest::Shape {
                path: path.to_string(),
            },
            canvas_width,
            canvas_height,
            shape_width,
            shape_height,
        )?;
        let mut transform_node = self.create_positioned_transform_node(
            [canvas_width as f64 / 2.0, canvas_height as f64 / 2.0],
            [shape_width as f64 / 2.0, shape_height as f64 / 2.0],
        )?;
        let mut fill_node = self.plugin_manager.create_style_operation_node("fill")?;
        let mut stroke_node = self.plugin_manager.create_style_operation_node("stroke")?;
        let mut merge_node = Node::new_merge("Merge");
        shape_node.ui_position = [0.0, 110.0];
        transform_node.ui_position = [320.0, 110.0];
        fill_node.ui_position = [640.0, 0.0];
        stroke_node.ui_position = [640.0, 220.0];
        merge_node.ui_position = [960.0, 110.0];
        let output_node_id = merge_node.id;
        let connections = vec![
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(shape_node.id), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(transform_node.id), SHAPE_INPUT_PORT),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(transform_node.id), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(fill_node.id), SHAPE_INPUT_PORT),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(transform_node.id), SHAPE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(stroke_node.id), SHAPE_INPUT_PORT),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(fill_node.id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge_node.id), MERGE_IMAGES_PORT),
                0,
            ),
            ProjectConnection::new(
                PortAddress::new(PortOwner::Node(stroke_node.id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(merge_node.id), MERGE_IMAGES_PORT),
                1,
            ),
        ];
        Ok(NodeGraphBundle::new(
            vec![
                shape_node,
                transform_node,
                fill_node,
                stroke_node,
                merge_node,
            ],
            connections,
            Some(output_node_id),
        ))
    }

    pub fn create_text_node(
        &self,
        text: &str,
        font: &str,
        canvas_width: u64,
        canvas_height: u64,
    ) -> Result<Node, LibraryError> {
        let (text_width, text_height) = measure_text_size(text, font, 100.0);
        self.create_generator_node(
            GeneratorNodeRequest::Text {
                text: text.to_string(),
                font: font.to_string(),
            },
            canvas_width,
            canvas_height,
            text_width as u64,
            text_height as u64,
        )
    }

    pub fn create_shape_node(
        &self,
        path: &str,
        canvas_width: u64,
        canvas_height: u64,
        shape_width: u64,
        shape_height: u64,
    ) -> Result<Node, LibraryError> {
        self.create_generator_node(
            GeneratorNodeRequest::Shape {
                path: path.to_string(),
            },
            canvas_width,
            canvas_height,
            shape_width,
            shape_height,
        )
    }

    pub fn create_sksl_node(
        &self,
        shader: &str,
        canvas_width: u64,
        canvas_height: u64,
    ) -> Result<Node, LibraryError> {
        self.create_generator_node(
            GeneratorNodeRequest::SkSL {
                shader: shader.to_string(),
            },
            canvas_width,
            canvas_height,
            canvas_width,
            canvas_height,
        )
    }

    pub fn create_solid_node(
        &self,
        color: Color,
        canvas_width: u64,
        canvas_height: u64,
    ) -> Result<Node, LibraryError> {
        self.create_generator_node(
            GeneratorNodeRequest::Solid { color },
            canvas_width,
            canvas_height,
            canvas_width,
            canvas_height,
        )
    }
}
