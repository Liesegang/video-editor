use super::super::{ExportFormat, ExportFrame, ExportPlugin, ExportSettings, Plugin};
use crate::error::LibraryError;
use png::{
    BitDepth, ColorType, Compression, Encoder, ScaledFloat, SourceChromaticities,
    SrgbRenderingIntent,
};
use std::fs::File;
use std::io::BufWriter;

#[derive(Default)]
pub struct PngExportPlugin;

impl PngExportPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for PngExportPlugin {
    fn id(&self) -> &'static str {
        "png_export"
    }

    fn name(&self) -> String {
        "PNG Export".to_string()
    }

    fn category(&self) -> String {
        "Export".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl ExportPlugin for PngExportPlugin {
    fn export_frame(
        &self,
        path: &str,
        frame: &ExportFrame,
        settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
        settings.require_matching_color_authority(frame)?;
        if settings.export_format() != ExportFormat::Png || settings.container != "png" {
            return Err(LibraryError::Render(format!(
                "PNG exporter requires the 'png' container, not '{}'",
                settings.container
            )));
        }
        if settings.pixel_format != "rgba" {
            return Err(LibraryError::Render(format!(
                "PNG exporter supports only typed straight RGBA8 storage, not '{}'",
                settings.pixel_format
            )));
        }
        let image = frame.image();
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        let mut encoder = Encoder::new(writer, image.width, image.height);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        encoder.set_compression(png_compression(settings));
        encoder.set_source_gamma(ScaledFloat::from_scaled(45_455));
        encoder.set_source_chromaticities(SourceChromaticities::new(
            (0.31270, 0.32900),
            (0.64000, 0.33000),
            (0.30000, 0.60000),
            (0.15000, 0.06000),
        ));
        encoder.set_source_srgb(SrgbRenderingIntent::Perceptual);
        let mut png_writer = encoder
            .write_header()
            .map_err(|error| LibraryError::Render(format!("cannot write PNG header: {error}")))?;
        png_writer
            .write_image_data(&image.data)
            .map_err(|error| LibraryError::Render(format!("cannot write PNG pixels: {error}")))?;
        png_writer
            .finish()
            .map_err(|error| LibraryError::Render(format!("cannot finish PNG: {error}")))?;
        Ok(())
    }

    fn properties(&self) -> Vec<crate::model::property::PropertyDefinition> {
        use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue};
        vec![PropertyDefinition::new(
            "compression",
            PropertyUiType::Dropdown {
                options: vec![
                    "Default".to_string(),
                    "Fast".to_string(),
                    "Best".to_string(),
                ],
            },
            "Compression",
            PropertyValue::String("Fast".to_string()),
        )]
    }
}

fn png_compression(settings: &ExportSettings) -> Compression {
    match settings.parameter_string("compression").as_deref() {
        Some("Best") => Compression::High,
        Some("Default") => Compression::Balanced,
        Some("Fast") | None => Compression::Fast,
        Some(_) => Compression::Fast,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::frame::Image;
    use crate::model::project::Project;
    use std::fs;
    use std::io::Cursor;
    use uuid::Uuid;

    struct TestPng(std::path::PathBuf);

    impl TestPng {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("ruvie-srgb-{}.png", Uuid::new_v4())))
        }
    }

    impl Drop for TestPng {
        fn drop(&mut self) {
            if let Err(error) = fs::remove_file(&self.0)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!("failed to remove test PNG: {error}");
            }
        }
    }

    fn test_frame(project: &Project) -> ExportFrame {
        ExportFrame::from_project_render(
            project,
            Image::new(2, 1, vec![255, 0, 0, 255, 0, 128, 255, 64]),
        )
        .unwrap()
    }

    #[test]
    fn encoded_png_carries_normative_srgb_chunk() {
        let project = Project::new("tagged PNG");
        let frame = test_frame(&project);
        let mut settings = ExportSettings::for_dimensions(2, 1, 24.0);
        settings.bind_project_color_authority(&project).unwrap();
        let path = TestPng::new();

        PngExportPlugin::new()
            .export_frame(path.0.to_str().unwrap(), &frame, &settings)
            .unwrap();

        let bytes = fs::read(&path.0).unwrap();
        let decoder = png::Decoder::new(Cursor::new(&bytes));
        let reader = decoder.read_info().unwrap();
        assert_eq!(reader.info().srgb, Some(SrgbRenderingIntent::Perceptual));
        assert_eq!(png_chunk(&bytes, b"sRGB"), Some(&[0][..]));
        assert_eq!(
            png_chunk(&bytes, b"gAMA"),
            Some(&45_455_u32.to_be_bytes()[..])
        );
        assert_eq!(
            png_chunk(&bytes, b"cHRM"),
            Some(
                &[
                    31_270_u32, 32_900, 64_000, 33_000, 30_000, 60_000, 15_000, 6_000,
                ]
                .into_iter()
                .flat_map(u32::to_be_bytes)
                .collect::<Vec<_>>()[..]
            )
        );
    }

    #[test]
    fn unbound_settings_are_rejected_instead_of_assuming_srgb() {
        let project = Project::new("unbound PNG");
        let frame = test_frame(&project);
        let settings = ExportSettings::for_dimensions(2, 1, 24.0);
        let path = TestPng::new();

        let error = PngExportPlugin::new()
            .export_frame(path.0.to_str().unwrap(), &frame, &settings)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("no Project-derived color authority")
        );
        assert!(!path.0.exists());
    }

    #[test]
    fn mismatched_dimensions_are_rejected_instead_of_ignoring_export_override() {
        let project = Project::new("mismatched PNG dimensions");
        let frame = test_frame(&project);
        let mut settings = ExportSettings::for_dimensions(4, 2, 24.0);
        settings.bind_project_color_authority(&project).unwrap();
        let path = TestPng::new();

        let error = PngExportPlugin::new()
            .export_frame(path.0.to_str().unwrap(), &frame, &settings)
            .unwrap_err();
        assert!(error.to_string().contains("implicit resizing is forbidden"));
        assert!(!path.0.exists());
    }

    fn png_chunk<'a>(bytes: &'a [u8], expected_kind: &[u8; 4]) -> Option<&'a [u8]> {
        let mut offset = 8_usize;
        while offset.checked_add(12)? <= bytes.len() {
            let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
            let kind_start = offset + 4;
            let data_start = kind_start + 4;
            let data_end = data_start.checked_add(length)?;
            let chunk_end = data_end.checked_add(4)?;
            if chunk_end > bytes.len() {
                return None;
            }
            if &bytes[kind_start..data_start] == expected_kind {
                return Some(&bytes[data_start..data_end]);
            }
            offset = chunk_end;
        }
        None
    }
}
