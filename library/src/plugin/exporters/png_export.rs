use super::super::{ExportFormat, ExportPlugin, ExportSettings, Plugin, PluginCategory};
use crate::loader::image::Image;
use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use crate::error::LibraryError;
use std::fs::File;
use std::io::BufWriter;

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
    ) -> Result<(), LibraryError> {
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