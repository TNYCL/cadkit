//! File container layer: turns the raw file into the logical sections the rest of
//! the reader needs (header variables, classes, object map, objects, template).
//!
//! - R13–R15 ([`r13`]): a plain file header with section-locator records; sections
//!   are byte ranges of the file and object offsets are absolute file offsets.
//! - R2004 and R2010+ ([`r2004`]): an XOR-masked file header, a page map and a
//!   section map in compressed system pages, and LZ77-compressed data pages.
//! - R2007 ([`r2007`]): Reed-Solomon interleaved pages, its own LZ77 variant and
//!   64-bit page/section maps.

use std::borrow::Cow;

use cadkit_core::{Error, Limits, Result, Warning};

use crate::version::DwgVersion;

pub mod lz77_r2004;
pub mod lz77_r2007;
pub mod r13;
pub mod r2004;
pub mod r2007;
pub mod reed_solomon;

/// One logical section.
#[derive(Debug, Clone)]
pub struct Section<'a> {
    /// Section bytes (borrowed from the file for R13–R15, decompressed otherwise).
    pub data: Cow<'a, [u8]>,
    /// File offset of `data[0]` for borrowed sections; 0 for decompressed ones, whose
    /// error offsets are relative to the logical section.
    pub base: u64,
    /// Section name (`AcDb:Header` …; the locator record for R13–R15).
    pub name: &'static str,
}

impl<'a> Section<'a> {
    fn borrowed(file: &'a [u8], start: u64, size: u64, name: &'static str) -> Option<Self> {
        let start_usize = usize::try_from(start).ok()?;
        let end = start_usize.checked_add(usize::try_from(size).ok()?)?;
        file.get(start_usize..end).map(|data| Self {
            data: Cow::Borrowed(data),
            base: start,
            name,
        })
    }

    fn owned(data: Vec<u8>, name: &'static str) -> Self {
        Self {
            data: Cow::Owned(data),
            base: 0,
            name,
        }
    }

    /// True when offsets in this section are file offsets (borrowed sections).
    pub fn is_file_range(&self) -> bool {
        matches!(self.data, Cow::Borrowed(_))
    }

    /// Describes `offset` (as produced by readers over this section) for messages:
    /// a file offset for borrowed sections, `name+offset` for decompressed ones.
    pub fn locate(&self, offset: u64) -> String {
        if self.is_file_range() {
            format!("{offset:#x}")
        } else {
            format!("{}+{offset:#x}", self.name)
        }
    }

    /// Suffix for messages carrying offsets read from this section: names a
    /// decompressed section, whose offsets are relative to it; empty for file ranges.
    pub fn offsets_note(&self) -> String {
        if self.is_file_range() {
            String::new()
        } else {
            format!(" (in {}; offsets relative to the section)", self.name)
        }
    }

    /// The offset to store in a [`Warning`]: only file offsets are meaningful there;
    /// positions inside decompressed sections go into the message via [`Section::locate`].
    pub fn warning_offset(&self, offset: u64) -> Option<u64> {
        self.is_file_range().then_some(offset)
    }
}

/// Names the decompressed section an error's offset refers to (offsets inside
/// decompressed sections are relative to the section, not the file).
pub(crate) fn in_section(section: &Section<'_>) -> impl Fn(Error) -> Error {
    let name = (!section.is_file_range()).then_some(section.name);
    move |e| match (name, e) {
        (Some(name), Error::Truncated { offset, needed }) => Error::invalid(
            offset,
            format!(
                "{name}: data ends early (needed {needed} more bytes; offset relative to the section)"
            ),
        ),
        (Some(name), Error::Invalid { offset, message }) => Error::invalid(
            offset,
            format!("{name}: {message} (offset relative to the section)"),
        ),
        (_, other) => other,
    }
}

/// The logical sections of a file plus file-header facts.
#[derive(Debug, Clone)]
pub struct Container<'a> {
    /// Format version.
    pub version: DwgVersion,
    /// Maintenance release byte at offset 0x0B.
    pub maintenance: u8,
    /// `$DWGCODEPAGE` index at offset 0x13.
    pub codepage: u16,
    /// Header variables section (AcDb:Header / locator record 0), starting at its sentinel.
    pub header: Option<Section<'a>>,
    /// Classes section (AcDb:Classes / locator record 1), starting at its sentinel.
    pub classes: Option<Section<'a>>,
    /// Object map (AcDb:Handles / locator record 2).
    pub handles: Option<Section<'a>>,
    /// Object stream the object map points into: the whole file for R13–R15,
    /// the decompressed AcDb:AcDbObjects section otherwise.
    pub objects: Option<Section<'a>>,
    /// Template section (AcDb:Template / locator record 4): holds MEASUREMENT.
    pub template: Option<Section<'a>>,
    /// Names of all sections present (R2004+), for diagnostics.
    pub section_names: Vec<String>,
    /// Recoverable problems.
    pub warnings: Vec<Warning>,
}

impl Container<'_> {
    fn new(version: DwgVersion) -> Self {
        Self {
            version,
            maintenance: 0,
            codepage: 0,
            header: None,
            classes: None,
            handles: None,
            objects: None,
            template: None,
            section_names: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn warn(&mut self, code: &str, message: impl Into<String>, offset: Option<u64>) {
        self.warnings.push(Warning {
            code: code.to_owned(),
            message: message.into(),
            offset,
            object: None,
        });
    }
}

/// Reads the container of any supported version.
pub fn read<'a>(bytes: &'a [u8], limits: &Limits) -> Result<Container<'a>> {
    if bytes.len() as u64 > limits.max_input_bytes {
        return Err(Error::LimitExceeded(format!(
            "input of {} bytes",
            bytes.len()
        )));
    }
    let magic = bytes.get(..6).ok_or(Error::Truncated {
        offset: 0,
        needed: 6,
    })?;
    let version = DwgVersion::from_magic(magic).ok_or(Error::UnknownFormat)?;
    match version {
        DwgVersion::R13 | DwgVersion::R14 | DwgVersion::R2000 => r13::read(bytes, version),
        DwgVersion::R2007 => r2007::read(bytes, version, limits),
        _ => r2004::read(bytes, version, limits),
    }
}

/// Adds what was being read to a container error (limit errors pass unchanged).
pub(crate) fn context(what: &'static str) -> impl Fn(Error) -> Error {
    move |e| match e {
        Error::LimitExceeded(_) | Error::Unsupported(_) => e,
        Error::Truncated { offset, needed } => Error::invalid(
            offset,
            format!("{what}: data ends early (needed {needed} more bytes)"),
        ),
        Error::Invalid { offset, message } => Error::invalid(offset, format!("{what}: {message}")),
        other => other,
    }
}

/// Largest accepted system page (page map, section map). Real ones hold 8–24 bytes
/// per data page of the file: a few hundred KiB for the largest drawings.
pub(crate) const MAX_SYSTEM_PAGE: u64 = 8 << 20;

/// Omitted all-zero pages tolerated per section (gaps between pages plus the tail up
/// to the declared size), in pages, and in bytes at most.
const ZERO_PAGES: u64 = 64;
const MAX_ZERO_FILL: u64 = 16 << 20;

/// Tracks decompressed bytes against `Limits::max_decompressed_bytes`.
#[derive(Debug)]
pub(crate) struct Budget {
    left: u64,
}

impl Budget {
    pub(crate) fn new(limits: &Limits) -> Self {
        Self {
            left: limits.max_decompressed_bytes,
        }
    }

    /// Reserves `n` bytes or fails with [`Error::LimitExceeded`].
    pub(crate) fn take(&mut self, n: u64) -> Result<usize> {
        if n > self.left {
            return Err(Error::LimitExceeded(format!(
                "decompressed data above {} bytes",
                self.left
            )));
        }
        self.left -= n;
        usize::try_from(n).map_err(|_| Error::LimitExceeded(format!("section of {n} bytes")))
    }

    /// Returns the unused part of a reservation, so that only bytes actually produced
    /// are charged.
    pub(crate) fn refund(&mut self, n: usize) {
        self.left = self.left.saturating_add(n as u64);
    }
}

/// A data section assembled from pages placed at their start offsets (§4.5, §5.2).
///
/// The buffer grows as pages arrive instead of being allocated at the declared size.
/// Bytes no page wrote — gaps between pages and the tail up to the declared size,
/// i.e. omitted all-zero pages — are limited in total, so a corrupt section map cannot
/// turn a few bytes of file into a huge zero buffer. Zero fill is charged to the
/// budget; page data is charged when it is decompressed.
#[derive(Debug)]
pub(crate) struct SectionBuffer {
    data: Vec<u8>,
    size: usize,
    zero_left: usize,
}

impl SectionBuffer {
    /// `size` is the declared (already plausibility-checked) section size and
    /// `page_size` the largest page, which scales the zero-fill allowance.
    pub(crate) fn new(size: usize, page_size: u64) -> Self {
        let allowance = page_size.saturating_mul(ZERO_PAGES).min(MAX_ZERO_FILL);
        Self {
            data: Vec::new(),
            size,
            zero_left: usize::try_from(allowance).unwrap_or(usize::MAX),
        }
    }

    /// Copies `page` to `start`, clipped to the section size. Returns `false` when the
    /// gap before it would exceed the zero-fill allowance (nothing is written then).
    pub(crate) fn place(&mut self, start: usize, page: &[u8], budget: &mut Budget) -> Result<bool> {
        if start >= self.size {
            return Ok(true);
        }
        let n = page.len().min(self.size - start);
        if start > self.data.len() {
            let gap = start - self.data.len();
            if gap > self.zero_left {
                return Ok(false);
            }
            budget.take(gap as u64)?;
            self.zero_left -= gap;
            self.data.resize(start, 0);
        }
        let end = start + n;
        if end > self.data.len() {
            self.data.resize(end, 0);
        }
        if let (Some(dst), Some(src)) = (self.data.get_mut(start..end), page.get(..n)) {
            dst.copy_from_slice(src);
        }
        Ok(true)
    }

    /// Pads the section to its declared size within the zero-fill allowance. Returns
    /// the data and how many declared bytes are missing.
    pub(crate) fn finish(mut self, budget: &mut Budget) -> Result<(Vec<u8>, usize)> {
        let tail = self.size.saturating_sub(self.data.len());
        let fill = tail.min(self.zero_left);
        budget.take(fill as u64)?;
        let len = self.data.len() + fill;
        self.data.resize(len, 0);
        Ok((self.data, tail - fill))
    }
}

/// 16-byte sentinels (spec §9, §10).
pub mod sentinels {
    /// Start of the header variables section.
    pub const HEADER_START: [u8; 16] = [
        0xCF, 0x7B, 0x1F, 0x23, 0xFD, 0xDE, 0x38, 0xA9, 0x5F, 0x7C, 0x68, 0xB8, 0x4E, 0x6D, 0x33,
        0x5F,
    ];
    /// End of the header variables section.
    pub const HEADER_END: [u8; 16] = [
        0x30, 0x84, 0xE0, 0xDC, 0x02, 0x21, 0xC7, 0x56, 0xA0, 0x83, 0x97, 0x47, 0xB1, 0x92, 0xCC,
        0xA0,
    ];
    /// Start of the classes section.
    pub const CLASSES_START: [u8; 16] = [
        0x8D, 0xA1, 0xC4, 0xB8, 0xC4, 0xA9, 0xF8, 0xC5, 0xC0, 0xDC, 0xF4, 0x5F, 0xE7, 0xCF, 0xB6,
        0x8A,
    ];
    /// End of the classes section.
    pub const CLASSES_END: [u8; 16] = [
        0x72, 0x5E, 0x3B, 0x47, 0x3B, 0x56, 0x07, 0x3A, 0x3F, 0x23, 0x0B, 0xA0, 0x18, 0x30, 0x49,
        0x75,
    ];
    /// End of the R13–R15 file header (after the locator records and CRC).
    pub const FILE_HEADER_END: [u8; 16] = [
        0x95, 0xA0, 0x4E, 0x28, 0x99, 0x82, 0x1A, 0xE5, 0x5E, 0x41, 0xE0, 0x5F, 0x9D, 0x3A, 0x4D,
        0x00,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_buffer_limits_zero_fill() {
        let mut budget = Budget::new(&Limits::default());
        // Section of 1 GiB declared, pages of 0x100: at most 64 zero pages may be added.
        let mut buf = SectionBuffer::new(1 << 30, 0x100);
        assert!(buf.place(0, &[1; 0x100], &mut budget).unwrap());
        // A page 32 pages further on is fine; one past the remaining allowance is not.
        assert!(buf.place(0x2100, &[2; 0x100], &mut budget).unwrap());
        assert!(!buf.place(0x10_0000, &[3; 0x100], &mut budget).unwrap());
        let (data, missing) = buf.finish(&mut budget).unwrap();
        assert_eq!(data.len(), 0x2200 + 32 * 0x100);
        assert_eq!(missing, (1 << 30) - data.len());
        assert_eq!(data[0x2100], 2);
    }

    #[test]
    fn decompressed_sections_are_named_in_messages() {
        let owned = Section::owned(vec![0; 4], "AcDb:AcDbObjects");
        assert_eq!(owned.locate(0x10), "AcDb:AcDbObjects+0x10");
        assert_eq!(owned.warning_offset(0x10), None);
        let e = in_section(&owned)(Error::invalid(0x10, "bad"));
        assert!(e.to_string().contains("AcDb:AcDbObjects"), "{e}");
        let file = [0u8; 8];
        let borrowed = Section::borrowed(&file, 2, 4, "objects").unwrap();
        assert_eq!(borrowed.locate(0x10), "0x10");
        assert_eq!(borrowed.warning_offset(0x10), Some(0x10));
        assert!(
            !in_section(&borrowed)(Error::invalid(0x10, "bad"))
                .to_string()
                .contains("objects")
        );
    }
}
