//! Format-specific source metadata which `image::ImageDecoder` cannot expose.
//!
//! These probes read only fields defined by the encoded format. They must not
//! infer source precision from the decoder's expanded RGBA output: doing so
//! would let a 10/16-bit source be silently quantized before color conversion.

use crate::model::asset::SourceColorProfile;
use crate::util::local_file::DirectRegularFile;
use image::{ExtendedColorType, ImageFormat};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};

const MAX_PROFILE_BYTES: u64 = 16 * 1024 * 1024;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug)]
pub(super) struct FormatSourceProbe {
    pub bit_depth: Option<u8>,
    pub declared_color: Option<DeclaredColor>,
    /// Whether the current native decoder preserves samples above eight bits
    /// when converted to the typed RGBA32F loader payload.
    pub preserves_high_precision: bool,
}

impl FormatSourceProbe {
    fn unverified() -> Self {
        Self {
            bit_depth: None,
            declared_color: None,
            preserves_high_precision: false,
        }
    }
}

#[derive(Debug)]
pub(super) enum DeclaredColor {
    Srgb,
    Profile(SourceColorProfile),
}

pub(super) fn probe_format_source(
    path: &str,
    format: Option<ImageFormat>,
    original_color_type: ExtendedColorType,
) -> io::Result<FormatSourceProbe> {
    match format {
        Some(ImageFormat::Bmp) => probe_bmp(path, 0, true, file_length(path)?),
        Some(ImageFormat::Gif) => probe_gif(path),
        Some(ImageFormat::Tga) => probe_tga(path, original_color_type),
        Some(ImageFormat::Ico) => probe_ico(path),
        Some(ImageFormat::Pnm) => probe_pnm(path),
        _ => Ok(FormatSourceProbe::unverified()),
    }
}

fn file_length(path: &str) -> io::Result<u64> {
    Ok(open_regular(path)?.metadata()?.len())
}

fn open_regular(path: &str) -> io::Result<File> {
    DirectRegularFile::open(path).map(DirectRegularFile::into_file)
}

fn probe_gif(path: &str) -> io::Result<FormatSourceProbe> {
    let mut header = [0_u8; 13];
    open_regular(path)?.read_exact(&mut header)?;
    if !matches!(&header[..6], b"GIF87a" | b"GIF89a") {
        return Ok(FormatSourceProbe::unverified());
    }

    // GIF color-table entries encode every primary as an eight-bit byte. The
    // logical-screen "color resolution" field describes the source device,
    // not the precision of the encoded palette samples.
    Ok(FormatSourceProbe {
        bit_depth: Some(8),
        declared_color: None,
        preserves_high_precision: false,
    })
}

fn probe_tga(path: &str, original_color_type: ExtendedColorType) -> io::Result<FormatSourceProbe> {
    let bit_depth = exact_channel_depth(original_color_type);
    let declared_color = tga_extension_color(path)?;
    Ok(FormatSourceProbe {
        bit_depth,
        declared_color,
        preserves_high_precision: false,
    })
}

fn exact_channel_depth(color: ExtendedColorType) -> Option<u8> {
    use ExtendedColorType as Color;
    match color {
        Color::L1 | Color::La1 | Color::Rgb1 | Color::Rgba1 => Some(1),
        Color::L2 | Color::La2 | Color::Rgb2 | Color::Rgba2 => Some(2),
        Color::L4 | Color::La4 | Color::Rgb4 | Color::Rgba4 => Some(4),
        Color::Rgb5x1 => Some(5),
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
        Color::Unknown(_) => None,
        _ => None,
    }
}

fn tga_extension_color(path: &str) -> io::Result<Option<DeclaredColor>> {
    const FOOTER_LENGTH: u64 = 26;
    const EXTENSION_LENGTH: usize = 495;
    const SIGNATURE: &[u8; 18] = b"TRUEVISION-XFILE.\0";
    let mut file = open_regular(path)?;
    let length = file.metadata()?.len();
    if length < FOOTER_LENGTH {
        return Ok(None);
    }
    file.seek(SeekFrom::End(-(FOOTER_LENGTH as i64)))?;
    let mut footer = [0_u8; FOOTER_LENGTH as usize];
    file.read_exact(&mut footer)?;
    if &footer[8..] != SIGNATURE {
        return Ok(None);
    }
    let extension_offset = u64::from(read_u32_le(&footer, 0).unwrap_or(0));
    if extension_offset == 0
        || extension_offset
            .checked_add(EXTENSION_LENGTH as u64)
            .is_none_or(|end| end > length - FOOTER_LENGTH)
    {
        return Ok(Some(other_profile(
            "tga-extension",
            format!("invalid-offset:{extension_offset}"),
        )));
    }
    file.seek(SeekFrom::Start(extension_offset))?;
    let mut extension = [0_u8; EXTENSION_LENGTH];
    file.read_exact(&mut extension)?;
    if read_u16_le(&extension, 0) != Some(EXTENSION_LENGTH as u16) {
        return Ok(Some(other_profile(
            "tga-extension",
            format!("invalid:sha256:{:x}", Sha256::digest(extension)),
        )));
    }

    let gamma_numerator = read_u16_le(&extension, 478).unwrap_or(0);
    let gamma_denominator = read_u16_le(&extension, 480).unwrap_or(0);
    let color_correction_offset = read_u32_le(&extension, 482).unwrap_or(0);
    if gamma_numerator == 0 && gamma_denominator == 0 && color_correction_offset == 0 {
        return Ok(None);
    }
    Ok(Some(other_profile(
        "tga-extension",
        format!("sha256:{:x}", Sha256::digest(extension)),
    )))
}

fn probe_pnm(path: &str) -> io::Result<FormatSourceProbe> {
    let mut tokens = PnmTokens::new(BufReader::new(open_regular(path)?));
    let magic = tokens
        .next_token()?
        .ok_or_else(|| invalid_data("PNM header is empty"))?;
    let bit_depth = match magic.as_slice() {
        b"P1" | b"P4" => Some(1),
        b"P2" | b"P3" | b"P5" | b"P6" => {
            let _width = tokens.required_u32("width")?;
            let _height = tokens.required_u32("height")?;
            pnm_maxval_depth(tokens.required_u32("MAXVAL")?)
        }
        b"P7" => {
            let mut maxval = None;
            loop {
                let key = tokens
                    .next_token()?
                    .ok_or_else(|| invalid_data("PAM header is missing ENDHDR"))?;
                if key == b"ENDHDR" {
                    break;
                }
                let value = tokens
                    .next_token()?
                    .ok_or_else(|| invalid_data("PAM header field has no value"))?;
                if key == b"MAXVAL" {
                    maxval = Some(parse_u32(&value, "MAXVAL")?);
                }
            }
            maxval.and_then(pnm_maxval_depth)
        }
        _ => None,
    };
    Ok(FormatSourceProbe {
        bit_depth,
        declared_color: None,
        preserves_high_precision: true,
    })
}

fn pnm_maxval_depth(maxval: u32) -> Option<u8> {
    (1..=u32::from(u16::MAX))
        .contains(&maxval)
        .then_some(u32::BITS - maxval.leading_zeros())
        .and_then(|depth| u8::try_from(depth).ok())
}

struct PnmTokens<R> {
    reader: R,
    consumed: usize,
}

impl<R: BufRead> PnmTokens<R> {
    const MAX_HEADER_BYTES: usize = 64 * 1024;

    fn new(reader: R) -> Self {
        Self {
            reader,
            consumed: 0,
        }
    }

    fn next_token(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut token = Vec::new();
        let mut in_comment = false;
        loop {
            let buffer = self.reader.fill_buf()?;
            let Some(&byte) = buffer.first() else {
                return Ok((!token.is_empty()).then_some(token));
            };
            self.reader.consume(1);
            self.consumed += 1;
            if self.consumed > Self::MAX_HEADER_BYTES {
                return Err(invalid_data("PNM header exceeds safety limit"));
            }
            if in_comment {
                if matches!(byte, b'\n' | b'\r') {
                    in_comment = false;
                }
                continue;
            }
            if byte == b'#' && token.is_empty() {
                in_comment = true;
                continue;
            }
            if byte.is_ascii_whitespace() {
                if token.is_empty() {
                    continue;
                }
                return Ok(Some(token));
            }
            token.push(byte);
            if token.len() > 256 {
                return Err(invalid_data("PNM header token exceeds safety limit"));
            }
        }
    }

    fn required_u32(&mut self, name: &str) -> io::Result<u32> {
        let token = self
            .next_token()?
            .ok_or_else(|| invalid_data(format!("PNM header is missing {name}")))?;
        parse_u32(&token, name)
    }
}

fn parse_u32(bytes: &[u8], name: &str) -> io::Result<u32> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| invalid_data(format!("invalid PNM {name}")))
}

fn probe_bmp(
    path: &str,
    offset: u64,
    has_file_header: bool,
    region_length: u64,
) -> io::Result<FormatSourceProbe> {
    let mut file = open_regular(path)?;
    let file_length = file.metadata()?.len();
    probe_bmp_reader(
        &mut file,
        offset,
        has_file_header,
        region_length,
        file_length,
    )
}

fn probe_bmp_reader<R: Read + Seek>(
    reader: &mut R,
    offset: u64,
    has_file_header: bool,
    region_length: u64,
    file_length: u64,
) -> io::Result<FormatSourceProbe> {
    let dib_offset = if has_file_header {
        let header = read_region(reader, offset, 14, file_length)?;
        if header.get(..2) != Some(b"BM") {
            return Ok(FormatSourceProbe::unverified());
        }
        offset
            .checked_add(14)
            .ok_or_else(|| invalid_data("BMP offset overflow"))?
    } else {
        offset
    };
    let dib_size_bytes = read_region(reader, dib_offset, 4, file_length)?;
    let dib_size = u64::from(read_u32_le(&dib_size_bytes, 0).unwrap_or(0));
    if !matches!(dib_size, 12 | 40 | 52 | 56 | 108 | 124)
        || dib_size > region_length.saturating_sub(dib_offset.saturating_sub(offset))
    {
        return Ok(FormatSourceProbe::unverified());
    }
    let dib = read_region(
        reader,
        dib_offset,
        usize::try_from(dib_size).map_err(|_| invalid_data("BMP header is too large"))?,
        file_length,
    )?;
    let (bit_count, compression) = if dib_size == 12 {
        (read_u16_le(&dib, 10), Some(0))
    } else {
        (read_u16_le(&dib, 14), read_u32_le(&dib, 16))
    };
    let bit_depth = match (bit_count, compression) {
        (Some(1 | 2 | 4 | 8), Some(0..=2)) => Some(8),
        (Some(16), Some(0)) => Some(5),
        (Some(24 | 32), Some(0)) => Some(8),
        (Some(16 | 32), Some(3)) => bmp_mask_depth(reader, dib_offset, &dib, file_length),
        _ => None,
    };
    let declared_color = bmp_declared_color(reader, dib_offset, &dib, file_length)?;
    Ok(FormatSourceProbe {
        bit_depth,
        declared_color,
        // The image BMP decoder exposes only RGB(A)8, including for 10-bit
        // bitfields. Never route such input through `to_rgba32f` after loss.
        preserves_high_precision: false,
    })
}

fn bmp_mask_depth<R: Read + Seek>(
    reader: &mut R,
    dib_offset: u64,
    dib: &[u8],
    file_length: u64,
) -> Option<u8> {
    let masks = if dib.len() >= 52 {
        dib.get(40..56.min(dib.len()))?.to_vec()
    } else {
        read_region(reader, dib_offset.checked_add(40)?, 12, file_length).ok()?
    };
    let mut occupied = 0_u32;
    let mut maximum = 0_u8;
    for index in 0..3 {
        let mask = read_u32_le(&masks, index * 4)?;
        let width = contiguous_mask_width(mask)?;
        if mask & occupied != 0 {
            return None;
        }
        occupied |= mask;
        maximum = maximum.max(width);
    }
    if masks.len() >= 16 {
        let alpha = read_u32_le(&masks, 12)?;
        if alpha != 0 {
            let width = contiguous_mask_width(alpha)?;
            if alpha & occupied != 0 {
                return None;
            }
            maximum = maximum.max(width);
        }
    }
    Some(maximum)
}

fn contiguous_mask_width(mask: u32) -> Option<u8> {
    if mask == 0 {
        return None;
    }
    let shifted = mask >> mask.trailing_zeros();
    ((shifted & shifted.wrapping_add(1)) == 0)
        .then_some(shifted.count_ones())
        .and_then(|width| u8::try_from(width).ok())
}

fn bmp_declared_color<R: Read + Seek>(
    reader: &mut R,
    dib_offset: u64,
    dib: &[u8],
    file_length: u64,
) -> io::Result<Option<DeclaredColor>> {
    if dib.len() < 108 {
        return Ok(None);
    }
    let color_space = read_u32_le(dib, 56).unwrap_or(u32::MAX);
    const LCS_CALIBRATED_RGB: u32 = 0;
    const LCS_SRGB: u32 = 0x7352_4742;
    const PROFILE_LINKED: u32 = 0x4c49_4e4b;
    const PROFILE_EMBEDDED: u32 = 0x4d42_4544;
    match color_space {
        LCS_SRGB => Ok(Some(DeclaredColor::Srgb)),
        LCS_CALIBRATED_RGB => Ok(Some(other_profile(
            "bmp-calibrated-rgb",
            format!("sha256:{:x}", Sha256::digest(&dib[56..108])),
        ))),
        PROFILE_LINKED | PROFILE_EMBEDDED if dib.len() >= 124 => {
            let profile_offset = u64::from(read_u32_le(dib, 112).unwrap_or(0));
            let profile_size = u64::from(read_u32_le(dib, 116).unwrap_or(0));
            let Some(absolute) = dib_offset.checked_add(profile_offset) else {
                return Ok(Some(other_profile(
                    "bmp-color-profile",
                    format!("invalid:{color_space:08x}:{profile_offset}:{profile_size}"),
                )));
            };
            let valid = profile_size > 0
                && profile_size <= MAX_PROFILE_BYTES
                && absolute
                    .checked_add(profile_size)
                    .is_some_and(|end| end <= file_length);
            if !valid {
                return Ok(Some(other_profile(
                    "bmp-color-profile",
                    format!("invalid:{color_space:08x}:{profile_offset}:{profile_size}"),
                )));
            }
            let profile = read_region(
                reader,
                absolute,
                usize::try_from(profile_size)
                    .map_err(|_| invalid_data("BMP profile size does not fit memory"))?,
                file_length,
            )?;
            if color_space == PROFILE_EMBEDDED {
                Ok(Some(DeclaredColor::Profile(icc_profile(&profile))))
            } else {
                Ok(Some(other_profile(
                    "bmp-linked-profile",
                    format!("sha256:{:x}", Sha256::digest(profile)),
                )))
            }
        }
        other => Ok(Some(other_profile(
            "bmp-color-space",
            format!("{other:08x}:sha256:{:x}", Sha256::digest(&dib[56..108])),
        ))),
    }
}

fn probe_ico(path: &str) -> io::Result<FormatSourceProbe> {
    let mut file = open_regular(path)?;
    let file_length = file.metadata()?.len();
    let header = read_region(&mut file, 0, 6, file_length)?;
    if read_u16_le(&header, 0) != Some(0) || read_u16_le(&header, 2) != Some(1) {
        return Ok(FormatSourceProbe::unverified());
    }
    let count = usize::from(read_u16_le(&header, 4).unwrap_or(0));
    if count == 0 {
        return Ok(FormatSourceProbe::unverified());
    }
    let mut best: Option<IcoEntry> = None;
    for index in 0..count {
        let offset = 6_u64
            .checked_add(u64::try_from(index).unwrap_or(u64::MAX).saturating_mul(16))
            .ok_or_else(|| invalid_data("ICO directory offset overflow"))?;
        let entry = read_region(&mut file, offset, 16, file_length)?;
        let candidate = IcoEntry {
            width: match entry[0] {
                0 => 256,
                value => u16::from(value),
            },
            height: match entry[1] {
                0 => 256,
                value => u16::from(value),
            },
            bits_per_pixel: read_u16_le(&entry, 6).unwrap_or(0),
            length: u64::from(read_u32_le(&entry, 8).unwrap_or(0)),
            offset: u64::from(read_u32_le(&entry, 12).unwrap_or(0)),
        };
        // `image` initializes its winner from the last directory entry and
        // replaces it only for a strictly higher score. Iterating forward and
        // replacing on equality is the equivalent tie rule: the last equal
        // entry owns the pixels and therefore must also own color metadata.
        if best.is_none_or(|current| candidate.score() >= current.score()) {
            best = Some(candidate);
        }
    }
    let Some(best) = best else {
        return Ok(FormatSourceProbe::unverified());
    };
    let end = best.offset.checked_add(best.length);
    if best.length < 4 || end.is_none_or(|end| end > file_length) {
        return Ok(FormatSourceProbe::unverified());
    }
    let signature = read_region(&mut file, best.offset, 8, file_length)?;
    if signature.as_slice() == PNG_SIGNATURE {
        probe_embedded_png(&mut file, best.offset, best.length, file_length)
    } else {
        probe_bmp_reader(&mut file, best.offset, false, best.length, file_length)
    }
}

#[derive(Clone, Copy)]
struct IcoEntry {
    width: u16,
    height: u16,
    bits_per_pixel: u16,
    length: u64,
    offset: u64,
}

impl IcoEntry {
    fn score(self) -> (u16, u32) {
        (
            self.bits_per_pixel,
            u32::from(self.width) * u32::from(self.height),
        )
    }
}

fn probe_embedded_png<R: Read + Seek>(
    reader: &mut R,
    offset: u64,
    length: u64,
    file_length: u64,
) -> io::Result<FormatSourceProbe> {
    if length < 33 {
        return Ok(FormatSourceProbe::unverified());
    }
    let ihdr = read_region(reader, offset, 33, file_length)?;
    if ihdr.as_slice().get(..8) != Some(PNG_SIGNATURE)
        || read_u32_be(&ihdr, 8) != Some(13)
        || ihdr.get(12..16) != Some(b"IHDR")
    {
        return Ok(FormatSourceProbe::unverified());
    }
    let depth = ihdr[24];
    let color_type = ihdr[25];
    let valid = match color_type {
        0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
        2 | 4 | 6 => matches!(depth, 8 | 16),
        3 => matches!(depth, 1 | 2 | 4 | 8),
        _ => false,
    };
    let mut cursor = 8_u64;
    let mut declaration = None;
    while cursor.saturating_add(12) <= length {
        let header = read_region(reader, offset + cursor, 8, file_length)?;
        let chunk_length = u64::from(read_u32_be(&header, 0).unwrap_or(u32::MAX));
        let kind = &header[4..8];
        let Some(chunk_total) = chunk_length.checked_add(12) else {
            declaration = Some(other_profile(
                "ico-png-color-chunk",
                format!("invalid:{}:{chunk_length}", String::from_utf8_lossy(kind)),
            ));
            break;
        };
        if cursor.saturating_add(chunk_total) > length {
            declaration = Some(other_profile(
                "ico-png-color-chunk",
                format!("invalid:{}:{chunk_length}", String::from_utf8_lossy(kind)),
            ));
            break;
        }
        if matches!(kind, b"IDAT" | b"IEND") {
            break;
        }
        if kind == b"sRGB" && declaration.is_none() {
            declaration = Some(DeclaredColor::Srgb);
        } else if matches!(kind, b"iCCP" | b"cICP" | b"cHRM" | b"gAMA") {
            if chunk_length > MAX_PROFILE_BYTES {
                declaration = Some(other_profile(
                    "ico-png-color-chunk",
                    format!("oversize:{}:{chunk_length}", String::from_utf8_lossy(kind)),
                ));
                break;
            }
            let payload = read_region(
                reader,
                offset + cursor + 8,
                usize::try_from(chunk_length)
                    .map_err(|_| invalid_data("ICO PNG color chunk does not fit memory"))?,
                file_length,
            )?;
            declaration = Some(other_profile(
                "ico-png-color-chunk",
                format!(
                    "{}:sha256:{:x}",
                    String::from_utf8_lossy(kind),
                    Sha256::digest(payload)
                ),
            ));
        }
        cursor += chunk_total;
    }
    Ok(FormatSourceProbe {
        bit_depth: valid.then_some(depth),
        declared_color: declaration,
        // ICO's PNG decoder intentionally exposes only RGBA8.
        preserves_high_precision: false,
    })
}

fn read_region<R: Read + Seek>(
    reader: &mut R,
    offset: u64,
    length: usize,
    file_length: u64,
) -> io::Result<Vec<u8>> {
    let end = offset
        .checked_add(u64::try_from(length).map_err(|_| invalid_data("read length overflow"))?)
        .ok_or_else(|| invalid_data("read range overflow"))?;
    if end > file_length {
        return Err(invalid_data("encoded header points outside the file"));
    }
    reader.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0_u8; length];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let bytes: [u8; 2] = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_le_bytes(bytes))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn read_u32_be(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

fn icc_profile(profile: &[u8]) -> SourceColorProfile {
    let profile_id = if profile.len() >= 128
        && profile.get(36..40) == Some(b"acsp")
        && profile[84..100].iter().any(|byte| *byte != 0)
    {
        Some(
            profile[84..100]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        )
    } else {
        None
    };
    SourceColorProfile::Icc {
        sha256: format!("{:x}", Sha256::digest(profile)),
        byte_length: u64::try_from(profile.len()).unwrap_or(u64::MAX),
        profile_id,
    }
}

fn other_profile(kind: &str, identity: String) -> DeclaredColor {
    DeclaredColor::Profile(SourceColorProfile::Other {
        profile_kind: kind.to_string(),
        identity,
    })
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::{contiguous_mask_width, pnm_maxval_depth};
    use crate::cache::CacheManager;
    use crate::core::rendering::managed_color_backend::{
        ManagedRenderDestination, ProjectColorPipeline,
    };
    use crate::core::rendering::managed_color_source::ingest_loaded_media;
    use crate::core::rendering::media_color_ingress::MediaAssetKind;
    use crate::model::asset::{Asset, AssetKind, SourceColorProfile};
    use crate::model::authoring::AuthoringProject;
    use crate::model::frame::entity::ImageSurface;
    use crate::model::frame::transform::Transform;
    use crate::plugin::{
        DecodedColorSpace, DecodedPixelBuffer, LoadPlugin, LoadRequest, NativeImageLoader,
        UntaggedSrgbPolicy,
    };
    use image::{ColorType, ImageFormat};
    use uuid::Uuid;

    #[test]
    fn pnm_maxval_is_converted_to_exact_encoded_precision() {
        assert_eq!(pnm_maxval_depth(1), Some(1));
        assert_eq!(pnm_maxval_depth(15), Some(4));
        assert_eq!(pnm_maxval_depth(255), Some(8));
        assert_eq!(pnm_maxval_depth(1023), Some(10));
        assert_eq!(pnm_maxval_depth(65_535), Some(16));
        assert_eq!(pnm_maxval_depth(0), None);
    }

    #[test]
    fn bmp_bitfield_width_requires_a_contiguous_mask() {
        assert_eq!(contiguous_mask_width(0x3ff0_0000), Some(10));
        assert_eq!(contiguous_mask_width(0x00ff_0000), Some(8));
        assert_eq!(contiguous_mask_width(0x00f5_0000), None);
        assert_eq!(contiguous_mask_width(0), None);
    }

    #[test]
    fn ordinary_bmp_gif_tga_ico_and_pnm_reach_production_ingress()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixtures = [
            bmp_fixture()?,
            ordinary_fixture("gif", ImageFormat::Gif)?,
            ordinary_fixture("tga", ImageFormat::Tga)?,
            ordinary_fixture("ico", ImageFormat::Ico)?,
            pnm_fixture()?,
        ];
        let loader = NativeImageLoader::new();
        for path in &fixtures {
            let path_text = path.to_string_lossy().into_owned();
            let metadata = loader.open(&path_text)?;
            assert_eq!(metadata[0].source_color.bit_depth, Some(8), "{path_text}");
            let response = loader.load(
                &LoadRequest::Image {
                    path: path_text.clone(),
                },
                &CacheManager::new(),
            )?;
            assert!(
                matches!(
                    response.decoded().color_space(),
                    DecodedColorSpace::AssumedSrgb(assumption)
                        if assumption.policy() == UntaggedSrgbPolicy::NativeStillImageV1
                            && assumption.detected_source().bit_depth == Some(8)
                ),
                "{path_text} did not retain versioned assumption provenance"
            );
            assert_production_ingress(path_text, metadata[0].source_color.clone(), response)?;
        }
        for path in fixtures {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    #[test]
    fn high_precision_pnm_is_float_and_never_gets_the_srgb_assumption()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = temporary_path("high-precision", "ppm");
        std::fs::write(&path, b"P6\n1 1\n65535\n\xff\xff\x80\x00\x00\x00")?;
        let path_text = path.to_string_lossy().into_owned();
        let loader = NativeImageLoader::new();
        let metadata = loader.open(&path_text)?;
        assert_eq!(metadata[0].source_color.bit_depth, Some(16));
        let response = loader.load(
            &LoadRequest::Image { path: path_text },
            &CacheManager::new(),
        )?;
        assert!(matches!(
            response.decoded().color_space(),
            DecodedColorSpace::SourceEncoded(source) if source.bit_depth == Some(16)
        ));
        let DecodedPixelBuffer::StraightRgba32F(pixels) = response.pixels() else {
            panic!("16-bit PNM was quantized before managed color ingress");
        };
        assert_eq!(pixels.data()[0][0], 1.0);
        assert!((pixels.data()[0][1] - (32_768.0 / 65_535.0)).abs() < 1.0e-6);
        assert_eq!(pixels.data()[0][2], 0.0);
        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn ten_bit_bmp_fails_before_the_rgba8_decoder_can_quantize_it()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = temporary_path("ten-bit-bitfields", "bmp");
        std::fs::write(&path, ten_bit_bmp())?;
        let path_text = path.to_string_lossy().into_owned();
        let loader = NativeImageLoader::new();
        let metadata = loader.open(&path_text)?;
        assert_eq!(metadata[0].source_color.bit_depth, Some(10));
        let error = loader
            .load(
                &LoadRequest::Image { path: path_text },
                &CacheManager::new(),
            )
            .expect_err("BMP decoder's RGBA8 expansion must not hide 10-bit precision");
        let message = error.to_string();
        assert!(message.contains("cannot preserve 10-bit"), "{message}");
        assert!(message.contains("refusing to quantize"), "{message}");
        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn tga_gamma_metadata_prevents_the_untagged_srgb_assumption()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = ordinary_fixture("tagged-tga", ImageFormat::Tga)?;
        let mut bytes = std::fs::read(&path)?;
        let extension_offset = u32::try_from(bytes.len())?;
        let mut extension = vec![0_u8; 495];
        extension[0..2].copy_from_slice(&495_u16.to_le_bytes());
        extension[478..480].copy_from_slice(&22_u16.to_le_bytes());
        extension[480..482].copy_from_slice(&10_u16.to_le_bytes());
        bytes.extend_from_slice(&extension);
        bytes.extend_from_slice(&extension_offset.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(b"TRUEVISION-XFILE.\0");
        std::fs::write(&path, bytes)?;

        let path_text = path.to_string_lossy().into_owned();
        let loader = NativeImageLoader::new();
        let metadata = loader.open(&path_text)?;
        assert!(matches!(
            metadata[0].source_color.profile,
            Some(SourceColorProfile::Other { ref profile_kind, .. })
                if profile_kind == "tga-extension"
        ));
        let response = loader.load(
            &LoadRequest::Image {
                path: path_text.clone(),
            },
            &CacheManager::new(),
        )?;
        assert!(matches!(
            response.decoded().color_space(),
            DecodedColorSpace::SourceEncoded(source) if source.profile.is_some()
        ));

        let mut asset = Asset::new("tagged", &path_text, AssetKind::Image);
        asset
            .source_color
            .replace_detected(metadata[0].source_color.clone());
        let mut project = AuthoringProject::new("tagged TGA", 1, 1, 24.0, 1.0).unwrap();
        project.assets.push(asset.clone());
        let pipeline =
            ProjectColorPipeline::for_project(&project, ManagedRenderDestination::Preview)?;
        let error = ingest_loaded_media(
            &project,
            &pipeline,
            &surface(&asset),
            MediaAssetKind::Image,
            response,
        )
        .expect_err("an exact embedded profile requires a config-owned transform");
        assert!(error.to_string().contains("embedded profile"));
        std::fs::remove_file(path)?;
        Ok(())
    }

    fn ordinary_fixture(
        label: &str,
        format: ImageFormat,
    ) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
        let path = temporary_path(label, format.extensions_str()[0]);
        image::save_buffer_with_format(&path, &[17, 99, 201, 255], 1, 1, ColorType::Rgba8, format)?;
        Ok(path)
    }

    fn pnm_fixture() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
        let path = temporary_path("ordinary", "ppm");
        std::fs::write(&path, b"P6\n1 1\n255\n\x11\x63\xc9")?;
        Ok(path)
    }

    fn bmp_fixture() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
        let path = temporary_path("ordinary", "bmp");
        let mut bytes = Vec::with_capacity(58);
        bytes.extend_from_slice(b"BM");
        bytes.extend_from_slice(&58_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&54_u32.to_le_bytes());
        bytes.extend_from_slice(&40_u32.to_le_bytes());
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&24_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        bytes.extend_from_slice(&0_i32.to_le_bytes());
        bytes.extend_from_slice(&0_i32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&[201, 99, 17, 0]);
        std::fs::write(&path, bytes)?;
        Ok(path)
    }

    fn temporary_path(label: &str, extension: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "video-editor-{label}-{}.{}",
            Uuid::new_v4(),
            extension
        ))
    }

    fn assert_production_ingress(
        path: String,
        detected: crate::model::asset::SourceColorDescription,
        response: crate::plugin::LoadResponse,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut project = AuthoringProject::new("ordinary still ingress", 1, 1, 24.0, 1.0).unwrap();
        let mut asset = Asset::new("ordinary", &path, AssetKind::Image);
        asset.source_color.replace_detected(detected);
        project.assets.push(asset.clone());
        let pipeline =
            ProjectColorPipeline::for_project(&project, ManagedRenderDestination::Preview)?;
        let working = ingest_loaded_media(
            &project,
            &pipeline,
            &surface(&asset),
            MediaAssetKind::Image,
            response,
        )?;
        assert_eq!(
            (working.pixels().width(), working.pixels().height()),
            (1, 1)
        );
        Ok(())
    }

    fn surface(asset: &Asset) -> ImageSurface {
        ImageSurface {
            asset_id: Some(asset.id),
            file_path: asset.path.clone(),
            effects: Vec::new(),
            input_color_space: None,
            output_color_space: None,
            transform: Transform::default(),
        }
    }

    fn ten_bit_bmp() -> Vec<u8> {
        let mut bytes = Vec::with_capacity(70);
        bytes.extend_from_slice(b"BM");
        bytes.extend_from_slice(&70_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&66_u32.to_le_bytes());
        bytes.extend_from_slice(&40_u32.to_le_bytes());
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&32_u16.to_le_bytes());
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        bytes.extend_from_slice(&0_i32.to_le_bytes());
        bytes.extend_from_slice(&0_i32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0x3ff0_0000_u32.to_le_bytes());
        bytes.extend_from_slice(&0x000f_fc00_u32.to_le_bytes());
        bytes.extend_from_slice(&0x0000_03ff_u32.to_le_bytes());
        bytes.extend_from_slice(&0x3ff8_0000_u32.to_le_bytes());
        bytes
    }
}
