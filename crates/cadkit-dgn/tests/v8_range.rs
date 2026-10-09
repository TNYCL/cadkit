//! The V8 element range stores a low corner and an extent (`high - low`), checked against
//! the vertex bounds of every line, line string and shape in the public ODA-written
//! sample `corpus/public/gdal/test_dgnv8.dgn`. Skips when the corpus is absent.
#![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used)]

mod common;

use cadkit_core::ReadOptions;
use cadkit_dgn::native::{ElementData, decode_v8, v8};
use common::corpus;

/// Rounding of the stored range to whole UOR: low is floored, high ceiled.
const TOL: f64 = 2.0;

#[test]
fn public_v8_ranges_are_low_corner_plus_extent() {
    let Some(bytes) = corpus("public/gdal/test_dgnv8.dgn") else {
        return;
    };
    let file = v8::read(&bytes, &ReadOptions::default()).unwrap();
    let enc = encoding_rs::WINDOWS_1252;
    let limits = ReadOptions::default().limits;
    let mut checked = 0;
    // Records whose low corner is not the origin tell the two readings apart.
    let mut distinguishing = 0;
    for raw in file
        .models
        .iter()
        .flat_map(|m| &m.graphic_pages)
        .flat_map(|p| &p.elements)
    {
        let element = decode_v8::decode(raw, enc, &limits);
        let points: Vec<[f64; 3]> = match element.data {
            ElementData::Line { start, end } => vec![start, end],
            ElementData::LineString { points } | ElementData::Shape { points } => points,
            _ => continue,
        };
        let [low, high] = element.header.range.expect("graphic range");
        for axis in 0..3 {
            let min = points.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min);
            let max = points
                .iter()
                .map(|p| p[axis])
                .fold(f64::NEG_INFINITY, f64::max);
            assert!((low[axis] - min).abs() <= TOL, "low corner is the minimum");
            assert!(
                (high[axis] - max).abs() <= TOL,
                "low + extent is the maximum"
            );
        }
        if low.iter().any(|v| v.abs() > TOL) {
            distinguishing += 1;
        }
        checked += 1;
    }
    assert!(
        checked >= 20,
        "public sample lines, line strings and shapes"
    );
    assert!(distinguishing >= 10, "records away from the origin");
}
