use super::{LoadPlugin, LoadRequest, LoadResponse};
use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::plugin::Plugin;
use crate::runtime::Image;
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
    fn open(&self, path: &str) -> Result<Vec<crate::plugin::AssetMetadata>, LibraryError> {
        // Check file extension
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());
        let is_supported = matches!(
            ext.as_deref(),
            Some("png" | "jpg" | "jpeg" | "bmp" | "webp" | "tiff" | "tga" | "gif" | "ico" | "pnm")
        );
        if !is_supported {
            return Err(LibraryError::plugin("Unsupported file type".to_string()));
        }

        let (w, h) = image::image_dimensions(path)
            .map_err(|e| LibraryError::from(Box::new(e) as Box<dyn std::error::Error>))?;

        Ok(vec![crate::plugin::AssetMetadata {
            kind: crate::project::asset::AssetKind::Image,
            duration: None,
            fps: None,
            width: Some(w),
            height: Some(h),
            stream_index: None,
        }])
    }

    fn load(
        &self,
        request: &LoadRequest,
        cache: &CacheManager,
    ) -> Result<LoadResponse, LibraryError> {
        if let LoadRequest::Image { path } = request {
            let image = if let Some(img) = cache.get_image(path) {
                img
            } else {
                let img = load_image(path)?;
                cache.put_image(path, &img);
                img
            };
            Ok(LoadResponse { image })
        } else {
            Err(LibraryError::plugin(
                "NativeImageLoader received unsupported request".to_string(),
            ))
        }
    }
}
