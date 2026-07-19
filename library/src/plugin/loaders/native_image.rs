use super::super::{
    LoadPlugin, LoadPluginError, LoadPluginResult, LoadRequest, LoadResponse, Plugin,
};
use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::model::frame::Image;
use std::error::Error;

/// Load an image from disk and return as RGBA.
pub fn load_image(path: &str) -> Result<Image, Box<dyn Error>> {
    let img = image::open(path).map_err(|e| format!("Failed to open image file: {}", e))?;
    let rgba_image = img.to_rgba8();
    Ok(Image::new(
        rgba_image.width(),
        rgba_image.height(),
        rgba_image.into_raw(),
    ))
}

#[derive(Default)]
pub struct NativeImageLoader;

impl NativeImageLoader {
    pub fn new() -> Self {
        Self
    }

    fn supports_path(path: &str) -> bool {
        let extension = std::path::Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        matches!(
            extension.as_deref(),
            Some("png" | "jpg" | "jpeg" | "bmp" | "webp" | "tiff" | "tga" | "gif" | "ico" | "pnm")
        )
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
    fn open(&self, path: &str) -> LoadPluginResult<Vec<crate::plugin::AssetMetadata>> {
        // Check file extension
        if !Self::supports_path(path) {
            return Err(LoadPluginError::Unsupported);
        }

        let (w, h) = image::image_dimensions(path)
            .map_err(|e| LibraryError::from(Box::new(e) as Box<dyn std::error::Error>))?;

        Ok(vec![crate::plugin::AssetMetadata {
            kind: crate::model::asset::AssetKind::Image,
            duration: None,
            fps: None,
            width: Some(w),
            height: Some(h),
            stream_index: None,
            frame_count: None,
            time_base: None,
        }])
    }

    fn load(&self, request: &LoadRequest, cache: &CacheManager) -> LoadPluginResult<LoadResponse> {
        if let LoadRequest::Image { path } = request {
            if !Self::supports_path(path) {
                return Err(LoadPluginError::Unsupported);
            }
            let image = if let Some(img) = cache.get_image(path) {
                img
            } else {
                let img = load_image(path)
                    .map_err(LibraryError::from)
                    .map_err(LoadPluginError::Failed)?;
                cache.put_image(path, &img);
                img
            };
            Ok(LoadResponse { image })
        } else {
            Err(LoadPluginError::Unsupported)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NativeImageLoader, load_image};
    use crate::cache::CacheManager;
    use crate::plugin::{LoadPlugin, LoadPluginError, LoadRequest};
    use uuid::Uuid;

    #[test]
    fn transparent_png_loads_as_canonical_straight_rgba() {
        let path = std::env::temp_dir().join(format!("video-editor-alpha-{}.png", Uuid::new_v4()));
        image::save_buffer(
            &path,
            &[240, 80, 20, 128, 55, 66, 77, 0],
            2,
            1,
            image::ColorType::Rgba8,
        )
        .unwrap();

        let loaded = load_image(path.to_str().unwrap()).unwrap();
        assert_eq!(&loaded.data[0..4], &[240, 80, 20, 128]);
        assert_eq!(&loaded.data[4..8], &[0, 0, 0, 0]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn image_request_with_an_unknown_extension_is_declined_before_decoding() {
        let request = LoadRequest::Image {
            path: "/does/not/exist/custom.runtime-fixture".to_string(),
        };
        assert!(matches!(
            NativeImageLoader::new().load(&request, &CacheManager::new()),
            Err(LoadPluginError::Unsupported)
        ));
    }
}
