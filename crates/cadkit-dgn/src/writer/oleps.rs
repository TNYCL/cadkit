//! Minimal `[MS-OLEPS]` property set streams for a DGN V8 seed: the
//! `\u{5}SummaryInformation` and `\u{5}DocumentSummaryInformation` streams with a code
//! page, an application name and optional creation / last-save times, and nothing that names
//! a person or an organisation.
//!
//! Layout ([MS-OLEPS] 2.21 `PropertySetStream`, 2.20 `PropertySet`, 2.15
//! `TypedPropertyValue`): a 28-byte stream header (byte order `0xFFFE`, version 0, system
//! identifier, null CLSID, one property set), the set's FMTID and offset, then the set: its
//! byte size, the property count, `(property id, offset)` pairs and the values, each padded
//! to 4 bytes. Code page 1200 (UTF-16LE) as `VT_I2`, times as `VT_FILETIME` and system
//! identifier `0x00020005` follow the three public V8 samples (test_dgnv8, TreeText, Water;
//! FORMAT_NOTES, "Property sets"). The application name is `VT_LPSTR`, the type [MS-OLEPS]
//! 2.25.1 gives `PIDSI_APPNAME`: a 2.5 `CodePageString`, which under code page 1200 holds
//! NUL-terminated UTF-16LE characters and a size in bytes. The public samples store it as
//! `VT_LPWSTR` (2.7 `UnicodeString`, a size in characters) instead; the reader reads both.

use cadkit_core::LimitKind;
use cadkit_core::{Error, Result};

/// FMTID_SummaryInformation `{F29F85E0-4FF9-1068-AB91-08002B27B3D9}` as stored.
const FMTID_SUMMARY: [u8; 16] = [
    0xe0, 0x85, 0x9f, 0xf2, 0xf9, 0x4f, 0x68, 0x10, 0xab, 0x91, 0x08, 0x00, 0x2b, 0x27, 0xb3, 0xd9,
];
/// FMTID_DocSummaryInformation `{D5CDD502-2E9C-101B-9397-08002B2CF9AE}` as stored.
const FMTID_DOC_SUMMARY: [u8; 16] = [
    0x02, 0xd5, 0xcd, 0xd5, 0x9c, 0x2e, 0x1b, 0x10, 0x93, 0x97, 0x08, 0x00, 0x2b, 0x2c, 0xf9, 0xae,
];
/// The system identifier of every observed DGN property set (3 public samples).
const SYSTEM_IDENTIFIER: u32 = 0x0002_0005;
/// `CP_WINUNICODE`, the code page of every observed DGN property set.
pub(super) const CODEPAGE_UTF16: u16 = 1200;

const PID_CODEPAGE: u32 = 0x01;
const PID_CREATE_DTM: u32 = 0x0c;
const PID_LASTSAVE_DTM: u32 = 0x0d;
const PID_APPNAME: u32 = 0x12;
const VT_I2: u16 = 0x0002;
const VT_LPSTR: u16 = 0x001e;
#[cfg(test)]
const VT_LPWSTR: u16 = 0x001f;
const VT_FILETIME: u16 = 0x0040;

/// Longest application name written, in UTF-16 units without the terminator.
pub(super) const MAX_APPLICATION_UNITS: usize = 255;
/// Property sets per stream read by [`property_ids`]: [MS-OLEPS] allows one or two.
const MAX_SETS: usize = 2;
/// Properties per set read by [`property_ids`]; the observed sets hold at most 17.
const MAX_PROPERTIES: usize = 1024;

/// FILETIME ticks (100 ns since 1601-01-01) of the Unix epoch.
const UNIX_EPOCH_FILETIME: u64 = 116_444_736_000_000_000;

/// FILETIME of `ms` milliseconds since the Unix epoch; `None` for 0 (no time), negative
/// or non-finite values and values past the FILETIME range.
pub(super) fn filetime(ms: f64) -> Option<u64> {
    if !ms.is_finite() || ms <= 0.0 {
        return None;
    }
    let ticks = (ms * 10_000.0).round();
    // An f64 at or above 2^64 does not convert to u64; checked below.
    if ticks >= (u64::MAX - UNIX_EPOCH_FILETIME) as f64 {
        return None;
    }
    (ticks as u64).checked_add(UNIX_EPOCH_FILETIME)
}

/// `\u{5}SummaryInformation`: code page 1200, `application` (omitted when empty) and, when
/// `time` is given, the creation and last-save time (FILETIME). No author, last author,
/// title, subject, keywords, comments, template, revision, editing time or thumbnail.
pub(super) fn summary_information(application: &str, time: Option<u64>) -> Result<Vec<u8>> {
    let mut properties = vec![(PID_CODEPAGE, codepage_value())];
    if let Some(t) = time {
        properties.push((PID_CREATE_DTM, filetime_value(t)));
        properties.push((PID_LASTSAVE_DTM, filetime_value(t)));
    }
    if !application.is_empty() {
        properties.push((PID_APPNAME, code_page_string_value(application)?));
    }
    stream(&FMTID_SUMMARY, &properties)
}

/// `\u{5}DocumentSummaryInformation`: the code page alone (no company, manager, category
/// or user-defined section).
pub(super) fn document_summary_information() -> Result<Vec<u8>> {
    stream(&FMTID_DOC_SUMMARY, &[(PID_CODEPAGE, codepage_value())])
}

fn codepage_value() -> Vec<u8> {
    let mut v = Vec::with_capacity(8);
    v.extend(VT_I2.to_le_bytes());
    v.extend([0, 0]);
    v.extend(CODEPAGE_UTF16.to_le_bytes());
    v.extend([0, 0]);
    v
}

fn filetime_value(t: u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(12);
    v.extend(VT_FILETIME.to_le_bytes());
    v.extend([0, 0]);
    v.extend(t.to_le_bytes());
    v
}

/// `VT_LPSTR` under code page 1200 ([MS-OLEPS] 2.5 `CodePageString`): the size in bytes of
/// the characters including the NUL, the UTF-16LE characters and the NUL, zero padded to 4
/// bytes.
fn code_page_string_value(s: &str) -> Result<Vec<u8>> {
    string_value(s, VT_LPSTR, 2)
}

/// `VT_LPWSTR` ([MS-OLEPS] 2.7 `UnicodeString`): the length in characters including the
/// NUL, then the characters as in [`code_page_string_value`]. The observed files store
/// their identifying strings this way.
#[cfg(test)]
fn wide_string_value(s: &str) -> Result<Vec<u8>> {
    string_value(s, VT_LPWSTR, 1)
}

/// A UTF-16LE string property of type `vt` whose stored size counts `unit_size` per UTF-16
/// unit, the NUL included.
fn string_value(s: &str, vt: u16, unit_size: usize) -> Result<Vec<u8>> {
    let units: Vec<u16> = s.encode_utf16().collect();
    if units.len() > MAX_APPLICATION_UNITS {
        return Err(Error::limit(
            LimitKind::Capacity,
            "DGN property set string exceeds 255 UTF-16 units".into(),
        ));
    }
    if units.contains(&0) {
        return Err(Error::invalid(0, "DGN property set string holds a NUL"));
    }
    let size = (units.len() + 1)
        .checked_mul(unit_size)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| Error::limit(LimitKind::Capacity, "DGN property set string".into()))?;
    let mut v = Vec::with_capacity(8 + 2 * units.len() + 4);
    v.extend(vt.to_le_bytes());
    v.extend([0, 0]);
    v.extend(size.to_le_bytes());
    for u in units {
        v.extend(u.to_le_bytes());
    }
    v.extend([0, 0]);
    while v.len() % 4 != 0 {
        v.push(0);
    }
    Ok(v)
}

fn stream(fmtid: &[u8; 16], properties: &[(u32, Vec<u8>)]) -> Result<Vec<u8>> {
    let too_large = || Error::limit(LimitKind::Capacity, "DGN property set".into());
    let mut set = Vec::new();
    let table = 8 + 8 * properties.len();
    let mut offset = table;
    let mut entries = Vec::with_capacity(8 * properties.len());
    for (pid, value) in properties {
        entries.extend(pid.to_le_bytes());
        entries.extend(
            u32::try_from(offset)
                .map_err(|_| too_large())?
                .to_le_bytes(),
        );
        offset = offset.checked_add(value.len()).ok_or_else(too_large)?;
    }
    set.extend(
        u32::try_from(offset)
            .map_err(|_| too_large())?
            .to_le_bytes(),
    );
    set.extend(
        u32::try_from(properties.len())
            .map_err(|_| too_large())?
            .to_le_bytes(),
    );
    set.extend(entries);
    for (_, value) in properties {
        set.extend_from_slice(value);
    }
    let mut out = Vec::with_capacity(48 + set.len());
    out.extend(0xfffeu16.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(SYSTEM_IDENTIFIER.to_le_bytes());
    out.extend([0u8; 16]);
    out.extend(1u32.to_le_bytes());
    out.extend_from_slice(fmtid);
    out.extend(48u32.to_le_bytes());
    out.extend(set);
    Ok(out)
}

/// Property ids of every property set in a property set stream, in stored order, read
/// within the stream's bytes: at most [`MAX_SETS`] sets of [`MAX_PROPERTIES`] properties
/// each. A stream that is not a property set stream gives no ids.
pub(super) fn property_ids(b: &[u8]) -> Vec<u32> {
    let mut ids = Vec::new();
    if crate::le::u16_at(b, 0) != Some(0xfffe) {
        return ids;
    }
    let sets = crate::le::u32_at(b, 24).unwrap_or(0) as usize;
    for s in 0..sets.min(MAX_SETS) {
        let Some(set) = crate::le::u32_at(b, 44 + 20 * s).and_then(|o| usize::try_from(o).ok())
        else {
            break;
        };
        let count = set
            .checked_add(4)
            .and_then(|o| crate::le::u32_at(b, o))
            .unwrap_or(0) as usize;
        for i in 0..count.min(MAX_PROPERTIES) {
            let Some(pid) = i
                .checked_mul(8)
                .and_then(|o| o.checked_add(set))
                .and_then(|o| o.checked_add(8))
                .and_then(|o| crate::le::u32_at(b, o))
            else {
                break;
            };
            ids.push(pid);
        }
    }
    ids
}

/// A property set stream of wide string properties, for tests that need the identifying
/// properties this module never writes.
#[cfg(test)]
pub(super) fn test_stream(document: bool, properties: &[(u32, &str)]) -> Vec<u8> {
    let fmtid = if document {
        &FMTID_DOC_SUMMARY
    } else {
        &FMTID_SUMMARY
    };
    let mut values = vec![(PID_CODEPAGE, codepage_value())];
    for (pid, text) in properties {
        values.push((*pid, wide_string_value(text).unwrap_or_default()));
    }
    stream(fmtid, &values).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    #[test]
    fn summary_has_the_documented_layout() {
        let t = filetime(1_700_000_000_000.0).unwrap();
        let s = summary_information("cadkit", Some(t)).unwrap();
        assert_eq!(&s[0..4], &[0xfe, 0xff, 0, 0]);
        assert_eq!(crate::le::u32_at(&s, 4), Some(SYSTEM_IDENTIFIER));
        assert_eq!(&s[8..24], &[0u8; 16]);
        assert_eq!(crate::le::u32_at(&s, 24), Some(1));
        assert_eq!(&s[28..44], &FMTID_SUMMARY);
        assert_eq!(crate::le::u32_at(&s, 44), Some(48));
        // Set: size, count 4, pids 1, 0x0c, 0x0d, 0x12.
        let set = &s[48..];
        assert_eq!(crate::le::u32_at(set, 0).unwrap() as usize, set.len());
        assert_eq!(crate::le::u32_at(set, 4), Some(4));
        assert_eq!(property_ids(&s), vec![1, 0x0c, 0x0d, 0x12]);
        // Code page value: VT_I2 1200 at the first offset (8 + 4 * 8 = 40).
        assert_eq!(crate::le::u32_at(set, 12), Some(40));
        assert_eq!(&set[40..48], &[2, 0, 0, 0, 0xb0, 0x04, 0, 0]);
        // FILETIME values.
        assert_eq!(&set[48..52], &[0x40, 0, 0, 0]);
        assert_eq!(crate::le::u64_at(set, 52), Some(t));
        // Application: VT_LPSTR, 7 UTF-16 characters with the NUL = 14 bytes, padded to 4.
        assert_eq!(crate::le::u32_at(set, 72), Some(0x1e));
        assert_eq!(crate::le::u32_at(set, 76), Some(14));
        assert_eq!(&set[80..94], &utf16z("cadkit")[..]);
        assert_eq!(set.len(), 96);
        // The observed VT_LPWSTR form counts characters instead.
        let wide = wide_string_value("cadkit").unwrap();
        assert_eq!(crate::le::u32_at(&wide, 0), Some(0x1f));
        assert_eq!(crate::le::u32_at(&wide, 4), Some(7));
        assert_eq!(&wide[8..22], &utf16z("cadkit")[..]);
        // 1970-01-01 is FILETIME 116444736000000000; one millisecond is 10,000 ticks.
        assert_eq!(filetime(1.0), Some(UNIX_EPOCH_FILETIME + 10_000));
        assert_eq!(filetime(0.0), None);
        assert_eq!(filetime(-1.0), None);
        assert_eq!(filetime(f64::NAN), None);
        assert_eq!(filetime(1e300), None);
    }

    fn utf16z(s: &str) -> Vec<u8> {
        s.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    #[test]
    fn summary_without_time_or_application_holds_the_code_page_only() {
        let s = summary_information("", None).unwrap();
        assert_eq!(property_ids(&s), vec![1]);
        let d = document_summary_information().unwrap();
        assert_eq!(&d[28..44], &FMTID_DOC_SUMMARY);
        assert_eq!(property_ids(&d), vec![1]);
        assert!(summary_information(&"x".repeat(256), None).is_err());
        assert!(summary_information("a\0b", None).is_err());
        assert!(summary_information(&"x".repeat(255), None).is_ok());
    }

    #[test]
    fn property_ids_survive_truncated_and_mutated_streams() {
        let s = summary_information("cadkit", filetime(1.0)).unwrap();
        for n in 0..s.len() {
            let ids = property_ids(&s[..n]);
            assert!(ids.len() <= 4);
        }
        for at in 0..s.len() {
            for v in [0x00, 0xff, 0x7f, 0x80] {
                let mut m = s.clone();
                m[at] = v;
                assert!(property_ids(&m).len() <= MAX_SETS * MAX_PROPERTIES);
            }
        }
        // A huge declared count reads no further than the stream.
        let mut m = s.clone();
        m[52..56].copy_from_slice(&u32::MAX.to_le_bytes());
        m[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(property_ids(&m).len() <= MAX_SETS * MAX_PROPERTIES);
        assert!(property_ids(b"not a property set").is_empty());
    }
}
