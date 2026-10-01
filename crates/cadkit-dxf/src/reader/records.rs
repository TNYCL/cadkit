//! Groups the pair stream into records (`0 NAME` followed by its group codes).

use cadkit_core::Result;

use crate::native::{Pair, Tokenizer, Value};

/// One DXF record: the text of its leading `0` group and every following group up to the
/// next `0`. Reactor blocks (`102 {...` to `102 }`) are removed.
#[derive(Debug, Clone)]
pub(crate) struct Record {
    /// Upper-cased record name (`SECTION`, `LINE`, `LAYER`, ...).
    pub name: String,
    /// Remaining groups in file order.
    pub pairs: Vec<Pair>,
    /// Byte offset where the record started.
    pub offset: u64,
}

impl Record {
    /// Groups before the first `1001` and before an `101 Embedded Object` part (the entity
    /// body). The embedded object reuses codes (10, 11, 40, ...) with other meanings.
    pub fn body(&self) -> &[Pair] {
        let end = self.xdata_start();
        let end = self.pairs.get(..end).unwrap_or(&[]);
        let cut = end.iter().position(is_embedded_marker).unwrap_or(end.len());
        end.get(..cut).unwrap_or(&[])
    }

    /// Groups of the `101 Embedded Object` part (R2018 multi-line ATTRIB/ATTDEF/MTEXT), if any.
    pub fn embedded(&self) -> &[Pair] {
        let end = self.xdata_start();
        let all = self.pairs.get(..end).unwrap_or(&[]);
        match all.iter().position(is_embedded_marker) {
            Some(i) => all.get(i + 1..).unwrap_or(&[]),
            None => &[],
        }
    }

    fn xdata_start(&self) -> usize {
        self.pairs
            .iter()
            .position(|p| p.code == 1001)
            .unwrap_or(self.pairs.len())
    }

    /// Groups from the first `1001` on (extended data).
    pub fn xdata(&self) -> &[Pair] {
        let start = self
            .pairs
            .iter()
            .position(|p| p.code == 1001)
            .unwrap_or(self.pairs.len());
        self.pairs.get(start..).unwrap_or(&[])
    }
}

fn is_embedded_marker(p: &Pair) -> bool {
    p.code == 101
}

/// Record reader with one record of push-back.
pub(crate) struct RecordReader<'a> {
    tok: Tokenizer<'a>,
    peeked: Option<Pair>,
    pushed_back: Option<Record>,
    error: Option<cadkit_core::Error>,
    max_pairs: usize,
    /// Number of stray groups (other than `999` comments) skipped outside of any record.
    pub stray: u64,
    /// First `999` comment of the file, if any.
    pub comment: Option<String>,
}

impl<'a> RecordReader<'a> {
    pub fn new(tok: Tokenizer<'a>, max_pairs: usize) -> Self {
        Self {
            tok,
            peeked: None,
            pushed_back: None,
            error: None,
            max_pairs,
            stray: 0,
            comment: None,
        }
    }

    /// Tokenizer error that ended the stream, if any (taken once).
    pub fn take_error(&mut self) -> Option<cadkit_core::Error> {
        self.error.take()
    }

    pub fn offset(&self) -> u64 {
        self.tok.offset()
    }

    pub fn push_back(&mut self, rec: Record) {
        self.pushed_back = Some(rec);
    }

    fn pair(&mut self) -> Option<Pair> {
        if let Some(p) = self.peeked.take() {
            return Some(p);
        }
        if self.error.is_some() {
            return None;
        }
        match self.tok.next_pair() {
            Ok(p) => p,
            Err(e) => {
                self.error = Some(e);
                None
            }
        }
    }

    /// Next record, or `None` at the end of data. A tokenizer error ends the stream after
    /// the partially read record has been returned; see [`Self::take_error`].
    pub fn next_record(&mut self) -> Result<Option<Record>> {
        if let Some(r) = self.pushed_back.take() {
            return Ok(Some(r));
        }
        // Find the `0` group that starts the record, skipping comments and stray groups.
        let (offset, first) = loop {
            let at = self.tok.offset();
            match self.pair() {
                None => return Ok(None),
                Some(p) if p.code == 0 => break (at, p),
                Some(p) if p.code == 999 => {
                    if self.comment.is_none() {
                        self.comment = p.value.as_str().map(|s| s.chars().take(200).collect());
                    }
                }
                Some(_) => self.stray += 1,
            }
        };
        let name = match &first.value {
            Value::Str(s) => s.trim().to_ascii_uppercase(),
            _ => String::new(),
        };
        let mut pairs = Vec::new();
        let mut in_reactors = false;
        while let Some(p) = self.pair() {
            if p.code == 0 {
                self.peeked = Some(p);
                break;
            }
            if p.code == 102 {
                let text = p.value.as_str().unwrap_or("");
                if text.starts_with('{') {
                    in_reactors = true;
                    continue;
                }
                if text.starts_with('}') {
                    in_reactors = false;
                    continue;
                }
            }
            if in_reactors {
                continue;
            }
            if pairs.len() >= self.max_pairs {
                return Err(cadkit_core::Error::LimitExceeded(format!(
                    "record `{name}` at offset {offset} has more than {} groups",
                    self.max_pairs
                )));
            }
            pairs.push(p);
        }
        Ok(Some(Record {
            name,
            pairs,
            offset,
        }))
    }
}

/// Convenience accessors over a slice of pairs.
#[derive(Clone, Copy)]
pub(crate) struct Group<'a>(pub &'a [Pair]);

impl<'a> Group<'a> {
    pub fn value(&self, code: i32) -> Option<&'a Value> {
        self.0.iter().find(|p| p.code == code).map(|p| &p.value)
    }

    pub fn f64(&self, code: i32) -> Option<f64> {
        self.value(code).and_then(Value::as_f64)
    }

    pub fn f64_or(&self, code: i32, default: f64) -> f64 {
        self.f64(code).unwrap_or(default)
    }

    pub fn i64(&self, code: i32) -> Option<i64> {
        self.value(code).and_then(Value::as_i64)
    }

    pub fn i64_or(&self, code: i32, default: i64) -> i64 {
        self.i64(code).unwrap_or(default)
    }

    pub fn text(&self, code: i32) -> Option<&'a str> {
        self.value(code).and_then(Value::as_str)
    }

    pub fn string(&self, code: i32) -> Option<String> {
        self.text(code).map(str::to_owned)
    }

    /// Values of every group with `code`, in order.
    pub fn all(&self, code: i32) -> impl Iterator<Item = &'a Value> + use<'a> {
        let pairs = self.0;
        pairs
            .iter()
            .filter(move |p| p.code == code)
            .map(|p| &p.value)
    }

    /// Point stored in codes `base`, `base + 10`, `base + 20` (first occurrence each).
    /// `None` when neither X nor Y is present.
    pub fn point(&self, base: i32) -> Option<cadkit_core::Point3> {
        let x = self.f64(base);
        let y = self.f64(base + 10);
        if x.is_none() && y.is_none() {
            return None;
        }
        Some(cadkit_core::Point3::new(
            x.unwrap_or(0.0),
            y.unwrap_or(0.0),
            self.f64_or(base + 20, 0.0),
        ))
    }

    pub fn point_or_zero(&self, base: i32) -> cadkit_core::Point3 {
        self.point(base).unwrap_or_default()
    }
}

/// Parses a hexadecimal handle.
pub(crate) fn parse_handle(s: &str) -> Option<u64> {
    u64::from_str_radix(s.trim(), 16).ok()
}
