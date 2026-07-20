use super::super::{
    LoadPlugin, LoadPluginError, LoadPluginResult, LoadRequest, LoadResponse, Plugin,
};
use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::model::frame::Image;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;

fn guessed_reader(path: &str) -> Result<image::ImageReader<BufReader<File>>, Box<dyn Error>> {
    let reader = image::ImageReader::open(path)?;
    Ok(reader.with_guessed_format()?)
}

/// Load an image from disk and return as RGBA.
pub fn load_image(path: &str) -> Result<Image, Box<dyn Error>> {
    let img = guessed_reader(path)?
        .decode()
        .map_err(|error| format!("Failed to open image file: {error}"))?;
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

    fn supports_extension(path: &str) -> bool {
        let extension = std::path::Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        matches!(
            extension.as_deref(),
            Some(
                "png"
                    | "jpg"
                    | "jpeg"
                    | "bmp"
                    | "webp"
                    | "tiff"
                    | "tga"
                    | "gif"
                    | "ico"
                    | "pnm"
                    | "ppm"
                    | "pgm"
                    | "pbm"
                    | "pam"
            )
        )
    }

    fn claims_path(path: &str) -> bool {
        Self::supports_extension(path)
            || guessed_reader(path)
                .ok()
                .and_then(|reader| reader.format())
                .is_some()
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
        if !Self::claims_path(path) {
            return Err(LoadPluginError::Unsupported);
        }

        let (w, h) = guessed_reader(path)
            .map_err(LibraryError::from)?
            .into_dimensions()
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
            if !Self::claims_path(path) {
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

    #[test]
    fn valid_image_magic_is_loaded_even_when_the_file_is_renamed()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = std::env::temp_dir().join(format!(
            "video-editor-renamed-image-{}.asset",
            Uuid::new_v4()
        ));
        let png_path = path.with_extension("png");
        image::save_buffer(&png_path, &[10, 20, 30, 255], 1, 1, image::ColorType::Rgba8)?;
        std::fs::rename(&png_path, &path)?;
        let path_text = path.to_string_lossy().into_owned();
        let loader = NativeImageLoader::new();
        let streams = loader.open(&path_text)?;
        assert_eq!(streams.first().and_then(|stream| stream.width), Some(1));
        let loaded = loader.load(
            &LoadRequest::Image { path: path_text },
            &CacheManager::new(),
        )?;
        assert_eq!(loaded.image.data, [10, 20, 30, 255]);
        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn concrete_pnm_extensions_remain_claimed_and_decodable()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixtures: [(&str, &[u8]); 4] = [
            ("ppm", b"P6\n1 1\n255\n\x01\x02\x03"),
            ("pgm", b"P5\n1 1\n255\n\x09"),
            ("pbm", b"P4\n1 1\n\x00"),
            (
                "pam",
                b"P7\nWIDTH 1\nHEIGHT 1\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n\x04\x05\x06\xff",
            ),
        ];
        let loader = NativeImageLoader::new();
        for (extension, bytes) in fixtures {
            let path = std::env::temp_dir().join(format!(
                "video-editor-pnm-{}.{}",
                Uuid::new_v4(),
                extension
            ));
            std::fs::write(&path, bytes)?;
            let path_text = path.to_string_lossy().into_owned();
            let streams = loader.open(&path_text)?;
            assert_eq!(streams.first().and_then(|stream| stream.width), Some(1));
            let loaded = loader.load(
                &LoadRequest::Image { path: path_text },
                &CacheManager::new(),
            )?;
            assert_eq!(loaded.image.width, 1);
            assert_eq!(loaded.image.height, 1);
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}
