use super::super::{LoadPlugin, LoadRequest, LoadResponse, Plugin, PluginCategory};
use crate::cache::CacheManager;
use crate::loader::image::load_image;
use crate::error::LibraryError;

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

    fn category(&self) -> PluginCategory {
        PluginCategory::Load
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl LoadPlugin for NativeImageLoader {
    fn supports(&self, request: &LoadRequest) -> bool {
        matches!(request, LoadRequest::Image { .. })
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
            Err(LibraryError::Plugin("NativeImageLoader received unsupported request".to_string()))
        }
    }
}
