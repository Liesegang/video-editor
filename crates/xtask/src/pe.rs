use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::{TaskError, TaskResult, io_error};

#[derive(Debug)]
pub(crate) struct PeFile {
    pub(crate) machine: u16,
    pub(crate) imports: Vec<String>,
}

struct Section {
    virtual_address: u32,
    virtual_size: u32,
    raw_offset: u32,
    raw_size: u32,
}

pub(crate) fn parse_file(path: &Path) -> TaskResult<PeFile> {
    let bytes = fs::read(path).map_err(|error| io_error("read PE file", path, error))?;
    parse(&bytes).map_err(|error| {
        TaskError::new(format!(
            "cannot inspect PE imports for {}: {error}",
            path.display()
        ))
    })
}

fn parse(bytes: &[u8]) -> TaskResult<PeFile> {
    if bytes.get(0..2) != Some(b"MZ") {
        return Err(TaskError::new("missing DOS MZ signature"));
    }
    let pe_offset = to_usize(read_u32(bytes, 0x3c)?)?;
    if bytes.get(pe_offset..add(pe_offset, 4)?) != Some(b"PE\0\0") {
        return Err(TaskError::new("missing PE signature"));
    }
    let coff = add(pe_offset, 4)?;
    let machine = read_u16(bytes, coff)?;
    let section_count = usize::from(read_u16(bytes, add(coff, 2)?)?);
    let optional_size = usize::from(read_u16(bytes, add(coff, 16)?)?);
    let optional = add(coff, 20)?;
    let optional_end = add(optional, optional_size)?;
    range(bytes, optional, optional_size)?;
    let magic = read_u16(bytes, optional)?;
    let (directories, directory_count_offset, image_base) = match magic {
        0x10b => (
            add(optional, 96)?,
            add(optional, 92)?,
            u64::from(read_u32(bytes, add(optional, 28)?)?),
        ),
        0x20b => (
            add(optional, 112)?,
            add(optional, 108)?,
            read_u64(bytes, add(optional, 24)?)?,
        ),
        other => {
            return Err(TaskError::new(format!(
                "unsupported PE optional magic {other:#x}"
            )));
        }
    };
    let directory_count = read_u32(bytes, directory_count_offset)?;
    let size_of_headers = read_u32(bytes, add(optional, 60)?)?;
    let mut sections = Vec::with_capacity(section_count);
    for index in 0..section_count {
        let offset = add(optional_end, multiply(index, 40)?)?;
        range(bytes, offset, 40)?;
        sections.push(Section {
            virtual_size: read_u32(bytes, add(offset, 8)?)?,
            virtual_address: read_u32(bytes, add(offset, 12)?)?,
            raw_size: read_u32(bytes, add(offset, 16)?)?,
            raw_offset: read_u32(bytes, add(offset, 20)?)?,
        });
    }

    let mut imports = BTreeSet::new();
    if directory_count > 1 {
        let entry = add(directories, 8)?;
        parse_descriptors(
            bytes,
            read_u32(bytes, entry)?,
            read_u32(bytes, add(entry, 4)?)?,
            &sections,
            size_of_headers,
            &mut imports,
        )?;
    }
    if directory_count > 13 {
        let entry = add(directories, multiply(13, 8)?)?;
        parse_delay_descriptors(
            bytes,
            read_u32(bytes, entry)?,
            read_u32(bytes, add(entry, 4)?)?,
            image_base,
            &sections,
            size_of_headers,
            &mut imports,
        )?;
    }
    Ok(PeFile {
        machine,
        imports: imports.into_iter().collect(),
    })
}

fn parse_descriptors(
    bytes: &[u8],
    rva: u32,
    size: u32,
    sections: &[Section],
    headers: u32,
    imports: &mut BTreeSet<String>,
) -> TaskResult<()> {
    if rva == 0 || size == 0 {
        return Ok(());
    }
    let table = rva_offset(bytes, rva, sections, headers)?;
    let count = (to_usize(size)? / 20).min(4096);
    for index in 0..count {
        let offset = add(table, multiply(index, 20)?)?;
        let descriptor = range(bytes, offset, 20)?;
        if descriptor.iter().all(|byte| *byte == 0) {
            return Ok(());
        }
        let name = read_u32(bytes, add(offset, 12)?)?;
        imports.insert(dll_name(bytes, name, sections, headers)?);
    }
    Err(TaskError::new("unterminated PE import descriptor table"))
}

#[allow(
    clippy::too_many_arguments,
    reason = "a delay descriptor needs both image and section mapping state"
)]
fn parse_delay_descriptors(
    bytes: &[u8],
    rva: u32,
    size: u32,
    image_base: u64,
    sections: &[Section],
    headers: u32,
    imports: &mut BTreeSet<String>,
) -> TaskResult<()> {
    if rva == 0 || size == 0 {
        return Ok(());
    }
    let table = rva_offset(bytes, rva, sections, headers)?;
    let count = (to_usize(size)? / 32).min(4096);
    for index in 0..count {
        let offset = add(table, multiply(index, 32)?)?;
        let descriptor = range(bytes, offset, 32)?;
        if descriptor.iter().all(|byte| *byte == 0) {
            return Ok(());
        }
        let attributes = read_u32(bytes, offset)?;
        let raw_name = u64::from(read_u32(bytes, add(offset, 4)?)?);
        let name_rva = if attributes & 1 == 1 {
            u32::try_from(raw_name)
                .map_err(|error| TaskError::new(format!("invalid delay import RVA: {error}")))?
        } else {
            let relative = raw_name
                .checked_sub(image_base)
                .ok_or_else(|| TaskError::new("delay import VA precedes image base"))?;
            u32::try_from(relative).map_err(|error| {
                TaskError::new(format!("delay import RVA is too large: {error}"))
            })?
        };
        imports.insert(dll_name(bytes, name_rva, sections, headers)?);
    }
    Err(TaskError::new(
        "unterminated PE delay import descriptor table",
    ))
}

fn dll_name(bytes: &[u8], rva: u32, sections: &[Section], headers: u32) -> TaskResult<String> {
    let offset = rva_offset(bytes, rva, sections, headers)?;
    let remaining = bytes
        .get(offset..)
        .ok_or_else(|| TaskError::new("DLL name points outside PE file"))?;
    let length = remaining
        .iter()
        .take(260)
        .position(|byte| *byte == 0)
        .ok_or_else(|| TaskError::new("DLL name is not NUL-terminated within 260 bytes"))?;
    let name = remaining
        .get(..length)
        .ok_or_else(|| TaskError::new("invalid DLL name range"))?;
    if name.is_empty()
        || !name.is_ascii()
        || name.iter().any(|byte| *byte == b'/' || *byte == b'\\')
    {
        return Err(TaskError::new(
            "DLL import name is empty, non-ASCII, or contains a path",
        ));
    }
    let name = std::str::from_utf8(name)
        .map_err(|error| TaskError::new(format!("invalid DLL import name: {error}")))?;
    Ok(name.to_ascii_lowercase())
}

fn rva_offset(bytes: &[u8], rva: u32, sections: &[Section], headers: u32) -> TaskResult<usize> {
    if rva < headers {
        let offset = to_usize(rva)?;
        range(bytes, offset, 1)?;
        return Ok(offset);
    }
    for section in sections {
        let span = section.virtual_size.max(section.raw_size);
        let Some(end) = section.virtual_address.checked_add(span) else {
            continue;
        };
        if rva < section.virtual_address || rva >= end {
            continue;
        }
        let relative = rva - section.virtual_address;
        if relative >= section.raw_size {
            return Err(TaskError::new(
                "PE RVA points into an uninitialized section tail",
            ));
        }
        let raw = section
            .raw_offset
            .checked_add(relative)
            .ok_or_else(arithmetic_error)?;
        let offset = to_usize(raw)?;
        range(bytes, offset, 1)?;
        return Ok(offset);
    }
    Err(TaskError::new(format!(
        "PE RVA {rva:#x} does not map to file data"
    )))
}

fn read_u16(bytes: &[u8], offset: usize) -> TaskResult<u16> {
    let data = range(bytes, offset, 2)?;
    let array = <[u8; 2]>::try_from(data)
        .map_err(|error| TaskError::new(format!("cannot decode PE u16: {error}")))?;
    Ok(u16::from_le_bytes(array))
}

fn read_u32(bytes: &[u8], offset: usize) -> TaskResult<u32> {
    let data = range(bytes, offset, 4)?;
    let array = <[u8; 4]>::try_from(data)
        .map_err(|error| TaskError::new(format!("cannot decode PE u32: {error}")))?;
    Ok(u32::from_le_bytes(array))
}

fn read_u64(bytes: &[u8], offset: usize) -> TaskResult<u64> {
    let data = range(bytes, offset, 8)?;
    let array = <[u8; 8]>::try_from(data)
        .map_err(|error| TaskError::new(format!("cannot decode PE u64: {error}")))?;
    Ok(u64::from_le_bytes(array))
}

fn range(bytes: &[u8], offset: usize, length: usize) -> TaskResult<&[u8]> {
    let end = add(offset, length)?;
    bytes
        .get(offset..end)
        .ok_or_else(|| TaskError::new("PE range exceeds file bounds"))
}

fn add(left: usize, right: usize) -> TaskResult<usize> {
    left.checked_add(right).ok_or_else(arithmetic_error)
}

fn multiply(left: usize, right: usize) -> TaskResult<usize> {
    left.checked_mul(right).ok_or_else(arithmetic_error)
}

fn arithmetic_error() -> TaskError {
    TaskError::new("integer overflow while parsing PE file")
}

fn to_usize(value: u32) -> TaskResult<usize> {
    usize::try_from(value)
        .map_err(|error| TaskError::new(format!("PE offset does not fit usize: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_checked_import_table() -> TaskResult<()> {
        let mut pe = vec![0_u8; 0x400];
        write_u16(&mut pe, 0, u16::from_le_bytes(*b"MZ"))?;
        write_u32(&mut pe, 0x3c, 0x80)?;
        pe.get_mut(0x80..0x84)
            .ok_or_else(|| TaskError::new("invalid test signature range"))?
            .copy_from_slice(b"PE\0\0");
        write_u16(&mut pe, 0x84, crate::X86_64_MACHINE)?;
        write_u16(&mut pe, 0x86, 1)?;
        write_u16(&mut pe, 0x94, 0xf0)?;
        let optional = 0x98;
        write_u16(&mut pe, optional, 0x20b)?;
        write_u32(&mut pe, optional + 60, 0x200)?;
        write_u32(&mut pe, optional + 108, 16)?;
        write_u32(&mut pe, optional + 120, 0x1000)?;
        write_u32(&mut pe, optional + 124, 40)?;
        let section = optional + 0xf0;
        write_u32(&mut pe, section + 8, 0x200)?;
        write_u32(&mut pe, section + 12, 0x1000)?;
        write_u32(&mut pe, section + 16, 0x200)?;
        write_u32(&mut pe, section + 20, 0x200)?;
        write_u32(&mut pe, 0x20c, 0x1040)?;
        let name = b"avcodec-61.dll\0";
        pe.get_mut(0x240..0x240 + name.len())
            .ok_or_else(|| TaskError::new("invalid test name range"))?
            .copy_from_slice(name);

        let parsed = parse(&pe)?;
        assert_eq!(parsed.machine, crate::X86_64_MACHINE);
        assert_eq!(parsed.imports, vec!["avcodec-61.dll"]);
        Ok(())
    }

    #[test]
    fn rejects_truncated_input() {
        assert!(parse(b"MZ").is_err());
    }

    fn write_u16(bytes: &mut [u8], offset: usize, value: u16) -> TaskResult<()> {
        bytes
            .get_mut(offset..add(offset, 2)?)
            .ok_or_else(|| TaskError::new("test u16 write exceeds buffer"))?
            .copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn write_u32(bytes: &mut [u8], offset: usize, value: u32) -> TaskResult<()> {
        bytes
            .get_mut(offset..add(offset, 4)?)
            .ok_or_else(|| TaskError::new("test u32 write exceeds buffer"))?
            .copy_from_slice(&value.to_le_bytes());
        Ok(())
    }
}
