// /// The bridge definition for our QObject
// use library::{load_project, render_frame_from_json};
// use serde::{Serialize, Deserialize};

// #[cxx_qt::bridge]
// pub mod qobject {
//   unsafe extern "C++" {
//     include!("cxx-qt-lib/qimage.h");
//     include!("cxx-qt-lib/qstring.h");
//     include!("cxx-qt-lib/qlistitem.h");
//     /// An alias to the QString type
//     type QImage = cxx_qt_lib::QImage;
//     type QString = cxx_qt_lib::QString;
//     type QVector = cxx_qt_lib::QListItem;
//   }

//   unsafe extern "RustQt" {
//     // The QObject definition
//     // We tell CXX-Qt that we want a QObject class with the name MyObject
//     // based on the Rust struct MyObjectRust.
//     #[qobject]
//     #[qml_element]
//     #[qproperty(QString, name)]
//     #[qproperty(u32, height)]
//     #[namespace = "canvas_image"]
//     type TrackInfo = super::TrackInfoRust;

//     #[qobject]
//     #[qml_element]
//     #[qproperty(QString, name)]
//     #[qproperty(f64, start)]
//     #[qproperty(f64, duration)]
//     #[qproperty(QString, color)]
//     #[qproperty(u32, track)]
//     #[namespace = "canvas_image"]
//     type VideoClipInfo = super::VideoClipInfoRust;

//     #[qobject]
//     #[qml_element]
//     #[qproperty(QImage, image)]
//     #[qproperty(QVector<TrackInfo>, tracks)]
//     #[qproperty(QVector<VideoClipInfo>, video_clips)]
//     #[namespace = "canvas_image"]
//     type CanvasImage = super::CanvasImageRust;
//   }

//   unsafe extern "RustQt" {
//     // Declare the invokable methods we want to expose on the QObject
//     #[qinvokable]
//     #[cxx_name = "updateImage"]
//     unsafe fn update_image(self: Pin<&mut CanvasImage>);

//     #[qinvokable]
//     #[cxx_name = "updateProject"]
//     unsafe fn update_project(self: Pin<&mut CanvasImage>);
//   }
// }

// use core::pin::Pin;
// use cxx_qt_lib::{QImage, QImageFormat};

// /// The Rust struct for the QObject
// #[derive(Default)]
// pub struct CanvasImageRust {
//   image: QImage,
//   tracks: Vec<TrackInfo>,
//   video_clips: Vec<VideoClipInfo>,
// }

// #[derive(Serialize, Deserialize, Clone, Default)]
// pub struct TrackInfoRust {
//   pub name: String,
//   pub height: u32,
// }

// #[derive(Serialize, Deserialize, Clone, Default)]
// pub struct VideoClipInfoRust {
//   pub start: f64,
//   pub duration: f64,
//   pub name: String,
//   pub color: String,
//   pub track: u32,
// }

// impl qobject::CanvasImage {
//   /// Increment the number Q_PROPERTY
//   pub unsafe fn update_image(self: Pin<&mut Self>) {
//     let image = render_frame_from_json(
//       r#"{
//   "width": 960,
//   "height": 540,
//   "background_color": {
//     "r": 10,
//     "g": 20,
//     "b": 30,
//     "a": 255
//   },
//   "color_profile": "sRGB",
//   "objects": [
//     {
//       "type": "Text",
//       "text": "Hello, world!",
//       "font": "Arial",
//       "size": 100,
//       "color": {
//         "r": 255,
//         "g": 255,
//         "b": 0,
//         "a": 255
//       },
//       "position": {
//         "x": 0,
//         "y": 200
//       },
//       "scale": {
//         "x": 1.0,
//         "y": 1.0
//       },
//       "anchor": {
//         "x": 0.0,
//         "y": 0.0
//       },
//       "rotation": 0.0
//     }
//   ]
// }"#,
//     );
//     if image.is_err() {
//       return;
//     }
//     let image = image.unwrap();
//     self.set_image(QImage::from_raw_bytes(image.data, image.width as i32, image.height as i32, QImageFormat::Format_RGBA8888));
//   }

//   pub unsafe fn update_project(self: Pin<&mut Self>) {
//     let project = load_project(project_path);
//     if project.is_err() {
//       return;
//     }
//     let project = project.unwrap();

//     // Convert tracks
//     let mut tracks = Vec::new();
//     for (i, track) in project.compositions[0].tracks.iter().enumerate() {
//       tracks.push(TrackInfo {
//         name: track.name.clone(),
//         height: match i {
//           0 => 50,  // ビデオ
//           1 => 40,  // オーバーレイ
//           _ => 35,  // エフェクト
//         },
//       });
//     }
//     self.set_tracks(tracks);

//     // Convert video clips
//     let mut video_clips = Vec::new();
//     for (track_index, track) in project.compositions[0].tracks.iter().enumerate() {
//       for entity in &track.entities {
//         match entity {
//           library::model::project::TrackEntity::Video { time_range, .. } => {
//             video_clips.push(VideoClipInfo {
//               start: time_range.start as f64 / time_range.fps,
//               duration: (time_range.end - time_range.start) as f64 / time_range.fps,
//               name: format!("ビデオ {}", video_clips.len() + 1),
//               color: match track_index {
//                 0 => "#4285F4".to_string(),  // ビデオ
//                 1 => "#34A853".to_string(),  // オーバーレイ
//                 _ => "#1ABC9C".to_string(),  // エフェクト
//               },
//               track: track_index as u32,
//             });
//           }
//           library::model::project::TrackEntity::Image { time_range, .. } => {
//             video_clips.push(VideoClipInfo {
//               start: time_range.start as f64 / time_range.fps,
//               duration: (time_range.end - time_range.start) as f64 / time_range.fps,
//               name: format!("画像 {}", video_clips.len() + 1),
//               color: match track_index {
//                 0 => "#4285F4".to_string(),  // ビデオ
//                 1 => "#34A853".to_string(),  // オーバーレイ
//                 _ => "#1ABC9C".to_string(),  // エフェクト
//               },
//               track: track_index as u32,
//             });
//           }
//         }
//       }
//     }
//     self.set_video_clips(video_clips);
//   }
// }
