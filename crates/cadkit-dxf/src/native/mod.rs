//! Low-level DXF group-code tokenizer (ASCII and binary).
//!
//! [`Tokenizer`] turns the bytes of a DXF file into a stream of [`Pair`]s: an integer
//! group code and a typed [`Value`]. It knows nothing about sections or entities, so it
//! can be used as a scanner or as an oracle for other readers.
//!
//! Value types follow the group-code ranges of the public DXF reference
//! (see [`code_type`]). The same table drives binary decoding and ASCII parsing.

mod text;

use cadkit_core::bytes::ByteReader;
use cadkit_core::{Error, Result};
use encoding_rs::Encoding;

pub use text::{decode_carets, decode_escapes, encoding_for_codepage};

/// First 22 bytes of every binary DXF file: `AutoCAD Binary DXF`, CR, LF, SUB, NUL.
pub const BINARY_SENTINEL: &[u8] = b"AutoCAD Binary DXF\r\n\x1a\0";

/// Typed value of a group.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// Text (already decoded to UTF-8).
    Str(String),
    /// Any integer group (16, 32 or 64 bit on disk).
    Int(i64),
    /// Floating point group. Non-finite numbers are replaced by `0.0`.
    Float(f64),
    /// Boolean group (codes 290-299).
    Bool(bool),
    /// Binary chunk (codes 310-319, 1004).
    Bytes(Vec<u8>),
}

impl Value {
    /// Numeric view; strings are parsed leniently.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f64),
            Value::Bool(b) => Some(f64::from(u8::from(*b))),
            Value::Str(s) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
            Value::Bytes(_) => None,
        }
    }

    /// Integer view; floats are truncated, strings are parsed leniently.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Bool(b) => Some(i64::from(*b)),
            Value::Float(f) => Some(*f as i64),
            Value::Str(s) => {
                let t = s.trim();
                t.parse::<i64>()
                    .ok()
                    .or_else(|| t.parse::<f64>().ok().map(|f| f as i64))
            }
            Value::Bytes(_) => None,
        }
    }

    /// Text view (only for string values).
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// One `(group code, value)` pair.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Pair {
    /// Group code.
    pub code: i32,
    /// Typed value.
    pub value: Value,
}

/// Value type implied by a group code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodeType {
    /// Text.
    Str,
    /// 64-bit float.
    Double,
    /// 16-bit integer.
    Int16,
    /// 32-bit integer.
    Int32,
    /// 64-bit integer.
    Int64,
    /// Boolean (one byte in binary files).
    Bool,
    /// Binary chunk (length-prefixed in binary files, hex text in ASCII files).
    Binary,
}

/// Returns the value type of `code` according to the DXF group-code ranges.
pub fn code_type(code: i32) -> CodeType {
    match code {
        10..=59 | 110..=149 | 210..=239 | 460..=469 | 1010..=1059 => CodeType::Double,
        60..=79 | 170..=179 | 270..=289 | 370..=389 | 400..=409 | 1060..=1070 => CodeType::Int16,
        90..=99 | 420..=429 | 440..=459 | 1071 => CodeType::Int32,
        160..=169 => CodeType::Int64,
        290..=299 => CodeType::Bool,
        310..=319 | 1004 => CodeType::Binary,
        _ => CodeType::Str,
    }
}

/// True when string values of `code` carry identifiers whose surrounding blanks are noise.
fn trims_text(code: i32) -> bool {
    matches!(code, 0 | 5 | 100..=102 | 105 | 330..=369 | 390..=399 | 480 | 481)
}

/// Returns true when `bytes` look like a DXF file (ASCII or binary).
///
/// ASCII files must start (after optional `999` comment pairs) with `0` / `SECTION`.
pub fn sniff(bytes: &[u8]) -> bool {
    if bytes.starts_with(BINARY_SENTINEL) {
        return true;
    }
    let head = bytes.get(..bytes.len().min(8192)).unwrap_or(&[]);
    let mut tok = Tokenizer::new(head);
    tok.set_decode_escapes(false);
    for _ in 0..64 {
        match tok.next_pair() {
            Ok(Some(Pair { code: 999, .. })) => continue,
            Ok(Some(Pair {
                code: 0,
                value: Value::Str(s),
            })) => {
                return s.eq_ignore_ascii_case("SECTION") || s.eq_ignore_ascii_case("EOF");
            }
            _ => return false,
        }
    }
    false
}

/// Group-code tokenizer over a byte slice. Also an [`Iterator`] of `Result<Pair>`
/// (it ends after the first error).
#[derive(Debug, Clone)]
pub struct Tokenizer<'a> {
    rd: ByteReader<'a>,
    binary: bool,
    wide_codes: bool,
    encoding: &'static Encoding,
    max_string: usize,
    decode_escapes: bool,
    failed: bool,
}

impl<'a> Tokenizer<'a> {
    /// Creates a tokenizer; the ASCII/binary flavor is detected from the sentinel.
    /// Defaults: windows-1252 text, escapes decoded, 16 MiB string limit.
    pub fn new(data: &'a [u8]) -> Self {
        let binary = data.starts_with(BINARY_SENTINEL);
        let mut rd = ByteReader::new(data);
        let mut wide_codes = false;
        if binary {
            let _ = rd.skip(BINARY_SENTINEL.len());
            // R13+ binary files use two-byte group codes: the first code (0) is `00 00`.
            wide_codes = rd.peek(2).map(|p| p.get(1) == Some(&0)).unwrap_or(false);
        } else if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
            let _ = rd.skip(3);
        }
        Self {
            rd,
            binary,
            wide_codes,
            encoding: encoding_rs::WINDOWS_1252,
            max_string: 16 << 20,
            decode_escapes: true,
            failed: false,
        }
    }

    /// Sets the text encoding for string values (UTF-8 for AC1021+ files).
    pub fn set_encoding(&mut self, encoding: &'static Encoding) {
        self.encoding = encoding;
    }

    /// Sets the maximum accepted length of one string value in bytes.
    pub fn set_max_string_bytes(&mut self, max: usize) {
        self.max_string = max;
    }

    /// Enables or disables `\U+XXXX` / `\M+nXXXX` decoding (on by default).
    pub fn set_decode_escapes(&mut self, on: bool) {
        self.decode_escapes = on;
    }

    /// True for binary DXF.
    pub fn is_binary(&self) -> bool {
        self.binary
    }

    /// Absolute byte offset of the next unread byte.
    pub fn offset(&self) -> u64 {
        self.rd.offset()
    }

    /// Reads the next pair; `Ok(None)` at a clean end of data.
    pub fn next_pair(&mut self) -> Result<Option<Pair>> {
        if self.failed {
            return Ok(None);
        }
        let r = if self.binary {
            self.next_binary()
        } else {
            self.next_ascii()
        };
        if r.is_err() {
            self.failed = true;
        }
        r
    }

    fn decode(&self, bytes: &[u8]) -> String {
        let text = if bytes.is_ascii() {
            String::from_utf8_lossy(bytes).into_owned()
        } else {
            self.encoding
                .decode_without_bom_handling(bytes)
                .0
                .into_owned()
        };
        let text = if !self.binary && text.contains('^') {
            decode_carets(&text)
        } else {
            text
        };
        if self.decode_escapes && text.contains('\\') {
            decode_escapes(&text)
        } else {
            text
        }
    }

    fn check_len(&self, len: usize) -> Result<()> {
        if len > self.max_string {
            return Err(Error::LimitExceeded(format!(
                "string of {len} bytes at offset {} exceeds the limit of {}",
                self.rd.offset(),
                self.max_string
            )));
        }
        Ok(())
    }

    // ---- ASCII -------------------------------------------------------------------------

    fn line(&mut self) -> Option<&'a [u8]> {
        let rest = self.rd.rest();
        if rest.is_empty() {
            return None;
        }
        let (line, advance) = match rest.iter().position(|&b| b == b'\n') {
            Some(i) => (rest.get(..i)?, i + 1),
            None => (rest, rest.len()),
        };
        self.rd.skip(advance).ok()?;
        Some(line.strip_suffix(b"\r").unwrap_or(line))
    }

    fn next_ascii(&mut self) -> Result<Option<Pair>> {
        let (code_at, code_line) = loop {
            let at = self.rd.offset();
            match self.line() {
                None => return Ok(None),
                Some(l) if l.iter().all(u8::is_ascii_whitespace) => continue,
                Some(l) => break (at, l),
            }
        };
        let code = std::str::from_utf8(code_line)
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok())
            .ok_or_else(|| Error::invalid(code_at, "group code is not an integer"))?;
        let value_at = self.rd.offset();
        let line = self.line().ok_or(Error::Truncated {
            offset: value_at,
            needed: 1,
        })?;
        self.check_len(line.len())?;
        let value = match code_type(code) {
            CodeType::Str => {
                let t = if trims_text(code) {
                    line.trim_ascii()
                } else {
                    line
                };
                Value::Str(self.decode(t))
            }
            CodeType::Double => {
                let t = std::str::from_utf8(line.trim_ascii()).unwrap_or("");
                match t.parse::<f64>() {
                    Ok(f) if f.is_finite() => Value::Float(f),
                    Ok(_) => Value::Float(0.0),
                    Err(_) => Value::Str(self.decode(line)),
                }
            }
            CodeType::Int16 | CodeType::Int32 | CodeType::Int64 => {
                let t = std::str::from_utf8(line.trim_ascii()).unwrap_or("");
                match t.parse::<i64>() {
                    Ok(i) => Value::Int(i),
                    Err(_) => Value::Str(self.decode(line)),
                }
            }
            CodeType::Bool => {
                let t = std::str::from_utf8(line.trim_ascii()).unwrap_or("");
                match t.parse::<i64>() {
                    Ok(i) => Value::Bool(i != 0),
                    Err(_) => Value::Str(self.decode(line)),
                }
            }
            CodeType::Binary => Value::Bytes(parse_hex(line)),
        };
        Ok(Some(Pair { code, value }))
    }

    // ---- binary ------------------------------------------------------------------------

    fn next_binary(&mut self) -> Result<Option<Pair>> {
        if self.rd.at_end() {
            return Ok(None);
        }
        let code = if self.wide_codes {
            i32::from(self.rd.u16()?)
        } else {
            let b = self.rd.u8()?;
            if b == 255 {
                i32::from(self.rd.u16()?)
            } else {
                i32::from(b)
            }
        };
        let value = match code_type(code) {
            CodeType::Str => {
                let rest = self.rd.rest();
                let end = rest.iter().position(|&b| b == 0);
                let Some(end) = end else {
                    return Err(self.rd.truncated(1));
                };
                self.check_len(end)?;
                let raw = rest.get(..end).unwrap_or(&[]);
                let t = if trims_text(code) {
                    raw.trim_ascii()
                } else {
                    raw
                };
                let s = self.decode(t);
                self.rd.skip(end + 1)?;
                Value::Str(s)
            }
            CodeType::Double => Value::Float(finite(self.rd.f64()?)),
            CodeType::Int16 => Value::Int(i64::from(self.rd.i16()?)),
            CodeType::Int32 => Value::Int(i64::from(self.rd.i32()?)),
            CodeType::Int64 => Value::Int(self.rd.i64()?),
            CodeType::Bool => Value::Bool(self.rd.u8()? != 0),
            CodeType::Binary => {
                let n = usize::from(self.rd.u8()?);
                Value::Bytes(self.rd.bytes(n)?.to_vec())
            }
        };
        Ok(Some(Pair { code, value }))
    }
}

impl Iterator for Tokenizer<'_> {
    type Item = Result<Pair>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next_pair() {
            Ok(Some(p)) => Some(Ok(p)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

fn finite(f: f64) -> f64 {
    if f.is_finite() { f } else { 0.0 }
}

fn parse_hex(line: &[u8]) -> Vec<u8> {
    let digits: Vec<u8> = line
        .iter()
        .filter_map(|&b| match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        })
        .collect();
    digits
        .chunks(2)
        .map(|c| (c.first().copied().unwrap_or(0) << 4) | c.get(1).copied().unwrap_or(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(data: &[u8]) -> Vec<Pair> {
        Tokenizer::new(data).map(|r| r.unwrap()).collect()
    }

    #[test]
    fn ascii_crlf_and_padding() {
        let p = pairs(
            b"  0\r\nSECTION\r\n  2\r\nHEADER\r\n 10\r\n1.5\r\n 70\r\n     3\r\n290\r\n1\r\n",
        );
        assert_eq!(p.len(), 5);
        assert_eq!(
            p[0],
            Pair {
                code: 0,
                value: Value::Str("SECTION".into())
            }
        );
        assert_eq!(p[2].value, Value::Float(1.5));
        assert_eq!(p[3].value, Value::Int(3));
        assert_eq!(p[4].value, Value::Bool(true));
    }

    #[test]
    fn ascii_lf_without_final_newline_and_hex_chunk() {
        let p = pairs(b"0\nTEXT\n310\nCAFE0\n1\nA\\U+00E9");
        assert_eq!(p[1].value, Value::Bytes(vec![0xCA, 0xFE, 0x00]));
        assert_eq!(p[2].value, Value::Str("A\u{e9}".into()));
    }

    #[test]
    fn ascii_truncated_value_is_an_error() {
        let mut t = Tokenizer::new(b"0\nSECTION\n2\n");
        assert!(t.next_pair().unwrap().is_some());
        assert!(t.next_pair().is_err());
        assert!(t.next_pair().unwrap().is_none());
    }

    #[test]
    fn binary_narrow_and_wide() {
        let mut narrow = BINARY_SENTINEL.to_vec();
        narrow.extend_from_slice(b"\x00SECTION\x00\x02HEADER\x00\x0a");
        narrow.extend_from_slice(&1.25f64.to_le_bytes());
        narrow.extend_from_slice(&[0xFF, 0x2A, 0x01]); // escaped code 298 (bool)
        narrow.push(1);
        let p = pairs(&narrow);
        assert_eq!(
            p[1],
            Pair {
                code: 2,
                value: Value::Str("HEADER".into())
            }
        );
        assert_eq!(
            p[2],
            Pair {
                code: 10,
                value: Value::Float(1.25)
            }
        );
        assert_eq!(
            p[3],
            Pair {
                code: 298,
                value: Value::Bool(true)
            }
        );

        let mut wide = BINARY_SENTINEL.to_vec();
        wide.extend_from_slice(b"\x00\x00SECTION\x00\x46\x00");
        wide.extend_from_slice(&7i16.to_le_bytes());
        let p = pairs(&wide);
        assert_eq!(
            p[1],
            Pair {
                code: 70,
                value: Value::Int(7)
            }
        );
    }

    #[test]
    fn string_limit() {
        let mut t = Tokenizer::new(b"1\nabcdefgh\n");
        t.set_max_string_bytes(4);
        assert!(matches!(t.next_pair(), Err(Error::LimitExceeded(_))));
    }

    #[test]
    fn sniffing() {
        assert!(sniff(b"  0\r\nSECTION\r\n  2\r\nHEADER\r\n"));
        assert!(sniff(b"999\ncomment\n0\nSECTION\n"));
        assert!(sniff(BINARY_SENTINEL));
        assert!(!sniff(b"hello world"));
        assert!(!sniff(b""));
        assert!(!sniff(b"AC1015\x00\x00\x00"));
    }

    #[test]
    fn codepage_decoding() {
        let mut t = Tokenizer::new(b"1\nSTR\xDC\n");
        t.set_encoding(encoding_rs::WINDOWS_1254);
        assert_eq!(
            t.next_pair().unwrap().unwrap().value,
            Value::Str("STR\u{dc}".into())
        );
    }
}
