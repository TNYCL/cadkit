//! Whole-document comparison against the DXF reader (`cadkit_dxf::read`) on the ASCII
//! DXF export of each sample: every entity is matched by handle and compared
//! structurally (serialized form, floats within a relative tolerance of 1e-9).
//!
//! This is the acceptance criterion for AC1015–AC1032. Each entity is classified as
//! exact, approximate (float noise only), mismatched (with the first differing path) or
//! missing. Differences that are expected are listed in `known_difference` with the
//! reason; anything else fails the test.

// Integration test crate: helpers assert by panicking, like test bodies.
#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::print_stdout
)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use cadkit_core::{Document, Entity, ReadOptions};
use serde_json::Value as Json;

fn corpus_file(name: &str) -> Option<Vec<u8>> {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "corpus",
        "public",
        "acadsharp",
        name,
    ]
    .iter()
    .collect();
    std::fs::read(path).ok()
}

/// Where an entity lives: "model", "paper:<layout>", "block:<name>".
fn locate(doc: &Document) -> BTreeMap<u64, (String, &Entity)> {
    let mut out = BTreeMap::new();
    for m in &doc.models {
        let loc = match m.kind {
            cadkit_core::ModelKind::Model => "model".to_owned(),
            _ => format!("paper:{}", m.name),
        };
        for e in &m.entities {
            if let Some(id) = e.id {
                out.insert(id, (loc.clone(), e));
            }
        }
    }
    for b in &doc.blocks {
        for e in &b.entities {
            if let Some(id) = e.id {
                out.insert(id, (format!("block:{}", b.name), e));
            }
        }
    }
    out
}

/// Relative float tolerance.
const TOLERANCE: f64 = 1e-9;

/// Compares two JSON trees; returns the first differing path. Sets `approx` when a
/// float differed within the tolerance.
fn diff(a: &Json, b: &Json, path: &str, approx: &mut bool) -> Option<String> {
    match (a, b) {
        (Json::Number(x), Json::Number(y)) => {
            let (x, y) = (
                x.as_f64().unwrap_or(f64::NAN),
                y.as_f64().unwrap_or(f64::NAN),
            );
            if x == y {
                None
            } else if (x - y).abs() <= TOLERANCE * x.abs().max(y.abs()).max(1.0) {
                *approx = true;
                None
            } else {
                Some(format!("{path}: {x} vs {y}"))
            }
        }
        (Json::Array(x), Json::Array(y)) => {
            if x.len() != y.len() {
                return Some(format!("{path}: length {} vs {}", x.len(), y.len()));
            }
            x.iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (p, q))| diff(p, q, &format!("{path}[{i}]"), approx))
        }
        (Json::Object(x), Json::Object(y)) => {
            for (k, v) in x {
                let other = y.get(k).unwrap_or(&Json::Null);
                if let Some(d) = diff(v, other, &format!("{path}.{k}"), approx) {
                    return Some(d);
                }
            }
            y.keys()
                .find(|k| !x.contains_key(*k))
                .map(|k| format!("{path}.{k}: missing in DWG"))
        }
        _ if a == b => None,
        _ => Some(format!("{path}: {a} vs {b}")),
    }
}

/// `dwg.DIMENSION_LINEAR` / `dxf.DIMENSION` → `DIMENSION`, etc.
fn normalize_unknown(name: &str) -> String {
    let bare = name.trim_start_matches("dwg.").trim_start_matches("dxf.");
    if bare.starts_with("DIMENSION_") {
        return "DIMENSION".into();
    }
    match bare {
        "POLYLINE_2D" | "POLYLINE_3D" | "POLYLINE_PFACE" | "POLYLINE_MESH" => "POLYLINE".into(),
        other => other.into(),
    }
}

/// The comparable part of an entity: geometry/type data and symbology. Props are
/// format-specific and not compared; Unknown type names are normalized.
fn comparable(e: &Entity) -> Json {
    let mut kind = serde_json::to_value(&e.kind).unwrap();
    if let Some(Json::String(name)) = kind.get_mut("type_name") {
        *name = normalize_unknown(name);
    }
    serde_json::json!({
        "kind": kind,
        "layer": e.layer.clone().unwrap_or_else(|| "0".into()),
        "color": serde_json::to_value(e.color).unwrap(),
        "linetype": e.linetype,
        "lineweight": serde_json::to_value(e.lineweight).unwrap(),
        "visible": e.visible,
        "attributes": serde_json::to_value(&e.attributes).unwrap(),
    })
}

fn kind_name(e: &Entity) -> String {
    match &e.kind {
        cadkit_core::EntityKind::Unknown { type_name } => type_name.clone(),
        k => serde_json::to_value(k)
            .ok()
            .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_owned))
            .unwrap_or_default(),
    }
}

#[derive(Default, Debug)]
struct Tally {
    exact: usize,
    approx: usize,
    known: usize,
    mismatch: usize,
    missing: usize,
}

/// An expected, documented difference: when it applies (its evidence is checked), it
/// reconciles the two JSON trees and returns the reason. Rules are narrow so they
/// cannot hide unrelated differences: the trees are compared again afterwards.
type Rule = fn(version: &str, ours: &mut Json, theirs: &mut Json) -> Option<&'static str>;

fn kind_type(j: &Json) -> &str {
    j.pointer("/kind/type").and_then(Json::as_str).unwrap_or("")
}

/// The DXF export omits group 7 for the default style; the DWG stores its handle.
fn default_text_style(_: &str, ours: &mut Json, theirs: &mut Json) -> Option<&'static str> {
    if !matches!(kind_type(ours), "text" | "m_text") {
        return None;
    }
    let ours_style = ours
        .pointer("/kind/style")
        .and_then(Json::as_str)?
        .to_owned();
    let slot = theirs.pointer_mut("/kind/style")?;
    if ours_style.eq_ignore_ascii_case("Standard") && slot.is_null() {
        *slot = Json::String(ours_style);
        return Some(
            "DXF export omits group 7 for the default text style `Standard`; the DWG stores the STYLE handle",
        );
    }
    None
}

/// Fit-point splines store only fit data in the DWG; the DXF export adds the computed
/// control polygon and knots.
fn fit_spline(_: &str, ours: &mut Json, theirs: &mut Json) -> Option<&'static str> {
    if kind_type(ours) != "spline" {
        return None;
    }
    let empty = |j: &Json, p: &str| {
        j.pointer(p)
            .and_then(Json::as_array)
            .is_some_and(Vec::is_empty)
    };
    if !empty(ours, "/kind/control_points")
        || !empty(ours, "/kind/knots")
        || empty(ours, "/kind/fit_points")
    {
        return None;
    }
    if empty(theirs, "/kind/control_points") {
        return None;
    }
    // The export may or may not repeat the fit points next to the control polygon.
    for key in ["control_points", "knots", "weights", "fit_points"] {
        let value = theirs.pointer(&format!("/kind/{key}"))?.clone();
        *ours.pointer_mut(&format!("/kind/{key}"))? = value;
    }
    Some(
        "fit-point SPLINE: the DWG stores fit points and tangents only; the DXF export stores the computed control polygon",
    )
}

/// The DXF export inserts the hook-line vertex before the last leader point.
fn leader_hook(_: &str, ours: &mut Json, theirs: &mut Json) -> Option<&'static str> {
    if kind_type(ours) != "leader" {
        return None;
    }
    let n = ours
        .pointer("/kind/vertices")
        .and_then(Json::as_array)?
        .len();
    let list = theirs
        .pointer_mut("/kind/vertices")
        .and_then(Json::as_array_mut)?;
    if n >= 2 && list.len() == n + 1 {
        list.remove(n - 1);
        return Some(
            "the DXF export inserts the hook-line vertex before the last LEADER point; the DWG does not store it",
        );
    }
    None
}

const RULES: [Rule; 3] = [default_text_style, fit_spline, leader_hook];

/// Block `*U25` is the anonymous block of an ACAD_TABLE; the DXF export regenerated it
/// with new handles. Its entities count as "known" when the DWG block holds the same
/// kinds of entities, all under handles the DXF does not use.
fn regenerated_table_block(a: &Document, b: &Document, block: &str) -> bool {
    let kinds = |d: &Document| -> Option<Vec<String>> {
        let mut k: Vec<String> = d
            .blocks
            .iter()
            .find(|x| x.name == block)?
            .entities
            .iter()
            .map(kind_name)
            .collect();
        k.sort();
        Some(k)
    };
    let ids = |d: &Document| -> Vec<u64> {
        d.blocks
            .iter()
            .filter(|x| x.name == block)
            .flat_map(|x| x.entities.iter().filter_map(|e| e.id))
            .collect()
    };
    let (ours, theirs) = (ids(a), ids(b));
    kinds(a).is_some() && kinds(a) == kinds(b) && ours.iter().all(|id| !theirs.contains(id))
}

fn compare(version: &str) -> Option<BTreeMap<String, Tally>> {
    let dwg = corpus_file(&format!("sample_{version}.dwg"))?;
    let dxf = corpus_file(&format!("sample_{version}_ascii.dxf"))?;
    let options = ReadOptions::default();
    let a = cadkit_dwg::read(&dwg, &options).unwrap();
    let b = cadkit_dxf::read(&dxf, &options).unwrap();
    let ours = locate(&a);
    let theirs = locate(&b);
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    let mut reasons: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut unexpected = Vec::new();
    for (id, (loc, e)) in &theirs {
        let area = loc.split(':').next().unwrap_or("").to_owned();
        let t = tallies.entry(area).or_default();
        let Some((our_loc, ours_e)) = ours.get(id) else {
            let block = loc.strip_prefix("block:").unwrap_or("");
            if block == "*U25" && regenerated_table_block(&a, &b, block) {
                t.known += 1;
                *reasons.entry("the DXF export regenerated the ACAD_TABLE block *U25 with new handles (same entity kinds)").or_default() += 1;
            } else {
                t.missing += 1;
                unexpected.push(format!(
                    "{loc} {id:X} {}: not in the DWG document",
                    kind_name(e)
                ));
            }
            continue;
        };
        if our_loc != loc {
            t.mismatch += 1;
            unexpected.push(format!(
                "{loc} {id:X} {}: DWG puts it in {our_loc}",
                kind_name(e)
            ));
            continue;
        }
        let (mut x, mut y) = (comparable(ours_e), comparable(e));
        let mut approx = false;
        if diff(&x, &y, "", &mut approx).is_none() {
            if approx {
                t.approx += 1;
            } else {
                t.exact += 1;
            }
            continue;
        }
        let applied: Vec<&'static str> = RULES
            .iter()
            .filter_map(|rule| rule(version, &mut x, &mut y))
            .collect();
        let mut approx = false;
        match diff(&x, &y, "", &mut approx) {
            None if !applied.is_empty() => {
                t.known += 1;
                for r in applied {
                    *reasons.entry(r).or_default() += 1;
                }
            }
            None if approx => t.approx += 1,
            None => t.exact += 1,
            Some(d) => {
                t.mismatch += 1;
                unexpected.push(format!("{loc} {id:X} {}: {d}", kind_name(e)));
            }
        }
    }
    let extra = ours.keys().filter(|id| !theirs.contains_key(id)).count();
    println!("== {version}: {tallies:?}; entities only in the DWG document: {extra}");
    for (r, n) in &reasons {
        println!("   known ({n}): {r}");
    }
    for u in &unexpected {
        println!("   UNEXPECTED {u}");
    }
    Some(tallies)
}

#[test]
fn documents_match_dxf_oracle() {
    let mut failures = 0;
    for version in ["AC1015", "AC1018", "AC1021", "AC1024", "AC1027", "AC1032"] {
        if let Some(t) = compare(version) {
            failures += t.values().map(|t| t.mismatch + t.missing).sum::<usize>();
        }
    }
    assert_eq!(failures, 0, "unexpected differences (see the report above)");
}
