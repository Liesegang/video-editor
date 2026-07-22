//! Bounded raw PNG color-chunk inventory.
//!
//! `png` remains the authority for chunk payload semantics and CRCs, but it
//! deliberately reports several malformed ancillary chunks as skippable. The
//! inventory records declaration presence before decoding so a malformed
//! higher-priority declaration can never disappear into a lower-priority
//! fallback.

use png::Info;
use std::io::{self, Read, Seek, SeekFrom};

const PNG_SIGNATURE: [u8; 8] = *b"\x89PNG\r\n\x1a\n";
const MAX_PNG_CHUNK_HEADERS: usize = 65_536;

#[derive(Default)]
pub(super) struct PngColorChunkInventory {
    cicp: bool,
    iccp: bool,
    srgb: bool,
    chrm: bool,
    gama: bool,
}

impl PngColorChunkInventory {
    pub(super) fn validate_decoded_info(&self, info: &Info<'_>) -> io::Result<()> {
        require_decoded(
            self.cicp,
            info.coding_independent_code_points.is_some(),
            "cICP",
        )?;
        require_decoded(self.iccp, info.icc_profile.is_some(), "iCCP")?;
        require_decoded(self.srgb, info.srgb.is_some(), "sRGB")?;
        require_decoded(self.chrm, info.chrm_chunk.is_some(), "cHRM")?;
        require_decoded(self.gama, info.gama_chunk.is_some(), "gAMA")
    }

    fn record(&mut self, kind: [u8; 4]) -> io::Result<()> {
        let slot = match &kind {
            b"cICP" => &mut self.cicp,
            b"iCCP" => &mut self.iccp,
            b"sRGB" => &mut self.srgb,
            b"cHRM" => &mut self.chrm,
            b"gAMA" => &mut self.gama,
            _ => return Ok(()),
        };
        if *slot {
            return Err(invalid_data(format!(
                "PNG contains duplicate {} color declarations",
                String::from_utf8_lossy(&kind)
            )));
        }
        *slot = true;
        Ok(())
    }
}

pub(super) fn inventory_png_color_chunks<R>(reader: &mut R) -> io::Result<PngColorChunkInventory>
where
    R: Read + Seek,
{
    let origin = reader.stream_position()?;
    let file_end = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(origin))?;

    let mut signature = [0_u8; 8];
    reader.read_exact(&mut signature)?;
    if signature != PNG_SIGNATURE {
        return Err(invalid_data("invalid PNG signature"));
    }

    let mut inventory = PngColorChunkInventory::default();
    let mut saw_palette = false;
    let mut saw_image_data = false;
    let mut saw_end = false;
    for _ in 0..MAX_PNG_CHUNK_HEADERS {
        let mut header = [0_u8; 8];
        reader.read_exact(&mut header)?;
        let length = u64::from(u32::from_be_bytes(
            header[..4]
                .try_into()
                .map_err(|_| invalid_data("invalid PNG chunk length"))?,
        ));
        let kind: [u8; 4] = header[4..]
            .try_into()
            .map_err(|_| invalid_data("invalid PNG chunk type"))?;
        let payload_start = reader.stream_position()?;
        let chunk_end = payload_start
            .checked_add(length)
            .and_then(|end| end.checked_add(4))
            .filter(|end| *end <= file_end)
            .ok_or_else(|| invalid_data("PNG chunk extends beyond the regular file"))?;

        let is_color = matches!(&kind, b"cICP" | b"iCCP" | b"sRGB" | b"cHRM" | b"gAMA");
        if is_color && (saw_palette || saw_image_data) {
            return Err(invalid_data(format!(
                "PNG color chunk {} appears after {}",
                String::from_utf8_lossy(&kind),
                if saw_image_data { "IDAT" } else { "PLTE" }
            )));
        }
        inventory.record(kind)?;

        match &kind {
            b"PLTE" => saw_palette = true,
            b"IDAT" => saw_image_data = true,
            b"IEND" => {
                if length != 0 {
                    return Err(invalid_data("PNG IEND chunk has a non-zero length"));
                }
                saw_end = true;
                break;
            }
            _ => {}
        }
        reader.seek(SeekFrom::Start(chunk_end))?;
    }

    if !saw_end {
        return Err(invalid_data(format!(
            "PNG did not reach IEND within {MAX_PNG_CHUNK_HEADERS} chunk headers"
        )));
    }
    reader.seek(SeekFrom::Start(origin))?;
    Ok(inventory)
}

fn require_decoded(present: bool, decoded: bool, kind: &str) -> io::Result<()> {
    if present && !decoded {
        Err(invalid_data(format!(
            "PNG {kind} declaration was present but invalid; refusing color fallback"
        )))
    } else {
        Ok(())
    }
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
