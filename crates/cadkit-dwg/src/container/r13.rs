//! R13–R15 container (ODA spec chapter 3).
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0x00 | 6 | version magic |
//! | 0x0B | 1 | maintenance release (R14+) |
//! | 0x0D | 4 | image seeker |
//! | 0x13 | 2 | DWGCODEPAGE |
//! | 0x15 | 4 | number of section-locator records |
//! | 0x19 | 9·n | records: RC number, RL seeker, RL size |
//! | … | 2 | CRC-8 of bytes 0..here, seed 0, XOR a count-dependent magic |
//! | … | 16 | sentinel |

use cadkit_core::bytes::ByteReader;
use cadkit_core::{Error, Result};

use super::{Container, Section, sentinels};
use crate::crc::crc8;
use crate::version::DwgVersion;

/// Records seen in the wild go up to 6; anything far above is corruption.
const MAX_RECORDS: u32 = 64;

/// XOR applied to the file header CRC, by record count (§3.2.6).
fn crc_magic(records: u32) -> Option<u16> {
    match records {
        3 => Some(0xA598),
        4 => Some(0x8101),
        5 => Some(0x3CC4),
        6 => Some(0x8461),
        _ => None,
    }
}

/// Reads the file header and section-locator records.
pub fn read(bytes: &[u8], version: DwgVersion) -> Result<Container<'_>> {
    let mut c = Container::new(version);
    let mut r = ByteReader::new(bytes);
    r.skip(0x0B)?;
    c.maintenance = r.u8()?;
    r.skip(1)?;
    let _image_seeker = r.u32()?;
    r.skip(2)?;
    c.codepage = r.u16()?;
    let count = r.u32()?;
    if count > MAX_RECORDS {
        return Err(Error::invalid(
            0x15,
            format!("{count} section-locator records"),
        ));
    }
    let mut records = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let number = r.u8()?;
        let seeker = r.u32()?;
        let size = r.u32()?;
        records.push((number, u64::from(seeker), u64::from(size)));
    }
    let crc_at = r.pos();
    let stored = r.u16()?;
    if let (Some(magic), Some(prefix)) = (crc_magic(count), bytes.get(..crc_at)) {
        let computed = crc8(0, prefix) ^ magic;
        if computed != stored {
            c.warn(
                "dwg.crc_mismatch",
                format!("file header CRC {stored:#06x}, computed {computed:#06x}"),
                Some(crc_at as u64),
            );
        }
    }
    if r.array::<16>()? != sentinels::FILE_HEADER_END {
        c.warn(
            "dwg.sentinel_mismatch",
            "file header end sentinel mismatch",
            Some(r.offset().saturating_sub(16)),
        );
    }

    for (number, seeker, size) in records {
        if size == 0 {
            continue;
        }
        let name = match number {
            0 => "header variables",
            1 => "classes",
            2 => "object map",
            4 => "template",
            _ => "section",
        };
        let Some(section) = Section::borrowed(bytes, seeker, size, name) else {
            c.warn(
                "dwg.section_out_of_file",
                format!(
                    "section-locator record {number} ({seeker:#x}+{size:#x}) lies outside the file"
                ),
                Some(seeker),
            );
            continue;
        };
        match number {
            0 => c.header = Some(section),
            1 => c.classes = Some(section),
            2 => c.handles = Some(section),
            4 => c.template = Some(section),
            _ => {}
        }
    }
    // Object map offsets are absolute file offsets.
    c.objects = Some(Section {
        data: std::borrow::Cow::Borrowed(bytes),
        base: 0,
        name: "objects",
    });
    Ok(c)
}
