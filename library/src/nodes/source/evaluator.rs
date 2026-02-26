//! Evaluator for Source nodes (Video, Image, Text, Shape, SkSL).

use uuid::Uuid;

use super::text as text_decompose;
use crate::builtin::loaders::LoadRequest;
use crate::error::LibraryError;
use crate::pipeline::context::EvalContext;
use crate::pipeline::evaluator::NodeEvaluator;
use crate::pipeline::output::{PinValue, ShapeData};
use crate::project::node::Node;
use crate::project::source::SourceKind;
use crate::rendering::renderer::{RenderOutput, Renderer};
use crate::runtime::transform::Transform;

pub struct SourceEvaluator;

impl NodeEvaluator for SourceEvaluator {
    fn handles(&self) -> &[&str] {
        &["clip."]
    }

    fn evaluate(
        &self,
        node_id: Uuid,
        pin_name: &str,
        ctx: &mut EvalContext,
    ) -> Result<PinValue, LibraryError> {
        let source = match ctx.project.get_node(node_id) {
            Some(Node::Source(s)) => s.clone(),
            _ => return Ok(PinValue::None),
        };

        // Timing check: in pull-based mode the engine may not pre-filter sources
        if ctx.frame_number < source.in_frame || ctx.frame_number > source.out_frame {
            return Ok(PinValue::None);
        }

        let eval_time = ctx.clip_eval_time(&source);
        let identity = Transform::default();

        match (&source.kind, pin_name) {
            // Text/Shape sources produce shape data (deferred rasterization)
            (SourceKind::Text, "shape_out") => self.text_shape(&source, ctx),
            (SourceKind::Shape, "shape_out") => self.path_shape(&source, ctx),
            // Video/Image/SkSL sources produce images directly
            (SourceKind::Image, "image_out") => self.evaluate_image(&source.properties, ctx),
            (SourceKind::Video, "image_out") => self.evaluate_video(&source, ctx),
            (SourceKind::SkSL, "image_out") => {
                self.evaluate_sksl(&source.properties, eval_time, &identity, ctx)
            }
            _ => Ok(PinValue::None),
        }
    }
}

impl SourceEvaluator {
    /// Text source: decompose text into per-character glyph outlines as ShapeData::Grouped.
    fn text_shape(
        &self,
        source: &crate::project::source::SourceData,
        ctx: &mut EvalContext,
    ) -> Result<PinValue, LibraryError> {
        let text = ctx.resolve_string(&source.properties, "text", "");
        if text.is_empty() {
            return Ok(PinValue::None);
        }
        let font = ctx.resolve_string(&source.properties, "font_family", "Arial");
        let size = ctx.resolve_number(&source.properties, "size", 12.0);

        let shape_data = text_decompose::decompose_text_to_shapes(&text, &font, size);
        Ok(PinValue::Shape(shape_data))
    }

    /// Shape source: produce ShapeData::Path for deferred rasterization.
    fn path_shape(
        &self,
        source: &crate::project::source::SourceData,
        ctx: &mut EvalContext,
    ) -> Result<PinValue, LibraryError> {
        let path = ctx.resolve_string(&source.properties, "path", "");
        if path.is_empty() {
            return Ok(PinValue::None);
        }
        Ok(PinValue::Shape(ShapeData::Path {
            path_data: path,
            path_effects: vec![],
        }))
    }

    fn evaluate_image(
        &self,
        properties: &crate::project::property::PropertyMap,
        ctx: &mut EvalContext,
    ) -> Result<PinValue, LibraryError> {
        let file_path = ctx.resolve_string(properties, "file_path", "");
        if file_path.is_empty() {
            return Ok(PinValue::None);
        }

        let request = LoadRequest::Image { path: file_path };
        let response = ctx
            .plugin_manager
            .load_resource(&request, ctx.cache_manager)?;
        Ok(PinValue::Image(RenderOutput::Image(response.image)))
    }

    fn evaluate_video(
        &self,
        source: &crate::project::source::SourceData,
        ctx: &mut EvalContext,
    ) -> Result<PinValue, LibraryError> {
        let file_path = ctx.resolve_string(&source.properties, "file_path", "");
        if file_path.is_empty() {
            return Ok(PinValue::None);
        }

        let time_offset = {
            let delta_frames = ctx.frame_number as f64 - source.in_frame as f64;
            delta_frames / ctx.composition.fps
        };
        let source_delta_frames = time_offset * source.fps;
        let source_frame_number = source.source_begin_frame + source_delta_frames.round() as i64;
        if source_frame_number < 0 {
            return Ok(PinValue::None);
        }

        let input_color_space = ctx.resolve_string(&source.properties, "input_color_space", "");
        let output_color_space = ctx.resolve_string(&source.properties, "output_color_space", "");

        let request = LoadRequest::VideoFrame {
            path: file_path,
            frame_number: source_frame_number as u64,
            stream_index: None,
            input_color_space: if input_color_space.is_empty() {
                None
            } else {
                Some(input_color_space)
            },
            output_color_space: if output_color_space.is_empty() {
                None
            } else {
                Some(output_color_space)
            },
        };
        let response = ctx
            .plugin_manager
            .load_resource(&request, ctx.cache_manager)?;
        Ok(PinValue::Image(RenderOutput::Image(response.image)))
    }

    fn evaluate_sksl(
        &self,
        properties: &crate::project::property::PropertyMap,
        eval_time: f64,
        transform: &Transform,
        ctx: &mut EvalContext,
    ) -> Result<PinValue, LibraryError> {
        let shader = ctx.resolve_string(properties, "shader", "");
        if shader.is_empty() {
            return Ok(PinValue::None);
        }

        let width = ctx.resolve_number(properties, "width", ctx.composition.width as f64);
        let height = ctx.resolve_number(properties, "height", ctx.composition.height as f64);
        let resolution = (width as f32, height as f32);

        let output =
            ctx.renderer
                .rasterize_sksl_layer(&shader, resolution, eval_time as f32, transform)?;
        Ok(PinValue::Image(output))
    }
}
