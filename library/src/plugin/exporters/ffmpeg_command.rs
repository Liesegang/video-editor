use super::{ExportColorAuthority, ExportSettings};
use crate::error::LibraryError;

const RAW_INPUT_PRIMARIES: &str = "bt709";
const RAW_INPUT_TRANSFER: &str = "iec61966-2-1";
const RAW_INPUT_MATRIX: &str = "rgb";
const RAW_INPUT_RANGE: &str = "pc";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FfmpegDeliveryPolicy {
    /// Keep the sRGB signal as full-range RGB. Alpha preservation is governed
    /// by the explicitly requested RGB pixel format and codec.
    SdrSrgbFullRangeRgb8,
    /// Convert sRGB transfer to BT.709 transfer, RGB to BT.709 YCbCr, and full
    /// range to studio/limited range before encoding.
    SdrBt709LimitedYuv8,
}

impl FfmpegDeliveryPolicy {
    pub(super) fn for_settings(
        settings: &ExportSettings,
        authority: &ExportColorAuthority,
    ) -> Result<Self, LibraryError> {
        match authority {
            ExportColorAuthority::SdrSrgbFullRangeStraightRgba8 { .. } => {}
        }
        match settings.pixel_format.as_str() {
            "rgb24" | "rgba" | "gbrp" | "bgr0" => Ok(Self::SdrSrgbFullRangeRgb8),
            "yuv420p" | "yuv422p" | "yuv444p" => Ok(Self::SdrBt709LimitedYuv8),
            unsupported => Err(LibraryError::Render(format!(
                "FFmpeg export pixel format '{unsupported}' is not a supported typed 8-bit SDR delivery; non-sRGB, HDR, 10-bit, and float export remain unsupported"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FfmpegCommand {
    pub(super) binary: String,
    pub(super) args: Vec<String>,
}

impl FfmpegCommand {
    pub(super) fn build(
        path: &str,
        settings: &ExportSettings,
        policy: FfmpegDeliveryPolicy,
    ) -> Result<Self, LibraryError> {
        validate_settings(settings)?;
        validate_encoder_contract(settings, policy)?;
        let binary = settings
            .trusted_ffmpeg_path()
            .unwrap_or("ffmpeg")
            .to_string();
        let mut args = vec![
            "-y".to_string(),
            "-f".to_string(),
            "rawvideo".to_string(),
            "-pixel_format".to_string(),
            "rgba".to_string(),
            "-video_size".to_string(),
            format!("{}x{}", settings.width, settings.height),
            "-framerate".to_string(),
            settings.fps.to_string(),
            "-color_primaries".to_string(),
            RAW_INPUT_PRIMARIES.to_string(),
            "-color_trc".to_string(),
            RAW_INPUT_TRANSFER.to_string(),
            "-colorspace".to_string(),
            RAW_INPUT_MATRIX.to_string(),
            "-color_range".to_string(),
            RAW_INPUT_RANGE.to_string(),
            "-i".to_string(),
            "-".to_string(),
        ];

        let has_audio = append_audio_input(&mut args, settings);
        if policy == FfmpegDeliveryPolicy::SdrBt709LimitedYuv8 {
            args.push("-vf".to_string());
            args.push(bt709_yuv_filter(&settings.pixel_format));
        }
        args.push("-c:v".to_string());
        args.push(settings.codec.clone());
        append_codec_parameters(&mut args, settings);
        if has_audio {
            append_audio_output(&mut args, settings);
        }
        append_output_color(&mut args, policy);
        args.push("-pix_fmt".to_string());
        args.push(settings.pixel_format.clone());
        args.push("-f".to_string());
        args.push(normalized_container(&settings.container).to_string());
        args.push(path.to_string());
        Ok(Self { binary, args })
    }
}

fn validate_settings(settings: &ExportSettings) -> Result<(), LibraryError> {
    if settings.width == 0 || settings.height == 0 {
        return Err(LibraryError::Render(
            "FFmpeg export dimensions must be non-zero".to_string(),
        ));
    }
    if !settings.fps.is_finite() || settings.fps <= 0.0 {
        return Err(LibraryError::Render(format!(
            "FFmpeg export fps must be finite and positive, not {}",
            settings.fps
        )));
    }
    if settings.codec.trim().is_empty() || settings.container.trim().is_empty() {
        return Err(LibraryError::Render(
            "FFmpeg export codec and container must be explicit".to_string(),
        ));
    }
    Ok(())
}

fn validate_encoder_contract(
    settings: &ExportSettings,
    policy: FfmpegDeliveryPolicy,
) -> Result<(), LibraryError> {
    let codec = settings.codec.as_str();
    let pixel_format = settings.pixel_format.as_str();
    let container = normalized_container(&settings.container);
    let supported = match policy {
        FfmpegDeliveryPolicy::SdrSrgbFullRangeRgb8 => {
            codec == "ffv1" && pixel_format == "bgr0" && container == "matroska"
        }
        FfmpegDeliveryPolicy::SdrBt709LimitedYuv8 => {
            (codec == "ffv1" && container == "matroska")
                || (codec == "libx264" && pixel_format == "yuv420p" && container == "mp4")
        }
    };
    if supported {
        return Ok(());
    }
    Err(LibraryError::Render(format!(
        "FFmpeg codec/container/pixel-format combination '{codec}/{}/{pixel_format}' has no verified {:?} color contract; choose a supported 8-bit SDR combination instead of allowing FFmpeg to silently change storage or metadata",
        settings.container, policy
    )))
}

fn normalized_container(container: &str) -> &str {
    if container == "mkv" {
        "matroska"
    } else {
        container
    }
}

fn append_audio_input(args: &mut Vec<String>, settings: &ExportSettings) -> bool {
    let Some((audio_path, channels, rate)) = settings.runtime_audio_source() else {
        return false;
    };
    args.extend([
        "-f".to_string(),
        "f32le".to_string(),
        "-ar".to_string(),
        rate.to_string(),
        "-ac".to_string(),
        channels.to_string(),
        "-i".to_string(),
        audio_path.to_string(),
    ]);
    true
}

fn append_codec_parameters(args: &mut Vec<String>, settings: &ExportSettings) {
    if let Some(bitrate) = settings.parameter_u64("bitrate") {
        args.extend(["-b:v".to_string(), format!("{bitrate}k")]);
    }
    if let Some(crf) = settings
        .parameter_f64("crf")
        .or_else(|| settings.parameter_f64("quality"))
    {
        args.extend(["-crf".to_string(), crf.to_string()]);
    }
    if let Some(preset) = settings.parameter_string("preset") {
        args.extend(["-preset".to_string(), preset]);
    }
    if let Some(profile) = settings.parameter_string("profile") {
        args.extend(["-profile:v".to_string(), profile]);
    }
}

fn append_audio_output(args: &mut Vec<String>, settings: &ExportSettings) {
    let audio_bitrate = settings.parameter_u64("audio_bitrate").unwrap_or(192);
    args.extend([
        "-c:a".to_string(),
        "aac".to_string(),
        "-b:a".to_string(),
        format!("{audio_bitrate}k"),
        "-map".to_string(),
        "0:v".to_string(),
        "-map".to_string(),
        "1:a".to_string(),
    ]);
}

fn append_output_color(args: &mut Vec<String>, policy: FfmpegDeliveryPolicy) {
    let (transfer, matrix, range) = match policy {
        FfmpegDeliveryPolicy::SdrSrgbFullRangeRgb8 => {
            (RAW_INPUT_TRANSFER, RAW_INPUT_MATRIX, RAW_INPUT_RANGE)
        }
        FfmpegDeliveryPolicy::SdrBt709LimitedYuv8 => ("bt709", "bt709", "tv"),
    };
    args.extend([
        "-color_primaries".to_string(),
        "bt709".to_string(),
        "-color_trc".to_string(),
        transfer.to_string(),
        "-colorspace".to_string(),
        matrix.to_string(),
        "-color_range".to_string(),
        range.to_string(),
    ]);
}

fn bt709_yuv_filter(pixel_format: &str) -> String {
    format!(
        "colorspace=space=bt709:primaries=bt709:trc=bt709:range=tv:ispace=gbr:iprimaries=bt709:itrc=iec61966-2-1:irange=pc:format={pixel_format}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::project::Project;

    fn settings(pixel_format: &str) -> ExportSettings {
        let project = Project::new("command test");
        let mut settings = ExportSettings::for_dimensions(1920, 1080, 24.0);
        settings.bind_project_color_authority(&project).unwrap();
        settings.container = "mp4".to_string();
        settings.codec = "libx264".to_string();
        settings.pixel_format = pixel_format.to_string();
        settings
    }

    fn value_after<'a>(args: &'a [String], option: &str, occurrence: usize) -> &'a str {
        let index = args
            .iter()
            .enumerate()
            .filter(|(_, arg)| arg.as_str() == option)
            .nth(occurrence)
            .unwrap()
            .0;
        &args[index + 1]
    }

    #[test]
    fn rgb_cli_declares_srgb_full_range_on_input_and_output() {
        let mut settings = settings("rgb24");
        settings.codec = "ffv1".to_string();
        settings.pixel_format = "bgr0".to_string();
        settings.container = "matroska".to_string();
        let policy =
            FfmpegDeliveryPolicy::for_settings(&settings, settings.color_authority().unwrap())
                .unwrap();
        let command = FfmpegCommand::build("out.mkv", &settings, policy).unwrap();

        assert_eq!(policy, FfmpegDeliveryPolicy::SdrSrgbFullRangeRgb8);
        assert_eq!(value_after(&command.args, "-color_primaries", 0), "bt709");
        assert_eq!(value_after(&command.args, "-color_trc", 0), "iec61966-2-1");
        assert_eq!(value_after(&command.args, "-colorspace", 0), "rgb");
        assert_eq!(value_after(&command.args, "-color_range", 0), "pc");
        assert_eq!(value_after(&command.args, "-color_primaries", 1), "bt709");
        assert_eq!(value_after(&command.args, "-color_trc", 1), "iec61966-2-1");
        assert_eq!(value_after(&command.args, "-colorspace", 1), "rgb");
        assert_eq!(value_after(&command.args, "-color_range", 1), "pc");
        assert!(!command.args.iter().any(|arg| arg == "-vf"));
    }

    #[test]
    fn yuv_cli_performs_and_declares_bt709_limited_delivery_conversion() {
        let settings = settings("yuv420p");
        let policy =
            FfmpegDeliveryPolicy::for_settings(&settings, settings.color_authority().unwrap())
                .unwrap();
        let command = FfmpegCommand::build("out.mp4", &settings, policy).unwrap();

        assert_eq!(policy, FfmpegDeliveryPolicy::SdrBt709LimitedYuv8);
        assert_eq!(
            value_after(&command.args, "-vf", 0),
            "colorspace=space=bt709:primaries=bt709:trc=bt709:range=tv:ispace=gbr:iprimaries=bt709:itrc=iec61966-2-1:irange=pc:format=yuv420p"
        );
        assert_eq!(value_after(&command.args, "-color_trc", 1), "bt709");
        assert_eq!(value_after(&command.args, "-colorspace", 1), "bt709");
        assert_eq!(value_after(&command.args, "-color_range", 1), "tv");
        assert_eq!(value_after(&command.args, "-pix_fmt", 0), "yuv420p");
    }

    #[test]
    fn higher_precision_hdr_and_unknown_formats_fail_closed() {
        for pixel_format in ["yuv420p10le", "p010le", "gbrpf32le", "nv12", "unknown"] {
            let settings = settings(pixel_format);
            let error =
                FfmpegDeliveryPolicy::for_settings(&settings, settings.color_authority().unwrap())
                    .unwrap_err();
            assert!(error.to_string().contains(pixel_format));
            assert!(error.to_string().contains("unsupported"));
        }
    }
}
