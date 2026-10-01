//! Helpers shared by the corpus tests.
#![allow(dead_code, clippy::indexing_slicing)]

use std::path::PathBuf;

use cadkit_core::{Entity, EntityKind, Point3, Value};

/// Bytes of `corpus/<rel>`, or `None` when the file is not present (tests then skip).
pub fn corpus(rel: &str) -> Option<Vec<u8>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(rel);
    std::fs::read(path).ok()
}

/// Every `.dgn` file under `corpus/private/<sub>` (empty when the directory is absent).
/// Files are discovered, never named, so no private identifier appears in the sources.
pub fn private_files(sub: &str) -> Vec<Vec<u8>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/private")
        .join(sub);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("dgn")))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| std::fs::read(p).ok())
        .collect()
}

pub fn prop_int(e: &Entity, key: &str) -> Option<i64> {
    match e.props.get(key) {
        Some(Value::Int(v)) => Some(*v),
        _ => None,
    }
}

/// A `[x, y, z]` list prop as a point.
pub fn prop_point(e: &Entity, key: &str) -> Point3 {
    match e.props.get(key) {
        Some(Value::List(v)) => {
            let f = |i: usize| match v.get(i) {
                Some(Value::Float(x)) => *x,
                _ => f64::NAN,
            };
            Point3::new(f(0), f(1), f(2))
        }
        _ => Point3::new(f64::NAN, f64::NAN, f64::NAN),
    }
}

pub fn walk<'a>(e: &'a Entity, out: &mut Vec<&'a Entity>) {
    out.push(e);
    if let EntityKind::Group { children, .. } = &e.kind {
        for c in children {
            walk(c, out);
        }
    }
}

/// Minimal RFC 4180 CSV reader (quoted fields, doubled quotes).
pub fn csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            ('"', true) => quoted = false,
            ('"', false) => quoted = true,
            (',', false) => row.push(std::mem::take(&mut field)),
            ('\n', false) => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            ('\r', false) => {}
            (c, _) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// Every coordinate tuple of a WKT string, grouped by innermost parenthesis.
pub fn wkt_sequences(wkt: &str) -> Vec<Vec<Point3>> {
    let mut seqs: Vec<Vec<Point3>> = Vec::new();
    let mut cur: Vec<Point3> = Vec::new();
    let mut nums: Vec<f64> = Vec::new();
    let mut tok = String::new();
    let flush_tok = |tok: &mut String, nums: &mut Vec<f64>| {
        if let Ok(v) = tok.parse::<f64>() {
            nums.push(v);
        }
        tok.clear();
    };
    let flush_pt = |nums: &mut Vec<f64>, cur: &mut Vec<Point3>| {
        if nums.len() >= 2 {
            cur.push(Point3::new(
                nums[0],
                nums[1],
                nums.get(2).copied().unwrap_or(0.0),
            ));
        }
        nums.clear();
    };
    for c in wkt.chars() {
        match c {
            '(' => {
                flush_tok(&mut tok, &mut nums);
                if !cur.is_empty() {
                    seqs.push(std::mem::take(&mut cur));
                }
            }
            ')' => {
                flush_tok(&mut tok, &mut nums);
                flush_pt(&mut nums, &mut cur);
                if !cur.is_empty() {
                    seqs.push(std::mem::take(&mut cur));
                }
            }
            ',' => {
                flush_tok(&mut tok, &mut nums);
                flush_pt(&mut nums, &mut cur);
            }
            ' ' => flush_tok(&mut tok, &mut nums),
            c if c.is_ascii_digit() || c == '.' || c == '-' || c == 'e' => tok.push(c),
            _ => tok.clear(),
        }
    }
    seqs
}

pub fn dist(a: Point3, b: Point3) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}
