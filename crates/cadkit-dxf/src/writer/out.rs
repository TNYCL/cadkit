//! Group-code output buffer and number/text formatting.

use std::fmt::Write;

use cadkit_core::Point3;

/// ASCII DXF output buffer.
pub(crate) struct Out {
    pub buf: String,
    /// Strings are written as UTF-8 (R2007+); otherwise non-ASCII becomes `\U+XXXX`.
    pub utf8: bool,
}

fn write_f64(buf: &mut String, v: f64) {
    if !v.is_finite() || v == 0.0 {
        buf.push_str("0.0");
        return;
    }
    let a = v.abs();
    if !(1e-5..1e16).contains(&a) {
        let _ = write!(buf, "{v:e}");
        return;
    }
    let start = buf.len();
    let _ = write!(buf, "{v}");
    if !buf.get(start..).unwrap_or("").contains('.') {
        buf.push_str(".0");
    }
}

/// Formats a float so that it parses back to exactly the same value.
pub(crate) fn fmt_f64(v: f64) -> String {
    let mut out = String::with_capacity(24);
    write_f64(&mut out, v);
    out
}

fn write_escaped_text(out: &mut String, s: &str, utf8: bool) {
    for c in s.chars() {
        let u = c as u32;
        if u < 0x20 {
            if c == '\r' {
                continue;
            }
            out.push('^');
            out.push(char::from((u as u8) + 64));
        } else if c == '^' {
            // A literal caret is `^ ` in caret notation.
            out.push_str("^ ");
        } else if u == 0x7F {
            out.push_str("^?");
        } else if u > 0x7E && !utf8 {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                let _ = write!(out, "\\U+{unit:04X}");
            }
        } else {
            out.push(c);
        }
    }
}

/// Escapes `s` for a group value: control characters become `^X`, and on pre-2007 files
/// every non-ASCII character becomes `\U+XXXX` (UTF-16 code units).
pub(crate) fn escape_text(s: &str, utf8: bool) -> String {
    let mut out = String::with_capacity(s.len());
    write_escaped_text(&mut out, s, utf8);
    out
}

impl Out {
    pub fn new(utf8: bool) -> Self {
        Self {
            buf: String::new(),
            utf8,
        }
    }

    fn code(&mut self, code: i32) {
        let _ = write!(self.buf, "{code:>3}\r\n");
    }

    pub fn int(&mut self, code: i32, v: i64) {
        let _ = write!(self.buf, "{code:>3}\r\n{v}\r\n");
    }

    pub fn real(&mut self, code: i32, v: f64) {
        self.code(code);
        write_f64(&mut self.buf, v);
        self.buf.push_str("\r\n");
    }

    pub fn text(&mut self, code: i32, s: &str) {
        self.code(code);
        write_escaped_text(&mut self.buf, s, self.utf8);
        self.buf.push_str("\r\n");
    }

    /// Writes `s` verbatim (already clean: entity names, subclass markers).
    pub fn word(&mut self, code: i32, s: &str) {
        self.code(code);
        self.buf.push_str(s);
        self.buf.push_str("\r\n");
    }

    pub fn handle(&mut self, code: i32, h: u64) {
        let _ = write!(self.buf, "{code:>3}\r\n{h:X}\r\n");
    }

    /// Hex-encoded binary chunk (codes 310-319, 1004); split into 127-byte chunks.
    pub fn bytes(&mut self, code: i32, data: &[u8]) {
        for chunk in data.chunks(127) {
            self.code(code);
            for b in chunk {
                let _ = write!(self.buf, "{b:02X}");
            }
            self.buf.push_str("\r\n");
        }
    }

    /// Point in codes `base`, `base + 10`, `base + 20`.
    pub fn point(&mut self, base: i32, p: Point3) {
        self.real(base, p.x);
        self.real(base + 10, p.y);
        self.real(base + 20, p.z);
    }

    /// 2D point in codes `base` and `base + 10`.
    pub fn point2(&mut self, base: i32, x: f64, y: f64) {
        self.real(base, x);
        self.real(base + 10, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_round_trip() {
        for v in [
            0.1,
            1.0,
            -2.5,
            1e-7,
            1.5e300,
            123_456_789.123_456_79,
            std::f64::consts::PI,
            1e15,
            1e16,
            -0.0,
        ] {
            let s = fmt_f64(v);
            let back: f64 = s.parse().unwrap();
            assert_eq!(back, if v == 0.0 { 0.0 } else { v }, "{s}");
        }
        assert_eq!(fmt_f64(5.0), "5.0");
        assert_eq!(fmt_f64(f64::NAN), "0.0");
    }

    #[test]
    fn text_escaping() {
        assert_eq!(escape_text("caf\u{e9}", false), "caf\\U+00E9");
        assert_eq!(escape_text("caf\u{e9}", true), "caf\u{e9}");
        assert_eq!(escape_text("\u{1F600}", false), "\\U+D83D\\U+DE00");
        assert_eq!(escape_text("a\nb", true), "a^Jb");
        assert_eq!(escape_text("2^3", true), "2^ 3");
    }
}
