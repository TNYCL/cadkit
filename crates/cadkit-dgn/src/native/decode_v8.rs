//! V8 element bodies -> [`Element`].
//!
//! Offsets are from the element start (after the 4-byte page record prefix). The standard
//! graphic header occupies `0x00..0x68`; see `docs/dgn/FORMAT_NOTES.md` for every table.

use cadkit_core::Limits;
use encoding_rs::Encoding;

use crate::le;
use crate::native::element::{
    CellData, Element, ElementData, ElementHeader, LevelData, Rotation, TagData, TagDef,
    TagSetData, TagValue, TextData, TextNodeData, UorPoint,
};
use crate::native::linkage::{self, LinkageData};
use crate::native::v8::RawElement;
use crate::text;

const HEADER_LEN: usize = 0x68;
const FLAG_3D: u32 = 0x0800;
/// Text size multipliers are stored in 1/1000 of 6 UOR (GDAL dgnlib, ezdgn).
const TEXT_MULT_TO_UOR: f64 = 6.0 / 1000.0;
/// Dependency application ids observed on tag elements.
pub(crate) const DEP_TAG_SET: u16 = 0x2717;
pub(crate) const DEP_TAG_TARGET: u16 = 0x2710;
/// Table numbers (the level slot of type 95/96 elements).
pub(crate) const TABLE_LEVELS: u32 = 1;
pub(crate) const TABLE_FONTS: u32 = 2;

/// Element types without the standard 0x68-byte display header.
fn has_display_header(t: u16) -> bool {
    !matches!(t, 0 | 1 | 5 | 8 | 9 | 10 | 39 | 57 | 63 | 66 | 90..=93 | 95..=99)
}

/// Decodes a raw V8 element. Never fails: undecodable bodies become
/// [`ElementData::Unknown`] with `problem` set.
pub fn decode(raw: &RawElement, enc: &'static Encoding, limits: &Limits) -> Element {
    let b = &raw.bytes;
    let t = raw.type_code();
    let display = has_display_header(t) && b.len() >= HEADER_LEN;
    let properties = if display {
        le::u32_at(b, 0x28).unwrap_or(0)
    } else {
        0
    };
    let range = if display {
        let r = |o: usize| le::i64_at(b, o).map(|v| v as f64).unwrap_or(0.0);
        Some([[r(0x38), r(0x40), r(0x48)], [r(0x50), r(0x58), r(0x60)]])
    } else {
        None
    };
    let header = ElementHeader {
        type_code: t,
        level: le::u32_at(b, 0x0c).unwrap_or(0),
        id: le::u64_at(b, 0x10),
        complex_header: raw.is_complex_header(),
        complex_component: raw.is_complex_component(),
        deleted: false,
        is_3d: properties & FLAG_3D != 0,
        has_display_header: display,
        graphic_group: if display {
            le::u32_at(b, 0x20).unwrap_or(0)
        } else {
            0
        },
        properties,
        style: if display {
            le::u32_at(b, 0x2c).unwrap_or(0)
        } else {
            0
        },
        weight: if display {
            le::u32_at(b, 0x30).unwrap_or(0)
        } else {
            0
        },
        color: if display {
            le::u32_at(b, 0x34).unwrap_or(0)
        } else {
            0
        },
        range,
        type_flags: (raw.type_word >> 16) as u16,
        modified_ms: le::f64_at(b, 0x18),
    };
    let attr = raw.attribute_offset();
    let linkages = linkage::parse(raw.attributes(), attr, true, enc);
    // Text carries its own code page linkage (observed 1252); prefer it for 8-bit text.
    let text_enc = linkages
        .iter()
        .find_map(|l| match l.data {
            LinkageData::CodePage(cp) => {
                u16::try_from(cp).ok().and_then(text::encoding_for_codepage)
            }
            _ => None,
        })
        .unwrap_or(enc);
    let d = Decoder {
        b: raw.primary(),
        whole: b,
        is_3d: header.is_3d,
        limits,
        enc: text_enc,
    };
    let result = match t {
        2 => d.cell().map(ElementData::Cell),
        3 => d.line(),
        4 => d.points(2).map(|points| ElementData::LineString { points }),
        // Type 5 is "group data"; only the one in level slot 1 is a color table (as in V7).
        5 if header.level == 1 => d.color_table(),
        6 => d.points(2).map(|points| ElementData::Shape { points }),
        7 => d.text_node().map(ElementData::TextNode),
        11 => d.points(2).map(|points| ElementData::Curve { points }),
        12 | 14 | 18 | 19 => le::u32_at(d.b, 0x68)
            .map(|children| ElementData::Complex { children })
            .ok_or_else(short),
        15 => d.ellipse(),
        16 => d.arc(),
        17 => d.text().map(ElementData::Text),
        21 => d
            .points(1)
            .map(|points| ElementData::BSplinePoles { points }),
        22 => d.point_string(),
        26 => Ok(ElementData::BSplineKnots {
            values: d.scalars(),
        }),
        27 => d.bspline_header(),
        28 => Ok(ElementData::BSplineWeights {
            values: d.scalars(),
        }),
        34 => d.cell().map(ElementData::SharedCellDefinition),
        35 => d.cell().map(ElementData::SharedCellInstance),
        37 => d.tag(&linkages).map(ElementData::Tag),
        39 => d.tag_set(&linkages).map(ElementData::TagSet),
        90 => Ok(ElementData::RasterReference {
            file_name: linkage::string(&linkages, 3),
            full_path: linkage::string(&linkages, 0x1f),
        }),
        94 => le::f64s::<16>(d.b, 0x78)
            .map(|matrix| ElementData::RasterFrame {
                matrix,
                corner: le::f64s::<2>(d.b, 0x108),
            })
            .ok_or_else(short),
        95 => d.table_entry(header.level, &linkages),
        96 => Ok(ElementData::TableHeader {
            table: header.level,
            children: le::u32_at(d.b, 0x20).unwrap_or(0),
        }),
        _ if raw.is_complex_header() && display => le::u32_at(d.b, 0x68)
            .map(|children| ElementData::Complex { children })
            .ok_or_else(short),
        _ => Ok(ElementData::Unknown),
    };
    let (data, problem) = match result {
        Ok(data) => (data, None),
        Err(p) => (ElementData::Unknown, Some(p)),
    };
    Element {
        header,
        data,
        linkages,
        problem,
    }
}

fn short() -> String {
    "element body is too short".to_owned()
}

struct Decoder<'a> {
    /// Primary bytes (before the attribute offset).
    b: &'a [u8],
    /// Whole element (some writers place fixed fields past a too-small attribute offset).
    whole: &'a [u8],
    is_3d: bool,
    limits: &'a Limits,
    enc: &'static Encoding,
}

type R<T> = std::result::Result<T, String>;

impl Decoder<'_> {
    fn pt(&self, off: usize) -> R<UorPoint> {
        le::point_at(self.b, off, self.is_3d)
            .ok_or_else(|| format!("point at 0x{off:x} missing or not finite"))
    }

    fn f(&self, off: usize) -> R<f64> {
        le::f64_at(self.b, off).ok_or_else(|| format!("value at 0x{off:x} missing or not finite"))
    }

    fn point_size(&self) -> usize {
        if self.is_3d { 24 } else { 16 }
    }

    fn line(&self) -> R<ElementData> {
        let start = self.pt(HEADER_LEN)?;
        let end = self.pt(HEADER_LEN + self.point_size())?;
        Ok(ElementData::Line { start, end })
    }

    /// `u32` count at 0x68, padding, points from 0x70.
    fn points(&self, min: usize) -> R<Vec<UorPoint>> {
        let count = le::u32_at(self.b, HEADER_LEN).ok_or_else(short)? as usize;
        self.check_count(count, min)?;
        let size = self.point_size();
        if count.saturating_mul(size).saturating_add(0x70) > self.b.len() {
            return Err(format!(
                "{count} points do not fit in {} bytes",
                self.b.len()
            ));
        }
        (0..count).map(|i| self.pt(0x70 + i * size)).collect()
    }

    fn check_count(&self, count: usize, min: usize) -> R<()> {
        if count < min {
            return Err(format!("{count} points, at least {min} required"));
        }
        if count as u64 > u64::from(self.limits.max_vertices) {
            return Err(format!("{count} points exceed the vertex limit"));
        }
        Ok(())
    }

    fn point_string(&self) -> R<ElementData> {
        let points = self.points(1)?;
        let base = 0x70 + points.len() * self.point_size();
        let orientations = (0..points.len())
            .map_while(|i| le::f64s::<4>(self.b, base + 32 * i))
            .collect();
        Ok(ElementData::PointString {
            points,
            orientations,
        })
    }

    fn rotation(&self, off: usize) -> R<Rotation> {
        if self.is_3d {
            le::f64s::<4>(self.b, off)
                .map(Rotation::Quaternion)
                .ok_or_else(short)
        } else {
            self.f(off).map(Rotation::Angle)
        }
    }

    fn ellipse(&self) -> R<ElementData> {
        let primary = self.f(0x68)?;
        let secondary = self.f(0x70)?;
        let rotation = self.rotation(0x78)?;
        let center = self.pt(if self.is_3d { 0x98 } else { 0x80 })?;
        Ok(ElementData::Ellipse {
            primary,
            secondary,
            rotation,
            center,
        })
    }

    fn arc(&self) -> R<ElementData> {
        let start = self.f(0x68)?;
        let sweep = self.f(0x70)?;
        let primary = self.f(0x78)?;
        let secondary = self.f(0x80)?;
        let rotation = self.rotation(0x88)?;
        let center = self.pt(if self.is_3d { 0xa8 } else { 0x90 })?;
        Ok(ElementData::Arc {
            start,
            sweep,
            primary,
            secondary,
            rotation,
            center,
        })
    }

    fn text(&self) -> R<TextData> {
        let font = le::u32_at(self.b, 0x68).ok_or_else(short)?;
        let justification = le::u16_at(self.b, 0x6c).ok_or_else(short)?;
        let len = usize::from(le::u16_at(self.b, 0x6e).ok_or_else(short)?);
        let width = self.f(0x70)? * TEXT_MULT_TO_UOR;
        let height = self.f(0x78)? * TEXT_MULT_TO_UOR;
        let rotation = self.rotation(0x90)?;
        let (origin_off, text_off) = if self.is_3d {
            (0xb0, 0xca)
        } else {
            (0x98, 0xaa)
        };
        let origin = self.pt(origin_off)?;
        // The payload is anchored on the fixed layout; elements may be padded after it.
        let raw_text = le::bytes(self.whole, text_off, len)
            .ok_or_else(|| format!("text of {len} bytes at 0x{text_off:x} exceeds the element"))?
            .to_vec();
        let (text, encoding) = text::decode_dgn_string(&raw_text, self.enc);
        // 0x80: text length along the baseline in UOR, written by MicroStation (TreeText:
        // 583.33 for 12 characters of width 50); zero when the writer did not measure.
        let measured_length = le::f64_at(self.b, 0x80).filter(|v| *v > 0.0);
        Ok(TextData {
            font,
            justification,
            width,
            height,
            rotation,
            origin,
            text,
            encoding,
            raw_text,
            measured_length,
        })
    }

    fn text_node(&self) -> R<TextNodeData> {
        Ok(TextNodeData {
            children: le::u32_at(self.b, 0x68).ok_or_else(short)?,
            node_number: le::u32_at(self.b, 0x6c).unwrap_or(0),
            font: le::u32_at(self.b, 0x70).unwrap_or(0),
            justification: le::u16_at(self.b, 0x76).unwrap_or(0),
            line_spacing: le::f64_at(self.b, 0x78).unwrap_or(0.0),
            width: le::f64_at(self.b, 0x80).unwrap_or(0.0) * TEXT_MULT_TO_UOR,
            height: le::f64_at(self.b, 0x88).unwrap_or(0.0) * TEXT_MULT_TO_UOR,
            rotation: self.rotation(0x90).unwrap_or(Rotation::None),
            origin: self
                .pt(if self.is_3d { 0xb0 } else { 0x98 })
                .unwrap_or([0.0; 3]),
        })
    }

    /// Cell header (2), shared cell definition (34) and instance (35) share one layout;
    /// the 2D/3D variant is chosen by size because definitions are stored 3D even when
    /// their flag word says 2D (test_dgnv8.dgn).
    fn cell(&self) -> R<CellData> {
        let children = le::u32_at(self.b, 0x68).ok_or_else(short)?;
        let three = if self.b.len() >= 0x100 {
            true
        } else if self.b.len() >= 0xc0 {
            false
        } else {
            return Err(short());
        };
        let (lo, hi, m, o) = if three {
            (0x70, 0x88, 0xa0, 0xe8)
        } else {
            (0x70, 0x80, 0x90, 0xb0)
        };
        let point = |off| le::point_at(self.b, off, three).ok_or_else(short);
        let matrix = if three {
            le::f64s::<9>(self.b, m).ok_or_else(short)?
        } else {
            let [a, b, c, d] = le::f64s::<4>(self.b, m).ok_or_else(short)?;
            [a, b, 0.0, c, d, 0.0, 0.0, 0.0, 1.0]
        };
        Ok(CellData {
            name: None,
            description: None,
            children,
            range: Some([point(lo)?, point(hi)?]),
            matrix,
            origin: point(o)?,
        })
    }

    fn bspline_header(&self) -> R<ElementData> {
        let children = le::u32_at(self.b, 0x68).ok_or_else(short)?;
        let of = le::u8_at(self.b, 0x6c).ok_or_else(short)?;
        let next = le::u8_at(self.b, 0x6d).unwrap_or(0);
        Ok(ElementData::BSplineCurve {
            children,
            order: (of & 0x0f) + 2,
            flags: of & 0xf0,
            curve_type: next,
            // V7 keeps "closed" in bit 0x80 of the order byte. Both V8 sample curves have
            // that bit clear but bit 0 of the next byte set, and the oracle evaluates them
            // as periodic, so either bit closes the curve (FORMAT_NOTES, open question).
            closed: of & 0x80 != 0 || next & 0x01 != 0,
            num_poles: le::u32_at(self.b, 0x70).unwrap_or(0),
            num_knots: le::u32_at(self.b, 0x74).unwrap_or(0),
        })
    }

    /// Knot / weight vectors: doubles after the 0x20-byte identification prefix
    /// (layout unverified, see FORMAT_NOTES).
    fn scalars(&self) -> Vec<f64> {
        let n = self.b.len().saturating_sub(0x20) / 8;
        let n = n.min(self.limits.max_vertices as usize);
        (0..n)
            .map_while(|i| le::f64_at(self.b, 0x20 + 8 * i))
            .collect()
    }

    fn color_table(&self) -> R<ElementData> {
        // Like V7: a u16 screen flag, then 256 RGB triples whose first entry is color 255.
        let rgb = le::bytes(self.b, 0x22, 768).ok_or_else(short)?;
        let mut colors = vec![[0u8; 3]; 256];
        for (i, c) in rgb.chunks_exact(3).enumerate() {
            let slot = if i == 0 { 255 } else { i - 1 };
            if let (Some(dst), [r, g, b]) = (colors.get_mut(slot), c) {
                *dst = [*r, *g, *b];
            }
        }
        Ok(ElementData::ColorTable { colors })
    }

    fn tag(&self, links: &[linkage::Linkage]) -> R<TagData> {
        let value_type = le::u16_at(self.b, 0xd2).ok_or_else(short)?;
        let len = le::u32_at(self.b, 0x138).ok_or_else(short)? as usize;
        let len = len.min(self.limits.max_string_bytes as usize);
        let raw = le::bytes(self.whole, 0x140, len)
            .ok_or_else(|| format!("tag value of {len} bytes exceeds the element"))?;
        Ok(TagData {
            set_id: linkage::dependency_root(links, DEP_TAG_SET),
            set_number: None,
            tag_index: le::u16_at(self.b, 0xd0).unwrap_or(0),
            value_type,
            value: tag_value(value_type, raw, self.enc),
            target: linkage::dependency_root(links, DEP_TAG_TARGET),
            origin: le::f64s::<3>(self.b, 0xa0).unwrap_or([0.0; 3]),
            offset: le::f64s::<3>(self.b, 0xb8).unwrap_or([0.0; 3]),
            size: [
                le::f64_at(self.b, 0xf0).unwrap_or(0.0) * TEXT_MULT_TO_UOR,
                le::f64_at(self.b, 0xf8).unwrap_or(0.0) * TEXT_MULT_TO_UOR,
            ],
        })
    }

    /// Tag set definition: magic `teSt` at 0x28, definitions byte length at 0x34,
    /// definitions from 0x3c; set name in string linkage 1.
    fn tag_set(&self, links: &[linkage::Linkage]) -> R<TagSetData> {
        let len = le::u32_at(self.b, 0x34).ok_or_else(short)? as usize;
        let defs = le::bytes(self.b, 0x3c, len.min(self.b.len().saturating_sub(0x3c)))
            .ok_or_else(short)?;
        Ok(TagSetData {
            name: linkage::string(links, 1),
            number: None,
            tags: parse_tag_defs(defs, self.enc),
        })
    }

    fn table_entry(&self, table: u32, links: &[linkage::Linkage]) -> R<ElementData> {
        match table {
            TABLE_LEVELS => Ok(ElementData::Level(LevelData {
                id: le::u32_at(self.b, 0x20).ok_or_else(short)?,
                name: linkage::string(links, 1),
                description: linkage::string(links, 2),
                parent: le::u32_at(self.b, 0x24).unwrap_or(u32::MAX),
                flags: le::u32_at(self.b, 0x28).unwrap_or(0),
            })),
            TABLE_FONTS => {
                let number = le::u32_at(self.b, 0x28).ok_or_else(short)?;
                let n = usize::from(le::u16_at(self.b, 0x2c).unwrap_or(0));
                let name = le::bytes(self.whole, 0x2e, n)
                    .map(text::decode_utf16)
                    .unwrap_or_default();
                Ok(ElementData::Font { number, name })
            }
            _ => Ok(ElementData::Unknown),
        }
    }
}

/// Decodes a tag value of `value_type` (1 text, 3 integer, 4 double, other binary).
pub(crate) fn tag_value(value_type: u16, raw: &[u8], enc: &'static Encoding) -> TagValue {
    match value_type {
        1 => TagValue::Text(text::decode_dgn_string(raw, enc).0),
        3 => le::array::<4>(raw, 0).map_or_else(
            || TagValue::Binary(raw.to_vec()),
            |a| TagValue::Int(i64::from(i32::from_le_bytes(a))),
        ),
        4 => le::f64_at(raw, 0).map_or_else(|| TagValue::Binary(raw.to_vec()), TagValue::Float),
        _ => TagValue::Binary(raw.to_vec()),
    }
}

/// Tag definitions (V7 type 66 and V8 type 39 share this record list, GDAL dgnlib
/// `DGNParseTagSet`): name, `u16` id, prompt, `u16` type, 5 bytes, default value.
pub(crate) fn parse_tag_defs(b: &[u8], enc: &'static Encoding) -> Vec<TagDef> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while off < b.len() && out.len() < 4096 {
        let Some((name, o)) = text::dgn_cstr_at(b, off, enc) else {
            break;
        };
        let Some(id) = le::u16_at(b, o) else { break };
        let Some((prompt, o)) = text::dgn_cstr_at(b, o + 2, enc) else {
            break;
        };
        let Some(value_type) = le::u16_at(b, o) else {
            break;
        };
        let Some(flags) = le::array::<5>(b, o + 2) else {
            break;
        };
        let o = o + 2 + 5;
        let (default, next) = match value_type {
            1 => match text::dgn_cstr_at(b, o, enc) {
                Some((s, n)) => (TagValue::Text(s), n),
                None => break,
            },
            3 | 5 => match le::array::<4>(b, o) {
                Some(a) => (TagValue::Int(i64::from(i32::from_le_bytes(a))), o + 4),
                None => break,
            },
            4 => match le::array::<8>(b, o) {
                Some(a) => (TagValue::Float(f64::from_le_bytes(a)), o + 8),
                None => break,
            },
            _ => match le::bytes(b, o, 4) {
                Some(v) => (TagValue::Binary(v.to_vec()), o + 4),
                None => break,
            },
        };
        if name.is_empty() && id == 0 {
            break;
        }
        out.push(TagDef {
            id,
            name,
            prompt,
            value_type,
            default,
            flags,
        });
        off = next;
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::native::v8::tests::element;

    fn raw(bytes: Vec<u8>) -> RawElement {
        let tw = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let words = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let attr = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        RawElement {
            offset: 0,
            prefix: 0,
            type_word: tw,
            words,
            attr_words: attr,
            bytes,
        }
    }

    /// Display header body starting at 0x0c: level, id, color etc.; `flags` at 0x28.
    pub(crate) fn header(level: u32, id: u64, flags: u32, color: u32) -> Vec<u8> {
        let mut h = vec![0u8; HEADER_LEN - 0x0c];
        h[0..4].copy_from_slice(&level.to_le_bytes());
        h[4..12].copy_from_slice(&id.to_le_bytes());
        h[0x28 - 0x0c..0x2c - 0x0c].copy_from_slice(&flags.to_le_bytes());
        h[0x34 - 0x0c..0x38 - 0x0c].copy_from_slice(&color.to_le_bytes());
        h
    }

    pub(crate) fn f64s(v: &[f64]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn lim() -> Limits {
        Limits::default()
    }

    fn enc() -> &'static Encoding {
        encoding_rs::WINDOWS_1252
    }

    #[test]
    fn decodes_line_shape_and_header_fields() {
        let mut body = header(64, 0x25, 0x800, 3);
        body.extend(f64s(&[0.0, 10000.0, 20000.0, 30000.0, 40000.0, 50000.0]));
        let e = decode(&raw(element(3, &body, &[])), enc(), &lim());
        assert_eq!(e.header.level, 64);
        assert_eq!(e.header.id, Some(0x25));
        assert!(e.header.is_3d);
        assert_eq!(e.header.color, 3);
        assert_eq!(
            e.data,
            ElementData::Line {
                start: [0.0, 10000.0, 20000.0],
                end: [30000.0, 40000.0, 50000.0]
            }
        );

        let mut body = header(64, 1, 0x200, 0);
        body.extend(&5u32.to_le_bytes());
        body.extend(&[0u8; 4]);
        body.extend(f64s(&[0., 0., 0., 1., 1., 1., 1., 0., 0., 0.]));
        let e = decode(&raw(element(6, &body, &[])), enc(), &lim());
        let ElementData::Shape { points } = &e.data else {
            panic!("{e:?}")
        };
        assert_eq!(points.len(), 5);
        assert_eq!(points[2], [1.0, 1.0, 0.0]);
    }

    #[test]
    fn vertex_count_is_validated() {
        let mut body = header(1, 1, 0, 0);
        body.extend(&u32::MAX.to_le_bytes());
        body.extend(&[0u8; 4]);
        let e = decode(&raw(element(4, &body, &[])), enc(), &lim());
        assert_eq!(e.data, ElementData::Unknown);
        assert!(e.problem.is_some());
    }

    #[test]
    fn decodes_2d_arc_and_3d_ellipse() {
        let mut body = header(1, 2, 0, 0);
        body.extend(f64s(&[0.1, 3.0, 10.0, 20.0, 0.5, 1.0, 2.0]));
        let e = decode(&raw(element(16, &body, &[])), enc(), &lim());
        assert_eq!(
            e.data,
            ElementData::Arc {
                start: 0.1,
                sweep: 3.0,
                primary: 10.0,
                secondary: 20.0,
                rotation: Rotation::Angle(0.5),
                center: [1.0, 2.0, 0.0]
            }
        );
        let mut body = header(1, 2, 0x800, 0);
        body.extend(f64s(&[1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 10000.0, 20000.0]));
        let e = decode(&raw(element(15, &body, &[])), enc(), &lim());
        assert_eq!(
            e.data,
            ElementData::Ellipse {
                primary: 1.0,
                secondary: 1.0,
                rotation: Rotation::Quaternion([1.0, 0.0, 0.0, 0.0]),
                center: [0.0, 10000.0, 20000.0]
            }
        );
    }

    #[test]
    fn decodes_2d_text_with_marker() {
        let mut body = header(64, 0x28, 0, 0);
        body.extend(&1024u32.to_le_bytes());
        body.extend(&0u16.to_le_bytes());
        body.extend(&10u16.to_le_bytes());
        body.extend(f64s(&[
            1_666_666.666_666_7,
            1_666_666.666_666_7,
            0.0,
            0.0,
            -0.785,
            0.0,
            10000.0,
        ]));
        body.extend(&0u16.to_le_bytes());
        body.extend(&[0xff, 0xfe, 1, 0, b'm', b'y', b'T', 0xe9, b'x', b't']);
        let e = decode(&raw(element(17, &body, &[])), enc(), &lim());
        let ElementData::Text(t) = &e.data else {
            panic!("{e:?}")
        };
        assert_eq!(t.text, "myTéxt");
        assert_eq!(t.font, 1024);
        assert!((t.height - 10_000.0).abs() < 1e-6);
        assert_eq!(t.origin, [0.0, 10000.0, 0.0]);
        assert_eq!(t.rotation, Rotation::Angle(-0.785));
    }

    #[test]
    fn decodes_tag_with_dependencies() {
        let mut body = header(1, 0x6f, 0x1280, 0);
        body.resize(0x140 - 0x0c, 0);
        let put = |b: &mut Vec<u8>, off: usize, v: &[u8]| {
            b[off - 0x0c..off - 0x0c + v.len()].copy_from_slice(v)
        };
        put(&mut body, 0xa0, &f64s(&[5.0, 6.0, 0.0, 0.0, -10.0, 0.0]));
        put(&mut body, 0xd0, &[1, 0, 1, 0]);
        put(&mut body, 0x138, &4u32.to_le_bytes());
        body.extend(b"Oak\0");
        let mut links = vec![0x0b, 0x10, 0xd0, 0x56, 0x17, 0x27, 0, 0, 0, 2, 1, 0];
        links.extend(&0x6e_u64.to_le_bytes());
        links.extend(&[0; 4]);
        let mut d = vec![0x1b, 0x10, 0xd0, 0x56, 0x10, 0x27, 1, 0, 0, 0x0a, 1, 0];
        d.extend(&7u64.to_le_bytes());
        d.extend(&0x6b_u64.to_le_bytes());
        d.resize(56, 0);
        links.extend(d);
        let e = decode(&raw(element(37, &body, &links)), enc(), &lim());
        let ElementData::Tag(t) = &e.data else {
            panic!("{e:?}")
        };
        assert_eq!(t.value, TagValue::Text("Oak".into()));
        assert_eq!(
            (t.set_id, t.target, t.tag_index),
            (Some(0x6e), Some(0x6b), 1)
        );
        assert_eq!(t.origin, [5.0, 6.0, 0.0]);
    }

    #[test]
    fn parses_tag_set_definitions() {
        let mut defs = Vec::new();
        defs.extend(b"Species\0");
        defs.extend(&1u16.to_le_bytes());
        defs.extend(b"Enter value\0");
        defs.extend(&1u16.to_le_bytes());
        defs.extend(&[13, 0, 1, 0, 0]);
        defs.extend(b"Default Value\0");
        defs.extend(b"Count\0");
        defs.extend(&2u16.to_le_bytes());
        defs.extend(b"\0");
        defs.extend(&3u16.to_le_bytes());
        defs.extend(&[0, 0, 3, 0, 0]);
        defs.extend(&7i32.to_le_bytes());
        let d = parse_tag_defs(&defs, enc());
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].name, "Species");
        assert_eq!(d[0].default, TagValue::Text("Default Value".into()));
        assert_eq!(
            (d[1].id, d[1].value_type, d[1].default.clone()),
            (2, 3, TagValue::Int(7))
        );
        // Truncated definitions stop cleanly.
        assert_eq!(parse_tag_defs(&defs[..10], enc()).len(), 0);
    }

    #[test]
    fn decodes_cells_in_both_layouts() {
        let mut body = header(0, 9, 0xc200, 0);
        body.extend(&2u32.to_le_bytes());
        body.extend(&1u32.to_le_bytes());
        body.extend(f64s(&[0., 0., 1., 1., 1., 0., 0., 1., 7., 8.]));
        let e = decode(&raw(element(0x2000_0002, &body, &[])), enc(), &lim());
        let ElementData::Cell(c) = &e.data else {
            panic!("{e:?}")
        };
        assert_eq!(c.children, 2);
        assert_eq!(c.origin, [7.0, 8.0, 0.0]);
        assert_eq!(c.matrix, [1., 0., 0., 0., 1., 0., 0., 0., 1.]);
        assert!(e.header.complex_header);

        let mut body = header(0, 9, 0, 0);
        body.extend(&1u32.to_le_bytes());
        body.extend(&0u32.to_le_bytes());
        body.extend(f64s(&[-1., -1., 0., 1., 1., 0.]));
        body.extend(f64s(&[10000., 0., 0., 0., 10000., 0., 0., 0., 10000.]));
        body.extend(f64s(&[0., 10000., 20000.]));
        let mut name = vec![0x0b, 0x10, 0xd2, 0x56, 1, 0, 0, 0, 3, 0, 0, 0];
        name.extend(b"Abc\0\0\0\0\0\0\0\0\0");
        let e = decode(&raw(element(35, &body, &name)), enc(), &lim());
        let ElementData::SharedCellInstance(c) = &e.data else {
            panic!("{e:?}")
        };
        assert_eq!(c.origin, [0.0, 10000.0, 20000.0]);
        assert_eq!(c.matrix[4], 10000.0);
        assert_eq!(linkage::string(&e.linkages, 1).as_deref(), Some("Abc"));
    }

    #[test]
    fn truncated_bodies_degrade() {
        for t in [
            2u32, 3, 4, 6, 7, 11, 12, 15, 16, 17, 21, 22, 27, 34, 35, 37, 39, 94, 95,
        ] {
            let e = decode(&raw(element(t, &header(1, 1, 0, 0), &[])), enc(), &lim());
            assert_eq!(e.header.type_code as u32, t);
            let _ = e.data;
        }
        let e = decode(&raw(element(3, &[1, 2, 3], &[])), enc(), &lim());
        assert!(!e.header.has_display_header);
    }
}
