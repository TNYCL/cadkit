//! R2007 container (ODA spec chapter 5).
//!
//! The 0x400-byte file header at 0x80 is RS(255,239) interleaved over 3 blocks and
//! holds an R2007-LZ77-compressed 0x110-byte header record. The page map and section
//! map are *system pages* (RS(255,239), data repeated `correction factor` times);
//! data pages are RS(255,251) interleaved and optionally compressed. Page offsets are
//! relative to 0x480.
//!
//! The overall flow follows ACadSharp's `DwgReader.readFileHeaderAC21` /
//! `getSectionBuffer21` (MIT, © DomCR); field layouts are from the spec.

use std::collections::{BTreeMap, HashSet};

use cadkit_core::bytes::ByteReader;
use cadkit_core::{Error, Limits, Result};

use super::reed_solomon::{CODEWORD, DATA_K, SYSTEM_K, deinterleave};
use super::{Budget, Container, MAX_SYSTEM_PAGE, Section, SectionBuffer, context, lz77_r2007};
use crate::version::DwgVersion;

/// Stream offset that page offsets are relative to.
const PAGE_BASE: u64 = 0x480;
/// Largest accepted uncompressed page; R2007 data pages hold at most 0xF800 bytes.
const MAX_PAGE_SIZE: u64 = 0x10_0000;
/// Size of the decompressed file header record.
const HEADER_RECORD_SIZE: usize = 0x110;

/// Fields of the decompressed file header record (§5.2) that the reader uses.
#[derive(Debug, Clone, Copy, Default)]
struct HeaderRecord {
    pages_map_crc_compressed: u64,
    pages_map_correction: u64,
    pages_map_offset: u64,
    pages_map_size_compressed: u64,
    pages_map_size_uncompressed: u64,
    pages_max_id: u64,
    sections_map_size_compressed: u64,
    sections_map_id: u64,
    sections_map_size_uncompressed: u64,
    sections_map_correction: u64,
}

fn parse_header_record(data: &[u8]) -> Result<HeaderRecord> {
    let mut r = ByteReader::new(data);
    let mut field = |at: usize| -> Result<u64> {
        r.seek(at)?;
        r.u64()
    };
    Ok(HeaderRecord {
        pages_map_crc_compressed: field(0x10)?,
        pages_map_correction: field(0x18)?,
        pages_map_offset: field(0x38)?,
        pages_map_size_compressed: field(0x50)?,
        pages_map_size_uncompressed: field(0x58)?,
        pages_max_id: field(0x68)?,
        sections_map_size_compressed: field(0xB0)?,
        sections_map_id: field(0xC0)?,
        sections_map_size_uncompressed: field(0xC8)?,
        sections_map_correction: field(0xD8)?,
    })
}

/// Reads the RS-encoded file header at 0x80 and returns the decompressed record.
fn read_file_header(bytes: &[u8]) -> Result<Vec<u8>> {
    let encoded = bytes.get(0x80..0x480).ok_or(Error::Truncated {
        offset: 0x80,
        needed: 0x400,
    })?;
    let decoded = deinterleave(encoded, 3, SYSTEM_K, 3 * SYSTEM_K);
    let mut r = ByteReader::with_base(&decoded, 0x80);
    r.seek(0x18)?;
    let compressed_len = r.i32()?;
    let _length2 = r.i32()?;
    let len = compressed_len.unsigned_abs() as usize;
    let payload = decoded
        .get(0x20..0x20 + len.min(decoded.len().saturating_sub(0x20)))
        .unwrap_or(&[]);
    if payload.len() != len {
        return Err(Error::invalid(
            0x80,
            format!("R2007 file header payload of {len} bytes"),
        ));
    }
    let mut out = if compressed_len > 0 {
        lz77_r2007::decompress(payload, HEADER_RECORD_SIZE, 0x80)?
    } else {
        // A negative length means the record is stored uncompressed (§5.2).
        payload
            .get(..HEADER_RECORD_SIZE.min(payload.len()))
            .unwrap_or(&[])
            .to_vec()
    };
    out.resize(HEADER_RECORD_SIZE, 0);
    Ok(out)
}

/// Reads a system page (§5.3) at `offset` (relative to 0x480). The declared sizes
/// come from the file header record, which is not checked by a CRC here, so the
/// uncompressed size is capped and only the bytes produced are allocated and charged.
fn read_system_page(
    bytes: &[u8],
    offset: u64,
    compressed: u64,
    uncompressed: u64,
    correction: u64,
    budget: &mut Budget,
) -> Result<Vec<u8>> {
    let address = PAGE_BASE
        .checked_add(offset)
        .ok_or_else(|| Error::invalid(0x80, "system page offset"))?;
    let aligned = compressed
        .checked_add(7)
        .map(|v| v & !7)
        .ok_or_else(|| Error::invalid(address, "page size"))?;
    let total = aligned
        .checked_mul(correction.max(1))
        .ok_or_else(|| Error::invalid(address, "page size"))?;
    let blocks = total.div_ceil(SYSTEM_K as u64);
    let encoded_len = blocks
        .checked_mul(CODEWORD as u64)
        .ok_or_else(|| Error::invalid(address, "page size"))?;
    if encoded_len > bytes.len() as u64 {
        return Err(Error::Truncated {
            offset: address,
            needed: encoded_len,
        });
    }
    let start = usize::try_from(address).map_err(|_| Error::invalid(address, "page address"))?;
    let encoded = start
        .checked_add(encoded_len as usize)
        .and_then(|end| bytes.get(start..end))
        .ok_or(Error::Truncated {
            offset: address,
            needed: encoded_len,
        })?;
    if uncompressed > MAX_SYSTEM_PAGE {
        return Err(Error::invalid(
            address,
            format!("system page of {uncompressed} bytes"),
        ));
    }
    let data = deinterleave(encoded, blocks as usize, SYSTEM_K, total as usize);
    let size = budget.take(uncompressed)?;
    let payload = data
        .get(..(compressed as usize).min(data.len()))
        .unwrap_or(&[]);
    let out = if compressed < uncompressed {
        lz77_r2007::decompress(payload, size, address)
    } else {
        Ok(payload
            .get(..size.min(payload.len()))
            .unwrap_or(&[])
            .to_vec())
    };
    budget.refund(size - out.as_ref().map_or(0, |o| o.len().min(size)));
    out
}

/// One page of a section (§5.2 section map).
#[derive(Debug, Clone, Copy)]
struct PageRef {
    data_offset: u64,
    id: u64,
    uncompressed: u64,
    compressed: u64,
}

#[derive(Debug, Clone)]
struct SectionInfo {
    data_size: u64,
    encrypted: u64,
    encoding: u64,
    pages: Vec<PageRef>,
}

fn parse_section_map(data: &[u8], max_pages: usize) -> Result<BTreeMap<String, SectionInfo>> {
    let mut r = ByteReader::new(data);
    let mut sections = BTreeMap::new();
    while r.remaining() >= 64 {
        let data_size = r.u64()?;
        let _max_size = r.u64()?;
        let encrypted = r.u64()?;
        let _hash = r.u64()?;
        let name_len = r.u64()?;
        let _unknown = r.u64()?;
        let encoding = r.u64()?;
        let page_count = r.u64()?;
        // The name length is a byte count of UTF-16 data including the terminator
        // (spec says characters; the sample files say bytes, as ACadSharp reads it).
        let name_len = usize::try_from(name_len)
            .ok()
            .filter(|&n| n <= r.remaining())
            .ok_or_else(|| r.invalid("section name length"))?;
        let name_bytes = r.bytes(name_len)?;
        let units: Vec<u16> = name_bytes
            .chunks_exact(2)
            .map(|c| {
                u16::from_le_bytes([
                    c.first().copied().unwrap_or(0),
                    c.get(1).copied().unwrap_or(0),
                ])
            })
            .collect();
        let name: String = String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_owned();
        let page_count = usize::try_from(page_count)
            .ok()
            .filter(|&n| n <= max_pages && n.saturating_mul(56) <= r.remaining())
            .ok_or_else(|| r.invalid(format!("section {name:?} page count")))?;
        let mut pages = Vec::with_capacity(page_count);
        for _ in 0..page_count {
            let data_offset = r.u64()?;
            let _size = r.u64()?;
            let id = r.u64()?;
            let uncompressed = r.u64()?;
            let compressed = r.u64()?;
            let _checksum = r.u64()?;
            let _crc = r.u64()?;
            pages.push(PageRef {
                data_offset,
                id,
                uncompressed,
                compressed,
            });
        }
        if !name.is_empty() {
            sections.insert(
                name,
                SectionInfo {
                    data_size,
                    encrypted,
                    encoding,
                    pages,
                },
            );
        }
    }
    Ok(sections)
}

fn read_data_page(
    bytes: &[u8],
    page: &PageRef,
    location: (u64, u64),
    encoding: u64,
) -> Result<Vec<u8>> {
    let (offset, size) = location;
    let address = PAGE_BASE
        .checked_add(offset)
        .ok_or_else(|| Error::invalid(0, "page offset"))?;
    let start = usize::try_from(address).map_err(|_| Error::invalid(address, "page address"))?;
    let raw = usize::try_from(size)
        .ok()
        .and_then(|s| start.checked_add(s))
        .and_then(|end| bytes.get(start..end))
        .ok_or(Error::Truncated {
            offset: address,
            needed: size,
        })?;
    let compressed =
        usize::try_from(page.compressed).map_err(|_| Error::invalid(address, "page size"))?;
    let uncompressed =
        usize::try_from(page.uncompressed).map_err(|_| Error::invalid(address, "page size"))?;
    let data = if encoding == 4 {
        let aligned = compressed
            .checked_add(7)
            .map(|v| v & !7)
            .ok_or_else(|| Error::invalid(address, "page size"))?;
        let blocks = aligned.div_ceil(DATA_K);
        if blocks.saturating_mul(CODEWORD) > raw.len() {
            return Err(Error::Truncated {
                offset: address,
                needed: (blocks * CODEWORD) as u64,
            });
        }
        deinterleave(raw, blocks, DATA_K, blocks * DATA_K)
    } else {
        raw.to_vec()
    };
    let payload = data.get(..compressed.min(data.len())).unwrap_or(&[]);
    if compressed < uncompressed {
        lz77_r2007::decompress(payload, uncompressed, address)
    } else {
        Ok(payload
            .get(..uncompressed.min(payload.len()))
            .unwrap_or(&[])
            .to_vec())
    }
}

fn read_section(
    bytes: &[u8],
    name: &str,
    info: &SectionInfo,
    pages: &BTreeMap<u64, (u64, u64)>,
    budget: &mut Budget,
    c: &mut Container<'_>,
) -> Result<Vec<u8>> {
    if info.encrypted == 1 {
        return Err(Error::Unsupported(format!("encrypted section {name}")));
    }
    let largest = info.pages.iter().map(|p| p.uncompressed).max().unwrap_or(0);
    if largest > MAX_PAGE_SIZE {
        return Err(Error::invalid(
            0,
            format!("section {name} has a page of {largest} bytes"),
        ));
    }
    // Each page id, file page and data offset is read once (repeats in a corrupt map
    // would otherwise be decompressed again for free), in data-offset order.
    let (mut ids, mut locations, mut offsets) = (HashSet::new(), HashSet::new(), HashSet::new());
    let mut list: Vec<(PageRef, (u64, u64))> = Vec::new();
    let mut repeated = 0usize;
    for page in &info.pages {
        let Some(&location) = pages.get(&page.id) else {
            c.warn(
                "dwg.missing_page",
                format!("section {name}: page {} not in page map", page.id),
                None,
            );
            continue;
        };
        if !ids.insert(page.id)
            || !locations.insert(location.0)
            || !offsets.insert(page.data_offset)
        {
            repeated += 1;
            continue;
        }
        list.push((*page, location));
    }
    if repeated > 0 {
        c.warn(
            "dwg.duplicate_page",
            format!("section {name}: {repeated} repeated page references ignored"),
            None,
        );
    }
    list.sort_by_key(|(page, _)| page.data_offset);

    // Plausibility bound on the declared size: the listed pages plus a few omitted
    // zero pages of the largest listed size.
    let listed: u64 = list.iter().map(|(p, _)| p.uncompressed).sum();
    let plausible = listed.saturating_add(64 * largest.max(0x400));
    let mut size = info.data_size;
    if size > plausible {
        c.warn(
            "dwg.section_size",
            format!("section {name} claims {size} bytes; using {plausible}"),
            None,
        );
        size = plausible;
    }
    let total = usize::try_from(size)
        .map_err(|_| Error::LimitExceeded(format!("section {name} of {size} bytes")))?;
    let mut out = SectionBuffer::new(total, largest.max(0x400));
    for (page, location) in list {
        let Ok(start) = usize::try_from(page.data_offset) else {
            continue;
        };
        if start >= total {
            continue;
        }
        if page.uncompressed > (total - start) as u64 + 0x10000 {
            c.warn(
                "dwg.size_mismatch",
                format!("section {name}: page larger than the section"),
                None,
            );
        }
        // Each decompression is charged: the declared page size, refunded down to the output.
        let reserved = budget.take(page.uncompressed)?;
        let data = read_data_page(bytes, &page, location, info.encoding);
        budget.refund(reserved - data.as_ref().map_or(0, |d| d.len().min(reserved)));
        match data {
            Ok(data) => {
                if !out.place(start, &data, budget)? {
                    c.warn(
                        "dwg.section_gap",
                        format!(
                            "section {name}: page at {start:#x} leaves too large a gap; skipped"
                        ),
                        None,
                    );
                }
            }
            Err(e) => c.warn(
                "dwg.page_error",
                format!("section {name}: page {}: {e}", page.id),
                None,
            ),
        }
    }
    let (data, missing) = out.finish(budget)?;
    if missing > 0 {
        c.warn(
            "dwg.section_size",
            format!("section {name}: last {missing} declared bytes are not covered by pages"),
            None,
        );
    }
    Ok(data)
}

/// Reads an R2007 file.
pub fn read<'a>(bytes: &'a [u8], version: DwgVersion, limits: &Limits) -> Result<Container<'a>> {
    let mut c = Container::new(version);
    let mut meta = ByteReader::new(bytes);
    meta.skip(0x0B)?;
    c.maintenance = meta.u8()?;
    meta.skip(7)?;
    c.codepage = meta.u16()?;
    meta.skip(3)?;
    let security = meta.u32()?;
    if security & 0x0001 != 0 {
        return Err(Error::Unsupported(
            "password-encrypted DWG data sections".into(),
        ));
    }

    let record = read_file_header(bytes).map_err(context("R2007 file header"))?;
    let h = parse_header_record(&record).map_err(context("R2007 file header"))?;
    let mut budget = Budget::new(limits);
    let _ = h.pages_map_crc_compressed;

    let page_map = read_system_page(
        bytes,
        h.pages_map_offset,
        h.pages_map_size_compressed,
        h.pages_map_size_uncompressed,
        h.pages_map_correction,
        &mut budget,
    )
    .map_err(context("page map"))?;
    let mut pages: BTreeMap<u64, (u64, u64)> = BTreeMap::new();
    let mut r = ByteReader::new(&page_map);
    let mut offset: u64 = 0;
    while r.remaining() >= 16 {
        let size = r.u64()?;
        let id = r.i64()?.unsigned_abs();
        if h.pages_max_id != 0 && id > h.pages_max_id {
            c.warn(
                "dwg.page_map",
                format!("page id {id} above the maximum {}", h.pages_max_id),
                None,
            );
        }
        pages.insert(id, (offset, size));
        offset = offset
            .checked_add(size)
            .ok_or_else(|| r.invalid("page map offset overflow"))?;
    }

    let &(section_map_offset, _) = pages.get(&h.sections_map_id).ok_or_else(|| {
        Error::invalid(
            0x80,
            format!("section map page {} not in page map", h.sections_map_id),
        )
    })?;
    let section_map = read_system_page(
        bytes,
        section_map_offset,
        h.sections_map_size_compressed,
        h.sections_map_size_uncompressed,
        h.sections_map_correction,
        &mut budget,
    )
    .map_err(context("section map"))?;
    let max_pages = bytes.len() / 32 + 1;
    let sections = parse_section_map(&section_map, max_pages).map_err(context("section map"))?;
    c.section_names = sections.keys().cloned().collect();

    for (name, slot) in [
        ("AcDb:Header", 0),
        ("AcDb:Classes", 1),
        ("AcDb:Handles", 2),
        ("AcDb:AcDbObjects", 3),
        ("AcDb:Template", 4),
    ] {
        let Some(info) = sections.get(name) else {
            if slot != 4 {
                c.warn(
                    "dwg.missing_section",
                    format!("section {name} not present"),
                    None,
                );
            }
            continue;
        };
        match read_section(bytes, name, info, &pages, &mut budget, &mut c) {
            Ok(data) => {
                let section = Some(Section::owned(data, name));
                match slot {
                    0 => c.header = section,
                    1 => c.classes = section,
                    2 => c.handles = section,
                    3 => c.objects = section,
                    _ => c.template = section,
                }
            }
            Err(e @ Error::LimitExceeded(_)) => return Err(e),
            Err(e) => c.warn("dwg.section_error", format!("section {name}: {e}"), None),
        }
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file whose single RS block at 0x480 starts with `data` (one block of a
    /// system page is a plain copy of its first 239 bytes).
    fn file_with_page(data: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0u8; 0x480 + CODEWORD];
        bytes[0x480..0x480 + data.len()].copy_from_slice(data);
        bytes
    }

    #[test]
    fn system_page_size_is_capped_and_charged_by_output() {
        // A 10-byte literal run (R2007 LZ77).
        let bytes = file_with_page(&[0x02, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        let limits = Limits::default();
        let mut budget = Budget::new(&limits);
        // A declared size of 4 GiB is rejected before anything is allocated.
        assert!(read_system_page(&bytes, 0, 11, u64::from(u32::MAX), 1, &mut budget).is_err());
        // The largest accepted size allocates and charges only the 10 bytes produced.
        let out = read_system_page(&bytes, 0, 11, MAX_SYSTEM_PAGE, 1, &mut budget).unwrap();
        assert_eq!(out, vec![9, 1, 2, 3, 4, 5, 6, 7, 8, 0]);
        assert_eq!(budget.left, limits.max_decompressed_bytes - 10);
    }

    #[test]
    fn repeated_pages_are_read_once_and_huge_sizes_stay_small() {
        // A stored 16-byte page right at 0x480, listed 100 times.
        let data: Vec<u8> = (1..=16).collect();
        let bytes = file_with_page(&data);
        let page = PageRef {
            data_offset: 0,
            id: 1,
            uncompressed: 16,
            compressed: 16,
        };
        let info = SectionInfo {
            data_size: 1 << 40,
            encrypted: 0,
            encoding: 1,
            pages: vec![page; 100],
        };
        let pages = BTreeMap::from([(1u64, (0u64, 16u64))]);
        let limits = Limits::default();
        let mut budget = Budget::new(&limits);
        let mut c = Container::new(DwgVersion::R2007);
        let out = read_section(&bytes, "AcDb:Test", &info, &pages, &mut budget, &mut c).unwrap();
        assert_eq!(out.get(..16), Some(&data[..]));
        // The claimed terabyte is cut to the page plus the zero-page allowance.
        assert!(out.len() <= 16 + 64 * 0x400, "{}", out.len());
        assert_eq!(
            limits.max_decompressed_bytes - budget.left,
            out.len() as u64
        );
        let codes: Vec<&str> = c.warnings.iter().map(|w| w.code.as_str()).collect();
        assert!(
            codes.contains(&"dwg.duplicate_page") && codes.contains(&"dwg.section_size"),
            "{codes:?}"
        );
    }
}
