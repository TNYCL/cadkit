//! `$DWGCODEPAGE` handling: the code page index stored in the file header (offset
//! 0x13) and its mapping to WHATWG and DOS/OEM encodings for pre-R2007 8-bit strings.

use cadkit_core::text::OemCodepage;
use encoding_rs::Encoding;

/// Resolved 8-bit text decoder for pre-R2007 DWG strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    /// WHATWG encoding supported by `encoding_rs`.
    Whatwg(&'static Encoding),
    /// Legacy single-byte DOS/OEM code page (`DOS437`..`DOS869`, except `DOS866` which is WHATWG `ibm866`).
    Oem(OemCodepage),
}

impl TextEncoding {
    /// Canonical encoding name stored in [`crate::native::DwgFile::encoding`].
    pub fn name(self) -> &'static str {
        match self {
            Self::Whatwg(e) => e.name(),
            Self::Oem(cp) => cp.name(),
        }
    }

    /// Decodes a single 8-bit byte for MTEXT `%%nnn` codes (`None` for multi-byte lead bytes or unmapped codes).
    pub fn decode_byte(self, byte: u8) -> Option<char> {
        if byte < 0x80 {
            return Some(char::from(byte));
        }
        match self {
            Self::Oem(cp) => {
                let ch = cp.decode_byte(byte);
                (ch != '\u{FFFD}').then_some(ch)
            }
            Self::Whatwg(e) => {
                let buf = [byte];
                let (text, had_errors) = e.decode_without_bom_handling(&buf);
                (!had_errors).then(|| text.chars().next()).flatten()
            }
        }
    }
}

/// DXF names of the DWG code page indices, in index order (0 = none).
const NAMES: [&str; 45] = [
    "",
    "ASCII",
    "ISO8859-1",
    "ISO8859-2",
    "ISO8859-3",
    "ISO8859-4",
    "ISO8859-5",
    "ISO8859-6",
    "ISO8859-7",
    "ISO8859-8",
    "ISO8859-9",
    "DOS437",
    "DOS850",
    "DOS852",
    "DOS855",
    "DOS857",
    "DOS860",
    "DOS861",
    "DOS863",
    "DOS864",
    "DOS865",
    "DOS869",
    "DOS932",
    "MACINTOSH",
    "BIG5",
    "KSC5601",
    "JOHAB",
    "DOS866",
    "ANSI_1250",
    "ANSI_1251",
    "ANSI_1252",
    "GB2312",
    "ANSI_1253",
    "ANSI_1254",
    "ANSI_1255",
    "ANSI_1256",
    "ANSI_1257",
    "ANSI_874",
    "ANSI_932",
    "ANSI_936",
    "ANSI_949",
    "ANSI_950",
    "ANSI_1361",
    "ANSI_1200",
    "ANSI_1258",
];

/// DXF-style name of a DWG code page index (`30` → `"ANSI_1252"`).
pub fn name(index: u16) -> Option<&'static str> {
    NAMES
        .get(usize::from(index))
        .copied()
        .filter(|n| !n.is_empty())
}

/// WHATWG label for a DWG code page index, when `encoding_rs` supports it.
/// DOS code pages other than 866, and Johab, have no WHATWG encoding.
pub fn whatwg_label(index: u16) -> Option<&'static str> {
    Some(match name(index)? {
        // ISO-8859-1 and ASCII are decoded as windows-1252 by WHATWG, a superset.
        "ASCII" | "ISO8859-1" | "ANSI_1252" => "windows-1252",
        "ISO8859-2" => "iso-8859-2",
        "ISO8859-3" => "iso-8859-3",
        "ISO8859-4" => "iso-8859-4",
        "ISO8859-5" => "iso-8859-5",
        "ISO8859-6" => "iso-8859-6",
        "ISO8859-7" => "iso-8859-7",
        "ISO8859-8" => "iso-8859-8",
        "ISO8859-9" | "ANSI_1254" => "windows-1254",
        "DOS866" => "ibm866",
        "DOS932" | "ANSI_932" => "shift_jis",
        "MACINTOSH" => "macintosh",
        "BIG5" | "ANSI_950" => "big5",
        "KSC5601" | "ANSI_949" => "euc-kr",
        "GB2312" | "ANSI_936" => "gbk",
        "ANSI_1250" => "windows-1250",
        "ANSI_1251" => "windows-1251",
        "ANSI_1253" => "windows-1253",
        "ANSI_1255" => "windows-1255",
        "ANSI_1256" => "windows-1256",
        "ANSI_1257" => "windows-1257",
        "ANSI_874" => "windows-874",
        "ANSI_1200" => "utf-16le",
        "ANSI_1258" => "windows-1258",
        _ => return None,
    })
}

/// Resolves a label (WHATWG label, `ANSI_xxxx`, `DOSxxx`, or `CPxxx`) to a [`TextEncoding`].
pub fn from_label(label: &str) -> Option<TextEncoding> {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(cp) = OemCodepage::from_label(trimmed) {
        return Some(TextEncoding::Oem(cp));
    }
    if let Some(enc) = Encoding::for_label(trimmed.as_bytes()) {
        return Some(TextEncoding::Whatwg(enc));
    }
    let upper = trimmed.to_ascii_uppercase();
    let mapped = match upper.as_str() {
        "DOS866" | "CP866" => "ibm866".to_owned(),
        "DOS932" | "ANSI_932" => "shift_jis".to_owned(),
        "ANSI_936" | "GB2312" => "gbk".to_owned(),
        "ANSI_949" | "KSC5601" => "euc-kr".to_owned(),
        "ANSI_950" | "BIG5" => "big5".to_owned(),
        "ANSI_874" => "windows-874".to_owned(),
        _ => {
            if let Some(num) = upper
                .strip_prefix("ANSI_")
                .or_else(|| upper.strip_prefix("CP"))
            {
                format!("windows-{num}")
            } else {
                let num = upper.strip_prefix("ISO8859-")?;
                format!("iso-8859-{num}")
            }
        }
    };
    Encoding::for_label(mapped.as_bytes()).map(TextEncoding::Whatwg)
}

/// Resolves the encoding for 8-bit strings: the file's code page if supported, else
/// `fallback` (a WHATWG or DOS/OEM label from `ReadOptions`), else windows-1252.
/// Returns the encoding and whether the file's own code page was used.
pub fn resolve(index: u16, fallback: Option<&str>) -> (TextEncoding, bool) {
    if let Some(enc) = whatwg_label(index).and_then(|l| Encoding::for_label(l.as_bytes())) {
        return (TextEncoding::Whatwg(enc), true);
    }
    if let Some(cp) = name(index).and_then(OemCodepage::from_label) {
        return (TextEncoding::Oem(cp), true);
    }
    if let Some(enc) = fallback.and_then(from_label) {
        return (enc, false);
    }
    (TextEncoding::Whatwg(encoding_rs::WINDOWS_1252), false)
}

/// Decodes 8-bit text, dropping trailing NULs (some writers include the terminator).
pub fn decode(bytes: &[u8], encoding: TextEncoding) -> String {
    let end = bytes.iter().rposition(|&b| b != 0).map_or(0, |p| p + 1);
    let slice = bytes.get(..end).unwrap_or(&[]);
    match encoding {
        TextEncoding::Whatwg(e) => {
            let (text, _, _) = e.decode(slice);
            text.into_owned()
        }
        TextEncoding::Oem(cp) => cp.decode(slice),
    }
}

/// Encoding of `\M+nXXXX` escapes by `n` (1 Shift-JIS, 2 Big5, 3 Korean, 4 GBK).
fn mif_encoding(n: u8) -> Option<&'static Encoding> {
    match n {
        b'1' => Some(encoding_rs::SHIFT_JIS),
        b'2' => Some(encoding_rs::BIG5),
        b'3' => Some(encoding_rs::EUC_KR),
        b'4' => Some(encoding_rs::GBK),
        _ => None,
    }
}

/// Decodes the `\U+XXXX` (UTF-16 code unit, surrogate pairs joined) and `\M+nXXXX`
/// (double-byte character) escapes used for characters outside the drawing code
/// page, via [`cadkit_core::text::decode_escapes`]. They appear in DWG strings of every
/// version (dimension text stores `°` as `\U+00B0`). Undecodable escapes are kept verbatim.
pub fn decode_escapes(s: &str) -> String {
    cadkit_core::text::decode_escapes(s, &|n, pair| {
        let enc = mif_encoding(n)?;
        let (text, had_errors) = enc.decode_without_bom_handling(&pair);
        (!had_errors).then(|| text.into_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_escapes() {
        assert_eq!(decode_escapes("94\\U+00B0"), "94\u{b0}");
        assert_eq!(decode_escapes("\\U+D83D\\U+DE00"), "\u{1F600}");
        assert_eq!(decode_escapes("\\U+D83Dx"), "\\U+D83Dx");
        assert_eq!(decode_escapes("C:\\Users\\x"), "C:\\Users\\x");
        assert_eq!(decode_escapes("\\M+1829F"), "\u{3041}");
    }

    #[test]
    fn index_table() {
        assert_eq!(name(30), Some("ANSI_1252"));
        assert_eq!(name(0), None);
        assert_eq!(name(1000), None);
        assert_eq!(whatwg_label(33), Some("windows-1254"));
        assert_eq!(whatwg_label(11), None);
    }

    #[test]
    fn resolve_falls_back() {
        assert_eq!(
            resolve(30, None),
            (TextEncoding::Whatwg(encoding_rs::WINDOWS_1252), true)
        );
        assert_eq!(
            resolve(11, None),
            (TextEncoding::Oem(OemCodepage::Cp437), true)
        );
        assert_eq!(
            resolve(15, None),
            (TextEncoding::Oem(OemCodepage::Cp857), true)
        );
        // Index 26 (`JOHAB`) has no WHATWG or single-byte OEM table, so it uses `fallback`.
        assert_eq!(
            resolve(26, Some("windows-1254")),
            (TextEncoding::Whatwg(encoding_rs::WINDOWS_1254), false)
        );
        assert_eq!(
            resolve(26, Some("DOS857")),
            (TextEncoding::Oem(OemCodepage::Cp857), false)
        );
        assert_eq!(
            resolve(26, Some("no-such-label")),
            (TextEncoding::Whatwg(encoding_rs::WINDOWS_1252), false)
        );
    }

    #[test]
    fn decode_text() {
        assert_eq!(
            decode(
                b"Kat\xfd\0\0",
                TextEncoding::Whatwg(encoding_rs::WINDOWS_1254)
            ),
            "Katı"
        );
        assert_eq!(
            decode(b"Kat\x8d\0", TextEncoding::Oem(OemCodepage::Cp857)),
            "Katı"
        );
        assert_eq!(
            decode(b"", TextEncoding::Whatwg(encoding_rs::WINDOWS_1252)),
            ""
        );
    }
}
