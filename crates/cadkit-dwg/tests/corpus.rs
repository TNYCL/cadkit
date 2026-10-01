//! Corpus tests against the ACadSharp samples (`corpus/public/acadsharp`, fetched by
//! `scripts/fetch-corpus.py`). Each DWG is compared with the ASCII DXF export of the
//! same drawing: layer names, block names and model-space counts of the entity types
//! this reader maps. Tests skip silently when the corpus is missing.

// Integration test crate: the helper functions assert by panicking, like test bodies.
#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use cadkit_core::{Document, EntityKind, ModelKind, ReadOptions, Value};
use cadkit_dwg::native::read_native;

/// Entity types mapped to typed geometry, by DXF record name.
const MAPPED: [&str; 7] = [
    "LINE",
    "POINT",
    "CIRCLE",
    "ARC",
    "LWPOLYLINE",
    "TEXT",
    "INSERT",
];

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

/// What the DXF says about the drawing.
#[derive(Debug, Default)]
struct Oracle {
    layers: Vec<String>,
    blocks: Vec<String>,
    model_counts: BTreeMap<String, usize>,
    insunits: Option<i64>,
}

/// Minimal group-code scanner for ASCII DXF (test-side only).
fn scan_dxf(bytes: &[u8]) -> Oracle {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().collect();
    let mut records: Vec<(String, Vec<(i32, String)>)> = Vec::new();
    for pair in lines.chunks(2) {
        let [code, value] = pair else { break };
        let Ok(code) = code.trim().parse::<i32>() else {
            continue;
        };
        let value = value.trim().to_owned();
        if code == 0 {
            records.push((value, Vec::new()));
        } else if let Some((_, groups)) = records.last_mut() {
            groups.push((code, value));
        }
    }
    let mut o = Oracle::default();
    let mut section = String::new();
    let mut header_var = String::new();
    for (kind, groups) in &records {
        let group = |c: i32| {
            groups
                .iter()
                .find(|(g, _)| *g == c)
                .map(|(_, v)| v.as_str())
        };
        match kind.as_str() {
            "SECTION" => section = group(2).unwrap_or_default().to_owned(),
            "ENDSEC" => section.clear(),
            _ => {}
        }
        match section.as_str() {
            "HEADER" if kind == "SECTION" => {
                for (code, value) in groups {
                    if *code == 9 {
                        header_var = value.clone();
                    } else if header_var == "$INSUNITS" && *code == 70 {
                        o.insunits = value.parse().ok();
                    }
                }
            }
            "TABLES" if kind == "LAYER" => o.layers.extend(group(2).map(str::to_owned)),
            "BLOCKS" if kind == "BLOCK" => o.blocks.extend(group(2).map(str::to_owned)),
            "ENTITIES" if kind != "SECTION" && group(67) != Some("1") => {
                *o.model_counts.entry(kind.clone()).or_default() += 1;
            }
            _ => {}
        }
    }
    o
}

fn kind_name(kind: &EntityKind) -> &'static str {
    match kind {
        EntityKind::Line { .. } => "LINE",
        EntityKind::Point { .. } => "POINT",
        EntityKind::Circle { .. } => "CIRCLE",
        EntityKind::Arc { .. } => "ARC",
        EntityKind::Polyline { .. } => "LWPOLYLINE",
        EntityKind::Text { .. } => "TEXT",
        EntityKind::Insert { .. } => "INSERT",
        _ => "OTHER",
    }
}

/// Layer names, block names (definitions plus the space blocks behind models) and
/// mapped model-space counts of a document.
/// `types` maps handles to native type names, to tell LWPOLYLINE from POLYLINE.
fn summarize(
    doc: &Document,
    types: &BTreeMap<u64, String>,
) -> (Vec<String>, Vec<String>, BTreeMap<String, usize>) {
    let layers = doc.layers.iter().map(|l| l.name.clone()).collect();
    let mut blocks: Vec<String> = doc.blocks.iter().map(|b| b.name.clone()).collect();
    for m in &doc.models {
        if let Some(Value::Text(name)) = m.props.get("dwg.block") {
            blocks.push(name.clone());
        }
    }
    let mut counts = BTreeMap::new();
    for m in doc.models.iter().filter(|m| m.kind == ModelKind::Model) {
        for e in &m.entities {
            let mut name = kind_name(&e.kind);
            if name == "LWPOLYLINE"
                && e.id
                    .and_then(|h| types.get(&h))
                    .is_some_and(|t| t != "LWPOLYLINE")
            {
                name = "POLYLINE";
            }
            *counts.entry(name.to_owned()).or_default() += 1;
        }
    }
    (layers, blocks, counts)
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

/// R13/R14 have no extended symbol names: the same drawing saved as R14 has its names
/// upper-cased with characters outside `[A-Z0-9_$-]` replaced by `_` (`0 @ 1` →
/// `0___1`). Applied to the R2000 oracle when checking the R14 sample.
fn r14_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            let c = c.to_ascii_uppercase();
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '-' | '*') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Reads `sample_<ver>.dwg` and compares it with `sample_<oracle>_ascii.dxf`.
fn check_against_oracle(version: &str, oracle_version: &str) {
    let (Some(dwg), Some(dxf)) = (
        corpus_file(&format!("sample_{version}.dwg")),
        corpus_file(&format!("sample_{oracle_version}_ascii.dxf")),
    ) else {
        return;
    };
    let mut oracle = scan_dxf(&dxf);
    if version == "AC1014" {
        oracle.layers = oracle.layers.iter().map(|n| r14_name(n)).collect();
        oracle.blocks = oracle.blocks.iter().map(|n| r14_name(n)).collect();
        // R14 has no $INSUNITS.
        oracle.insunits = None;
    }
    let doc = cadkit_dwg::read(&dwg, &ReadOptions::default())
        .unwrap_or_else(|e| panic!("{version}: {e}"));
    assert_eq!(doc.source.version, version);
    let native = read_native(&dwg, &ReadOptions::default()).unwrap();
    let types: BTreeMap<u64, String> = native
        .objects
        .iter()
        .map(|(h, o)| (*h, o.type_name.clone()))
        .collect();
    let (layers, blocks, counts) = summarize(&doc, &types);

    assert_eq!(layers, oracle.layers, "{version}: layer names and order");
    assert_eq!(
        sorted(blocks),
        sorted(oracle.blocks.clone()),
        "{version}: block names"
    );
    for kind in MAPPED {
        let expected = oracle.model_counts.get(kind).copied().unwrap_or(0);
        let got = counts.get(kind).copied().unwrap_or(0);
        assert_eq!(got, expected, "{version}: model-space {kind} count");
    }
    if let Some(units) = oracle.insunits {
        assert_eq!(
            doc.props.get("dwg.header.INSUNITS"),
            Some(&Value::Int(units)),
            "{version}: $INSUNITS"
        );
    }
    // Everything else in model space is kept as Unknown, never dropped: the total
    // number of model-space entities equals the DXF count minus the records the
    // model folds into their parent (ATTRIB into INSERT, VERTEX/SEQEND into POLYLINE).
    let folded = ["ATTRIB", "VERTEX", "SEQEND"];
    let oracle_total: usize = oracle
        .model_counts
        .iter()
        .filter(|(k, _)| !folded.contains(&k.as_str()))
        .map(|(_, n)| n)
        .sum();
    let total: usize = counts.values().sum();
    assert_eq!(total, oracle_total, "{version}: model-space entity total");
}

/// No DXF export exists for the R14 sample; it is the same drawing as the R2000 one.
#[test]
fn ac1014_matches_r2000_dxf_oracle() {
    check_against_oracle("AC1014", "AC1015");
}

#[test]
fn ac1015_matches_dxf_oracle() {
    check_against_oracle("AC1015", "AC1015");
}

#[test]
fn ac1018_matches_dxf_oracle() {
    check_against_oracle("AC1018", "AC1018");
}

#[test]
fn ac1021_matches_dxf_oracle() {
    check_against_oracle("AC1021", "AC1021");
}

#[test]
fn ac1024_matches_dxf_oracle() {
    check_against_oracle("AC1024", "AC1024");
}

#[test]
fn ac1027_matches_dxf_oracle() {
    check_against_oracle("AC1027", "AC1027");
}

#[test]
fn ac1032_matches_dxf_oracle() {
    check_against_oracle("AC1032", "AC1032");
}

#[test]
fn every_sample_decodes_cleanly() {
    for version in [
        "AC1014", "AC1015", "AC1018", "AC1021", "AC1024", "AC1027", "AC1032",
    ] {
        let Some(dwg) = corpus_file(&format!("sample_{version}.dwg")) else {
            continue;
        };
        let file =
            read_native(&dwg, &ReadOptions::default()).unwrap_or_else(|e| panic!("{version}: {e}"));
        assert_eq!(file.version.magic(), version);
        assert!(file.header.complete, "{version}: header incomplete");
        assert_eq!(file.header.crc_ok, Some(true), "{version}: header CRC");
        assert_eq!(file.classes_crc_ok, Some(true), "{version}: classes CRC");
        assert!(
            file.warnings
                .iter()
                .all(|w| w.code != "dwg.checksum_mismatch" && w.code != "dwg.crc_mismatch"),
            "{version}: {:?}",
            file.warnings
        );
        assert_eq!(
            file.objects.len(),
            file.handle_map.len(),
            "{version}: objects that failed to parse"
        );
        let crc_failures = file
            .objects
            .values()
            .filter(|o| o.crc_ok == Some(false))
            .count();
        assert_eq!(crc_failures, 0, "{version}: object CRC failures");
        // Header-derived values the DXF export shows for every version.
        let ext = file.header.point3("EXTMAX").unwrap_or_default();
        assert!(
            (ext[0] - 1852.185633549592).abs() < 1e-9,
            "{version}: EXTMAX {ext:?}"
        );
        assert_eq!(file.measurement, Some(0), "{version}: MEASUREMENT");
        for (h, o) in &file.objects {
            assert_eq!(o.handle, *h, "{version}: handle mismatch");
        }
    }
}

/// Private group codes used to attach ATTRIB tag/value pairs to their INSERT.
const ATTRIB_TAG: i32 = -1002;
const ATTRIB_VALUE: i32 = -1001;

/// A DXF record: (type, handle, group codes and values).
type DxfRecord = (String, u64, Vec<(i32, String)>);

/// Model-space DXF records of the mapped types.
fn dxf_entities(bytes: &[u8]) -> Vec<DxfRecord> {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut section = String::new();
    let mut current: Option<(String, Vec<(i32, String)>)> = None;
    let flush = |rec: Option<(String, Vec<(i32, String)>)>, section: &str, out: &mut Vec<_>| {
        let Some((kind, groups)) = rec else { return };
        let group = |c: i32| {
            groups
                .iter()
                .find(|(g, _)| *g == c)
                .map(|(_, v)| v.as_str())
        };
        if section == "ENTITIES" && MAPPED.contains(&kind.as_str()) && group(67) != Some("1") {
            let handle = group(5)
                .and_then(|h| u64::from_str_radix(h, 16).ok())
                .unwrap_or(0);
            out.push((kind, handle, groups));
        } else if section == "ENTITIES" && kind == "ATTRIB" {
            // Attach the attribute's tag and value to the preceding INSERT under
            // private codes (ATTRIB_TAG / ATTRIB_VALUE).
            if let Some((last_kind, _, insert_groups)) = out.last_mut() {
                if last_kind == "INSERT" {
                    insert_groups.push((ATTRIB_TAG, group(2).unwrap_or_default().to_owned()));
                    // R2018 multi-line attributes keep their text in the embedded MTEXT
                    // (the group 1 after the `101 Embedded Object` marker).
                    let embedded = groups.iter().position(|(c, _)| *c == 101);
                    let value = match embedded {
                        Some(at) => groups
                            .iter()
                            .skip(at)
                            .find(|(c, _)| *c == 1)
                            .map(|(_, v)| v.as_str()),
                        None => group(1),
                    };
                    insert_groups.push((ATTRIB_VALUE, value.unwrap_or_default().to_owned()));
                }
            }
        }
    };
    for pair in lines.chunks(2) {
        let [code, value] = pair else { break };
        let Ok(code) = code.trim().parse::<i32>() else {
            continue;
        };
        let value = value.trim().to_owned();
        if code == 0 {
            flush(current.take(), &section, &mut out);
            current = Some((value, Vec::new()));
        } else if let Some((kind, groups)) = current.as_mut() {
            if kind == "SECTION" && code == 2 {
                section = value.clone();
            }
            groups.push((code, value));
        }
        if current.as_ref().is_some_and(|(k, _)| k == "ENDSEC") {
            section.clear();
        }
    }
    flush(current, &section, &mut out);
    out
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// Compares every mapped model-space entity with the DXF record of the same handle.
/// Returns the number of DXF attributes not present on the mapped INSERTs.
fn check_geometry(version: &str, oracle_version: &str) -> usize {
    let mut missing = 0;
    check_geometry_into(version, oracle_version, &mut missing);
    missing
}

fn check_geometry_into(version: &str, oracle_version: &str, missing_attribs: &mut usize) {
    let (Some(dwg), Some(dxf)) = (
        corpus_file(&format!("sample_{version}.dwg")),
        corpus_file(&format!("sample_{oracle_version}_ascii.dxf")),
    ) else {
        return;
    };
    let doc = cadkit_dwg::read(&dwg, &ReadOptions::default())
        .unwrap_or_else(|e| panic!("{version}: {e}"));
    let model = doc
        .models
        .iter()
        .find(|m| m.kind == ModelKind::Model)
        .expect("model space");
    let by_handle: BTreeMap<u64, &cadkit_core::Entity> = model
        .entities
        .iter()
        .filter_map(|e| Some((e.id?, e)))
        .collect();
    let records = dxf_entities(&dxf);
    assert!(!records.is_empty());
    let mut compared = 0;
    for (kind, handle, groups) in &records {
        let f = |c: i32| {
            groups
                .iter()
                .find(|(g, _)| *g == c)
                .and_then(|(_, v)| v.parse::<f64>().ok())
        };
        let all = |c: i32| {
            groups
                .iter()
                .filter(|(g, _)| *g == c)
                .filter_map(|(_, v)| v.parse::<f64>().ok())
                .collect::<Vec<_>>()
        };
        let s = |c: i32| {
            groups
                .iter()
                .find(|(g, _)| *g == c)
                .map(|(_, v)| v.as_str())
        };
        let pt = |c: i32| {
            [
                f(c).unwrap_or(0.0),
                f(c + 10).unwrap_or(0.0),
                f(c + 20).unwrap_or(0.0),
            ]
        };
        let e = by_handle
            .get(handle)
            .unwrap_or_else(|| panic!("{version}: {kind} {handle:X} missing"));
        let ctx = format!("{version} {kind} {handle:X}");
        let same = |p: cadkit_core::Point3, q: [f64; 3]| {
            close(p.x, q[0]) && close(p.y, q[1]) && close(p.z, q[2])
        };
        match (&e.kind, kind.as_str()) {
            (EntityKind::Line { start, end }, "LINE") => {
                assert!(
                    same(*start, pt(10)) && same(*end, pt(11)),
                    "{ctx}: {start:?} {end:?}"
                );
            }
            (EntityKind::Point { position }, "POINT") => assert!(same(*position, pt(10)), "{ctx}"),
            (EntityKind::Circle { center, radius, .. }, "CIRCLE") => {
                assert!(
                    same(*center, pt(10)) && close(*radius, f(40).unwrap_or(0.0)),
                    "{ctx}"
                );
            }
            (
                EntityKind::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                    ..
                },
                "ARC",
            ) => {
                assert!(
                    same(*center, pt(10)) && close(*radius, f(40).unwrap_or(0.0)),
                    "{ctx}"
                );
                assert!(
                    close(start_angle.to_degrees(), f(50).unwrap_or(0.0)),
                    "{ctx}: start {start_angle}"
                );
                assert!(
                    close(end_angle.to_degrees(), f(51).unwrap_or(0.0)),
                    "{ctx}: end {end_angle}"
                );
            }
            (
                EntityKind::Polyline {
                    vertices, closed, ..
                },
                "LWPOLYLINE",
            ) => {
                let (xs, ys) = (all(10), all(20));
                assert_eq!(vertices.len(), xs.len(), "{ctx}: vertex count");
                for (v, (x, y)) in vertices.iter().zip(xs.iter().zip(&ys)) {
                    assert!(
                        close(v.position.x, *x) && close(v.position.y, *y),
                        "{ctx}: vertex {v:?}"
                    );
                }
                let flags = f(70).unwrap_or(0.0) as i64;
                assert_eq!(*closed, flags & 1 != 0, "{ctx}: closed");
                let bulges: Vec<f64> = vertices
                    .iter()
                    .map(|v| v.bulge)
                    .filter(|b| *b != 0.0)
                    .collect();
                let dxf_bulges: Vec<f64> = all(42).into_iter().filter(|b| *b != 0.0).collect();
                assert_eq!(bulges.len(), dxf_bulges.len(), "{ctx}: bulges");
                assert!(
                    bulges.iter().zip(&dxf_bulges).all(|(a, b)| close(*a, *b)),
                    "{ctx}: bulges"
                );
            }
            (
                EntityKind::Text {
                    height,
                    rotation,
                    value,
                    ..
                },
                "TEXT",
            ) => {
                let Some(Value::List(ins)) = e.props.get("dwg.insertion_point") else {
                    panic!("{ctx}: no insertion point")
                };
                let ins: Vec<f64> = ins
                    .iter()
                    .filter_map(|v| {
                        if let Value::Float(x) = v {
                            Some(*x)
                        } else {
                            None
                        }
                    })
                    .collect();
                let p = pt(10);
                assert!(
                    ins.len() == 3 && close(ins[0], p[0]) && close(ins[1], p[1]),
                    "{ctx}: insertion {ins:?}"
                );
                assert!(close(*height, f(40).unwrap_or(0.0)), "{ctx}: height");
                assert!(
                    close(rotation.to_degrees(), f(50).unwrap_or(0.0)),
                    "{ctx}: rotation"
                );
                if let Some(v) = s(1).filter(|v| v.is_ascii()) {
                    assert_eq!(value, v, "{ctx}: value");
                }
            }
            (
                EntityKind::Insert {
                    block,
                    position,
                    scale,
                    rotation,
                    ..
                },
                "INSERT",
            ) => {
                assert_eq!(Some(block.as_str()), s(2), "{ctx}: block");
                assert!(same(*position, pt(10)), "{ctx}: position");
                let sc = [
                    f(41).unwrap_or(1.0),
                    f(42).unwrap_or(1.0),
                    f(43).unwrap_or(1.0),
                ];
                assert!(
                    close(scale.x, sc[0]) && close(scale.y, sc[1]) && close(scale.z, sc[2]),
                    "{ctx}: scale {scale:?}"
                );
                assert!(
                    close(rotation.to_degrees(), f(50).unwrap_or(0.0)),
                    "{ctx}: rotation"
                );
                let tags: Vec<&str> = groups
                    .iter()
                    .filter(|(g, _)| *g == ATTRIB_TAG)
                    .map(|(_, v)| v.as_str())
                    .collect();
                let values: Vec<&str> = groups
                    .iter()
                    .filter(|(g, _)| *g == ATTRIB_VALUE)
                    .map(|(_, v)| v.as_str())
                    .collect();
                for (tag, value) in tags.iter().zip(&values) {
                    match e.attributes.iter().find(|a| a.tag == *tag) {
                        Some(a) => assert_eq!(
                            a.value,
                            Value::Text((*value).to_owned()),
                            "{ctx}: attribute {tag}"
                        ),
                        // R2018 multi-line attributes (embedded MTEXT) are not decoded yet;
                        // they must be reported, not silently lost.
                        None => {
                            *missing_attribs += 1;
                            assert!(
                                doc.warnings.iter().any(|w| w.code == "dwg.decode_error"
                                    && w.message.contains("multi-line attribute")),
                                "{ctx}: attribute {tag} missing without a warning"
                            );
                        }
                    }
                }
            }
            (other, _) => panic!("{ctx}: mapped as {other:?}"),
        }
        assert_eq!(e.color, dxf_color(groups), "{ctx}: color");
        compared += 1;
    }
    assert_eq!(compared, records.len());
}

/// Color from DXF groups 420 (true color) / 62 (ACI); absent means BYLAYER.
fn dxf_color(groups: &[(i32, String)]) -> cadkit_core::Color {
    use cadkit_core::Color;
    let get = |c: i32| {
        groups
            .iter()
            .find(|(g, _)| *g == c)
            .and_then(|(_, v)| v.parse::<i64>().ok())
    };
    if let Some(rgb) = get(420) {
        return Color::Rgb {
            r: (rgb >> 16) as u8,
            g: (rgb >> 8) as u8,
            b: rgb as u8,
        };
    }
    match get(62).map(i64::abs) {
        None | Some(256) => Color::ByLayer,
        Some(0) => Color::ByBlock,
        Some(i) => Color::Aci { index: i as u8 },
    }
}

/// LAYER table records of a DXF file.
fn dxf_layers(bytes: &[u8]) -> Vec<Vec<(i32, String)>> {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut current: Option<(String, Vec<(i32, String)>)> = None;
    let mut in_tables = false;
    for pair in lines.chunks(2) {
        let [code, value] = pair else { break };
        let Ok(code) = code.trim().parse::<i32>() else {
            continue;
        };
        let value = value.trim().to_owned();
        if code == 0 {
            if let Some((kind, groups)) = current.take() {
                if in_tables && kind == "LAYER" {
                    out.push(groups);
                }
            }
            if value == "ENDSEC" {
                in_tables = false;
            }
            current = Some((value, Vec::new()));
        } else if let Some((kind, groups)) = current.as_mut() {
            if kind == "SECTION" && code == 2 {
                in_tables = value == "TABLES";
            }
            groups.push((code, value));
        }
    }
    out
}

#[test]
fn layer_properties_match_dxf() {
    use cadkit_core::Lineweight;
    for version in ["AC1015", "AC1018", "AC1021", "AC1024", "AC1027", "AC1032"] {
        let (Some(dwg), Some(dxf)) = (
            corpus_file(&format!("sample_{version}.dwg")),
            corpus_file(&format!("sample_{version}_ascii.dxf")),
        ) else {
            continue;
        };
        let doc = cadkit_dwg::read(&dwg, &ReadOptions::default()).unwrap();
        for groups in dxf_layers(&dxf) {
            let get = |c: i32| {
                groups
                    .iter()
                    .find(|(g, _)| *g == c)
                    .map(|(_, v)| v.as_str())
            };
            let int = |c: i32| get(c).and_then(|v| v.parse::<i64>().ok());
            let name = get(2).unwrap();
            let layer = doc
                .layers
                .iter()
                .find(|l| l.name == name)
                .unwrap_or_else(|| panic!("{version}: layer {name}"));
            let ctx = format!("{version} layer {name}");
            let flags = int(70).unwrap_or(0);
            assert_eq!(layer.frozen, flags & 1 != 0, "{ctx}: frozen");
            assert_eq!(layer.locked, flags & 4 != 0, "{ctx}: locked");
            assert_eq!(layer.visible, int(62).unwrap_or(7) >= 0, "{ctx}: on");
            assert_eq!(layer.plottable, int(290) != Some(0), "{ctx}: plot");
            assert_eq!(layer.linetype.as_deref(), get(6), "{ctx}: linetype");
            let lw = match int(370).unwrap_or(-3) {
                -1 => Lineweight::ByLayer,
                -2 => Lineweight::ByBlock,
                -3 => Lineweight::Default,
                v => Lineweight::Millimeters(v as f64 / 100.0),
            };
            assert_eq!(layer.lineweight, lw, "{ctx}: lineweight");
            let color = dxf_color(&groups);
            assert_eq!(layer.color, color, "{ctx}: color");
        }
        let described = doc
            .layers
            .iter()
            .find(|l| l.name == "Layer_desc")
            .and_then(|l| l.description.clone());
        assert_eq!(
            described.as_deref(),
            Some("This layer has it's own description"),
            "{version}: layer description"
        );
    }
}

#[test]
fn geometry_matches_dxf_by_handle() {
    for version in ["AC1015", "AC1018", "AC1021", "AC1024", "AC1027", "AC1032"] {
        assert_eq!(
            check_geometry(version, version),
            0,
            "{version}: missing attributes"
        );
    }
}

/// The R14 sample is the R2000 drawing without extended names: its layer state
/// flags (off/frozen/locked) must equal the R2000 ones. This pins the R13/R14 "On"
/// bit, which the spec describes as "1 if on" but the file stores as "1 if off".
#[test]
fn r14_layer_flags_match_r2000() {
    let (Some(r14), Some(r2000)) = (
        corpus_file("sample_AC1014.dwg"),
        corpus_file("sample_AC1015.dwg"),
    ) else {
        return;
    };
    let a = cadkit_dwg::read(&r14, &ReadOptions::default()).unwrap();
    let b = cadkit_dwg::read(&r2000, &ReadOptions::default()).unwrap();
    assert_eq!(a.layers.len(), b.layers.len());
    for (x, y) in a.layers.iter().zip(&b.layers) {
        assert_eq!(x.name, r14_name(&y.name));
        assert_eq!(
            (x.visible, x.frozen, x.locked),
            (y.visible, y.frozen, y.locked),
            "layer {}",
            y.name
        );
    }
    assert!(
        a.layers.iter().any(|l| !l.visible)
            && a.layers.iter().any(|l| l.frozen)
            && a.layers.iter().any(|l| l.locked)
    );
}

/// Types whose decoders read every field. Since R2000 the main-data size is exact, so
/// such a decoder must consume the main stream completely (one bit is left in R2007+
/// objects without strings: the string-present flag) and the handle stream up to its
/// byte padding. Add new complete decoders here.
const COMPLETE_DECODERS: [&str; 67] = [
    "LINE",
    "POINT",
    "CIRCLE",
    "ARC",
    "LWPOLYLINE",
    "TEXT",
    "ATTRIB",
    "ATTDEF",
    "INSERT",
    "MINSERT",
    "BLOCK",
    "ENDBLK",
    "SEQEND",
    "BLOCK_CONTROL",
    "LAYER_CONTROL",
    "STYLE_CONTROL",
    "LTYPE_CONTROL",
    "VIEW_CONTROL",
    "UCS_CONTROL",
    "VPORT_CONTROL",
    "APPID_CONTROL",
    "DIMSTYLE_CONTROL",
    "VP_ENT_HDR_CONTROL",
    "BLOCK_HEADER",
    "LAYER",
    "STYLE",
    "LTYPE",
    "APPID",
    "DICTIONARY",
    "ACDBDICTIONARYWDFLT",
    "DICTIONARYWDFLT",
    "LAYOUT",
    "DBCOLOR",
    "ELLIPSE",
    "SPLINE",
    "VERTEX_2D",
    "VERTEX_3D",
    "VERTEX_MESH",
    "VERTEX_PFACE",
    "VERTEX_PFACE_FACE",
    "POLYLINE_2D",
    "POLYLINE_3D",
    "POLYLINE_PFACE",
    "POLYLINE_MESH",
    "SOLID",
    "TRACE",
    "3DFACE",
    "MTEXT",
    "DIMENSION_ORDINATE",
    "DIMENSION_LINEAR",
    "DIMENSION_ALIGNED",
    "DIMENSION_ANG_3PT",
    "DIMENSION_ANG_2LN",
    "DIMENSION_RADIUS",
    "DIMENSION_DIAMETER",
    "LEADER",
    "TOLERANCE",
    "HATCH",
    "IMAGE",
    "WIPEOUT",
    "IMAGEDEF",
    "VIEWPORT",
    "RAY",
    "XLINE",
    "SHAPE",
    "MLINE",
    "OLE2FRAME",
];

#[test]
fn complete_decoders_consume_their_streams_exactly() {
    for version in [
        "AC1014", "AC1015", "AC1018", "AC1021", "AC1024", "AC1027", "AC1032",
    ] {
        let Some(dwg) = corpus_file(&format!("sample_{version}.dwg")) else {
            continue;
        };
        let file = read_native(&dwg, &ReadOptions::default()).unwrap();
        let mut checked = 0;
        for o in file.objects.values() {
            if !COMPLETE_DECODERS.contains(&o.type_name.as_str()) {
                continue;
            }
            // R14 stores classes introduced later as proxies with extra trailing data.
            if version == "AC1014" && o.type_code >= 500 {
                continue;
            }
            let ctx = format!("{version} {} {:X}", o.type_name, o.handle);
            let Some(data_left) = o.data_bits_left else {
                panic!("{ctx}: decode failed: {:?}", o.error);
            };
            let allowed = if file.version.r2007_plus() { 1 } else { 0 };
            assert!(
                data_left <= allowed,
                "{ctx}: {data_left} main-stream bits left"
            );
            assert!(
                o.handle_bits_left.unwrap_or(0) < 8,
                "{ctx}: {:?} handle-stream bits left",
                o.handle_bits_left
            );
            checked += 1;
        }
        assert!(checked > 400, "{version}: only {checked} objects checked");
    }
}

/// STYLE records: names, fonts and the shape-file flag (DXF 70 bit 1) — pins the
/// shape-file/vertical bit order.
#[test]
fn text_styles_match_dxf() {
    for version in ["AC1015", "AC1018", "AC1021", "AC1024", "AC1027", "AC1032"] {
        let (Some(dwg), Some(dxf)) = (
            corpus_file(&format!("sample_{version}.dwg")),
            corpus_file(&format!("sample_{version}_ascii.dxf")),
        ) else {
            continue;
        };
        let doc = cadkit_dwg::read(&dwg, &ReadOptions::default()).unwrap();
        let text = String::from_utf8_lossy(&dxf);
        let lines: Vec<&str> = text.lines().collect();
        let mut styles: Vec<BTreeMap<i32, String>> = Vec::new();
        let mut current: Option<(String, BTreeMap<i32, String>)> = None;
        for pair in lines.chunks(2) {
            let [code, value] = pair else { break };
            let Ok(code) = code.trim().parse::<i32>() else {
                continue;
            };
            if code == 0 {
                if let Some((kind, groups)) = current.take() {
                    if kind == "STYLE" {
                        styles.push(groups);
                    }
                }
                current = Some((value.trim().to_owned(), BTreeMap::new()));
            } else if let Some((_, groups)) = current.as_mut() {
                groups
                    .entry(code)
                    .or_insert_with(|| value.trim().to_owned());
            }
        }
        assert_eq!(
            doc.text_styles.len(),
            styles.len(),
            "{version}: style count"
        );
        for (style, groups) in doc.text_styles.iter().zip(&styles) {
            let ctx = format!("{version} style {:?}", style.name);
            assert_eq!(
                style.name,
                groups.get(&2).cloned().unwrap_or_default(),
                "{ctx}: name"
            );
            assert_eq!(
                style.font.clone().unwrap_or_default(),
                groups.get(&3).cloned().unwrap_or_default(),
                "{ctx}: font"
            );
            let shape = groups
                .get(&70)
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0)
                & 1
                != 0;
            assert_eq!(
                style.props.get("dwg.shape_file") == Some(&Value::Bool(true)),
                shape,
                "{ctx}: shape flag"
            );
        }
    }
}

/// Optional extra public samples: `corpus/public/tika/*.dwg` (Apache Tika test
/// documents, Apache-2.0; see docs/dwg/FORMAT_NOTES.md for the URLs), or the directory
/// named by `CADKIT_DWG_EXTRA_SAMPLES`. Skipped when the directory is missing. Structural checks only: every object decodes, every complete
/// decoder consumes its streams exactly, and the document has a model space.
#[test]
fn extra_public_samples_decode_cleanly() {
    let dir: PathBuf = match std::env::var_os("CADKIT_DWG_EXTRA_SAMPLES") {
        Some(d) => d.into(),
        None => [
            env!("CARGO_MANIFEST_DIR"),
            "..",
            "..",
            "corpus",
            "public",
            "tika",
        ]
        .iter()
        .collect(),
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut checked = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dwg"))
        {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bytes = std::fs::read(&path).unwrap();
        let file =
            read_native(&bytes, &ReadOptions::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            file.header.complete && file.header.crc_ok == Some(true),
            "{name}: header"
        );
        assert_eq!(
            file.objects.len(),
            file.handle_map.len(),
            "{name}: objects that failed to parse"
        );
        assert!(file.warnings.is_empty(), "{name}: {:?}", file.warnings);
        for o in file
            .objects
            .values()
            .filter(|o| COMPLETE_DECODERS.contains(&o.type_name.as_str()))
        {
            let left = o
                .data_bits_left
                .unwrap_or_else(|| panic!("{name} {} {:X}: {:?}", o.type_name, o.handle, o.error));
            assert!(
                left <= u64::from(file.version.r2007_plus()),
                "{name} {} {:X}: {left} bits left",
                o.type_name,
                o.handle
            );
            assert!(
                o.handle_bits_left.unwrap_or(0) < 8,
                "{name} {} {:X}: handle bits left",
                o.type_name,
                o.handle
            );
        }
        let doc = cadkit_dwg::read(&bytes, &ReadOptions::default()).unwrap();
        assert!(
            doc.models.iter().any(|m| m.kind == ModelKind::Model),
            "{name}: no model space"
        );
        checked += 1;
    }
    println!("extra public samples checked: {checked}");
}
