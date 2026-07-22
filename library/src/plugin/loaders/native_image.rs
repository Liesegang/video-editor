use super::super::{
    DecodedColorSpace, DecodedPixelDescription, LoadPlugin, LoadPluginError, LoadPluginResult,
    LoadRequest, LoadResponse, Plugin,
};
pub use super::native_image_decode::load_image;
use super::native_image_decode::{guessed_reader, load_high_precision_image, open_regular};
use super::native_png_metadata::{icc_source_profile, probe_png_metadata};
use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::model::asset::{SourceColorDescription, SourceColorProfile};
use crate::plugin::loaders::native_still_format_probe::{DeclaredColor, probe_format_source};
use crate::plugin::loaders::{DecodedPixelStorage, FileIdentity, validate_decoded_pixel_layout};
use image::ImageDecoder;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};

struct ImageProbe {
    width: u32,
    height: u32,
    source_color: SourceColorDescription,
    preserves_high_precision: bool,
}

fn probe_image(path: &str) -> Result<ImageProbe, Box<dyn std::error::Error>> {
    let reader = guessed_reader(path)?;
    let format = reader.format();
    if format == Some(image::ImageFormat::Png) {
        let png = probe_png_metadata(reader.into_inner())?;
        return Ok(ImageProbe {
            width: png.width,
            height: png.height,
            source_color: png.source_color,
            preserves_high_precision: true,
        });
    }

    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    let supplemental = probe_format_source(path, format, decoder.original_color_type())?;
    let bit_depth = match format {
        // JPEG's generic decoded color type does not expose the encoded
        // sample precision. Read the SOF marker so 12-bit JPEG can never be
        // quantized to RGBA8 and mislabeled by the untagged-still policy.
        Some(image::ImageFormat::Jpeg) => jpeg_source_bit_depth(path)?,
        // WebP's VP8 and VP8L sample formats are specified as 8-bit.
        Some(image::ImageFormat::WebP) => Some(8),
        // The TIFF backend preserves its encoded color type separately from
        // the decoder output color type.
        Some(image::ImageFormat::Tiff) => uniform_channel_bit_depth(decoder.original_color_type()),
        // In particular, do not infer JPEG source precision from its decoded
        // output type: the generic API does not expose encoded precision.
        _ => supplemental.bit_depth,
    };
    let profile = decoder
        .icc_profile()?
        .map(|bytes| icc_source_profile(&bytes));

    let mut source = SourceColorDescription {
        bit_depth,
        profile,
        ..SourceColorDescription::default()
    };
    if format == Some(image::ImageFormat::Jpeg) && source.profile.is_none() {
        match jpeg_declared_color(path)? {
            Some(JpegDeclaredColor::Srgb) => {
                source.primaries = Some(crate::model::asset::SourceColorPrimaries::Bt709);
                source.transfer = Some(crate::model::asset::SourceTransferCharacteristic::Srgb);
            }
            Some(JpegDeclaredColor::Other { identity }) => {
                source.profile = Some(SourceColorProfile::Other {
                    profile_kind: "jpeg_exif_color_space".to_string(),
                    identity,
                });
            }
            None => {}
        }
    }

    if source.profile.is_none() && source.primaries.is_none() && source.transfer.is_none() {
        match supplemental.declared_color {
            Some(DeclaredColor::Srgb) => {
                source.primaries = Some(crate::model::asset::SourceColorPrimaries::Bt709);
                source.transfer = Some(crate::model::asset::SourceTransferCharacteristic::Srgb);
            }
            Some(DeclaredColor::Profile(profile)) => source.profile = Some(profile),
            None => {}
        }
    }

    Ok(ImageProbe {
        width,
        height,
        source_color: source,
        preserves_high_precision: supplemental.preserves_high_precision
            || matches!(
                format,
                Some(image::ImageFormat::Png | image::ImageFormat::Tiff)
            ),
    })
}

fn jpeg_source_bit_depth(path: &str) -> Result<Option<u8>, std::io::Error> {
    let mut reader = BufReader::new(open_regular(path)?);
    let mut signature = [0_u8; 2];
    reader.read_exact(&mut signature)?;
    if signature != [0xff, 0xd8] {
        return Ok(None);
    }

    loop {
        let marker = match read_jpeg_marker(&mut reader)? {
            Some(marker) => marker,
            None => return Ok(None),
        };
        if marker == 0xda || marker == 0xd9 {
            return Ok(None);
        }
        if marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
            continue;
        }

        let mut length = [0_u8; 2];
        reader.read_exact(&mut length)?;
        let segment_length = u16::from_be_bytes(length);
        if segment_length < 2 {
            return Ok(None);
        }
        if is_jpeg_start_of_frame(marker) {
            if segment_length < 3 {
                return Ok(None);
            }
            let mut precision = [0_u8; 1];
            reader.read_exact(&mut precision)?;
            return Ok(Some(precision[0]));
        }
        reader.seek(SeekFrom::Current(i64::from(segment_length) - 2))?;
    }
}

fn read_jpeg_marker(reader: &mut BufReader<File>) -> Result<Option<u8>, std::io::Error> {
    let mut byte = [0_u8; 1];
    loop {
        if reader.read(&mut byte)? == 0 {
            return Ok(None);
        }
        if byte[0] != 0xff {
            continue;
        }
        loop {
            if reader.read(&mut byte)? == 0 {
                return Ok(None);
            }
            match byte[0] {
                0xff => continue,
                0x00 => break,
                marker => return Ok(Some(marker)),
            }
        }
    }
}

fn is_jpeg_start_of_frame(marker: u8) -> bool {
    matches!(
        marker,
        0xc0 | 0xc1 | 0xc2 | 0xc3 | 0xc5 | 0xc6 | 0xc7 | 0xc9 | 0xca | 0xcb | 0xcd | 0xce | 0xcf
    )
}

enum JpegDeclaredColor {
    Srgb,
    Other { identity: String },
}

fn jpeg_declared_color(path: &str) -> Result<Option<JpegDeclaredColor>, std::io::Error> {
    let mut reader = BufReader::new(open_regular(path)?);
    let mut signature = [0_u8; 2];
    reader.read_exact(&mut signature)?;
    if signature != [0xff, 0xd8] {
        return Ok(None);
    }

    loop {
        let Some(marker) = read_jpeg_marker(&mut reader)? else {
            return Ok(None);
        };
        if marker == 0xda || marker == 0xd9 {
            return Ok(None);
        }
        if marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
            continue;
        }
        let mut length = [0_u8; 2];
        reader.read_exact(&mut length)?;
        let payload_length = u16::from_be_bytes(length).checked_sub(2);
        let Some(payload_length) = payload_length else {
            return Ok(None);
        };
        if marker != 0xe1 {
            reader.seek(SeekFrom::Current(i64::from(payload_length)))?;
            continue;
        }

        let mut payload = vec![0_u8; usize::from(payload_length)];
        reader.read_exact(&mut payload)?;
        let Some(tiff) = payload.strip_prefix(b"Exif\0\0") else {
            continue;
        };
        return Ok(Some(match exif_color_space(tiff) {
            Ok(Some(1)) => JpegDeclaredColor::Srgb,
            Ok(None) => return Ok(None),
            Ok(Some(value)) => JpegDeclaredColor::Other {
                identity: format!("exif-color-space:{value}"),
            },
            Err(()) => JpegDeclaredColor::Other {
                identity: format!("malformed-exif:sha256:{:x}", Sha256::digest(&payload)),
            },
        }));
    }
}

#[derive(Clone, Copy)]
enum TiffByteOrder {
    Little,
    Big,
}

fn exif_color_space(tiff: &[u8]) -> Result<Option<u16>, ()> {
    let order = match tiff.get(..2) {
        Some(b"II") => TiffByteOrder::Little,
        Some(b"MM") => TiffByteOrder::Big,
        _ => return Err(()),
    };
    if read_tiff_u16(tiff, 2, order)? != 42 {
        return Err(());
    }
    let ifd0 = usize::try_from(read_tiff_u32(tiff, 4, order)?).map_err(|_| ())?;
    let Some(exif_pointer) = find_tiff_ifd_value(tiff, ifd0, 0x8769, 4, order)? else {
        return Ok(None);
    };
    let exif_ifd = usize::try_from(exif_pointer).map_err(|_| ())?;
    let Some(color_space) = find_tiff_ifd_value(tiff, exif_ifd, 0xa001, 3, order)? else {
        return Ok(None);
    };
    u16::try_from(color_space).map(Some).map_err(|_| ())
}

fn find_tiff_ifd_value(
    tiff: &[u8],
    ifd_offset: usize,
    wanted_tag: u16,
    wanted_type: u16,
    order: TiffByteOrder,
) -> Result<Option<u32>, ()> {
    let count = usize::from(read_tiff_u16(tiff, ifd_offset, order)?);
    let entries = ifd_offset.checked_add(2).ok_or(())?;
    let entries_length = count.checked_mul(12).ok_or(())?;
    let entries_end = entries.checked_add(entries_length).ok_or(())?;
    if entries_end.checked_add(4).ok_or(())? > tiff.len() {
        return Err(());
    }
    for index in 0..count {
        let entry = entries
            .checked_add(index.checked_mul(12).ok_or(())?)
            .ok_or(())?;
        if read_tiff_u16(tiff, entry, order)? != wanted_tag {
            continue;
        }
        if read_tiff_u16(tiff, entry + 2, order)? != wanted_type
            || read_tiff_u32(tiff, entry + 4, order)? != 1
        {
            return Err(());
        }
        return match wanted_type {
            3 => read_tiff_u16(tiff, entry + 8, order)
                .map(u32::from)
                .map(Some),
            4 => read_tiff_u32(tiff, entry + 8, order).map(Some),
            _ => Err(()),
        };
    }
    Ok(None)
}

fn read_tiff_u16(tiff: &[u8], offset: usize, order: TiffByteOrder) -> Result<u16, ()> {
    let bytes = tiff
        .get(offset..offset.checked_add(2).ok_or(())?)
        .ok_or(())?;
    let bytes = [bytes[0], bytes[1]];
    Ok(match order {
        TiffByteOrder::Little => u16::from_le_bytes(bytes),
        TiffByteOrder::Big => u16::from_be_bytes(bytes),
    })
}

fn read_tiff_u32(tiff: &[u8], offset: usize, order: TiffByteOrder) -> Result<u32, ()> {
    let bytes = tiff
        .get(offset..offset.checked_add(4).ok_or(())?)
        .ok_or(())?;
    let bytes = [bytes[0], bytes[1], bytes[2], bytes[3]];
    Ok(match order {
        TiffByteOrder::Little => u32::from_le_bytes(bytes),
        TiffByteOrder::Big => u32::from_be_bytes(bytes),
    })
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

        let probe = probe_image(path).map_err(LibraryError::from)?;

        Ok(vec![crate::plugin::AssetMetadata {
            kind: crate::model::asset::AssetKind::Image,
            duration: None,
            fps: None,
            width: Some(probe.width),
            height: Some(probe.height),
            stream_index: None,
            frame_count: None,
            time_base: None,
            source_color: probe.source_color,
        }])
    }

    fn load(&self, request: &LoadRequest, cache: &CacheManager) -> LoadPluginResult<LoadResponse> {
        if let LoadRequest::Image { path } = request {
            if !Self::claims_path(path) {
                return Err(LoadPluginError::Unsupported);
            }
            for _attempt in 0..3 {
                let identity = FileIdentity::read(path).map_err(LoadPluginError::Failed)?;
                let probe = probe_image(path)
                    .map_err(LibraryError::from)
                    .map_err(LoadPluginError::Failed)?;
                let source_bit_depth = probe.source_color.bit_depth.ok_or_else(|| {
                    LoadPluginError::Failed(LibraryError::Plugin(format!(
                        "Cannot prove encoded channel precision for image {path:?}; refusing a potentially quantizing decode"
                    )))
                })?;
                let target_storage = if source_bit_depth > 8 {
                    DecodedPixelStorage::StraightRgba32F
                } else {
                    DecodedPixelStorage::StraightRgba8
                };
                // This must precede `DynamicImage::from_decoder` and every
                // RGBA conversion. Encoded Gray16 bytes are not an estimate of
                // the much larger typed RGBA32F payload we adopt.
                validate_decoded_pixel_layout(probe.width, probe.height, target_storage)?;
                let source_color = probe.source_color;
                let decoded = match DecodedPixelDescription::assumed_untagged_still_srgb_v1(
                    source_color.clone(),
                ) {
                    Ok(decoded) => decoded,
                    Err(_) if source_color.matrix.is_some() || source_color.range.is_some() => {
                        DecodedPixelDescription::already_full_range_rgb_from_source(source_color)
                    }
                    Err(_) => DecodedPixelDescription::full_range_rgb(
                        DecodedColorSpace::SourceEncoded(source_color),
                    ),
                };
                if source_bit_depth > 8 {
                    if !probe.preserves_high_precision {
                        return Err(LoadPluginError::Failed(LibraryError::Plugin(format!(
                            "Native decoder cannot preserve {source_bit_depth}-bit samples from image {path:?}; refusing to quantize before color conversion"
                        ))));
                    }
                    let pixels = load_high_precision_image(path)
                        .map_err(LibraryError::from)
                        .map_err(LoadPluginError::Failed)?;
                    if FileIdentity::read(path).map_err(LoadPluginError::Failed)? != identity {
                        continue;
                    }
                    return Ok(LoadResponse::new(pixels, decoded));
                }
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
                if FileIdentity::read(path).map_err(LoadPluginError::Failed)? != identity {
                    continue;
                }
                cache.put_image(&cache_key, &image);
                return Ok(LoadResponse::rgba8(image, decoded)?);
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
    use super::{NativeImageLoader, jpeg_source_bit_depth, load_image, uniform_channel_bit_depth};
    use crate::cache::CacheManager;
    use crate::plugin::{
        DecodedColorSpace, DecodedPixelBuffer, LoadPlugin, LoadPluginError, LoadRequest,
        UntaggedSrgbPolicy,
    };
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
    fn untagged_eight_bit_png_retains_the_versioned_srgb_assumption() {
        let path = std::env::temp_dir().join(format!(
            "video-editor-untagged-policy-{}.png",
            Uuid::new_v4()
        ));
        image::save_buffer(&path, &[10, 20, 30, 255], 1, 1, image::ColorType::Rgba8).unwrap();

        let response = NativeImageLoader::new()
            .load(
                &LoadRequest::Image {
                    path: path.to_string_lossy().into_owned(),
                },
                &CacheManager::new(),
            )
            .unwrap();
        let DecodedColorSpace::AssumedSrgb(assumption) = response.decoded().color_space() else {
            panic!("ordinary untagged PNG did not retain assumption provenance");
        };
        assert_eq!(assumption.policy(), UntaggedSrgbPolicy::NativeStillImageV1);
        assert_eq!(assumption.detected_source().bit_depth, Some(8));

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn jpeg_sof_precision_makes_ordinary_jpeg_policy_safe() {
        let path = std::env::temp_dir().join(format!(
            "video-editor-untagged-policy-{}.jpg",
            Uuid::new_v4()
        ));
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
            std::fs::File::create(&path).unwrap(),
            90,
        );
        encoder
            .write_image(&[10, 20, 30], 1, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        assert_eq!(
            jpeg_source_bit_depth(path.to_str().unwrap()).unwrap(),
            Some(8)
        );

        let response = NativeImageLoader::new()
            .load(
                &LoadRequest::Image {
                    path: path.to_string_lossy().into_owned(),
                },
                &CacheManager::new(),
            )
            .unwrap();
        assert!(matches!(
            response.decoded().color_space(),
            DecodedColorSpace::AssumedSrgb(assumption)
                if assumption.policy() == UntaggedSrgbPolicy::NativeStillImageV1
                    && assumption.detected_source().bit_depth == Some(8)
        ));

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
        let loaded = loader
            .load(
                &LoadRequest::Image { path: path_text },
                &CacheManager::new(),
            )?
            .into_rgba8()?;
        assert_eq!(loaded.data, [10, 20, 30, 255]);
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
        assert_eq!(
            first.as_rgba8().map(|image| image.data.as_slice()),
            Some([10, 20, 30, 255].as_slice())
        );

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
        assert_eq!(
            second.as_rgba8().map(|image| (image.width, image.height)),
            Some((2, 1))
        );
        assert_eq!(
            second.as_rgba8().map(|image| image.data.as_slice()),
            Some([40, 50, 60, 255, 70, 80, 90, 255].as_slice())
        );
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
            let loaded = loader
                .load(
                    &LoadRequest::Image { path: path_text },
                    &CacheManager::new(),
                )?
                .into_rgba8()?;
            assert_eq!(loaded.width, 1);
            assert_eq!(loaded.height, 1);
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
        let encoded_samples = [u16::MAX, 32_768, 0]
            .into_iter()
            .flat_map(u16::to_ne_bytes)
            .collect::<Vec<_>>();
        encoder.write_image(&encoded_samples, 1, 1, image::ExtendedColorType::Rgb16)?;

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

        let response = NativeImageLoader::new().load(
            &LoadRequest::Image {
                path: path.to_string_lossy().into_owned(),
            },
            &CacheManager::new(),
        )?;
        assert!(matches!(
            response.decoded().color_space(),
            DecodedColorSpace::SourceEncoded(source)
                if source.bit_depth == Some(16) && source.profile.is_some()
        ));
        let DecodedPixelBuffer::StraightRgba32F(pixels) = response.pixels() else {
            panic!("16-bit ICC source was quantized before color conversion");
        };
        assert_eq!(pixels.data()[0][0], 1.0);
        assert!((pixels.data()[0][1] - (32_768.0 / 65_535.0)).abs() < 1.0e-6);
        assert_eq!(pixels.data()[0][2], 0.0);
        assert_eq!(pixels.data()[0][3], 1.0);

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
