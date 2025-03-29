pub mod renderer;
mod skia_renderer;

use crate::loader::image::load_image;
use crate::loader::image::Image;
use crate::loader::video;
use crate::rendering::renderer::Renderer;
use crate::rendering::skia_renderer::SkiaRenderer;
use std::error::Error;
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::entity::FrameEntity;
use crate::model::frame::frame::FrameInfo;

pub fn render_frame(frame_info: FrameInfo) -> Result<Image, Box<dyn Error>> {
  let mut renderer: Box<dyn Renderer> = Box::new(SkiaRenderer::new(
    frame_info.width as u32,
    frame_info.height as u32,
    frame_info.background_color,
  ));

  // Process each SceneObject
  for object in frame_info.objects {
    match object {
      FrameEntity::Video {
        file_path,
        frame_number,
        transform,
      } => {
        let video_frame = video::decode_video_frame(&file_path, frame_number)?;
        renderer.draw_image(&video_frame, &transform)?;
      }
      FrameEntity::Image {
        file_path,
        transform,
      } => {
        let image_frame = load_image(&file_path)?;
        renderer.draw_image(&image_frame, &transform)?;
      }
      FrameEntity::Text {
        text,
        font,
        size,
        color,
        transform,
      } => {
        renderer.draw_text(&text, size, &font, &color, &transform)?;
      }
      FrameEntity::Shape {
        path,
        styles,
        path_effects,
        transform,
      } => {
        for style in styles {
          match style {
            DrawStyle::Fill { color } => {
              renderer.draw_shape_fill(&path, &color, &path_effects, &transform)?;
            }
            DrawStyle::Stroke {
              color,
              width,
              cap,
              join,
              miter,
            } => {
              renderer.draw_shape_stroke(
                &path, &color, &path_effects, width, cap, join, miter, &transform,
              )?;
            }
          }
        }
      }
    }
  }
  renderer.finalize()
}
