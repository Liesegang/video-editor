use super::super::{ExportPlugin, ExportSettings, Plugin};
use crate::error::LibraryError;
use crate::model::frame::Image;
use log::{info, warn};
use std::collections::HashMap;
use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
// use skia_safe::M44; // Removed, as it's not directly used here

pub struct FfmpegExportPlugin {
    sessions: Mutex<HashMap<String, FfmpegSession>>,
}

impl FfmpegExportPlugin {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }
}

impl Plugin for FfmpegExportPlugin {
    fn id(&self) -> &'static str {
        "ffmpeg_export"
    }

    fn name(&self) -> String {
        "FFmpeg Export".to_string()
    }

    fn category(&self) -> String {
        "Export".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl ExportPlugin for FfmpegExportPlugin {
    fn export_image(
        &self,
        path: &str,
        image: &Image,
        settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
        // ... (check dimensions) ...
        if image.width != settings.width {
            warn!(
                "FFmpeg exporter: frame width {} does not match {}; resizing not supported",
                image.width, settings.width
            );
        }
        if image.height != settings.height {
            warn!(
                "FFmpeg exporter: frame height {} does not match {}; resizing not supported",
                image.height, settings.height
            );
        }

        let mut sessions = self.sessions.lock().unwrap();
        if !sessions.contains_key(path) {
            info!(
                "Starting ffmpeg export session: codec={} container={} pixel_format={}",
                settings.codec, settings.container, settings.pixel_format
            );
            let session = FfmpegSession::spawn(path, settings)?;
            sessions.insert(path.to_string(), session);
        }
        if let Some(session) = sessions.get_mut(path) {
            session.write_frame(&image.data)
        } else {
            Err(LibraryError::render(
                "Failed to start ffmpeg session".to_string(),
            ))
        }
    }

    fn finish_export(&self, path: &str) -> Result<(), LibraryError> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(_session) = sessions.remove(path) {
            info!("Finishing ffmpeg export session for {}", path);
            // session is dropped here, which closes stdin and waits for child
            Ok(())
        } else {
            // It's possible it was never started or already finished
            Ok(())
        }
    }

    fn properties(&self) -> Vec<crate::model::project::property::PropertyDefinition> {
        use crate::model::project::property::{PropertyDefinition, PropertyUiType, PropertyValue};
        use ordered_float::OrderedFloat;
        vec![
            PropertyDefinition::new(
                "container",
                PropertyUiType::Dropdown {
                    options: vec![
                        "mp4".to_string(),
                        "mkv".to_string(),
                        "avi".to_string(),
                        "mov".to_string(),
                    ],
                },
                "Container",
                PropertyValue::String("mp4".to_string()),
            ),
            PropertyDefinition::new(
                "codec",
                PropertyUiType::Dropdown {
                    options: vec![
                        "libx264".to_string(),
                        "libx265".to_string(),
                        "mpeg4".to_string(),
                        "prores_ks".to_string(),
                    ],
                },
                "Video Codec",
                PropertyValue::String("libx264".to_string()),
            ),
            PropertyDefinition::new(
                "pixel_format",
                PropertyUiType::Dropdown {
                    options: vec![
                        "yuv420p".to_string(),
                        "yuv444p".to_string(),
                        "rgb24".to_string(),
                        "rgba".to_string(),
                    ],
                },
                "Pixel Format",
                PropertyValue::String("yuv420p".to_string()),
            ),
            PropertyDefinition::new(
                "bitrate",
                PropertyUiType::Integer {
                    min: 0,
                    max: 100000,
                    suffix: " kbps".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Bitrate (kbps)",
                PropertyValue::Number(OrderedFloat(5000.0)),
            ),
            PropertyDefinition::new(
                "crf",
                PropertyUiType::Integer {
                    min: 0,
                    max: 51,
                    suffix: "".to_string(),
                    min_hard_limit: true,
                    max_hard_limit: true,
                },
                "CRF (Quality, 0-51)",
                PropertyValue::Number(OrderedFloat(23.0)),
            ),
            PropertyDefinition::new(
                "preset",
                PropertyUiType::Dropdown {
                    options: vec![
                        "ultrafast".to_string(),
                        "superfast".to_string(),
                        "veryfast".to_string(),
                        "faster".to_string(),
                        "fast".to_string(),
                        "medium".to_string(),
                        "slow".to_string(),
                        "slower".to_string(),
                        "veryslow".to_string(),
                    ],
                },
                "Preset",
                PropertyValue::String("medium".to_string()),
            ),
            PropertyDefinition::new(
                "audio_bitrate",
                PropertyUiType::Integer {
                    min: 64,
                    max: 320,
                    suffix: " kbps".to_string(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Audio Bitrate (kbps)",
                PropertyValue::Number(OrderedFloat(192.0)),
            ),
        ]
    }
}

struct FfmpegSession {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl FfmpegSession {
    fn spawn(path: &str, settings: &ExportSettings) -> Result<Self, LibraryError> {
        let binary = settings
            .ffmpeg_path
            .as_deref()
            .unwrap_or("ffmpeg")
            .to_string();
        let mut cmd = Command::new(binary);
        cmd.arg("-y")
            .arg("-f")
            .arg("rawvideo")
            .arg("-pix_fmt")
            .arg("rgba")
            .arg("-s")
            .arg(format!("{}x{}", settings.width, settings.height))
            .arg("-r")
            .arg(format!("{}", settings.fps))
            .arg("-i")
            .arg("-");

        // Audio Input
        let mut has_audio = false;
        if let Some(audio_path) = settings.parameter_string("audio_source") {
            let channels = settings.parameter_u64("audio_channels").unwrap_or(2);
            let rate = settings.parameter_u64("audio_sample_rate").unwrap_or(48000);

            cmd.arg("-f")
                .arg("f32le")
                .arg("-ar")
                .arg(format!("{}", rate))
                .arg("-ac")
                .arg(format!("{}", channels))
                .arg("-i")
                .arg(audio_path);
            has_audio = true;
        }

        cmd.arg("-c:v").arg(&settings.codec);

        if let Some(bitrate) = settings.parameter_u64("bitrate") {
            cmd.arg("-b:v").arg(format!("{}k", bitrate));
        }

        if let Some(crf) = settings.parameter_f64("crf") {
            cmd.arg("-crf").arg(format!("{}", crf));
        } else if let Some(quality) = settings.parameter_f64("quality") {
            cmd.arg("-crf").arg(format!("{}", quality));
        }

        if let Some(preset) = settings.parameter_string("preset") {
            cmd.arg("-preset").arg(preset);
        }

        if let Some(profile) = settings.parameter_string("profile") {
            cmd.arg("-profile:v").arg(profile);
        }

        if has_audio {
            let audio_bitrate = settings.parameter_u64("audio_bitrate").unwrap_or(192);
            cmd.arg("-c:a")
                .arg("aac")
                .arg("-b:a")
                .arg(format!("{}k", audio_bitrate))
                .arg("-map")
                .arg("0:v")
                .arg("-map")
                .arg("1:a");
        }

        cmd.arg("-pix_fmt")
            .arg(&settings.pixel_format)
            .arg("-f")
            .arg(&settings.container)
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());

        let mut child = cmd.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LibraryError::render("Failed to capture ffmpeg stdin".to_string()))?;
        Ok(Self {
            child,
            stdin: Some(stdin),
        })
    }

    fn write_frame(&mut self, data: &[u8]) -> Result<(), LibraryError> {
        if let Some(stdin) = self.stdin.as_mut() {
            stdin.write_all(data)?;
            stdin.flush()?;
            Ok(())
        } else {
            Err(LibraryError::render("FFmpeg stdin is closed".to_string()))
        }
    }
}

impl Drop for FfmpegSession {
    fn drop(&mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.flush();
        }
        let _ = self.child.wait();
    }
}
