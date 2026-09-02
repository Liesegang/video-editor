use super::super::{ExportFrame, ExportPlugin, ExportSettings, Plugin};
use super::ffmpeg_command::{FfmpegCommand, FfmpegDeliveryPolicy};
use super::ffmpeg_destination;
use crate::error::LibraryError;
use log::{info, warn};
use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
// use skia_safe::M44; // Removed, as it's not directly used here

#[derive(Default)]
pub struct FfmpegExportPlugin {
    sessions: Mutex<Vec<FfmpegSession>>,
}

impl FfmpegExportPlugin {
    pub fn new() -> Self {
        Self::default()
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
    fn export_frame(
        &self,
        path: &str,
        frame: &ExportFrame,
        settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
        let authority = settings.require_matching_color_authority(frame)?;
        let policy = FfmpegDeliveryPolicy::for_settings(settings, authority)?;
        let image = frame.image();
        if let Some(pixel_index) = image
            .data
            .chunks_exact(4)
            .position(|pixel| pixel[3] != u8::MAX)
        {
            return Err(LibraryError::Render(format!(
                "FFmpeg 8-bit video delivery has no alpha channel, but pixel {pixel_index} has alpha {}; composite a matte in the scene-linear render pipeline before export",
                image.data[pixel_index * 4 + 3]
            )));
        }
        let command = FfmpegCommand::build(path, settings, policy)?;
        let destination = ffmpeg_destination::identity(path)?;
        if image.width != settings.width {
            return Err(LibraryError::Render(format!(
                "FFmpeg exporter frame width {} does not match {}; implicit resizing is forbidden",
                image.width, settings.width
            )));
        }
        if image.height != settings.height {
            return Err(LibraryError::Render(format!(
                "FFmpeg exporter frame height {} does not match {}; implicit resizing is forbidden",
                image.height, settings.height
            )));
        }

        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| LibraryError::Runtime("FFmpeg session lock poisoned".to_string()))?;
        for session in sessions.iter_mut() {
            session.destination.refresh_existing_file();
        }
        if let Some(session) = sessions
            .iter_mut()
            .find(|session| session.destination.aliases(&destination))
        {
            if session.job_id != settings.job_id() {
                return Err(LibraryError::Render(format!(
                    "FFmpeg destination '{path}' is busy in export job {}; job {} must wait until it is finalized",
                    session.job_id,
                    settings.job_id()
                )));
            }
            if session.command != command {
                return Err(LibraryError::Render(format!(
                    "FFmpeg export settings changed while session '{path}' was active"
                )));
            }
            session.write_frame(&image.data)
        } else {
            info!(
                "Starting ffmpeg export session: codec={} container={} pixel_format={}",
                settings.codec, settings.container, settings.pixel_format
            );
            let mut session = FfmpegSession::spawn(command, settings.job_id(), destination)?;
            session.write_frame(&image.data)?;
            sessions.push(session);
            Ok(())
        }
    }

    fn finish_export(&self, path: &str, settings: &ExportSettings) -> Result<(), LibraryError> {
        let destination = ffmpeg_destination::identity(path)?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| LibraryError::Runtime("FFmpeg session lock poisoned".to_string()))?;
        for session in sessions.iter_mut() {
            session.destination.refresh_existing_file();
        }
        let Some(index) = sessions
            .iter()
            .position(|session| session.destination.aliases(&destination))
        else {
            return Ok(());
        };
        let session = &sessions[index];
        if session.job_id != settings.job_id() {
            return Err(LibraryError::Render(format!(
                "export job {} cannot finalize FFmpeg destination '{path}' owned by job {}",
                settings.job_id(),
                session.job_id
            )));
        }
        let session = sessions.remove(index);
        info!("Finishing ffmpeg export session for {path}");
        session.finish()
    }

    fn properties(&self) -> Vec<crate::model::property::PropertyDefinition> {
        use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};
        vec![
            PropertyDefinition::new(
                "container",
                PropertyUiType::Dropdown {
                    options: vec!["mp4".to_string(), "mkv".to_string()],
                },
                "Container",
                PropertyValue::String("mp4".to_string()),
            ),
            PropertyDefinition::new(
                "codec",
                PropertyUiType::Dropdown {
                    options: vec!["libx264".to_string(), "ffv1".to_string()],
                },
                "Video Codec",
                PropertyValue::String("libx264".to_string()),
            ),
            PropertyDefinition::new(
                "pixel_format",
                PropertyUiType::Dropdown {
                    options: vec![
                        "yuv420p".to_string(),
                        "yuv422p".to_string(),
                        "yuv444p".to_string(),
                        "bgr0".to_string(),
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
                PropertyValue::Integer(5000),
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
                PropertyValue::Integer(23),
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
                PropertyValue::Integer(192),
            ),
        ]
    }
}

struct FfmpegSession {
    command: FfmpegCommand,
    job_id: crate::plugin::ExportJobId,
    destination: ffmpeg_destination::DestinationIdentity,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
}

impl FfmpegSession {
    fn spawn(
        command: FfmpegCommand,
        job_id: crate::plugin::ExportJobId,
        destination: ffmpeg_destination::DestinationIdentity,
    ) -> Result<Self, LibraryError> {
        let mut cmd = Command::new(&command.binary);
        cmd.args(&command.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());

        let mut child = cmd.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LibraryError::Render("Failed to capture ffmpeg stdin".to_string()))?;
        Ok(Self {
            command,
            job_id,
            destination,
            child: Some(child),
            stdin: Some(stdin),
        })
    }

    fn finish(mut self) -> Result<(), LibraryError> {
        self.stdin.take();
        let status = self
            .child
            .take()
            .ok_or_else(|| LibraryError::Render("FFmpeg child is already closed".to_string()))?
            .wait()?;
        if !status.success() {
            return Err(LibraryError::Render(format!(
                "FFmpeg export process exited with {status}"
            )));
        }
        Ok(())
    }

    fn write_frame(&mut self, data: &[u8]) -> Result<(), LibraryError> {
        if let Some(stdin) = self.stdin.as_mut() {
            stdin.write_all(data)?;
            stdin.flush()?;
            Ok(())
        } else {
            Err(LibraryError::Render("FFmpeg stdin is closed".to_string()))
        }
    }
}

impl Drop for FfmpegSession {
    fn drop(&mut self) {
        if let Some(mut stdin) = self.stdin.take()
            && let Err(error) = stdin.flush()
        {
            warn!("failed to flush FFmpeg stdin during shutdown: {error}");
        }
        if let Some(mut child) = self.child.take()
            && let Err(error) = child.wait()
        {
            warn!("failed to wait for FFmpeg during shutdown: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::authoring::AuthoringProject;
    use crate::model::frame::Image;
    use std::fs;
    use std::process::Output;
    use uuid::Uuid;

    struct TestOutput(std::path::PathBuf);

    impl TestOutput {
        fn new(extension: &str) -> Self {
            Self(std::env::temp_dir().join(format!(
                "ruvie-export-color-{}.{}",
                Uuid::new_v4(),
                extension
            )))
        }
    }

    impl Drop for TestOutput {
        fn drop(&mut self) {
            if let Err(error) = fs::remove_file(&self.0)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!("failed to remove FFmpeg test output: {error}");
            }
        }
    }

    fn tools_available() -> bool {
        let available = Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
            && Command::new("ffprobe")
                .arg("-version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
        let required =
            std::env::var_os("RUVIE_REQUIRE_FFMPEG_E2E").is_some_and(|value| value == "1");
        assert!(
            available || !required,
            "RUVIE_REQUIRE_FFMPEG_E2E=1 but ffmpeg/ffprobe are unavailable"
        );
        available
    }

    fn frame_and_settings(pixel_format: &str) -> (ExportFrame, ExportSettings) {
        let project = AuthoringProject::new("FFmpeg color metadata", 4, 4, 24.0, 1.0).unwrap();
        let pixels = [
            [24, 64, 112, 255],
            [48, 96, 144, 255],
            [72, 128, 176, 255],
            [96, 160, 208, 255],
        ]
        .into_iter()
        .cycle()
        .take(16)
        .flatten()
        .collect();
        let frame = ExportFrame::from_project_render(&project, Image::new(4, 4, pixels)).unwrap();
        let mut settings = ExportSettings::for_dimensions(4, 4, 24.0);
        settings.bind_project_color_authority(&project).unwrap();
        settings.pixel_format = pixel_format.to_string();
        (frame, settings)
    }

    fn run_probe(path: &std::path::Path) -> Output {
        Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=pix_fmt,color_range,color_space,color_transfer,color_primaries",
                "-of",
                "default=nw=1",
            ])
            .arg(path)
            .output()
            .unwrap()
    }

    #[test]
    fn real_rgb_export_preserves_exact_srgb_signal_metadata() {
        if !tools_available() {
            eprintln!("skipping real FFmpeg metadata test: ffmpeg/ffprobe unavailable");
            return;
        }
        let (frame, mut settings) = frame_and_settings("bgr0");
        settings.codec = "ffv1".to_string();
        settings.container = "matroska".to_string();
        let output = TestOutput::new("mkv");
        let plugin = FfmpegExportPlugin::new();

        plugin
            .export_frame(output.0.to_str().unwrap(), &frame, &settings)
            .unwrap();
        plugin
            .finish_export(output.0.to_str().unwrap(), &settings)
            .unwrap();

        let probe = run_probe(&output.0);
        assert!(
            probe.status.success(),
            "{}",
            String::from_utf8_lossy(&probe.stderr)
        );
        let metadata = String::from_utf8(probe.stdout).unwrap();
        assert!(metadata.contains("pix_fmt=bgr0"), "{metadata}");
        assert!(metadata.contains("color_range=pc"), "{metadata}");
        assert!(metadata.contains("color_space=gbr"), "{metadata}");
        assert!(
            metadata.contains("color_transfer=iec61966-2-1"),
            "{metadata}"
        );
        assert!(metadata.contains("color_primaries=bt709"), "{metadata}");
    }

    #[test]
    fn incompatible_rgb_codec_is_rejected_before_ffmpeg_can_fallback_to_yuv() {
        let (frame, mut settings) = frame_and_settings("rgba");
        settings.codec = "libx264".to_string();
        settings.container = "mp4".to_string();
        let output = TestOutput::new("mp4");

        let error = FfmpegExportPlugin::new()
            .export_frame(output.0.to_str().unwrap(), &frame, &settings)
            .unwrap_err();
        assert!(error.to_string().contains("no verified"));
        assert!(error.to_string().contains("libx264/mp4/rgba"));
        assert!(!output.0.exists());
    }

    #[test]
    fn concurrent_job_cannot_write_or_finalize_an_owned_destination() {
        if !tools_available() {
            eprintln!("skipping real FFmpeg session isolation test: ffmpeg unavailable");
            return;
        }
        let (frame, mut owner_settings) = frame_and_settings("yuv420p");
        owner_settings.codec = "libx264".to_string();
        owner_settings.container = "mp4".to_string();
        let mut contender_settings = owner_settings.clone();
        contender_settings.begin_new_job();
        assert_ne!(owner_settings.job_id(), contender_settings.job_id());
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("shared.mp4");
        let alias = directory.path().join("hard-link-alias.mp4");
        fs::write(&output, b"pre-existing user output").unwrap();
        fs::hard_link(&output, &alias).unwrap();
        let owner_path = output.to_str().unwrap();
        let contender_path = alias.to_str().unwrap();
        let plugin = FfmpegExportPlugin::new();

        plugin
            .export_frame(owner_path, &frame, &owner_settings)
            .unwrap();
        let write_error = plugin
            .export_frame(contender_path, &frame, &contender_settings)
            .unwrap_err();
        assert!(write_error.to_string().contains("is busy in export job"));

        let finish_error = plugin
            .finish_export(contender_path, &contender_settings)
            .unwrap_err();
        assert!(finish_error.to_string().contains("cannot finalize"));

        plugin
            .export_frame(owner_path, &frame, &owner_settings)
            .unwrap();
        plugin.finish_export(owner_path, &owner_settings).unwrap();
        assert!(output.exists());
    }

    #[test]
    fn mismatched_dimensions_are_rejected_before_starting_ffmpeg() {
        let (frame, mut settings) = frame_and_settings("yuv420p");
        settings.width = 8;
        settings.codec = "libx264".to_string();
        settings.container = "mp4".to_string();
        let output = TestOutput::new("mp4");

        let error = FfmpegExportPlugin::new()
            .export_frame(output.0.to_str().unwrap(), &frame, &settings)
            .unwrap_err();
        assert!(error.to_string().contains("implicit resizing is forbidden"));
        assert!(!output.0.exists());
    }

    #[test]
    fn translucent_pixels_are_rejected_instead_of_dropping_straight_alpha() {
        let project = AuthoringProject::new("translucent video", 1, 1, 24.0, 1.0).unwrap();
        let frame =
            ExportFrame::from_project_render(&project, Image::new(1, 1, vec![200, 80, 20, 128]))
                .unwrap();
        let mut settings = ExportSettings::for_dimensions(1, 1, 24.0);
        settings.bind_project_color_authority(&project).unwrap();
        settings.codec = "libx264".to_string();
        settings.container = "mp4".to_string();
        settings.pixel_format = "yuv420p".to_string();
        let output = TestOutput::new("mp4");

        let error = FfmpegExportPlugin::new()
            .export_frame(output.0.to_str().unwrap(), &frame, &settings)
            .unwrap_err();
        assert!(error.to_string().contains("has no alpha channel"));
        assert!(error.to_string().contains("alpha 128"));
        assert!(!output.0.exists());
    }

    #[test]
    fn real_yuv_export_has_exact_bt709_limited_metadata_and_color_round_trip() {
        if !tools_available() {
            eprintln!("skipping real FFmpeg metadata test: ffmpeg/ffprobe unavailable");
            return;
        }
        let (frame, mut settings) = frame_and_settings("yuv444p");
        settings.codec = "ffv1".to_string();
        settings.container = "matroska".to_string();
        let output = TestOutput::new("mkv");
        let plugin = FfmpegExportPlugin::new();

        plugin
            .export_frame(output.0.to_str().unwrap(), &frame, &settings)
            .unwrap();
        plugin
            .finish_export(output.0.to_str().unwrap(), &settings)
            .unwrap();

        let probe = run_probe(&output.0);
        assert!(
            probe.status.success(),
            "{}",
            String::from_utf8_lossy(&probe.stderr)
        );
        let metadata = String::from_utf8(probe.stdout).unwrap();
        assert_bt709_limited_metadata(&metadata, "yuv444p");

        let decoded = decode_bt709_to_srgb_rgba(&output.0);
        assert!(
            decoded.status.success(),
            "{}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        assert_eq!(decoded.stdout.len(), frame.image().data.len());
        for (expected, actual) in frame
            .image()
            .data
            .chunks_exact(4)
            .zip(decoded.stdout.chunks_exact(4))
        {
            for channel in 0..3 {
                assert!(
                    expected[channel].abs_diff(actual[channel]) <= 16,
                    "RGB round-trip exceeded tolerance: expected={expected:?}, actual={actual:?}"
                );
            }
            assert_eq!(actual[3], 255);
        }
    }

    #[test]
    fn real_default_libx264_export_proves_bt709_metadata_and_color_round_trip() {
        if !tools_available() {
            eprintln!("skipping real FFmpeg metadata test: ffmpeg/ffprobe unavailable");
            return;
        }
        let project =
            AuthoringProject::new("default H.264 color contract", 4, 4, 24.0, 1.0).unwrap();
        let pixels = [64, 128, 192, 255].repeat(16);
        let frame = ExportFrame::from_project_render(&project, Image::new(4, 4, pixels)).unwrap();
        let mut settings = ExportSettings::for_dimensions(4, 4, 24.0);
        settings.bind_project_color_authority(&project).unwrap();
        settings.container = "mp4".to_string();
        settings.codec = "libx264".to_string();
        settings.pixel_format = "yuv420p".to_string();
        let output = TestOutput::new("mp4");
        let plugin = FfmpegExportPlugin::new();

        plugin
            .export_frame(output.0.to_str().unwrap(), &frame, &settings)
            .unwrap();
        plugin
            .finish_export(output.0.to_str().unwrap(), &settings)
            .unwrap();

        let probe = run_probe(&output.0);
        assert!(
            probe.status.success(),
            "{}",
            String::from_utf8_lossy(&probe.stderr)
        );
        let metadata = String::from_utf8(probe.stdout).unwrap();
        assert_bt709_limited_metadata(&metadata, "yuv420p");

        let decoded = decode_bt709_to_srgb_rgba(&output.0);
        assert!(
            decoded.status.success(),
            "{}",
            String::from_utf8_lossy(&decoded.stderr)
        );
        assert_eq!(decoded.stdout.len(), frame.image().data.len());
        for (expected, actual) in frame
            .image()
            .data
            .chunks_exact(4)
            .zip(decoded.stdout.chunks_exact(4))
        {
            for channel in 0..3 {
                assert!(
                    expected[channel].abs_diff(actual[channel]) <= 16,
                    "default H.264 RGB round-trip exceeded tolerance: expected={expected:?}, actual={actual:?}"
                );
            }
        }
    }

    fn assert_bt709_limited_metadata(metadata: &str, pixel_format: &str) {
        assert!(
            metadata.contains(&format!("pix_fmt={pixel_format}")),
            "{metadata}"
        );
        assert!(metadata.contains("color_range=tv"), "{metadata}");
        assert!(metadata.contains("color_space=bt709"), "{metadata}");
        assert!(metadata.contains("color_transfer=bt709"), "{metadata}");
        assert!(metadata.contains("color_primaries=bt709"), "{metadata}");
    }

    fn decode_bt709_to_srgb_rgba(path: &std::path::Path) -> Output {
        Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
            ])
            .arg(path)
            .args([
                "-vf",
                "colorspace=space=gbr:primaries=bt709:trc=iec61966-2-1:range=pc:ispace=bt709:iprimaries=bt709:itrc=bt709:irange=tv,format=rgba",
                "-frames:v",
                "1",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-",
            ])
            .output()
            .unwrap()
    }
}
