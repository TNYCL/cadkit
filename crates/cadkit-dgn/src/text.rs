//! String decoding for DGN text payloads.
//!
//! Observed encodings (see `docs/dgn/FORMAT_NOTES.md`, "Strings"):
//! - `ff fd` prefix: UTF-16LE follows (V8 string linkages, V7 multibyte text).
//! - `ff fe 01 00` prefix: 8-bit text in the file / element code page follows (V8 text).
//! - otherwise: 8-bit text in the code page (no UTF-8 sniffing: MicroStation stores
//!   locale code page text, and guessing UTF-8 would mis-decode valid code page bytes).

use encoding_rs::Encoding;

/// How a string was stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringEncoding {
    /// `ff fd` marker followed by UTF-16LE.
    Utf16,
    /// `ff fe 01 00` marker followed by 8-bit code page text.
    MarkedCodePage,
    /// Plain 8-bit code page text (ASCII-only text also lands here).
    CodePage,
}

impl StringEncoding {
    /// Stable name used in props.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Utf16 => "utf-16le",
            Self::MarkedCodePage => "marked-codepage",
            Self::CodePage => "codepage",
        }
    }
}

/// Resolves a WHATWG label (e.g. `"windows-1254"`) or a Windows code page number
/// (e.g. `"1254"`, `"cp1254"`) to an encoding.
pub(crate) fn encoding_for(label: Option<&str>) -> &'static Encoding {
    let Some(label) = label.map(str::trim).filter(|l| !l.is_empty()) else {
        return encoding_rs::WINDOWS_1252;
    };
    if let Some(e) = Encoding::for_label(label.as_bytes()) {
        return e;
    }
    let digits = label.trim_start_matches(|c: char| !c.is_ascii_digit());
    digits
        .parse::<u16>()
        .ok()
        .and_then(encoding_for_codepage)
        .unwrap_or(encoding_rs::WINDOWS_1252)
}

/// Encoding for a Windows code page number, when known.
pub(crate) fn encoding_for_codepage(cp: u16) -> Option<&'static Encoding> {
    let label = match cp {
        65001 => "utf-8",
        1200 => "utf-16le",
        932 => "shift_jis",
        936 => "gbk",
        949 => "euc-kr",
        950 => "big5",
        874 => "windows-874",
        437 | 850 | 1252 => "windows-1252",
        866 => "ibm866",
        c @ 1250..=1258 => return Encoding::for_label(format!("windows-{c}").as_bytes()),
        c @ 28591..=28606 => {
            return Encoding::for_label(format!("iso-8859-{}", c - 28590).as_bytes());
        }
        _ => return None,
    };
    Encoding::for_label(label.as_bytes())
}

/// Decodes 8-bit text with `enc`, stopping at the first NUL.
pub(crate) fn decode_8bit(bytes: &[u8], enc: &'static Encoding) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let s = bytes.get(..end).unwrap_or(&[]);
    if s.is_ascii() {
        return s.iter().map(|&b| char::from(b)).collect();
    }
    enc.decode_without_bom_handling(s).0.into_owned()
}

/// Decodes UTF-16LE units, stopping at the first NUL unit. Odd trailing bytes are ignored.
pub(crate) fn decode_utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| {
            u16::from_le_bytes([
                c.first().copied().unwrap_or(0),
                c.get(1).copied().unwrap_or(0),
            ])
        })
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Decodes a DGN string payload, honoring the `ff fd` / `ff fe 01 00` markers.
pub(crate) fn decode_dgn_string(bytes: &[u8], enc: &'static Encoding) -> (String, StringEncoding) {
    if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfd]) {
        return (decode_utf16(rest), StringEncoding::Utf16);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe, 0x01, 0x00]) {
        return (decode_8bit(rest, enc), StringEncoding::MarkedCodePage);
    }
    (decode_8bit(bytes, enc), StringEncoding::CodePage)
}

/// V7 multibyte text: `ff fd` then 16-bit units. Units below 256 are single bytes, larger
/// units are double-byte code page characters (high byte first), as GDAL's dgnlib reads
/// them; the resulting byte string is decoded with `enc`.
pub(crate) fn decode_v7_text(bytes: &[u8], enc: &'static Encoding) -> (String, StringEncoding) {
    let Some(rest) = bytes.strip_prefix(&[0xff, 0xfd]) else {
        return (decode_8bit(bytes, enc), StringEncoding::CodePage);
    };
    let mut out = Vec::with_capacity(rest.len());
    for c in rest.chunks_exact(2) {
        let u = u16::from_le_bytes([
            c.first().copied().unwrap_or(0),
            c.get(1).copied().unwrap_or(0),
        ]);
        if u == 0 {
            break;
        }
        if u < 256 {
            out.push(u as u8);
        } else {
            out.extend_from_slice(&u.to_be_bytes());
        }
    }
    (
        enc.decode_without_bom_handling(&out).0.into_owned(),
        StringEncoding::Utf16,
    )
}

/// Reads a NUL-terminated 8-bit string at `off`, returning it and the offset after the NUL.
pub(crate) fn cstr_at(b: &[u8], off: usize, enc: &'static Encoding) -> Option<(String, usize)> {
    let rest = b.get(off..)?;
    let len = rest.iter().position(|&c| c == 0)?;
    let s = decode_8bit(rest.get(..len)?, enc);
    Some((s, off.checked_add(len)?.checked_add(1)?))
}

/// Reads a terminated DGN string at `off`: either `ff fd` + UTF-16LE ending with a 16-bit
/// NUL (observed in V8 tag set definitions with non-ASCII names) or 8-bit text ending with
/// a NUL byte. Returns the text and the offset after the terminator.
pub(crate) fn dgn_cstr_at(b: &[u8], off: usize, enc: &'static Encoding) -> Option<(String, usize)> {
    let rest = b.get(off..)?;
    if let Some(units) = rest.strip_prefix(&[0xff, 0xfd]) {
        let n = units.chunks_exact(2).position(|c| c == [0, 0])?;
        let text = decode_utf16(units.get(..2 * n)?);
        return Some((text, off.checked_add(2 + 2 * n + 2)?));
    }
    cstr_at(b, off, enc)
}

/// Decodes a RAD-50 word into 3 characters (V7 cell names).
pub(crate) fn rad50(word: u16) -> String {
    let mut v = word;
    let mut out = String::with_capacity(3);
    for div in [1600u16, 40, 1] {
        let d = v / div;
        v -= d * div;
        out.push(match d {
            1..=26 => char::from(b'A' + (d - 1) as u8),
            27 => '$',
            28 => '.',
            30..=39 => char::from(b'0' + (d - 30) as u8),
            _ => ' ',
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_select_the_decoder() {
        let w1252 = encoding_for(None);
        let (s, e) = decode_dgn_string(
            &[0xff, 0xfe, 1, 0, b'm', b'y', b'T', 0xe9, b'x', b't'],
            w1252,
        );
        assert_eq!((s.as_str(), e), ("myTéxt", StringEncoding::MarkedCodePage));
        let mut u16 = vec![0xff, 0xfd];
        for u in "Şişe".encode_utf16() {
            u16.extend_from_slice(&u.to_le_bytes());
        }
        u16.extend_from_slice(&[0, 0, 0x41, 0]);
        assert_eq!(decode_dgn_string(&u16, w1252).0, "Şişe");
        assert_eq!(decode_dgn_string(b"plain\0junk", w1252).0, "plain");
    }

    #[test]
    fn code_pages_resolve() {
        let tr = encoding_for(Some("windows-1254"));
        assert_eq!(decode_8bit(&[0xdd, 0x53], tr), "İS");
        assert_eq!(encoding_for(Some("1254")).name(), "windows-1254");
        assert_eq!(encoding_for(Some("cp1254")).name(), "windows-1254");
        assert_eq!(encoding_for(Some("bogus")).name(), "windows-1252");
        assert_eq!(
            encoding_for_codepage(28599).map(|e| e.name()),
            Some("windows-1254")
        );
    }

    #[test]
    fn rad50_and_cstr() {
        // "ABC" = 1*1600 + 2*40 + 3
        assert_eq!(rad50(1683), "ABC");
        assert_eq!(rad50(0), "   ");
        let enc = encoding_for(None);
        assert_eq!(cstr_at(b"ab\0cd\0", 3, enc), Some(("cd".to_string(), 6)));
        assert_eq!(cstr_at(b"ab", 0, enc), None);
        let mut w = vec![b'x', 0xff, 0xfd];
        for u in "Iİ".encode_utf16() {
            w.extend_from_slice(&u.to_le_bytes());
        }
        w.extend_from_slice(&[0, 0, 7]);
        assert_eq!(dgn_cstr_at(&w, 1, enc), Some(("Iİ".to_string(), 9)));
        assert_eq!(dgn_cstr_at(b"ok\0", 0, enc), Some(("ok".to_string(), 3)));
        assert_eq!(dgn_cstr_at(&[0xff, 0xfd, 0x41, 0], 0, enc), None);
    }

    #[test]
    fn v7_multibyte() {
        let enc = encoding_for(Some("windows-1252"));
        let b = [0xff, 0xfd, b'A', 0, b'B', 0, 0, 0];
        assert_eq!(decode_v7_text(&b, enc).0, "AB");
    }
}
