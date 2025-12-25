use super::super::{LoadPlugin, LoadRequest, LoadResponse, Plugin};
use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::model::frame::Image;
use std::error::Error;

/// Load an image from disk and return as RGBA.
pub fn load_image(path: &str) -> Result<Image, Box<dyn Error>> {
    let img = image::open(path).map_err(|e| format!("Failed to open image file: {}", e))?;
    let rgba_image = img.to_rgba8();
    Ok(Image {
        width: rgba_image.width(),
        height: rgba_image.height(),
        data: rgba_image.into_raw(),
    })
}

pub struct NativeImageLoader;

impl NativeImageLoader {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for NativeImageLoader {
    fn id(&self) -> &'static str {
        "native_image_loader"
    }

    fn name(&self) -> String {
        "Native Image Loader".to_string()
    }

    fn category(&self) -> String {
        "Media".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl LoadPlugin for NativeImageLoader {
    fn supports(&self, request: &LoadRequest) -> bool {
        matches!(request, LoadRequest::Image { .. })
    }

    fn priority(&self) -> u32 {
        100
    }

    fn load(
        &self,
        request: &LoadRequest,
        cache: &CacheManager,
    ) -> Result<LoadResponse, LibraryError> {
        if let LoadRequest::Image { path } = request {
            if let Some(image) = cache.get_image(path) {
                return Ok(LoadResponse::Image(image));
            }

            let image = load_image(path)?;
            cache.put_image(path, &image);
            Ok(LoadResponse::Image(image))
        } else {
            Err(LibraryError::Plugin(
                "NativeImageLoader received unsupported request".to_string(),
            ))
        }
    }

    fn get_asset_kind(&self, path: &str) -> Option<crate::model::project::asset::AssetKind> {
        let ext = std::path::Path::new(path)
            .extension()?
            .to_str()?
            .to_lowercase();
        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "bmp" | "webp" | "tiff" | "tga" | "gif" | "ico" | "pnm" => {
                Some(crate::model::project::asset::AssetKind::Image)
            }
            _ => None,
        }
    }

    fn get_dimensions(&self, path: &str) -> Option<(u32, u32)> {
        image::image_dimensions(path).ok()
    }
    fn get_metadata(&self, path: &str) -> Option<crate::plugin::AssetMetadata> {
        let ext = std::path::Path::new(path)
            .extension()?
            .to_str()?
            .to_lowercase();

        let kind = match ext.as_str() {
            "png" | "jpg" | "jpeg" | "bmp" | "webp" | "tiff" | "tga" | "gif" | "ico" | "pnm" => {
                crate::model::project::asset::AssetKind::Image
            }
            _ => return None,
        };

        let dim = image::image_dimensions(path).ok();
        let (w, h) = if let Some((w, h)) = dim {
            (Some(w), Some(h))
        } else {
            (None, None)
        };

        Some(crate::plugin::AssetMetadata {
            kind,
            duration: None,
            fps: None,
            width: w,
            height: h,
            stream_index: None,
        })
    }
}
