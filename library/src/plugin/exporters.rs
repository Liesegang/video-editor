use super::{ExportFormat, ExportPlugin, ExportSettings, Plugin, PluginCategory};
use crate::loader::image::Image;
use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use log::{info, warn};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;

pub struct PngExportPlugin;

impl PngExportPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for PngExportPlugin {
    fn id(&self) -> &'static str {
        "png_export_plugin"
    }

    fn category(&self) -> PluginCategory {
        PluginCategory::Export
    }
}

impl ExportPlugin for PngExportPlugin {
    fn supports(&self, format: ExportFormat) -> bool {
        matches!(format, ExportFormat::Png)
    }

    fn export_image(
        &self,
        _format: ExportFormat,
        path: &str,
        image: &Image,
        _settings: &ExportSettings,
    ) -> Result<(), Box<dyn Error>> {
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        let encoder =
            PngEncoder::new_with_quality(writer, CompressionType::Fast, FilterType::NoFilter);
        encoder.write_image(
            &image.data,
            image.width,
            image.height,
            image::ExtendedColorType::Rgba8,
        )?;
        Ok(())
    }
}

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
        "ffmpeg_export_plugin"
    }

    fn category(&self) -> PluginCategory {
        PluginCategory::Export
    }
}

impl ExportPlugin for FfmpegExportPlugin {
    fn supports(&self, format: ExportFormat) -> bool {
        matches!(format, ExportFormat::Video)
    }

    fn export_image(
        &self,
        _format: ExportFormat,
        path: &str,
        image: &Image,
        settings: &ExportSettings,
    ) -> Result<(), Box<dyn Error>> {
        if image.width != settings.width {
            warn!(
                "FFmpeg exporter: frame width {} does not match expected {}; resizing is not supported",
                image.width, settings.width
            );
        }
        if image.height != settings.height {
            warn!(
                "FFmpeg exporter: frame height {} does not match expected {}; resizing is not supported",
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
            Err("Failed to start ffmpeg session".into())
        }
    }
}

struct FfmpegSession {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl FfmpegSession {
    fn spawn(path: &str, settings: &ExportSettings) -> Result<Self, Box<dyn Error>> {
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
            .arg("-")
            .arg("-c:v")
            .arg(&settings.codec);

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
            .ok_or_else(|| "Failed to capture ffmpeg stdin".to_string())?;
        Ok(Self {
            child,
            stdin: Some(stdin),
        })
    }

    fn write_frame(&mut self, data: &[u8]) -> Result<(), Box<dyn Error>> {
        if let Some(stdin) = self.stdin.as_mut() {
            stdin.write_all(data)?;
            stdin.flush()?;
            Ok(())
        } else {
            Err("FFmpeg stdin is closed".into())
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
