/// The bridge definition for our QObject
use library::render_frame_from_json;
#[cxx_qt::bridge]
pub mod qobject {

  unsafe extern "C++" {
    include!("cxx-qt-lib/qimage.h");
    /// An alias to the QString type
    type QImage = cxx_qt_lib::QImage;
  }

  unsafe extern "RustQt" {
    // The QObject definition
    // We tell CXX-Qt that we want a QObject class with the name MyObject
    // based on the Rust struct MyObjectRust.
    #[qobject]
    #[qml_element]
    #[qproperty(QImage, image)]
    #[namespace = "canvas_image"]
    type CanvasImage = super::CanvasImageRust;
  }

  unsafe extern "RustQt" {
    // Declare the invokable methods we want to expose on the QObject
    #[qinvokable]
    #[cxx_name = "updateImage"]
    unsafe fn update_image(self: Pin<&mut CanvasImage>);
  }
}

use core::pin::Pin;
use cxx_qt_lib::{QImage, QImageFormat};

/// The Rust struct for the QObject
#[derive(Default)]
pub struct CanvasImageRust {
  image: QImage
}

impl qobject::CanvasImage {
  /// Increment the number Q_PROPERTY
  pub unsafe fn update_image(self: Pin<&mut Self>) {
    let image = render_frame_from_json(
      r#"{
  "width": 960,
  "height": 540,
  "background_color": {
    "r": 10,
    "g": 20,
    "b": 30,
    "a": 255
  },
  "color_profile": "sRGB",
  "objects": [
    {
      "type": "Text",
      "text": "Hello, world!",
      "font": "Arial",
      "size": 100,
      "color": {
        "r": 255,
        "g": 255,
        "b": 0,
        "a": 255
      },
      "position": {
        "x": 0,
        "y": 200
      },
      "scale": {
        "x": 1.0,
        "y": 1.0
      },
      "anchor": {
        "x": 0.0,
        "y": 0.0
      },
      "rotation": 0.0
    },
    {
      "type": "Shape",
      "path": "M140 20C73 20 20 74 20 140c0 135 136 170 228 303 88-132 229-173 229-303 0-66-54-120-120-120-48 0-90 28-109 69-19-41-60-69-108-69z",
      "path_effects": [
        {
          "type": "Discrete",
          "seg_length": 10.0,
          "deviation": 10.0,
          "seed": 41413
        },
        {
          "type": "Dash",
          "intervals": [
            40.0,
            20.0
          ],
          "phase": 0.0
        }
      ],
      "styles": [
        {
          "type": "Fill",
          "color": {
            "r": 255,
            "g": 255,
            "b": 255,
            "a": 255
          }
        },
        {
          "type": "Stroke",
          "color": {
            "r": 0,
            "g": 0,
            "b": 255,
            "a": 255
          },
          "width": 20,
          "cap": "Square",
          "join": "Round",
          "miter": 4
        },
        {
          "type": "Stroke",
          "color": {
            "r": 0,
            "g": 255,
            "b": 0,
            "a": 255
          },
          "width": 10,
          "cap": "Square",
          "join": "Round",
          "miter": 4
        }
      ],
      "position": {
        "x": 0,
        "y": 0
      },
      "scale": {
        "x": 1.0,
        "y": 1.0
      },
      "anchor": {
        "x": 0.0,
        "y": 0.0
      },
      "rotation": 0.0
    }
  ]
}"#,
    );
    if image.is_err() {
      return;
    }
    let image = image.unwrap();
    self.set_image(QImage::from_raw_bytes(image.data, image.width as i32, image.height as i32, QImageFormat::Format_RGBA8888));
  }
}
