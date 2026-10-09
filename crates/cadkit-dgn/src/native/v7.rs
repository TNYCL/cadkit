//! DGN V7 (ISFF) records.
//!
//! A V7 file is a flat sequence of records ending with `ff ff`. Record header: byte 0 =
//! level (low 6 bits) and complex bit `0x80`; byte 1 = type (low 7 bits) and deleted bit
//! `0x80`; `u16` words to follow. Most elements then carry the 36-byte display header
//! (range, graphic group, attribute index, properties, symbology). 32-bit integers are
//! stored as two little-endian words, high word first; doubles are VAX D-float.
//! Layouts follow GDAL dgnlib (`dgnread.cpp`, MIT/X); see `docs/dgn/FORMAT_NOTES.md`.

use cadkit_core::{Error, Limits, ReadOptions, Result, Warning};
use encoding_rs::Encoding;

use crate::budget::Budget;
use crate::le;
use crate::native::decode_v8::{parse_tag_defs, tag_value};
use crate::native::element::{
    CellData, Element, ElementData, ElementHeader, Rotation, TagData, TagSetData, TagValue,
    TcbData, TextData, TextNodeData, UorPoint,
};
use crate::native::linkage::{self, LinkageData};
use crate::text;

/// V7 angles are stored in 1/360000 degree.
const ANGLE_UNIT: f64 = std::f64::consts::PI / 180.0 / 360_000.0;
/// Text size multipliers are in 1/1000 of 6 UOR (GDAL dgnlib).
const TEXT_MULT_TO_UOR: f64 = 6.0 / 1000.0;
/// Cell matrices are integers scaled so that 1.0 = 2^31 / 10000 (GDAL `214748`).
const CELL_MATRIX_UNIT: f64 = 10_000.0 / 2_147_483_648.0;
/// Knots and weights are fractions of `2^31 - 1`.
const SCALAR_UNIT: f64 = 2_147_483_647.0;

/// One V7 record exactly as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V7Record {
    /// File offset of the record.
    pub offset: usize,
    /// Complete record bytes including the 4-byte header.
    pub bytes: Vec<u8>,
}

impl V7Record {
    /// Element type.
    pub fn type_code(&self) -> u16 {
        u16::from(le::u8_at(&self.bytes, 1).unwrap_or(0) & 0x7f)
    }

    /// Level 0..=63.
    pub fn level(&self) -> u8 {
        le::u8_at(&self.bytes, 0).unwrap_or(0) & 0x3f
    }

    /// Complex component bit.
    pub fn is_complex(&self) -> bool {
        le::u8_at(&self.bytes, 0).unwrap_or(0) & 0x80 != 0
    }

    /// Deleted bit.
    pub fn is_deleted(&self) -> bool {
        le::u8_at(&self.bytes, 1).unwrap_or(0) & 0x80 != 0
    }
}

/// A whole V7 file at the record level.
#[derive(Debug, Clone, PartialEq)]
pub struct V7File {
    /// The design is 3D (header byte `0xC8` or TCB flag).
    pub is_3d: bool,
    /// Records in file order (including the TCB).
    pub records: Vec<V7Record>,
    /// Decoded design header (first type-9 record).
    pub tcb: Option<TcbData>,
    /// Offset where the walk stopped (the `ff ff` end marker when present).
    pub end_offset: usize,
    /// Recoverable problems.
    pub warnings: Vec<Warning>,
}

/// True for a V7 design file or cell library header (GDAL `DGNTestOpen`).
pub fn sniff(b: &[u8]) -> bool {
    match b.get(..4) {
        Some([0x08, 0x05, 0x17, 0x00]) => true,
        Some([b0, 0x09, 0xFE, 0x02]) => *b0 == 0x08 || *b0 == 0xC8,
        _ => false,
    }
}

/// Walks every record of a V7 file.
pub fn read(bytes: &[u8], options: &ReadOptions) -> Result<V7File> {
    let mut budget = Budget::new(options.limits);
    let enc = text::encoding_for(options.fallback_codepage.as_deref());
    read_with(bytes, &mut budget, enc)
}

pub(crate) fn read_with(
    bytes: &[u8],
    budget: &mut Budget,
    enc: &'static Encoding,
) -> Result<V7File> {
    if !sniff(bytes) {
        return Err(Error::UnknownFormat);
    }
    if bytes.len() as u64 > budget.limits.max_input_bytes {
        return Err(Error::LimitExceeded(format!(
            "input exceeds {} bytes",
            budget.limits.max_input_bytes
        )));
    }
    let mut warnings = Vec::new();
    let mut records = Vec::new();
    let mut off = 0usize;
    while let Some(head) = le::bytes(bytes, off, 4) {
        if head.starts_with(&[0xff, 0xff]) {
            break;
        }
        let words = usize::from(le::u16_at(head, 2).unwrap_or(0));
        let len = 4 + 2 * words;
        let Some(rec) = le::bytes(bytes, off, len) else {
            warnings.push(Warning {
                code: "dgn.v7_truncated".into(),
                message: format!("record at {off} declares {len} bytes but the file ends"),
                offset: Some(off as u64),
                object: None,
            });
            break;
        };
        budget.charge_objects(1)?;
        records.push(V7Record {
            offset: off,
            bytes: rec.to_vec(),
        });
        off += len;
    }
    let tcb = records
        .iter()
        .find(|r| r.type_code() == 9)
        .and_then(|r| parse_tcb(&r.bytes, enc));
    let is_3d = bytes.first() == Some(&0xC8) || tcb.as_ref().is_some_and(|t| t.is_3d);
    Ok(V7File {
        is_3d,
        records,
        tcb,
        end_offset: off,
        warnings,
    })
}

/// Design file header (GDAL `DGNParseTCB`).
fn parse_tcb(b: &[u8], enc: &'static Encoding) -> Option<TcbData> {
    let label = |off| {
        text::decode_8bit(le::bytes(b, off, 2).unwrap_or(&[]), enc)
            .trim()
            .to_owned()
    };
    Some(TcbData {
        is_3d: le::u8_at(b, 1214)? & 0x40 != 0,
        subunits_per_master: le::v7_i32_at(b, 1112)?,
        uor_per_subunit: le::v7_i32_at(b, 1116)?,
        master_label: label(1120),
        sub_label: label(1122),
        origin: [
            le::vax_f64_at(b, 1240)?,
            le::vax_f64_at(b, 1248)?,
            le::vax_f64_at(b, 1256)?,
        ],
    })
}

/// Types without the 36-byte display header (GDAL `DGNElemTypeHasDispHdr`).
fn has_display_header(t: u16) -> bool {
    !matches!(t, 0 | 1 | 9 | 10 | 32 | 44 | 48..=51 | 57 | 60..=63)
}

/// Decodes one V7 record. Never fails; problems are reported in [`Element::problem`].
pub fn decode(rec: &V7Record, is_3d: bool, enc: &'static Encoding, limits: &Limits) -> Element {
    let b = &rec.bytes;
    let t = rec.type_code();
    let display = has_display_header(t) && b.len() >= 36;
    let properties = if display {
        u32::from(le::u16_at(b, 32).unwrap_or(0))
    } else {
        0
    };
    let symbology = if display {
        le::u16_at(b, 34).unwrap_or(0)
    } else {
        0
    };
    let range = display.then(|| {
        let r = |o| {
            le::v7_u32_at(b, o)
                .map(|v| f64::from(v) - 2_147_483_648.0)
                .unwrap_or(0.0)
        };
        [
            [r(4), r(8), if is_3d { r(12) } else { 0.0 }],
            [r(16), r(20), if is_3d { r(24) } else { 0.0 }],
        ]
    });
    let header = ElementHeader {
        type_code: t,
        level: u32::from(rec.level()),
        id: None,
        complex_header: matches!(t, 2 | 7 | 12 | 14 | 18 | 19 | 24 | 27 | 34),
        complex_component: rec.is_complex() && t != 9,
        deleted: rec.is_deleted(),
        is_3d,
        has_display_header: display,
        graphic_group: if display {
            u32::from(le::u16_at(b, 28).unwrap_or(0))
        } else {
            0
        },
        properties,
        style: u32::from(symbology & 0x7),
        weight: u32::from((symbology >> 3) & 0x1f),
        color: u32::from(symbology >> 8),
        range,
        type_flags: 0,
        modified_ms: None,
    };
    // Attribute data follows when the A bit (0x0800) is set: offset 32 + 2 * index.
    let attr_start = (display && properties & 0x0800 != 0)
        .then(|| le::u16_at(b, 30).map(|i| 32 + 2 * usize::from(i)))
        .flatten()
        .filter(|&s| s <= b.len());
    let linkages = attr_start
        .map(|s| linkage::parse(b.get(s..).unwrap_or(&[]), s, false, enc))
        .unwrap_or_default();
    let d = V7 {
        b,
        end: attr_start.unwrap_or(b.len()),
        is_3d,
        enc,
        limits,
    };
    let result = match t {
        2 => d.cell(),
        3 => d.line(),
        4 => d.points(2).map(|points| ElementData::LineString { points }),
        5 if rec.level() == 1 => d.color_table(),
        6 => d.points(2).map(|points| ElementData::Shape { points }),
        7 => d.text_node(),
        9 => parse_tcb(b, enc).map(ElementData::Tcb).ok_or_else(short),
        11 => d.points(2).map(|points| ElementData::Curve { points }),
        12 | 14 | 18 | 19 => le::u16_at(b, 38)
            .map(|n| ElementData::Complex {
                children: u32::from(n),
            })
            .ok_or_else(short),
        15 => d.ellipse(),
        16 => d.arc(),
        17 => d.text(),
        21 => d
            .points(1)
            .map(|points| ElementData::BSplinePoles { points }),
        22 => d.points(1).map(|points| ElementData::PointString {
            points,
            orientations: Vec::new(),
        }),
        26 => Ok(ElementData::BSplineKnots {
            values: d.scalars(),
        }),
        27 => d.bspline_header(),
        28 => Ok(ElementData::BSplineWeights {
            values: d.scalars(),
        }),
        34 => d.shared_cell(true),
        35 => d.shared_cell(false),
        37 => d.tag(),
        66 if rec.level() == 24 => d.tag_set(&linkages),
        _ => Ok(ElementData::Unknown),
    };
    let (mut data, problem) = match result {
        Ok(data) => (data, None),
        Err(p) => (ElementData::Unknown, Some(p)),
    };
    // V7 stores no text length. For unrotated text whose range starts at the origin (the
    // origin is the lower-left corner, GDAL dgnlib), the range width is the text length
    // (smalltest.dgn: origin x = range low x).
    if let (ElementData::Text(t), Some([lo, hi])) = (&mut data, header.range) {
        let unrotated = matches!(t.rotation, Rotation::Angle(a) if a == 0.0);
        let [ox, ..] = t.origin;
        let [lx, ..] = lo;
        let [hx, ..] = hi;
        if unrotated && (lx - ox).abs() <= 1.0 && hx > lx {
            t.measured_length = Some(hx - lx);
        }
    }
    Element {
        header,
        data,
        linkages,
        problem,
    }
}

fn short() -> String {
    "record is too short".to_owned()
}

type R<T> = std::result::Result<T, String>;

struct V7<'a> {
    b: &'a [u8],
    /// End of the element data (start of the attribute linkages).
    end: usize,
    is_3d: bool,
    enc: &'static Encoding,
    limits: &'a Limits,
}

impl V7<'_> {
    fn int(&self, off: usize) -> R<f64> {
        le::v7_i32_at(self.b, off).map(f64::from).ok_or_else(short)
    }

    fn vax(&self, off: usize) -> R<f64> {
        le::vax_f64_at(self.b, off).ok_or_else(short)
    }

    fn ipt(&self, off: usize) -> R<UorPoint> {
        Ok([
            self.int(off)?,
            self.int(off + 4)?,
            if self.is_3d { self.int(off + 8)? } else { 0.0 },
        ])
    }

    fn dpt(&self, off: usize) -> R<UorPoint> {
        Ok([
            self.vax(off)?,
            self.vax(off + 8)?,
            if self.is_3d { self.vax(off + 16)? } else { 0.0 },
        ])
    }

    fn psize(&self) -> usize {
        if self.is_3d { 12 } else { 8 }
    }

    /// Quaternion stored as four 32-bit integers (scaled by 2^31).
    fn quat(&self, off: usize) -> R<Rotation> {
        let q = [
            self.int(off)?,
            self.int(off + 4)?,
            self.int(off + 8)?,
            self.int(off + 12)?,
        ];
        Ok(Rotation::Quaternion(q.map(|v| v / 2_147_483_648.0)))
    }

    fn angle(&self, off: usize) -> R<Rotation> {
        Ok(Rotation::Angle(self.int(off)? * ANGLE_UNIT))
    }

    fn line(&self) -> R<ElementData> {
        Ok(ElementData::Line {
            start: self.ipt(36)?,
            end: self.ipt(36 + self.psize())?,
        })
    }

    fn points(&self, min: usize) -> R<Vec<UorPoint>> {
        let count = usize::from(le::u16_at(self.b, 36).ok_or_else(short)?);
        if count < min {
            return Err(format!("{count} points, at least {min} required"));
        }
        if count as u64 > u64::from(self.limits.max_vertices) {
            return Err(format!("{count} points exceed the vertex limit"));
        }
        // Like GDAL, keep the points that fit when the record is short.
        let fit = self.b.len().saturating_sub(38) / self.psize();
        (0..count.min(fit))
            .map(|i| self.ipt(38 + i * self.psize()))
            .collect()
    }

    fn ellipse(&self) -> R<ElementData> {
        let primary = self.vax(36)?;
        let secondary = self.vax(44)?;
        let (rotation, center) = if self.is_3d {
            (self.quat(52)?, self.dpt(68)?)
        } else {
            (self.angle(52)?, self.dpt(56)?)
        };
        Ok(ElementData::Ellipse {
            primary,
            secondary,
            rotation,
            center,
        })
    }

    fn arc(&self) -> R<ElementData> {
        let start = self.int(36)? * ANGLE_UNIT;
        // Sweep is sign-magnitude; zero means a full turn.
        let raw = le::v7_u32_at(self.b, 40).ok_or_else(short)?;
        let magnitude = f64::from(raw & 0x7fff_ffff) * ANGLE_UNIT;
        let sweep = match (raw & 0x7fff_ffff, raw & 0x8000_0000 != 0) {
            (0, _) => std::f64::consts::TAU,
            (_, true) => -magnitude,
            _ => magnitude,
        };
        let primary = self.vax(44)?;
        let secondary = self.vax(52)?;
        let (rotation, center) = if self.is_3d {
            (self.quat(60)?, self.dpt(76)?)
        } else {
            (self.angle(60)?, self.dpt(64)?)
        };
        Ok(ElementData::Arc {
            start,
            sweep,
            primary,
            secondary,
            rotation,
            center,
        })
    }

    fn text(&self) -> R<ElementData> {
        let (rotation, origin, len_off) = if self.is_3d {
            (self.quat(46)?, self.ipt(62)?, 74)
        } else {
            (self.angle(46)?, self.ipt(50)?, 58)
        };
        let len = usize::from(le::u8_at(self.b, len_off).ok_or_else(short)?);
        let raw_text = le::bytes(self.b, len_off + 2, len)
            .ok_or_else(short)?
            .to_vec();
        let (text, encoding) = text::decode_v7_text(&raw_text, self.enc);
        Ok(ElementData::Text(TextData {
            font: u32::from(le::u8_at(self.b, 36).unwrap_or(0)),
            justification: u16::from(le::u8_at(self.b, 37).unwrap_or(0)),
            width: self.int(38)? * TEXT_MULT_TO_UOR,
            height: self.int(42)? * TEXT_MULT_TO_UOR,
            rotation,
            origin,
            text,
            encoding,
            raw_text,
            measured_length: None,
        }))
    }

    fn text_node(&self) -> R<ElementData> {
        let (rotation, origin) = if self.is_3d {
            (self.quat(58)?, self.ipt(74)?)
        } else {
            (self.angle(58)?, self.ipt(62)?)
        };
        Ok(ElementData::TextNode(TextNodeData {
            children: u32::from(le::u16_at(self.b, 38).ok_or_else(short)?),
            node_number: u32::from(le::u16_at(self.b, 40).unwrap_or(0)),
            font: u32::from(le::u8_at(self.b, 44).unwrap_or(0)),
            justification: u16::from(le::u8_at(self.b, 45).unwrap_or(0)),
            line_spacing: self.int(46)?,
            width: self.int(50)? * TEXT_MULT_TO_UOR,
            height: self.int(54)? * TEXT_MULT_TO_UOR,
            rotation,
            origin,
        }))
    }

    fn cell(&self) -> R<ElementData> {
        let name = format!(
            "{}{}",
            text::rad50(le::u16_at(self.b, 38).ok_or_else(short)?),
            text::rad50(le::u16_at(self.b, 40).ok_or_else(short)?)
        );
        let (lo, hi, m, n, o) = if self.is_3d {
            (52, 64, 76, 9, 112)
        } else {
            (52, 60, 68, 4, 84)
        };
        let mut raw = [0.0f64; 9];
        for (i, v) in raw.iter_mut().take(n).enumerate() {
            *v = self.int(m + 4 * i)? * CELL_MATRIX_UNIT;
        }
        let matrix = if self.is_3d {
            raw
        } else {
            let [a, b, c, d, ..] = raw;
            [a, b, 0.0, c, d, 0.0, 0.0, 0.0, 1.0]
        };
        Ok(ElementData::Cell(CellData {
            name: Some(name.trim().to_owned()).filter(|s| !s.is_empty()),
            description: None,
            children: 0,
            range: Some([self.ipt(lo)?, self.ipt(hi)?]),
            matrix,
            origin: self.ipt(o)?,
        }))
    }

    /// V7 shared cells (2D layout from ezdgn `entities.rs`): VAX 2x2 transform at 76,
    /// origin (plain little-endian i32) at 148, 16-byte name at 164.
    fn shared_cell(&self, definition: bool) -> R<ElementData> {
        let name =
            le::bytes(self.b, 164, 16).map(|n| text::decode_8bit(n, self.enc).trim().to_owned());
        let t = [self.vax(76)?, self.vax(84)?, self.vax(92)?, self.vax(100)?];
        let o = |i: usize| {
            le::array::<4>(self.b, 148 + 4 * i).map(|a| f64::from(i32::from_le_bytes(a)))
        };
        let cell = CellData {
            name: name.filter(|s| !s.is_empty()),
            description: None,
            children: 0,
            range: None,
            matrix: [t[0], t[1], 0.0, t[2], t[3], 0.0, 0.0, 0.0, 1.0],
            origin: if definition {
                [0.0; 3]
            } else {
                [o(0).ok_or_else(short)?, o(1).ok_or_else(short)?, 0.0]
            },
        };
        Ok(if definition {
            ElementData::SharedCellDefinition(cell)
        } else {
            ElementData::SharedCellInstance(cell)
        })
    }

    fn bspline_header(&self) -> R<ElementData> {
        let of = le::u8_at(self.b, 40).ok_or_else(short)?;
        Ok(ElementData::BSplineCurve {
            children: 0,
            order: (of & 0x0f) + 2,
            flags: of & 0xf0,
            curve_type: le::u8_at(self.b, 41).unwrap_or(0),
            closed: of & 0x80 != 0,
            num_poles: u32::from(le::u16_at(self.b, 42).unwrap_or(0)),
            num_knots: u32::from(le::u16_at(self.b, 44).unwrap_or(0)),
        })
    }

    fn scalars(&self) -> Vec<f64> {
        let n = (self.end.saturating_sub(36) / 4).min(self.limits.max_vertices as usize);
        (0..n)
            .map_while(|i| le::v7_i32_at(self.b, 36 + 4 * i))
            .map(|v| f64::from(v) / SCALAR_UNIT)
            .collect()
    }

    fn color_table(&self) -> R<ElementData> {
        // First triple is color 255, then colors 0..=254 (GDAL `DGNParseColorTable`).
        let rgb = le::bytes(self.b, 38, 768).ok_or_else(short)?;
        let mut colors = vec![[0u8; 3]; 256];
        for (i, c) in rgb.chunks_exact(3).enumerate() {
            let slot = if i == 0 { 255 } else { i - 1 };
            if let (Some(dst), [r, g, b]) = (colors.get_mut(slot), c) {
                *dst = [*r, *g, *b];
            }
        }
        Ok(ElementData::ColorTable { colors })
    }

    /// Tag value (GDAL `DGNT_TAG_VALUE`): set number at 68, tag index at 72, type at 74,
    /// value length at 150, value at 154. The tagged element is not identified here.
    fn tag(&self) -> R<ElementData> {
        let value_type = le::u16_at(self.b, 74).ok_or_else(short)?;
        let value = match value_type {
            1 => TagValue::Text(
                text::cstr_at(self.b, 154, self.enc)
                    .map(|(s, _)| s)
                    .ok_or_else(short)?,
            ),
            4 => TagValue::Float(self.vax(154)?),
            _ => {
                let len = usize::from(le::u16_at(self.b, 150).unwrap_or(0));
                let raw = le::bytes(self.b, 154, len.min(self.b.len().saturating_sub(154)))
                    .unwrap_or(&[]);
                tag_value(value_type, raw, self.enc)
            }
        };
        Ok(ElementData::Tag(TagData {
            set_id: None,
            set_number: le::u32_at(self.b, 68),
            tag_index: le::u16_at(self.b, 72).unwrap_or(0),
            value_type,
            value,
            target: None,
            origin: [0.0; 3],
            offset: [0.0; 3],
            size: [0.0; 2],
            font: 0,
            justification: 0,
            quaternion: [0.0; 4],
        }))
    }

    /// Tag set (type 66, level 24, GDAL `DGNParseTagSet`): name at 48, definitions follow;
    /// the set number is in an association linkage `03 10 2f 7d`.
    fn tag_set(&self, links: &[linkage::Linkage]) -> R<ElementData> {
        let (name, next) = text::cstr_at(self.b, 48, self.enc).ok_or_else(short)?;
        let count = usize::from(le::u16_at(self.b, 44).unwrap_or(0));
        let defs = self.b.get(next + 1..self.end.max(next + 1)).unwrap_or(&[]);
        let mut tags = parse_tag_defs(defs, self.enc);
        tags.truncate(count);
        let number = links.iter().find_map(|l| match l.data {
            LinkageData::AssocId(v) => Some(v & 0xffff),
            _ => None,
        });
        Ok(ElementData::TagSet(TagSetData {
            name: Some(name),
            number,
            tags,
        }))
    }
}

/// Byte offset where a V7 complex header's description ends (ezdgn `container_descriptor`):
/// cells, text nodes and complex chains/shapes count words after byte 38; B-spline headers
/// count words after byte 40.
pub(crate) fn description_end(rec: &V7Record) -> Option<usize> {
    let (base, words) = match rec.type_code() {
        2 | 7 | 12 | 14 | 18 | 19 | 34 => (38usize, usize::from(le::u16_at(&rec.bytes, 36)?)),
        24 | 27 => (40, usize::try_from(le::v7_i32_at(&rec.bytes, 36)?).ok()?),
        _ => return None,
    };
    rec.offset
        .checked_add(base)?
        .checked_add(words.checked_mul(2)?)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A V7 record: type, level, then `body` from byte 4.
    pub(crate) fn record(t: u8, level: u8, body: &[u8]) -> Vec<u8> {
        let mut r = vec![level, t, 0, 0];
        r.extend_from_slice(body);
        if r.len() % 2 == 1 {
            r.push(0);
        }
        let words = ((r.len() - 4) / 2) as u16;
        r[2..4].copy_from_slice(&words.to_le_bytes());
        r
    }

    pub(crate) fn mid(v: i32) -> [u8; 4] {
        let b = v.to_le_bytes();
        [b[2], b[3], b[0], b[1]]
    }

    /// Display header bytes 4..36 with the given symbology.
    pub(crate) fn disp(color: u8, weight: u8, style: u8) -> Vec<u8> {
        let mut h = vec![0u8; 32];
        h[30..32].copy_from_slice(&[(weight << 3) | style, color]);
        h
    }

    fn rec(bytes: Vec<u8>) -> V7Record {
        V7Record { offset: 0, bytes }
    }

    fn enc() -> &'static Encoding {
        encoding_rs::WINDOWS_1252
    }

    #[test]
    fn sniff_accepts_design_files_only() {
        assert!(sniff(&[0x08, 0x09, 0xFE, 0x02, 0, 0]));
        assert!(sniff(&[0xC8, 0x09, 0xFE, 0x02]));
        assert!(sniff(&[0x08, 0x05, 0x17, 0x00]));
        assert!(!sniff(&[0x08, 0x09, 0xFE]));
        assert!(!sniff(b"PK\x03\x04"));
    }

    #[test]
    fn decodes_line_and_symbology() {
        let mut body = disp(3, 5, 4);
        for v in [25562, 57218, 25242, 60709] {
            body.extend(mid(v));
        }
        let e = decode(&rec(record(3, 2, &body)), false, enc(), &Limits::default());
        assert_eq!(
            (
                e.header.level,
                e.header.color,
                e.header.weight,
                e.header.style
            ),
            (2, 3, 5, 4)
        );
        assert_eq!(
            e.data,
            ElementData::Line {
                start: [25562.0, 57218.0, 0.0],
                end: [25242.0, 60709.0, 0.0]
            }
        );
    }

    #[test]
    fn decodes_text_and_arc_sweep_sign() {
        let mut body = disp(0, 0, 0);
        body.extend(&[3, 7]);
        body.extend(mid(1000));
        body.extend(mid(2000));
        body.extend(mid(90 * 360_000));
        body.extend(mid(7365));
        body.extend(mid(42198));
        body.extend(&[4, 0]);
        body.extend(b"Demo");
        let e = decode(&rec(record(17, 1, &body)), false, enc(), &Limits::default());
        let ElementData::Text(t) = &e.data else {
            panic!("{e:?}")
        };
        assert_eq!(t.text, "Demo");
        assert_eq!((t.font, t.justification), (3, 7));
        assert_eq!(t.origin, [7365.0, 42198.0, 0.0]);
        assert!((t.height - 12.0).abs() < 1e-9);
        assert_eq!(t.rotation, Rotation::Angle(std::f64::consts::FRAC_PI_2));

        let mut body = disp(0, 0, 0);
        body.extend(mid(0));
        let sweep = (90u32 * 360_000) | 0x8000_0000;
        body.extend(mid(sweep as i32));
        body.extend([0u8; 40]);
        let e = decode(&rec(record(16, 1, &body)), false, enc(), &Limits::default());
        let ElementData::Arc { sweep, .. } = e.data else {
            panic!("{e:?}")
        };
        assert!((sweep + std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn walks_records_until_end_marker() {
        let mut file = record(9, 8, &[0u8; 1532]);
        file[0] = 0x08;
        file[1] = 0x09;
        file[2..4].copy_from_slice(&766u16.to_le_bytes());
        file[2] = 0xFE;
        file[3] = 0x02;
        let mut body = disp(0, 0, 0);
        body.extend([0u8; 16]);
        file.extend(record(3, 1, &body));
        file.extend([0xff, 0xff, 0x12, 0x34]);
        let f = read(&file, &ReadOptions::default()).unwrap();
        assert_eq!(f.records.len(), 2);
        assert_eq!(f.records[1].type_code(), 3);
        assert_eq!(f.end_offset, file.len() - 4);
        // Truncation inside a record is a warning, not an error.
        let f = read(&file[..file.len() - 10], &ReadOptions::default()).unwrap();
        assert_eq!(f.records.len(), 1);
        assert_eq!(f.warnings.len(), 1);
    }

    #[test]
    fn description_end_uses_the_total_length() {
        let mut body = disp(0, 0, 0);
        body.extend(&10u16.to_le_bytes());
        body.extend(&2u16.to_le_bytes());
        let r = V7Record {
            offset: 100,
            bytes: record(12, 1, &body),
        };
        assert_eq!(description_end(&r), Some(100 + 38 + 20));
        let r = V7Record {
            offset: 0,
            bytes: vec![0, 12, 0, 0],
        };
        assert_eq!(description_end(&r), None);
    }

    #[test]
    fn short_records_degrade() {
        for t in [
            2u8, 3, 4, 5, 6, 7, 9, 11, 12, 15, 16, 17, 21, 22, 26, 27, 34, 35, 37, 66,
        ] {
            let e = decode(
                &rec(record(t, 1, &disp(0, 0, 0))),
                true,
                enc(),
                &Limits::default(),
            );
            let _ = e.data;
            let e = decode(&rec(vec![1, t, 0, 0]), false, enc(), &Limits::default());
            assert!(!e.header.has_display_header || t == 0);
        }
    }
}
