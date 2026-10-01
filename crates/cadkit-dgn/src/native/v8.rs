//! DGN V8 container layer: compound-file streams, zlib pages and raw elements.
//!
//! Layout (see `docs/dgn/FORMAT_NOTES.md` for offsets and evidence):
//!
//! ```text
//! Dgn~H                     file header (zlib at 0x14)
//! Dgn^Ix/Dgn~Mix            model index (zlib at 0)
//! Dgn-Md/#NNNNNN/Dgn~Mh     model header: one type-66 element (zlib at 0)
//! Dgn-Md/#NNNNNN/Dgn^G/$n   graphic element pages
//! Dgn-Md/#NNNNNN/Dgn^C/$n   control element pages
//! Dgn-Md/#NNNNNN/Dgn^GA/$n  auxiliary (XAttribute) pages of the graphic elements
//! Dgn^Nm/$n, Dgn^NmA/$n     file-level (non-model) elements: tables, tag sets, cells
//! ```
//!
//! A page stream is a 16-byte header `{record_count, format_version, page_number,
//! population}` followed by one zlib stream holding back-to-back records: a 4-byte
//! prefix, then the element (`u32` type word, `u32` total length in words, `u32`
//! attribute offset in words, ...).

use std::collections::HashMap;

use cadkit_core::{Error, ReadOptions, Result, Warning};
use encoding_rs::Encoding;

use crate::budget::Budget;
use crate::cfb::{CompoundFile, EntryKind};
use crate::le;
use crate::native::element::UorPoint;
use crate::native::linkage::{self, Linkage};
use crate::text;
use crate::zlib;

const PAGE_HEADER_LEN: usize = 16;
const RECORD_PREFIX_LEN: usize = 4;
const ELEMENT_MIN_LEN: usize = 12;
const AUX_MAGIC: u32 = 0x0000_a11b;
const AUX_HEADER_LEN: usize = 28;
const MODEL_INDEX_MAGIC: u32 = 0xaa00_ba11;
const MODEL_2D_FLAG: u32 = 0x0080_0000;

/// A stream of the compound file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamInfo {
    /// Path such as `Dgn-Md/#000000/Dgn^G/$1`.
    pub path: String,
    /// Size in bytes as stored (compressed).
    pub size: u64,
}

/// Uncompressed 16-byte header of a page stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PageHeader {
    /// Records in the page.
    pub record_count: u32,
    /// Page format version (2 or 3 observed).
    pub format_version: u32,
    /// Page number (0 = unspecified in some writers).
    pub page_number: u32,
    /// Population (0 = unspecified in some writers).
    pub population: u32,
}

/// One element exactly as stored in an inflated page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawElement {
    /// Offset of the element (after its 4-byte record prefix) in the inflated page.
    pub offset: usize,
    /// The 4-byte record prefix (0 in all observed files).
    pub prefix: u32,
    /// First `u32`: type in the low 16 bits; `0x2000_0000` complex header,
    /// `0x4000_0000` complex component.
    pub type_word: u32,
    /// Total element length in 16-bit words.
    pub words: u32,
    /// Attribute (linkage) offset in 16-bit words from the element start.
    pub attr_words: u32,
    /// The complete element bytes (`2 * words`).
    pub bytes: Vec<u8>,
}

impl RawElement {
    /// Element type number.
    pub fn type_code(&self) -> u16 {
        (self.type_word & 0xffff) as u16
    }

    /// Starts a complex group.
    pub fn is_complex_header(&self) -> bool {
        self.type_word & 0x2000_0000 != 0
    }

    /// Member of a complex group.
    pub fn is_complex_component(&self) -> bool {
        self.type_word & 0x4000_0000 != 0
    }

    /// Level id (graphic elements) or table number (table elements), `u32` at `0x0c`.
    pub fn level(&self) -> Option<u32> {
        le::u32_at(&self.bytes, 0x0c)
    }

    /// Element id, `u64` at `0x10`.
    pub fn id(&self) -> Option<u64> {
        le::u64_at(&self.bytes, 0x10)
    }

    /// Byte offset of the attribute linkages (clamped to the element length).
    pub fn attribute_offset(&self) -> usize {
        usize::try_from(self.attr_words)
            .unwrap_or(usize::MAX)
            .saturating_mul(2)
            .min(self.bytes.len())
    }

    /// Bytes before the attribute offset.
    pub fn primary(&self) -> &[u8] {
        self.bytes.get(..self.attribute_offset()).unwrap_or(&[])
    }

    /// Linkage bytes after the attribute offset.
    pub fn attributes(&self) -> &[u8] {
        self.bytes.get(self.attribute_offset()..).unwrap_or(&[])
    }
}

/// An element page.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// Stream path.
    pub path: String,
    /// Number from the stream name (`$n`).
    pub number: u32,
    /// Page header.
    pub header: PageHeader,
    /// Elements in stored order.
    pub elements: Vec<RawElement>,
    /// Inflated payload size.
    pub inflated_len: usize,
    /// The payload inflated completely and every record fit.
    pub complete: bool,
}

/// One auxiliary record (XAttribute) associated with an element id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuxRecord {
    /// Offset in the inflated page.
    pub offset: usize,
    /// Record kind (`u32` at `+0x08`).
    pub kind: u32,
    /// `u32` at `+0x0c`.
    pub reserved: u32,
    /// Element the record belongs to (`u64` at `+0x10`).
    pub element_id: u64,
    /// Flags (`u32` at `+0x18`).
    pub flags: u32,
    /// Payload after the 28-byte header.
    pub payload: Vec<u8>,
}

/// An auxiliary page (`Dgn^GA`, `Dgn^CA`, `Dgn^NmA`).
#[derive(Debug, Clone, PartialEq)]
pub struct AuxPage {
    /// Stream path.
    pub path: String,
    /// Number from the stream name.
    pub number: u32,
    /// Page header.
    pub header: PageHeader,
    /// Records in stored order.
    pub records: Vec<AuxRecord>,
    /// The payload inflated completely and every record fit.
    pub complete: bool,
}

/// One entry of the model index (`Dgn^Ix/Dgn~Mix`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelIndexEntry {
    /// Storage number (`#NNNNNN`).
    pub storage_index: u16,
    /// Model number.
    pub model_number: u16,
    /// Flags.
    pub flags: u32,
    /// `u64` at `+8` (equals the header's `0x18` value in observed files).
    pub id: u64,
    /// Model name.
    pub name: String,
    /// Model description.
    pub description: String,
}

/// A unit definition from the model header.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UnitDef {
    /// Flag word (base / system; `0x11` observed for metric meter-based units).
    pub flags: u32,
    /// Units per base unit, numerator.
    pub numerator: f64,
    /// Units per base unit, denominator.
    pub denominator: f64,
    /// Label (`"m"`, `"mm"` ...).
    pub label: Option<String>,
}

impl UnitDef {
    /// Size of one unit in meters (`denominator / numerator`), when plausible.
    pub fn meters(&self) -> Option<f64> {
        let m = self.denominator / self.numerator;
        (self.numerator > 0.0 && self.denominator > 0.0 && m.is_finite()).then_some(m)
    }
}

/// Decoded model header (the type-66 element in `Dgn~Mh`).
#[derive(Debug, Clone, PartialEq)]
pub struct ModelHeader {
    /// Type word (`0x0080_0000` set = 2D model).
    pub type_word: u32,
    /// The model is 3D.
    pub is_3d: bool,
    /// Storage units (UOR) per master unit, `f64` at `0xe0`.
    pub uor_per_master: f64,
    /// Global origin in UOR, 3 x `f64` at `0xc8`.
    pub global_origin: UorPoint,
    /// Model extents in UOR, 6 x `i64` at `0x90`.
    pub extents: [i64; 6],
    /// Master unit definition (`f64` numerator/denominator at `0x50`/`0x58`).
    pub master: UnitDef,
    /// Sub unit definition (`0x60`/`0x68`).
    pub sub: UnitDef,
    /// Name from string linkage 1.
    pub name: Option<String>,
    /// Description from string linkage 2.
    pub description: Option<String>,
    /// All linkages.
    pub linkages: Vec<Linkage>,
    /// The complete element bytes.
    pub raw: Vec<u8>,
}

/// One model storage with its pages.
#[derive(Debug, Clone, PartialEq)]
pub struct V8Model {
    /// Storage name, e.g. `#000000`.
    pub storage: String,
    /// Matching model index entry.
    pub index_entry: Option<ModelIndexEntry>,
    /// Model header, when present and decodable.
    pub header: Option<ModelHeader>,
    /// Graphic element pages.
    pub graphic_pages: Vec<Page>,
    /// Control element pages.
    pub control_pages: Vec<Page>,
    /// Auxiliary pages of graphic elements.
    pub graphic_aux: Vec<AuxPage>,
    /// Auxiliary pages of control elements.
    pub control_aux: Vec<AuxPage>,
}

/// Fields from the `\u{5}SummaryInformation` property set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SummaryInfo {
    /// Creating application (PIDSI_APPNAME).
    pub application: Option<String>,
    /// Property set code page.
    pub codepage: Option<u16>,
    /// Locale id.
    pub locale: Option<u32>,
}

/// A whole V8 file at the container/record level.
#[derive(Debug, Clone, PartialEq)]
pub struct V8File {
    /// Compound file major version.
    pub cfb_version: u16,
    /// Every stream in the container.
    pub streams: Vec<StreamInfo>,
    /// Summary information.
    pub summary: SummaryInfo,
    /// Model index entries.
    pub model_index: Vec<ModelIndexEntry>,
    /// Models in storage order.
    pub models: Vec<V8Model>,
    /// File-level element pages (`Dgn^Nm`).
    pub named_pages: Vec<Page>,
    /// File-level auxiliary pages (`Dgn^NmA`).
    pub named_aux: Vec<AuxPage>,
    /// Recoverable problems.
    pub warnings: Vec<Warning>,
}

/// True when `bytes` is a compound file whose root holds the `Dgn~H` stream.
pub fn sniff(bytes: &[u8]) -> bool {
    bytes.starts_with(&crate::cfb::SIGNATURE) && crate::cfb::root_contains(bytes, "Dgn~H")
}

/// Reads the container, every page and every raw element.
pub fn read(bytes: &[u8], options: &ReadOptions) -> Result<V8File> {
    let mut budget = Budget::new(options.limits);
    let enc = text::encoding_for(options.fallback_codepage.as_deref());
    read_with(bytes, &mut budget, enc)
}

pub(crate) fn read_with(
    bytes: &[u8],
    budget: &mut Budget,
    enc: &'static Encoding,
) -> Result<V8File> {
    if bytes.len() as u64 > budget.limits.max_input_bytes {
        return Err(Error::LimitExceeded(format!(
            "input exceeds {} bytes",
            budget.limits.max_input_bytes
        )));
    }
    let cf = CompoundFile::parse(bytes)?;
    let paths = cf.paths();
    if !paths.iter().any(|p| p.path.eq_ignore_ascii_case("Dgn~H")) {
        return Err(Error::UnknownFormat);
    }
    let mut warnings = Vec::new();
    let streams: Vec<StreamInfo> = paths
        .iter()
        .filter(|p| p.kind == EntryKind::Stream)
        .map(|p| StreamInfo {
            path: p.path.clone(),
            size: p.size,
        })
        .collect();

    let mut ctx = Ctx {
        cf: &cf,
        paths,
        by_path: index_streams(paths),
        numbered: index_numbered(paths),
        budget,
        warnings: &mut warnings,
        enc,
    };

    let summary = ctx
        .stream("\u{5}SummaryInformation")?
        .map(|b| parse_summary(&b))
        .unwrap_or_default();
    let model_index = match ctx.stream("Dgn^Ix/Dgn~Mix")? {
        Some(raw) => {
            let inflated = ctx.inflate("Dgn^Ix/Dgn~Mix", &raw, 0)?;
            parse_model_index(&inflated, ctx.enc, ctx.warnings)
        }
        None => Vec::new(),
    };

    let mut storages: Vec<String> = paths
        .iter()
        .filter(|p| p.kind == EntryKind::Storage)
        .filter_map(|p| p.path.strip_prefix("Dgn-Md/"))
        .filter(|rest| rest.starts_with('#') && !rest.contains('/'))
        .map(str::to_owned)
        .collect();
    storages.sort();
    storages.dedup();

    let mut models = Vec::new();
    for storage in storages {
        let base = format!("Dgn-Md/{storage}");
        let number = storage.trim_start_matches('#').parse::<u16>().ok();
        let index_entry = model_index
            .iter()
            .find(|e| Some(e.storage_index) == number)
            .cloned();
        let header = match ctx.stream(&format!("{base}/Dgn~Mh"))? {
            Some(raw) => {
                let inflated = ctx.inflate(&format!("{base}/Dgn~Mh"), &raw, 0)?;
                let h = parse_model_header(&inflated, ctx.enc);
                if h.is_none() {
                    ctx.warn(
                        "dgn.model_header",
                        format!("{base}: no decodable model header"),
                    );
                }
                h
            }
            None => {
                ctx.warn(
                    "dgn.model_header",
                    format!("{base}: model header stream missing"),
                );
                None
            }
        };
        let graphic_pages = ctx.pages(&format!("{base}/Dgn^G/"))?;
        let control_pages = ctx.pages(&format!("{base}/Dgn^C/"))?;
        let graphic_aux = ctx.aux_pages(&format!("{base}/Dgn^GA/"))?;
        let control_aux = ctx.aux_pages(&format!("{base}/Dgn^CA/"))?;
        models.push(V8Model {
            storage,
            index_entry,
            header,
            graphic_pages,
            control_pages,
            graphic_aux,
            control_aux,
        });
    }
    let named_pages = ctx.pages("Dgn^Nm/")?;
    let named_aux = ctx.aux_pages("Dgn^NmA/")?;

    Ok(V8File {
        cfb_version: cf.major_version,
        streams,
        summary,
        model_index,
        models,
        named_pages,
        named_aux,
        warnings,
    })
}

struct Ctx<'a, 'b> {
    cf: &'a CompoundFile<'a>,
    paths: &'a [crate::cfb::PathEntry],
    /// Lower-case stream path -> index into `paths`.
    by_path: HashMap<String, usize>,
    /// Lower-case parent path (with trailing `/`) -> numbered `$n` streams, sorted by `n`.
    numbered: HashMap<String, Vec<(u32, usize)>>,
    budget: &'b mut Budget,
    warnings: &'b mut Vec<Warning>,
    enc: &'static Encoding,
}

impl Ctx<'_, '_> {
    fn warn(&mut self, code: &str, message: String) {
        self.warnings.push(Warning {
            code: code.into(),
            message,
            offset: None,
            object: None,
        });
    }

    /// Reads a stream by path; a missing or unreadable stream is a warning, not an error.
    ///
    /// Stream bytes are charged to the decompression budget before they are copied:
    /// directory entries may share sector chains, so without this a crafted directory could
    /// make the reader copy the same large chain once per entry.
    fn stream(&mut self, path: &str) -> Result<Option<Vec<u8>>> {
        let Some(&i) = self.by_path.get(&path.to_ascii_lowercase()) else {
            return Ok(None);
        };
        let Some(entry) = self.paths.get(i) else {
            return Ok(None);
        };
        self.read_entry(entry.index, entry.size, path)
    }

    fn read_entry(&mut self, index: usize, size: u64, path: &str) -> Result<Option<Vec<u8>>> {
        if size > self.budget.remaining_bytes() as u64 {
            return Err(Error::LimitExceeded(format!(
                "{path}: stream of {size} bytes exceeds the remaining data budget"
            )));
        }
        match self.cf.read_stream(index) {
            Ok(b) => {
                self.budget.charge_bytes(b.len())?;
                Ok(Some(b))
            }
            Err(e) => {
                self.warn("dgn.stream_unreadable", format!("{path}: {e}"));
                Ok(None)
            }
        }
    }

    fn inflate(&mut self, path: &str, raw: &[u8], at: usize) -> Result<Vec<u8>> {
        let r = zlib::inflate(raw.get(at..).unwrap_or(&[]), self.budget.remaining_bytes());
        self.budget.charge_bytes(r.data.len())?;
        if let Some(p) = r.problem {
            if p.contains("exceeds") {
                return Err(Error::LimitExceeded(format!("{path}: {p}")));
            }
            self.warn("dgn.zlib", format!("{path}: {p}"));
        }
        Ok(r.data)
    }

    /// Numbered streams `prefix$n` as (n, path, directory index, size), sorted by `n`.
    fn numbered(&self, prefix: &str) -> Vec<(u32, String, usize, u64)> {
        self.numbered
            .get(&prefix.to_ascii_lowercase())
            .map(|v| {
                v.iter()
                    .filter_map(|&(n, i)| {
                        let p = self.paths.get(i)?;
                        Some((n, p.path.clone(), p.index, p.size))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn pages(&mut self, prefix: &str) -> Result<Vec<Page>> {
        let mut out = Vec::new();
        for (number, path, index, size) in self.numbered(prefix) {
            let Some(raw) = self.read_entry(index, size, &path)? else {
                continue;
            };
            out.push(self.page(path, number, &raw)?);
        }
        Ok(out)
    }

    fn page(&mut self, path: String, number: u32, raw: &[u8]) -> Result<Page> {
        let header = page_header(raw);
        if raw.len() < PAGE_HEADER_LEN {
            self.warn("dgn.page_header", format!("{path}: page header truncated"));
        }
        let data = if raw.len() > PAGE_HEADER_LEN {
            self.inflate(&path, raw, PAGE_HEADER_LEN)?
        } else {
            Vec::new()
        };
        let (elements, end) = walk_records(&data, self.budget)?;
        let mut complete = end == data.len();
        if !complete {
            self.warn(
                "dgn.page_records",
                format!(
                    "{path}: record walk stopped at {end} of {} bytes",
                    data.len()
                ),
            );
        }
        if elements.len() != header.record_count as usize {
            complete = false;
            self.warn(
                "dgn.page_count",
                format!(
                    "{path}: header declares {} records, found {}",
                    header.record_count,
                    elements.len()
                ),
            );
        }
        Ok(Page {
            path,
            number,
            header,
            elements,
            inflated_len: data.len(),
            complete,
        })
    }

    fn aux_pages(&mut self, prefix: &str) -> Result<Vec<AuxPage>> {
        let mut out = Vec::new();
        for (number, path, index, size) in self.numbered(prefix) {
            let Some(raw) = self.read_entry(index, size, &path)? else {
                continue;
            };
            let header = page_header(&raw);
            let data = if raw.len() > PAGE_HEADER_LEN {
                self.inflate(&path, &raw, PAGE_HEADER_LEN)?
            } else {
                Vec::new()
            };
            let (records, end) = walk_aux(&data, self.budget)?;
            let complete = end == data.len();
            if !complete {
                self.warn(
                    "dgn.aux_records",
                    format!("{path}: auxiliary record walk stopped at {end}"),
                );
            }
            out.push(AuxPage {
                path,
                number,
                header,
                records,
                complete,
            });
        }
        Ok(out)
    }
}

/// Lower-case stream path -> index into `paths` (first entry wins on duplicates).
fn index_streams(paths: &[crate::cfb::PathEntry]) -> HashMap<String, usize> {
    let mut map = HashMap::with_capacity(paths.len());
    for (i, p) in paths.iter().enumerate() {
        if p.kind == EntryKind::Stream {
            map.entry(p.path.to_ascii_lowercase()).or_insert(i);
        }
    }
    map
}

/// Groups numbered `$n` streams by their lower-case parent path (one pass, sorted by `n`).
fn index_numbered(paths: &[crate::cfb::PathEntry]) -> HashMap<String, Vec<(u32, usize)>> {
    let mut map: HashMap<String, Vec<(u32, usize)>> = HashMap::new();
    for (i, p) in paths.iter().enumerate() {
        if p.kind != EntryKind::Stream {
            continue;
        }
        let Some((parent, leaf)) = p.path.rsplit_once('/') else {
            continue;
        };
        let Some(n) = leaf.strip_prefix('$').and_then(|d| d.parse::<u32>().ok()) else {
            continue;
        };
        map.entry(format!("{}/", parent.to_ascii_lowercase()))
            .or_default()
            .push((n, i));
    }
    for v in map.values_mut() {
        v.sort_unstable();
    }
    map
}

fn page_header(raw: &[u8]) -> PageHeader {
    PageHeader {
        record_count: le::u32_at(raw, 0).unwrap_or(0),
        format_version: le::u32_at(raw, 4).unwrap_or(0),
        page_number: le::u32_at(raw, 8).unwrap_or(0),
        population: le::u32_at(raw, 12).unwrap_or(0),
    }
}

/// Splits an inflated page into elements. Returns the elements and where the walk stopped.
pub(crate) fn walk_records(data: &[u8], budget: &mut Budget) -> Result<(Vec<RawElement>, usize)> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while let (Some(prefix), Some(type_word), Some(words), Some(attr_words)) = (
        le::u32_at(data, off),
        le::u32_at(data, off + 4),
        le::u32_at(data, off + 8),
        le::u32_at(data, off + 12),
    ) {
        let start = off + RECORD_PREFIX_LEN;
        let len = usize::try_from(words)
            .unwrap_or(usize::MAX)
            .saturating_mul(2);
        if len < ELEMENT_MIN_LEN {
            break;
        }
        let Some(bytes) = le::bytes(data, start, len) else {
            break;
        };
        budget.charge_objects(1)?;
        out.push(RawElement {
            offset: start,
            prefix,
            type_word,
            words,
            attr_words,
            bytes: bytes.to_vec(),
        });
        off = start + len;
    }
    Ok((out, off))
}

fn walk_aux(data: &[u8], budget: &mut Budget) -> Result<(Vec<AuxRecord>, usize)> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while let Some(head) = le::bytes(data, off, AUX_HEADER_LEN) {
        if le::u32_at(head, 0) != Some(AUX_MAGIC) {
            break;
        }
        let len = usize::try_from(le::u32_at(head, 4).unwrap_or(0)).unwrap_or(usize::MAX);
        let Some(payload) = le::bytes(data, off + AUX_HEADER_LEN, len) else {
            break;
        };
        budget.charge_objects(1)?;
        out.push(AuxRecord {
            offset: off,
            kind: le::u32_at(head, 8).unwrap_or(0),
            reserved: le::u32_at(head, 12).unwrap_or(0),
            element_id: le::u64_at(head, 16).unwrap_or(0),
            flags: le::u32_at(head, 24).unwrap_or(0),
            payload: payload.to_vec(),
        });
        off += AUX_HEADER_LEN + len;
    }
    Ok((out, off))
}

fn parse_model_index(
    b: &[u8],
    _enc: &'static Encoding,
    warnings: &mut Vec<Warning>,
) -> Vec<ModelIndexEntry> {
    let mut out = Vec::new();
    if le::u32_at(b, 0) != Some(MODEL_INDEX_MAGIC) {
        if !b.is_empty() {
            warnings.push(Warning {
                code: "dgn.model_index".into(),
                message: "model index has an unexpected signature".into(),
                offset: Some(0),
                object: None,
            });
        }
        return out;
    }
    let count = le::u32_at(b, 8).unwrap_or(0) as usize;
    let mut off = 16usize;
    for _ in 0..count.min(b.len() / 32) {
        if off >= b.len() {
            break;
        }
        let (Some(raw), Some(flags), Some(id), Some(size), Some(name_len), Some(desc_len)) = (
            le::u32_at(b, off),
            le::u32_at(b, off + 4),
            le::u64_at(b, off + 8),
            le::u16_at(b, off + 16),
            le::u16_at(b, off + 18),
            le::u32_at(b, off + 20),
        ) else {
            break;
        };
        let size = usize::from(size);
        if size < 32 {
            break;
        }
        let name_len = usize::from(name_len);
        let desc_len = usize::try_from(desc_len).unwrap_or(usize::MAX);
        let name = le::bytes(b, off + 32, name_len)
            .map(text::decode_utf16)
            .unwrap_or_default();
        let description = le::bytes(b, off + 32 + name_len, desc_len)
            .map(text::decode_utf16)
            .unwrap_or_default();
        out.push(ModelIndexEntry {
            storage_index: (raw & 0xffff) as u16,
            model_number: (raw >> 16) as u16,
            flags,
            id,
            name,
            description,
        });
        let Some(next) = off.checked_add(size) else {
            break;
        };
        off = next;
    }
    out
}

/// Finds the type-66 element that ends exactly at the end of the model header stream
/// (approach from ezdgn `find_model_header`) and decodes the fields used for mapping.
pub(crate) fn parse_model_header(b: &[u8], enc: &'static Encoding) -> Option<ModelHeader> {
    let rec = (0..b.len().saturating_sub(16)).step_by(4).find_map(|off| {
        if le::u32_at(b, off)? != 0 || le::u16_at(b, off + 4)? != 66 {
            return None;
        }
        let words = usize::try_from(le::u32_at(b, off + 8)?).ok()?;
        let start = off + 4;
        (start.checked_add(words.checked_mul(2)?)? == b.len() && words >= 16)
            .then(|| b.get(start..))?
    })?;
    let type_word = le::u32_at(rec, 0)?;
    let attr = usize::try_from(le::u32_at(rec, 8)?)
        .ok()?
        .saturating_mul(2)
        .min(rec.len());
    let linkages = linkage::parse(rec.get(attr..).unwrap_or(&[]), attr, true, enc);
    let uor_per_master = le::f64_at(rec, 0xe0).filter(|v| *v > 0.0)?;
    let mut extents = [0i64; 6];
    for (i, e) in extents.iter_mut().enumerate() {
        *e = le::i64_at(rec, 0x90 + 8 * i).unwrap_or(0);
    }
    let unit = |flags_off: usize, num_off: usize, label_id: u32| UnitDef {
        flags: le::u32_at(rec, flags_off).unwrap_or(0),
        numerator: le::f64_at(rec, num_off).unwrap_or(0.0),
        denominator: le::f64_at(rec, num_off + 8).unwrap_or(0.0),
        label: linkage::string(&linkages, label_id),
    };
    Some(ModelHeader {
        type_word,
        is_3d: type_word & MODEL_2D_FLAG == 0,
        uor_per_master,
        global_origin: le::f64s::<3>(rec, 0xc8).unwrap_or([0.0; 3]),
        extents,
        master: unit(0x48, 0x50, 0x13),
        sub: unit(0x4c, 0x60, 0x14),
        name: linkage::string(&linkages, 1),
        description: linkage::string(&linkages, 2),
        linkages,
        raw: rec.to_vec(),
    })
}

/// Parses the `[MS-OLEPS]` summary property set: code page (pid 1), application name
/// (pid 0x12) and locale (pid 0x80000000).
fn parse_summary(b: &[u8]) -> SummaryInfo {
    let mut s = SummaryInfo::default();
    // Offsets come from the file: every addition is checked (usize may be 32 bits).
    let at = |base: usize, add: usize| base.checked_add(add);
    let Some(section) = le::u32_at(b, 44).and_then(|o| usize::try_from(o).ok()) else {
        return s;
    };
    let count = at(section, 4).and_then(|o| le::u32_at(b, o)).unwrap_or(0) as usize;
    for i in 0..count.min(256) {
        let entry = at(section, 8 + 8 * i);
        let (Some(pid), Some(rel)) = (
            entry.and_then(|o| le::u32_at(b, o)),
            entry.and_then(|o| at(o, 4)).and_then(|o| le::u32_at(b, o)),
        ) else {
            break;
        };
        let Some(value) = usize::try_from(rel).ok().and_then(|r| at(section, r)) else {
            continue;
        };
        let (Some(v4), Some(v8)) = (at(value, 4), at(value, 8)) else {
            continue;
        };
        let vt = le::u32_at(b, value).unwrap_or(0);
        let n = le::u32_at(b, v4).unwrap_or(0) as usize;
        match (pid, vt) {
            (1, 2) => s.codepage = le::u16_at(b, v4),
            (0x8000_0000, 19 | 3) => s.locale = le::u32_at(b, v4),
            (0x12, 30) => {
                s.application = le::bytes(b, v8, n.min(4096)).map(|v| {
                    text::decode_8bit(v, encoding_rs::WINDOWS_1252)
                        .trim()
                        .to_owned()
                });
            }
            (0x12, 31) => {
                s.application = le::bytes(b, v8, n.min(4096).saturating_mul(2))
                    .map(|v| text::decode_utf16(v).trim().to_owned());
            }
            _ => {}
        }
    }
    s.application = s.application.filter(|a| !a.is_empty());
    s
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cadkit_core::Limits;

    /// Builds one element: type word, words, attribute words, then `body` from 0x0c.
    pub(crate) fn element(type_word: u32, body_from_0c: &[u8], linkages: &[u8]) -> Vec<u8> {
        let mut e = vec![0u8; 12];
        e.extend_from_slice(body_from_0c);
        if e.len() % 2 == 1 {
            e.push(0);
        }
        let attr = e.len() / 2;
        e.extend_from_slice(linkages);
        if e.len() % 2 == 1 {
            e.push(0);
        }
        let words = e.len() / 2;
        e[0..4].copy_from_slice(&type_word.to_le_bytes());
        e[4..8].copy_from_slice(&(words as u32).to_le_bytes());
        e[8..12].copy_from_slice(&(attr as u32).to_le_bytes());
        e
    }

    /// Builds a page stream (16-byte header + zlib payload) from elements.
    #[allow(dead_code)]
    pub(crate) fn page(elements: &[Vec<u8>]) -> Vec<u8> {
        let mut payload = Vec::new();
        for e in elements {
            payload.extend_from_slice(&0u32.to_le_bytes());
            payload.extend_from_slice(e);
        }
        let mut out = Vec::new();
        for v in [elements.len() as u32, 2, 1, elements.len() as u32] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&zlib::compress(&payload));
        out
    }

    #[test]
    fn walks_records_and_stops_on_bad_lengths() {
        let mut budget = Budget::new(Limits::default());
        let a = element(3, &[0u8; 20], &[]);
        let b = element(
            0x4000_0006,
            &[1u8; 30],
            &[7, 0x10, 0x41, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0],
        );
        let mut data = Vec::new();
        for e in [&a, &b] {
            data.extend_from_slice(&[0, 0, 0, 0]);
            data.extend_from_slice(e);
        }
        let (els, end) = walk_records(&data, &mut budget).unwrap();
        assert_eq!(els.len(), 2);
        assert_eq!(end, data.len());
        assert_eq!(els[1].type_code(), 6);
        assert!(els[1].is_complex_component());
        assert_eq!(els[1].attributes().len(), 16);
        // A record that declares more words than remain stops the walk without panicking.
        let mut bad = data.clone();
        bad[4 + 4..4 + 8].copy_from_slice(&u32::MAX.to_le_bytes());
        let (els, end) = walk_records(&bad, &mut budget).unwrap();
        assert!(els.is_empty());
        assert_eq!(end, 0);
    }

    #[test]
    fn object_limit_is_enforced() {
        let mut budget = Budget::new(Limits {
            max_objects: 1,
            ..Limits::default()
        });
        let mut data = Vec::new();
        for _ in 0..2 {
            data.extend_from_slice(&[0, 0, 0, 0]);
            data.extend_from_slice(&element(3, &[0u8; 20], &[]));
        }
        assert!(walk_records(&data, &mut budget).is_err());
    }

    /// A model header stream: 0x1000 filler, a zero delimiter, then a type-66 element.
    pub(crate) fn model_header_stream(is_3d: bool, uor_per_master: f64, name: &str) -> Vec<u8> {
        let mut body = vec![0u8; 0x1f0 - 0x0c];
        let put = |b: &mut Vec<u8>, off: usize, v: &[u8]| {
            b[off - 0x0c..off - 0x0c + v.len()].copy_from_slice(v)
        };
        put(&mut body, 0x48, &0x11u32.to_le_bytes());
        put(&mut body, 0x50, &1.0f64.to_le_bytes());
        put(&mut body, 0x58, &1.0f64.to_le_bytes());
        put(&mut body, 0x60, &1000.0f64.to_le_bytes());
        put(&mut body, 0x68, &1.0f64.to_le_bytes());
        put(&mut body, 0xc8, &5.0f64.to_le_bytes());
        put(&mut body, 0xe0, &uor_per_master.to_le_bytes());
        let mut link = Vec::new();
        let mut s = vec![0xff, 0xfd];
        for u in name.encode_utf16() {
            s.extend_from_slice(&u.to_le_bytes());
        }
        s.extend_from_slice(&[0, 0]);
        let total = (12 + s.len()).div_ceil(2) * 2;
        link.extend_from_slice(&[((total / 2) - 1) as u8, 0x10, 0xd2, 0x56]);
        link.extend_from_slice(&1u32.to_le_bytes());
        link.extend_from_slice(&(s.len() as u32).to_le_bytes());
        link.extend_from_slice(&s);
        link.resize(total, 0);
        for (id, label) in [(0x13u32, "m"), (0x14, "mm")] {
            link.extend_from_slice(&[7, 0x10, 0xd2, 0x56]);
            link.extend_from_slice(&id.to_le_bytes());
            link.extend_from_slice(&(label.len() as u32).to_le_bytes());
            let mut l = label.as_bytes().to_vec();
            l.resize(4, 0);
            link.extend_from_slice(&l);
        }
        let flags = if is_3d { 0 } else { MODEL_2D_FLAG };
        let rec = element(66 | flags, &body, &link);
        let mut stream = vec![0u8; 0x1000];
        stream.extend_from_slice(&[0, 0, 0, 0]);
        stream.extend_from_slice(&rec);
        zlib::compress(&stream)
    }

    #[test]
    fn decodes_model_header_fields() {
        let z = model_header_stream(false, 10_000.0, "my_model");
        let raw = zlib::inflate(&z, 1 << 20).data;
        let h = parse_model_header(&raw, encoding_rs::WINDOWS_1252).unwrap();
        assert!(!h.is_3d);
        assert_eq!(h.uor_per_master, 10_000.0);
        assert_eq!(h.global_origin, [5.0, 0.0, 0.0]);
        assert_eq!(h.name.as_deref(), Some("my_model"));
        assert_eq!(h.master.label.as_deref(), Some("m"));
        assert_eq!(h.master.meters(), Some(1.0));
        assert_eq!(h.sub.meters(), Some(0.001));
        assert_eq!(h.sub.label.as_deref(), Some("mm"));
        assert!(parse_model_header(&raw[..raw.len() - 2], encoding_rs::WINDOWS_1252).is_none());
    }

    #[test]
    fn shared_sector_chains_are_charged_to_the_budget() {
        // 100 page streams whose directory entries all point at the same 5000-byte chain:
        // reading them copies the chain 100 times, which must count against the budget.
        let mut streams: Vec<(String, Vec<u8>)> = vec![("Dgn~H".into(), vec![0; 64])];
        for i in 0..100 {
            streams.push((format!("Dgn^Nm/${i}"), vec![i as u8; 5000]));
        }
        let refs: Vec<(&str, Vec<u8>)> = streams
            .iter()
            .map(|(p, d)| (p.as_str(), d.clone()))
            .collect();
        let mut file = crate::cfb::tests::build(&refs);
        let cf = CompoundFile::parse(&file).unwrap();
        let first = cf.entries().iter().find(|e| e.name == "$0").unwrap().start;
        let dir_start = u32::from_le_bytes(file[0x30..0x34].try_into().unwrap()) as usize;
        let names: Vec<String> = cf.entries().iter().map(|e| e.name.clone()).collect();
        for (i, name) in names.iter().enumerate() {
            if name.starts_with('$') {
                let at = (dir_start + 1) * 512 + i * 128 + 116;
                file[at..at + 4].copy_from_slice(&first.to_le_bytes());
            }
        }
        let tight = ReadOptions {
            limits: Limits {
                max_decompressed_bytes: 200_000,
                ..Limits::default()
            },
            ..ReadOptions::default()
        };
        assert!(matches!(read(&file, &tight), Err(Error::LimitExceeded(_))));
        // With room for the copies the file reads (its pages are garbage: warnings only).
        let f = read(&file, &ReadOptions::default()).unwrap();
        assert_eq!(f.named_pages.len(), 100);
        assert!(!f.warnings.is_empty());
    }

    #[test]
    fn summary_offsets_are_checked() {
        // Section offset and property offsets near u32::MAX must not overflow or panic.
        let mut b = vec![0u8; 64];
        b[44..48].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse_summary(&b), SummaryInfo::default());
        let mut b = vec![0u8; 96];
        b[44..48].copy_from_slice(&48u32.to_le_bytes());
        b[52..56].copy_from_slice(&1u32.to_le_bytes());
        b[56..60].copy_from_slice(&0x12u32.to_le_bytes());
        b[60..64].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse_summary(&b).application, None);
        // A well-formed UTF-16 application name.
        let mut b = vec![0u8; 48];
        b[44..48].copy_from_slice(&48u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&0x12u32.to_le_bytes());
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&31u32.to_le_bytes());
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&[b'X', 0, b'Y', 0]);
        assert_eq!(parse_summary(&b).application.as_deref(), Some("XY"));
    }

    #[test]
    fn parses_model_index_entries() {
        let mut b = Vec::new();
        for v in [MODEL_INDEX_MAGIC, 4, 1, 0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let name: Vec<u8> = "M".encode_utf16().flat_map(u16::to_le_bytes).collect();
        b.extend_from_slice(&0x0001_0002u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&9u64.to_le_bytes());
        b.extend_from_slice(&(34u16).to_le_bytes());
        b.extend_from_slice(&(2u16).to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&[0u8; 8]);
        b.extend_from_slice(&name);
        let mut w = Vec::new();
        let e = parse_model_index(&b, encoding_rs::WINDOWS_1252, &mut w);
        assert_eq!(e.len(), 1);
        assert_eq!((e[0].storage_index, e[0].model_number, e[0].id), (2, 1, 9));
        assert_eq!(e[0].name, "M");
        assert!(parse_model_index(&[1, 2, 3], encoding_rs::WINDOWS_1252, &mut w).is_empty());
    }
}
