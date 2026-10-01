//! Text helpers shared by the DXF and DWG readers: MTEXT format-code stripping and the
//! `\U+XXXX` / `\M+nXXXX` escape decoding. Panic-free; output size is bounded by the input.

/// Strips MTEXT formatting from `value` and returns plain text with `\n` line breaks.
///
/// Handled: `\P` / `\X` (paragraph), `\~` (non-breaking space), `\\`, `\{`, `\}`, toggles
/// (`\L \l \O \o \K \k`), parameterised codes terminated by `;` (`\f \F \C \c \H \T \Q \W \A \p`),
/// stacked fractions (`\Snum^den;` becomes `num/den`), grouping braces and the `%%d %%p %%c`
/// special characters. There is no recursion: braces are simply dropped.
///
/// `%%nnn` (decimal character code) is decoded as Latin-1 / Unicode scalar `nnn`; use
/// [`plain_text_in`] to decode codes below 256 through the drawing's code page instead.
pub fn plain_text(value: &str) -> String {
    plain_text_in(value, None)
}

/// Like [`plain_text`], but `%%nnn` with `nnn < 256` is a byte of the drawing's code page and
/// is decoded by `byte` (return `None` to fall back to the Latin-1 scalar). Codes of 256 and
/// above are always taken as Unicode scalars.
pub fn plain_text_in(value: &str, byte: Option<&dyn Fn(u8) -> Option<char>>) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    let mut i = 0usize;
    while let Some(&c) = chars.get(i) {
        match c {
            '\\' => {
                let Some(&n) = chars.get(i + 1) else {
                    break;
                };
                i += 2;
                match n {
                    'P' | 'X' => out.push('\n'),
                    '~' => out.push(' '),
                    '\\' | '{' | '}' => out.push(n),
                    'L' | 'l' | 'O' | 'o' | 'K' | 'k' => {}
                    'S' => {
                        let (body, next) = take_until_semicolon(&chars, i);
                        i = next;
                        out.push_str(&stack_text(&body));
                    }
                    'f' | 'F' | 'C' | 'c' | 'H' | 'T' | 'Q' | 'W' | 'A' | 'p' => {
                        let (_, next) = take_until_semicolon(&chars, i);
                        i = next;
                    }
                    other => out.push(other),
                }
            }
            '{' | '}' => i += 1,
            '%' if chars.get(i + 1) == Some(&'%') => {
                let code = chars.get(i + 2).copied();
                match code {
                    Some('d' | 'D') => out.push('\u{b0}'),
                    Some('p' | 'P') => out.push('\u{b1}'),
                    Some('c' | 'C') => out.push('\u{2205}'),
                    Some('%') => out.push('%'),
                    Some('u' | 'U' | 'o' | 'O' | 'k' | 'K') => {}
                    Some(d) if d.is_ascii_digit() => {
                        let digits: String = chars
                            .iter()
                            .skip(i + 2)
                            .take(3)
                            .take_while(|c| c.is_ascii_digit())
                            .collect();
                        let code = digits.parse::<u32>().ok();
                        let via_page = match (code, byte) {
                            (Some(c), Some(f)) if c < 256 => f(c as u8),
                            _ => None,
                        };
                        if let Some(ch) = via_page.or_else(|| code.and_then(char::from_u32)) {
                            out.push(ch);
                        }
                        i += 2 + digits.len();
                        continue;
                    }
                    _ => {
                        out.push('%');
                        i += 1;
                        continue;
                    }
                }
                i += 3;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn hex_val(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some(u32::from(b - b'0')),
        b'a'..=b'f' => Some(u32::from(b - b'a') + 10),
        b'A'..=b'F' => Some(u32::from(b - b'A') + 10),
        _ => None,
    }
}

fn hex4(bytes: &[u8], at: usize) -> Option<u32> {
    let digits = bytes.get(at..at.checked_add(4)?)?;
    digits
        .iter()
        .try_fold(0u32, |v, &d| Some(v * 16 + hex_val(d)?))
}

/// Decodes `\U+XXXX` (a UTF-16 code unit; surrogate pairs are joined) and `\M+nXXXX`
/// escapes. `\M+` carries a double-byte character of a legacy code page selected by the
/// digit `n`; the caller supplies the decoder `mbcs(n, [high, low])` (`n` is the ASCII
/// digit) because the code page tables live in `encoding_rs`, which this crate does not
/// depend on. Escapes that cannot be decoded are kept verbatim.
pub fn decode_escapes(s: &str, mbcs: &dyn Fn(u8, [u8; 2]) -> Option<String>) -> String {
    if !s.contains("\\U+") && !s.contains("\\u+") && !s.contains("\\M+") {
        return s.to_owned();
    }
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let (mut i, mut literal) = (0usize, 0usize);
    while i < bytes.len() {
        let escape = bytes.get(i) == Some(&b'\\') && bytes.get(i + 2) == Some(&b'+');
        let kind = bytes.get(i + 1).copied();
        // An escaped backslash (two backslashes) is literal text: it never starts an escape.
        if bytes.get(i) == Some(&b'\\') && kind == Some(b'\\') {
            i += 2;
            continue;
        }
        let mut decoded: Option<(String, usize)> = None;
        if escape && matches!(kind, Some(b'U' | b'u')) {
            if let Some(unit) = hex4(bytes, i + 3) {
                if (0xD800..0xDC00).contains(&unit) {
                    let low_follows = bytes.get(i + 7) == Some(&b'\\')
                        && matches!(bytes.get(i + 8), Some(b'U' | b'u'))
                        && bytes.get(i + 9) == Some(&b'+');
                    if let Some(low) = hex4(bytes, i + 10).filter(|_| low_follows) {
                        if let Some(Ok(c)) = char::decode_utf16([unit as u16, low as u16]).next() {
                            decoded = Some((c.to_string(), 14));
                        }
                    }
                } else if let Some(c) = char::from_u32(unit) {
                    decoded = Some((c.to_string(), 7));
                }
            }
        } else if escape && kind == Some(b'M') {
            if let (Some(n), Some(code)) = (bytes.get(i + 3).copied(), hex4(bytes, i + 4)) {
                let pair = [(code >> 8) as u8, code as u8];
                if let Some(text) = mbcs(n, pair).filter(|t| !t.is_empty()) {
                    decoded = Some((text, 8));
                }
            }
        }
        match decoded {
            Some((text, len)) => {
                out.push_str(s.get(literal..i).unwrap_or(""));
                out.push_str(&text);
                i += len;
                literal = i;
            }
            None => i += 1,
        }
    }
    out.push_str(s.get(literal..).unwrap_or(""));
    out
}

fn take_until_semicolon(chars: &[char], start: usize) -> (String, usize) {
    let mut s = String::new();
    let mut i = start;
    while let Some(&c) = chars.get(i) {
        i += 1;
        if c == ';' {
            return (s, i);
        }
        s.push(c);
        if s.len() > 4096 {
            break;
        }
    }
    (s, i)
}

fn stack_text(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut escaped = false;
    for c in body.chars() {
        if escaped {
            out.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '^' | '#' | '/' => out.push('/'),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_mbcs(n: u8, pair: [u8; 2]) -> Option<String> {
        (n == b'1' && pair == [0x82, 0xA0]).then(|| "\u{3042}".to_owned())
    }

    #[test]
    fn escapes() {
        assert_eq!(decode_escapes("a\\U+00E9b", &fake_mbcs), "a\u{e9}b");
        assert_eq!(decode_escapes("\\U+D83D\\U+DE00", &fake_mbcs), "\u{1F600}");
        assert_eq!(decode_escapes("\\U+D83Dx", &fake_mbcs), "\\U+D83Dx");
        assert_eq!(decode_escapes("C:\\Users\\x", &fake_mbcs), "C:\\Users\\x");
        assert_eq!(decode_escapes("\\U+12", &fake_mbcs), "\\U+12");
        assert_eq!(decode_escapes("\\M+182A0", &fake_mbcs), "\u{3042}");
        assert_eq!(decode_escapes("\\M+982A0", &fake_mbcs), "\\M+982A0");
        assert_eq!(decode_escapes("\\", &fake_mbcs), "\\");
        // An escaped backslash is literal; an escape after an odd run still decodes.
        assert_eq!(decode_escapes("\\\\U+0041", &fake_mbcs), "\\\\U+0041");
        assert_eq!(decode_escapes("\\\\\\U+0041", &fake_mbcs), "\\\\A");
        assert_eq!(decode_escapes("\\\\M+182A0", &fake_mbcs), "\\\\M+182A0");
    }

    #[test]
    fn percent_codes_use_the_code_page() {
        assert_eq!(plain_text("%%233"), "\u{e9}");
        let page = |b: u8| (b == 233).then_some('\u{13a}');
        assert_eq!(
            plain_text_in("%%233 %%065 %%400", Some(&page)),
            "\u{13a} A \u{190}"
        );
    }

    #[test]
    fn strips_codes() {
        assert_eq!(plain_text("{\\fArial|b0;Hello}\\Pworld"), "Hello\nworld");
        assert_eq!(
            plain_text("\\C1;red \\H2.5;big\\L under\\l"),
            "red big under"
        );
        assert_eq!(plain_text("a\\~b \\\\ \\{x\\}"), "a b \\ {x}");
        assert_eq!(plain_text("\\S1^2;"), "1/2");
        assert_eq!(
            plain_text("45%%d %%c10 %%p1 100%%%"),
            "45\u{b0} \u{2205}10 \u{b1}1 100%"
        );
        assert_eq!(plain_text("trailing\\"), "trailing");
    }
}
