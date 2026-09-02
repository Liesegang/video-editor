use super::{EntityConverterPlugin, FrameEvaluationContext};
use crate::model::frame::entity::{FrameBounds, FrameContent, FrameObject, ImageSurface};

#[derive(Default)]
pub struct VideoEntityConverterPlugin;

impl VideoEntityConverterPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl crate::plugin::Plugin for VideoEntityConverterPlugin {
    fn id(&self) -> &'static str {
        "video_entity_converter"
    }

    fn name(&self) -> String {
        "Video Entity Converter".to_string()
    }

    fn category(&self) -> String {
        "Converter".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EntityConverterPlugin for VideoEntityConverterPlugin {
    fn supports_kind(&self, kind: &str) -> bool {
        kind == "video"
    }

    fn get_property_definitions(
        &self,
        _canvas_width: u64,
        _canvas_height: u64,
        _clip_width: u64,
        _clip_height: u64,
    ) -> Vec<crate::model::property::PropertyDefinition> {
        // Source interpretation belongs to the authoritative Asset binding
        // under the Project's exact color configuration. A media Node must not
        // manufacture config-less color-space names for the loader boundary.
        Vec::new()
    }

    fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        node: &crate::model::Node,
        time: f64,
    ) -> Option<FrameObject> {
        let legacy_color = crate::model::active_legacy_media_color_properties(node);
        if !legacy_color.is_empty() {
            log::error!(
                "Media Node {} cannot render because it retains deprecated config-less color authoring ({}). Assign source color on its Asset in the Clip Inspector, then explicitly clear the legacy Node fields",
                node.id,
                legacy_color
                    .iter()
                    .map(|property| format!("{}: {}", property.key(), property.authored_state()))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return None;
        }
        let props = node.properties();
        let media = match node.content() {
            crate::model::NodeContent::Media(media) => Some(media),
            _ => None,
        };
        let asset = media.and_then(|media| {
            evaluator
                .project
                .assets
                .iter()
                .find(|asset| asset.id == media.asset_id)
        });

        // Calculate evaluation time based on Node timeframe
        let eval_time = time;

        let file_path = asset
            .map(|asset| asset.path.clone())
            .or_else(|| evaluator.require_string(props, "file_path", eval_time, "video"))?;
        if !eval_time.is_finite() || eval_time < 0.0 {
            return None;
        }
        if asset.is_some_and(|asset| asset.duration.is_some_and(|duration| eval_time >= duration)) {
            return None;
        }
        let stream_index = media
            .and_then(|media| media.stream_index)
            .or_else(|| asset.and_then(|asset| asset.stream_index));

        let surface = ImageSurface {
            asset_id: asset.map(|asset| asset.id),
            file_path,
            effects: Vec::new(),
            transform: Default::default(),
            input_color_space: None,
            output_color_space: None,
        };

        Some(FrameObject {
            source_node_id: node.id,
            spatial_transform_node_id: None,
            spatial_transform: Box::default(),
            content_bounds: asset.and_then(|asset| match (asset.width, asset.height) {
                (Some(width), Some(height)) => {
                    Some(FrameBounds::new(0.0, 0.0, width as f32, height as f32))
                }
                _ => None,
            }),
            content: FrameContent::Video {
                surface,
                source_time: eval_time,
                stream_index,
            },
        })
    }
}
