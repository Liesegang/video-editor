use crate::extensions::traits::{ExportPlugin, ExportSettings, Plugin};
use crate::error::LibraryError;
use crate::io::image::Image;
use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
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
    fn export_image(
        &self,
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

    fn properties(&self) -> Vec<crate::extensions::traits::PropertyDefinition> {
        use crate::extensions::traits::{PropertyDefinition, PropertyUiType};
        use crate::core::model::property::PropertyValue;
        vec![PropertyDefinition {
            name: "compression".to_string(),
            label: "Compression".to_string(),
            ui_type: PropertyUiType::Dropdown {
                options: vec![
                    "Default".to_string(),
                    "Fast".to_string(),
                    "Best".to_string(),
                ],
            },
            default_value: PropertyValue::String("Fast".to_string()),
            category: "Settings".to_string(),
        }]
    }
}
