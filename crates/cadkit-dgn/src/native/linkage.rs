//! Attribute linkages: the variable-length records after an element's attribute offset.
//!
//! Framing (GDAL dgnlib `DGNGetAttrLinkSize`, confirmed on V8 samples): byte 0 is the
//! number of 16-bit words after the first word, byte 1 holds flags (`0x10` = user data
//! linkage), and the `u16` at `+2` is the linkage id. A linkage whose first two bytes are
//! `00 00` or `00 80` is an 8-byte V7 DMRS database linkage.

use encoding_rs::Encoding;

use crate::le;
use crate::text::{self, StringEncoding};

/// V8 string linkage id (model names, cell names, level names, file names ...).
pub const STRING_LINKAGE: u16 = 0x56d2;
/// V8 dependency linkage id (references to other elements by id).
pub const DEPENDENCY_LINKAGE: u16 = 0x56d0;
/// Shape fill linkage id.
pub const FILL_LINKAGE: u16 = 0x0041;
/// V7 association id linkage.
pub const ASSOC_ID_LINKAGE: u16 = 0x7d2f;
/// V8 linkage observed on text elements; its `u32` at `+12` holds a Windows code page.
pub const CODEPAGE_LINKAGE: u16 = 0x80d4;

/// Decoded content of a linkage.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkageData {
    /// V8 string linkage: `u32` string id at `+4`, `u32` byte length at `+8`, text at `+12`.
    String {
        /// Which property this string is (1 = name, 2 = description, ...).
        string_id: u32,
        /// Decoded text.
        text: String,
        /// Storage encoding.
        encoding: StringEncoding,
    },
    /// V8 dependency linkage: `u16` application id, `u16` application value, `u8` copy
    /// option, `u8` root type, `u16` root count, then roots.
    Dependency {
        /// Application id (`0x2717` tag -> tag set, `0x2710` tag -> target observed).
        app_id: u16,
        /// Application value.
        app_value: u16,
        /// Root data type (2, 3 = plain element ids; 9 / 10 = 16-byte roots, id in the second half).
        root_type: u8,
        /// Copy option.
        copy_option: u8,
        /// Element ids of the roots that could be decoded.
        roots: Vec<u64>,
    },
    /// Shape fill linkage; the stored fill color value.
    Fill {
        /// Fill color (V7: byte at `+8`; V8: `u32` at `+8`).
        color: u32,
    },
    /// V7 association id.
    AssocId(u32),
    /// Database linkage (DMRS or external database).
    Database {
        /// Entity (table) number.
        entity: u32,
        /// Row key.
        mslink: u32,
    },
    /// Code page linkage (hypothesis, see FORMAT_NOTES).
    CodePage(u32),
    /// Kept as raw bytes only.
    Raw,
}

/// One attribute linkage.
#[derive(Debug, Clone, PartialEq)]
pub struct Linkage {
    /// Byte offset of the linkage within the element.
    pub offset: usize,
    /// Linkage id (`u16` at `+2`); 0 for DMRS linkages.
    pub id: u16,
    /// Flag byte (`+1`).
    pub flags: u8,
    /// Decoded content.
    pub data: LinkageData,
    /// The complete linkage bytes.
    pub bytes: Vec<u8>,
}

/// Splits the attribute area `attr` (starting at element offset `base`) into linkages.
///
/// `v8` selects V8 rules: zero padding ends the area instead of being read as DMRS.
pub(crate) fn parse(attr: &[u8], base: usize, v8: bool, enc: &'static Encoding) -> Vec<Linkage> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while off < attr.len() {
        let rest = attr.get(off..).unwrap_or(&[]);
        if rest.iter().all(|&b| b == 0) {
            // Zero padding (always in V8; a V7 DMRS linkage needs at least 8 bytes).
            if v8 || rest.len() < 8 {
                break;
            }
        }
        if rest.len() < 4 {
            out.push(Linkage {
                offset: base + off,
                id: 0,
                flags: 0,
                data: LinkageData::Raw,
                bytes: rest.to_vec(),
            });
            break;
        }
        let (b0, b1) = (
            rest.first().copied().unwrap_or(0),
            rest.get(1).copied().unwrap_or(0),
        );
        let dmrs = !v8 && b0 == 0 && (b1 == 0 || b1 == 0x80);
        let size = if dmrs {
            8
        } else if b1 & 0x10 != 0 || v8 {
            (usize::from(b0) + 1) * 2
        } else {
            0
        };
        let framed = if size >= 4 { rest.get(..size) } else { None };
        let Some(bytes) = framed else {
            // Unframed tail: keep it so nothing is lost.
            out.push(Linkage {
                offset: base + off,
                id: 0,
                flags: b1,
                data: LinkageData::Raw,
                bytes: rest.to_vec(),
            });
            break;
        };
        let id = if dmrs {
            0
        } else {
            le::u16_at(bytes, 2).unwrap_or(0)
        };
        let data = if dmrs {
            LinkageData::Database {
                entity: u32::from(le::u16_at(bytes, 2).unwrap_or(0)),
                mslink: u32::from_le_bytes([
                    le::u8_at(bytes, 4).unwrap_or(0),
                    le::u8_at(bytes, 5).unwrap_or(0),
                    le::u8_at(bytes, 6).unwrap_or(0),
                    0,
                ]),
            }
        } else {
            decode(id, bytes, v8, enc)
        };
        out.push(Linkage {
            offset: base + off,
            id,
            flags: b1,
            data,
            bytes: bytes.to_vec(),
        });
        off += size;
    }
    out
}

fn decode(id: u16, b: &[u8], v8: bool, enc: &'static Encoding) -> LinkageData {
    match id {
        STRING_LINKAGE if v8 => {
            let (Some(string_id), Some(len)) = (le::u32_at(b, 4), le::u32_at(b, 8)) else {
                return LinkageData::Raw;
            };
            let len = usize::try_from(len).unwrap_or(usize::MAX);
            // Some writers declare the length without the terminator; never read past the linkage.
            let payload = b.get(12..).unwrap_or(&[]);
            let payload = payload.get(..len.min(payload.len())).unwrap_or(&[]);
            let (text, encoding) = text::decode_dgn_string(payload, enc);
            LinkageData::String {
                string_id,
                text,
                encoding,
            }
        }
        DEPENDENCY_LINKAGE if v8 => {
            let app_id = le::u16_at(b, 4).unwrap_or(0);
            let app_value = le::u16_at(b, 6).unwrap_or(0);
            let copy_option = le::u8_at(b, 8).unwrap_or(0);
            let root_type = le::u8_at(b, 9).unwrap_or(0);
            let count = usize::from(le::u16_at(b, 10).unwrap_or(0));
            // Types 2 and 3 were observed with plain u64 element ids; types 9/10 with a
            // 16-byte root whose second half is the element id (one root per linkage in
            // every sample). Other types keep their bytes only.
            let (first, stride, skip) = match root_type {
                2 | 3 => (12usize, 8usize, 0usize),
                9 | 10 => (12, 16, 8),
                _ => (0, 0, 0),
            };
            let roots = if stride == 0 {
                Vec::new()
            } else {
                (0..count)
                    .map_while(|i| le::u64_at(b, first + i * stride + skip))
                    .take(b.len() / 8)
                    .collect()
            };
            LinkageData::Dependency {
                app_id,
                app_value,
                root_type,
                copy_option,
                roots,
            }
        }
        FILL_LINKAGE => {
            let color = if v8 {
                le::u32_at(b, 8)
            } else {
                le::u8_at(b, 8).map(u32::from)
            };
            color.map_or(LinkageData::Raw, |color| LinkageData::Fill { color })
        }
        ASSOC_ID_LINKAGE => le::u32_at(b, 4).map_or(LinkageData::Raw, LinkageData::AssocId),
        CODEPAGE_LINKAGE if v8 => le::u32_at(b, 12)
            .filter(|&cp| cp > 0 && cp < 65536)
            .map_or(LinkageData::Raw, LinkageData::CodePage),
        _ if b.len() == 16 && !v8 => LinkageData::Database {
            // External database linkage (GDAL dgnlib): entity at +6, mslink at +8.
            entity: u32::from(le::u16_at(b, 6).unwrap_or(0)),
            mslink: le::u32_at(b, 8).unwrap_or(0),
        },
        _ => LinkageData::Raw,
    }
}

/// The text of the first string linkage with `string_id`.
pub(crate) fn string(linkages: &[Linkage], string_id: u32) -> Option<String> {
    linkages.iter().find_map(|l| match &l.data {
        LinkageData::String {
            string_id: id,
            text,
            ..
        } if *id == string_id => Some(text.clone()),
        _ => None,
    })
}

/// First root of the first dependency linkage with `app_id`.
pub(crate) fn dependency_root(linkages: &[Linkage], app_id: u16) -> Option<u64> {
    linkages.iter().find_map(|l| match &l.data {
        LinkageData::Dependency {
            app_id: a, roots, ..
        } if *a == app_id => roots.first().copied(),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc() -> &'static Encoding {
        encoding_rs::WINDOWS_1252
    }

    #[test]
    fn parses_string_and_dependency_linkages() {
        // String linkage: 0x0b words follow, id 0x56d2, string id 1, 7 bytes "Default".
        let mut a = vec![0x0b, 0x10, 0xd2, 0x56, 1, 0, 0, 0, 7, 0, 0, 0];
        a.extend_from_slice(b"Default\0\0\0\0\0");
        // Dependency linkage, root type 2: app 0x2717, one root 0x6e.
        a.extend_from_slice(&[0x0b, 0x10, 0xd0, 0x56, 0x17, 0x27, 0, 0, 0, 2, 1, 0]);
        a.extend_from_slice(&0x6e_u64.to_le_bytes());
        a.extend_from_slice(&[0; 4]);
        // Dependency linkage, type 10: root id in the second half of a 16-byte root.
        let mut d = vec![0x1b, 0x10, 0xd0, 0x56, 0x10, 0x27, 1, 0, 0, 10, 1, 0];
        d.extend_from_slice(&7u64.to_le_bytes());
        d.extend_from_slice(&0x6b_u64.to_le_bytes());
        d.resize(56, 0);
        a.extend_from_slice(&d);
        a.extend_from_slice(&[0; 6]); // padding
        let l = parse(&a, 100, true, enc());
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].offset, 100);
        assert_eq!(string(&l, 1).as_deref(), Some("Default"));
        assert_eq!(dependency_root(&l, 0x2717), Some(0x6e));
        assert_eq!(dependency_root(&l, 0x2710), Some(0x6b));
    }

    #[test]
    fn v7_dmrs_fill_and_garbage() {
        let a = [
            0x00, 0x00, 0x05, 0x00, 0x01, 0x02, 0x03, 0x00, // DMRS entity 5, mslink 0x030201
            0x07, 0x10, 0x41, 0x00, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, // fill, color 9
            0x03, 0x00, 0xff, // unframed tail
        ];
        let l = parse(&a, 0, false, enc());
        assert_eq!(
            l[0].data,
            LinkageData::Database {
                entity: 5,
                mslink: 0x030201
            }
        );
        assert_eq!(l[1].data, LinkageData::Fill { color: 9 });
        assert_eq!(l[2].data, LinkageData::Raw);
        // Declared size beyond the end is kept raw, never read out of bounds.
        let l = parse(&[0xff, 0x10, 0xd2, 0x56, 1], 0, true, enc());
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].data, LinkageData::Raw);
    }
}
