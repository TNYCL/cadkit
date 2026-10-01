//! Text decoding helpers: code pages and the DXF `\U+XXXX` / `\M+nXXXX` escapes.

use encoding_rs::Encoding;

/// Maps a `$DWGCODEPAGE` value (for example `ANSI_1252`, `ANSI_932`, `DOS866`,
/// `UTF-8`) to an `encoding_rs` encoding. Returns `None` for unknown names.
///
/// DOS code pages that `encoding_rs` does not implement (437, 850, ...) are not
/// mapped; callers fall back to the next candidate.
pub fn encoding_for_codepage(name: &str) -> Option<&'static Encoding> {
    let upper = name.trim().to_ascii_uppercase();
    if upper.is_empty() {
        return None;
    }
    let label: String = match upper.as_str() {
        "UTF-8" | "UTF8" => "utf-8".into(),
        "ASCII" | "US-ASCII" => "windows-1252".into(),
        "ANSI_932" | "DOS932" | "SHIFT_JIS" | "SJIS" => "shift_jis".into(),
        "ANSI_936" | "DOS936" | "GB2312" | "GBK" => "gbk".into(),
        "ANSI_949" | "DOS949" => "euc-kr".into(),
        "ANSI_950" | "DOS950" | "BIG5" => "big5".into(),
        "DOS866" | "CP866" => "ibm866".into(),
        "MACINTOSH" | "MACROMAN" => "macintosh".into(),
        "ANSI_874" | "DOS874" => "windows-874".into(),
        _ => {
            if let Some(num) = upper.strip_prefix("ANSI_") {
                format!("windows-{num}")
            } else if let Some(num) = upper.strip_prefix("ISO8859-") {
                format!("iso-8859-{num}")
            } else if let Some(num) = upper.strip_prefix("ISO-8859-") {
                format!("iso-8859-{num}")
            } else if let Some(num) = upper.strip_prefix("CP") {
                format!("windows-{num}")
            } else {
                upper.to_ascii_lowercase()
            }
        }
    };
    Encoding::for_label(label.as_bytes())
}

/// Encoding used by `\M+nXXXX` for each `n` (best effort; see `docs/dxf/NOTES.md`).
fn mif_encoding(n: u8) -> Option<&'static Encoding> {
    match n {
        b'1' => Some(encoding_rs::SHIFT_JIS),
        b'2' => Some(encoding_rs::BIG5),
        b'3' => Some(encoding_rs::EUC_KR),
        b'4' => Some(encoding_rs::GBK),
        _ => None,
    }
}

/// Decodes `\U+XXXX` (UTF-16 code unit) and `\M+nXXXX` (multi-byte character in a legacy
/// code page) escapes via [`cadkit_core::text::decode_escapes`]. Escapes that cannot be
/// decoded are kept verbatim.
pub fn decode_escapes(s: &str) -> String {
    cadkit_core::text::decode_escapes(s, &|n, pair| {
        let enc = mif_encoding(n)?;
        let (text, had_errors) = enc.decode_without_bom_handling(&pair);
        (!had_errors).then(|| text.into_owned())
    })
}

/// Decodes the ASCII-DXF caret notation for control characters: `^J` is a line feed, `^I` a
/// tab, and `^ ` (caret, space) is a literal caret. Binary DXF stores such characters raw.
pub fn decode_carets(s: &str) -> String {
    if !s.contains('^') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '^' {
            match chars.peek().copied() {
                Some(' ') => {
                    chars.next();
                    out.push('^');
                    continue;
                }
                Some(n) if ('@'..='_').contains(&n) => {
                    chars.next();
                    out.push(char::from((n as u8) - 64));
                    continue;
                }
                Some('?') => {
                    chars.next();
                    out.push('\u{7f}');
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_notation() {
        assert_eq!(decode_carets("a^Jb"), "a\nb");
        assert_eq!(decode_carets("x^ y"), "x^y");
        assert_eq!(decode_carets("2^3"), "2^3");
        assert_eq!(decode_carets("end^"), "end^");
    }

    #[test]
    fn unicode_escapes() {
        assert_eq!(decode_escapes("a\\U+00E9b"), "a\u{e9}b");
        assert_eq!(decode_escapes("\\U+D83D\\U+DE00"), "\u{1F600}");
        assert_eq!(decode_escapes("\\U+D83Dx"), "\\U+D83Dx");
        assert_eq!(decode_escapes("C:\\Users\\x"), "C:\\Users\\x");
        assert_eq!(decode_escapes("\\U+12"), "\\U+12");
    }

    #[test]
    fn mif_escape() {
        // 0x82A0 in Shift_JIS is HIRAGANA LETTER A.
        assert_eq!(decode_escapes("\\M+182A0"), "\u{3042}");
        assert_eq!(decode_escapes("\\M+982A0"), "\\M+982A0");
    }

    #[test]
    fn codepages() {
        assert_eq!(
            encoding_for_codepage("ANSI_1252"),
            Some(encoding_rs::WINDOWS_1252)
        );
        assert_eq!(
            encoding_for_codepage("ANSI_1254"),
            Some(encoding_rs::WINDOWS_1254)
        );
        assert_eq!(
            encoding_for_codepage("ANSI_932"),
            Some(encoding_rs::SHIFT_JIS)
        );
        assert_eq!(encoding_for_codepage("dos866"), Some(encoding_rs::IBM866));
        assert_eq!(encoding_for_codepage("nonsense"), None);
    }
}
