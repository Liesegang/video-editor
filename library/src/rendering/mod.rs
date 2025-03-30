pub mod renderer;
pub mod skia_renderer;

use crate::loader::image::load_image;
use crate::loader::image::Image;
use crate::loader::video;
use crate::rendering::renderer::Renderer;
use std::error::Error;
use crate::model::frame::draw_type::DrawStyle;
use crate::model::frame::entity::FrameEntity;
use crate::model::frame::frame::FrameInfo;

pub fn render_frame<T: Renderer>(frame_info: FrameInfo, renderer: &mut T) -> Result<Image, Box<dyn Error>> {
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
