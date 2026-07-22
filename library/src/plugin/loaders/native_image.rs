use super::super::{
    DecodedColorSpace, DecodedPixelDescription, LoadPlugin, LoadPluginError, LoadPluginResult,
    LoadRequest, LoadResponse, Plugin,
};
use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::model::asset::{SourceColorDescription, SourceColorProfile};
use crate::model::frame::Image;
use crate::plugin::loaders::ffmpeg_video::FileIdentity;
use image::ImageDecoder;
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fs::File;
use std::io::{BufReader, Read};

fn guessed_reader(path: &str) -> Result<image::ImageReader<BufReader<File>>, Box<dyn Error>> {
    let reader = image::ImageReader::open(path)?;
    Ok(reader.with_guessed_format()?)
}

fn probe_image(
    path: &str,
) -> Result<(u32, u32, SourceColorDescription), Box<dyn std::error::Error>> {
    let reader = guessed_reader(path)?;
    let format = reader.format();
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    let bit_depth = match format {
        // The generic PNG decoder expands 1/2/4-bit samples to 8-bit output,
        // so only the encoded IHDR is authoritative for source precision.
        Some(image::ImageFormat::Png) => png_source_bit_depth(path)?,
        // WebP's VP8 and VP8L sample formats are specified as 8-bit.
        Some(image::ImageFormat::WebP) => Some(8),
        // The TIFF backend preserves its encoded color type separately from
        // the decoder output color type.
        Some(image::ImageFormat::Tiff) => uniform_channel_bit_depth(decoder.original_color_type()),
        // In particular, do not infer JPEG source precision from its decoded
        // output type: the generic API does not expose encoded precision.
        _ => None,
    };
    let profile = decoder.icc_profile()?.map(|bytes| {
        let profile_id = icc_profile_id(&bytes);
        SourceColorProfile::Icc {
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            byte_length: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            profile_id,
        }
    });

    Ok((
        width,
        height,
        SourceColorDescription {
            bit_depth,
            profile,
            ..SourceColorDescription::default()
        },
    ))
}

fn png_source_bit_depth(path: &str) -> Result<Option<u8>, std::io::Error> {
    const PNG_HEADER_LENGTH: usize = 29;
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    let mut header = [0_u8; PNG_HEADER_LENGTH];
    File::open(path)?.read_exact(&mut header)?;
    if &header[..8] != PNG_SIGNATURE
        || u32::from_be_bytes([header[8], header[9], header[10], header[11]]) != 13
        || &header[12..16] != b"IHDR"
    {
        return Ok(None);
    }

    let depth = header[24];
    let color_type = header[25];
    let valid = match color_type {
        0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
        2 | 4 | 6 => matches!(depth, 8 | 16),
        3 => matches!(depth, 1 | 2 | 4 | 8),
        _ => false,
    };
    Ok(valid.then_some(depth))
}

fn uniform_channel_bit_depth(color: image::ExtendedColorType) -> Option<u8> {
    use image::ExtendedColorType as Color;
    match color {
        Color::L1 | Color::La1 | Color::Rgb1 | Color::Rgba1 => Some(1),
        Color::L2 | Color::La2 | Color::Rgb2 | Color::Rgba2 => Some(2),
        Color::L4 | Color::La4 | Color::Rgb4 | Color::Rgba4 => Some(4),
        Color::A8
        | Color::L8
        | Color::La8
        | Color::Rgb8
        | Color::Rgba8
        | Color::Bgr8
        | Color::Bgra8
        | Color::Cmyk8 => Some(8),
        Color::L16 | Color::La16 | Color::Rgb16 | Color::Rgba16 | Color::Cmyk16 => Some(16),
        Color::Rgb32F | Color::Rgba32F => Some(32),
        // `Unknown(bits)` reports bits per pixel, not a proven uniform source
        // channel depth. Packed RGB5x1 and future variants are unknown too.
        _ => None,
    }
}

fn icc_profile_id(profile: &[u8]) -> Option<String> {
    const ICC_SIGNATURE: std::ops::Range<usize> = 36..40;
    const ICC_PROFILE_ID: std::ops::Range<usize> = 84..100;
    if profile.len() < 128 || profile.get(ICC_SIGNATURE)? != b"acsp" {
        return None;
    }
    let id = profile.get(ICC_PROFILE_ID)?;
    id.iter().any(|byte| *byte != 0).then(|| {
        id.iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    })
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

        let (w, h, source_color) = probe_image(path).map_err(LibraryError::from)?;

        Ok(vec![crate::plugin::AssetMetadata {
            kind: crate::model::asset::AssetKind::Image,
            duration: None,
            fps: None,
            width: Some(w),
            height: Some(h),
            stream_index: None,
            frame_count: None,
            time_base: None,
            source_color,
        }])
    }

    fn load(&self, request: &LoadRequest, cache: &CacheManager) -> LoadPluginResult<LoadResponse> {
        if let LoadRequest::Image { path } = request {
            if !Self::claims_path(path) {
                return Err(LoadPluginError::Unsupported);
            }
            for _attempt in 0..3 {
                let identity = FileIdentity::read(path).map_err(LoadPluginError::Failed)?;
                let cache_key = format!(
                    "native-image\0{}\0identity={}",
                    identity.canonical_path().display(),
                    identity.cache_token()
                );
                let image = if let Some(image) = cache.get_image(&cache_key) {
                    image
                } else {
                    load_image(path)
                        .map_err(LibraryError::from)
                        .map_err(LoadPluginError::Failed)?
                };
                let (_, _, source_color) = probe_image(path)
                    .map_err(LibraryError::from)
                    .map_err(LoadPluginError::Failed)?;
                if FileIdentity::read(path).map_err(LoadPluginError::Failed)? != identity {
                    continue;
                }
                cache.put_image(&cache_key, &image);
                return Ok(LoadResponse {
                    image,
                    decoded: DecodedPixelDescription::straight_rgba8(
                        DecodedColorSpace::SourceEncoded(source_color),
                    ),
                });
            }
            Err(LoadPluginError::Failed(LibraryError::Plugin(format!(
                "Image file changed repeatedly while decoding {path:?}"
            ))))
        } else {
            Err(LoadPluginError::Unsupported)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NativeImageLoader, load_image, uniform_channel_bit_depth};
    use crate::cache::CacheManager;
    use crate::plugin::{LoadPlugin, LoadPluginError, LoadRequest};
    use image::ImageEncoder;
    use sha2::Digest;
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
    fn replacing_an_image_at_the_same_path_invalidates_the_decode_cache() {
        let path = std::env::temp_dir().join(format!(
            "video-editor-replaced-image-{}.png",
            Uuid::new_v4()
        ));
        let path_text = path.to_string_lossy().into_owned();
        let loader = NativeImageLoader::new();
        let cache = CacheManager::new();
        image::save_buffer(&path, &[10, 20, 30, 255], 1, 1, image::ColorType::Rgba8).unwrap();
        let first = loader
            .load(
                &LoadRequest::Image {
                    path: path_text.clone(),
                },
                &cache,
            )
            .unwrap();
        assert_eq!(first.image.data, [10, 20, 30, 255]);

        image::save_buffer(
            &path,
            &[40, 50, 60, 255, 70, 80, 90, 255],
            2,
            1,
            image::ColorType::Rgba8,
        )
        .unwrap();
        let second = loader
            .load(&LoadRequest::Image { path: path_text }, &cache)
            .unwrap();
        assert_eq!((second.image.width, second.image.height), (2, 1));
        assert_eq!(second.image.data, [40, 50, 60, 255, 70, 80, 90, 255]);
        std::fs::remove_file(path).unwrap();
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

    #[test]
    fn image_probe_preserves_reliable_bit_depth_and_icc_identity()
    -> Result<(), Box<dyn std::error::Error>> {
        let path =
            std::env::temp_dir().join(format!("video-editor-icc-16bit-{}.png", Uuid::new_v4()));
        let mut icc = vec![0_u8; 128];
        icc[0..4].copy_from_slice(&128_u32.to_be_bytes());
        icc[36..40].copy_from_slice(b"acsp");
        let expected_profile_id = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x10, 0x32, 0x54, 0x76, 0x98, 0xba,
            0xdc, 0xfe,
        ];
        icc[84..100].copy_from_slice(&expected_profile_id);
        let expected_sha = format!("{:x}", sha2::Sha256::digest(&icc));

        let file = std::fs::File::create(&path)?;
        let writer = std::io::BufWriter::new(file);
        let mut encoder = image::codecs::png::PngEncoder::new(writer);
        encoder.set_icc_profile(icc)?;
        encoder.write_image(
            &[0xff, 0xff, 0x80, 0x00, 0x00, 0x00],
            1,
            1,
            image::ExtendedColorType::Rgb16,
        )?;

        let streams = NativeImageLoader::new().open(path.to_str().unwrap())?;
        let source = &streams[0].source_color;
        assert_eq!(source.bit_depth, Some(16));
        assert_eq!(
            source.profile,
            Some(crate::model::asset::SourceColorProfile::Icc {
                sha256: expected_sha,
                byte_length: 128,
                profile_id: Some("0123456789abcdef1032547698badcfe".to_string()),
            })
        );

        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn image_probe_reads_low_bit_png_precision_from_ihdr() -> Result<(), Box<dyn std::error::Error>>
    {
        for (depth, bytes) in low_bit_png_fixtures() {
            let path = std::env::temp_dir()
                .join(format!("video-editor-{depth}-bit-{}.png", Uuid::new_v4()));
            std::fs::write(&path, bytes)?;

            let streams = NativeImageLoader::new().open(path.to_str().unwrap())?;
            assert_eq!(
                streams[0].source_color.bit_depth,
                Some(depth),
                "PNG decoder output expansion must not replace encoded precision"
            );

            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    #[test]
    fn unknown_extended_color_bits_are_not_called_channel_depth() {
        assert_eq!(
            uniform_channel_bit_depth(image::ExtendedColorType::Unknown(12)),
            None
        );
    }

    fn low_bit_png_fixtures() -> [(u8, &'static [u8]); 3] {
        const L1: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00,
            0x00, 0xcb, 0x7b, 0xd2, 0xee, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0x08, 0x05, 0x00, 0x00, 0x57, 0x00, 0x56, 0x3f, 0x43, 0x1f, 0x4c, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        const L2: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00,
            0x00, 0x8c, 0xdb, 0xa8, 0x3e, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0x90, 0x96, 0x06, 0x00, 0x00, 0x54, 0x00, 0x37, 0x30, 0x78, 0x6f, 0x7b,
            0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        const L4: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x01, 0x04, 0x00, 0x00, 0x00,
            0x00, 0x03, 0x9b, 0x5d, 0x9e, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0x10, 0x32, 0x09, 0xab, 0x00, 0x00, 0x02, 0x0d, 0x01, 0x15, 0xa9, 0x7e,
            0xa5, 0xc6, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        [(1, L1), (2, L2), (4, L4)]
    }
}
