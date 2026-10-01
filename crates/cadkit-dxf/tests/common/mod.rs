//! Shared helpers for the integration tests.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::print_stdout
)]
#![allow(dead_code, missing_docs)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use cadkit_core::*;
use cadkit_dxf::DxfVersion;

pub const VERSIONS: [(&str, &str); 7] = [
    ("AC1009", "R12"),
    ("AC1015", "R2000"),
    ("AC1018", "R2004"),
    ("AC1021", "R2007"),
    ("AC1024", "R2010"),
    ("AC1027", "R2013"),
    ("AC1032", "R2018"),
];

pub fn corpus_file(name: &str) -> Option<Vec<u8>> {
    let p: PathBuf = [
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
    std::fs::read(p).ok()
}

pub fn read(bytes: &[u8]) -> Document {
    cadkit_dxf::read(bytes, &ReadOptions::default()).expect("read")
}

pub fn type_name(e: &Entity) -> String {
    match &e.kind {
        EntityKind::Unknown { type_name } => type_name.clone(),
        k => {
            let dbg = format!("{k:?}");
            dbg.split(|c: char| !c.is_alphanumeric())
                .next()
                .unwrap_or("")
                .to_owned()
        }
    }
}

/// Entity histogram by type name.
pub fn histogram(entities: &[Entity]) -> BTreeMap<String, usize> {
    let mut h = BTreeMap::new();
    for e in entities {
        *h.entry(type_name(e)).or_default() += 1;
    }
    h
}

/// Approximate structural equality of two JSON values: numbers within `tol`
/// (absolute or relative), everything else exact. Returns the first difference.
pub fn json_diff(
    a: &serde_json::Value,
    b: &serde_json::Value,
    tol: f64,
    path: &str,
) -> Option<String> {
    use serde_json::Value as J;
    match (a, b) {
        (J::Number(x), J::Number(y)) => {
            let (x, y) = (
                x.as_f64().unwrap_or(f64::NAN),
                y.as_f64().unwrap_or(f64::NAN),
            );
            let scale = x.abs().max(y.abs()).max(1.0);
            if (x - y).abs() <= tol * scale {
                None
            } else {
                Some(format!("{path}: {x} != {y}"))
            }
        }
        (J::Array(x), J::Array(y)) => {
            if x.len() != y.len() {
                return Some(format!("{path}: array length {} != {}", x.len(), y.len()));
            }
            x.iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (p, q))| json_diff(p, q, tol, &format!("{path}[{i}]")))
        }
        (J::Object(x), J::Object(y)) => {
            for (k, v) in x {
                match y.get(k) {
                    Some(w) => {
                        if let Some(d) = json_diff(v, w, tol, &format!("{path}.{k}")) {
                            return Some(d);
                        }
                    }
                    None => return Some(format!("{path}.{k}: missing on the right")),
                }
            }
            y.keys()
                .find(|k| !x.contains_key(*k))
                .map(|k| format!("{path}.{k}: missing on the left"))
        }
        _ => {
            if a == b {
                None
            } else {
                Some(format!("{path}: {a} != {b}"))
            }
        }
    }
}

/// Largest numeric deviation (relative to max(1, |x|, |y|)) between two structurally equal
/// JSON values, or `None` when the structure or a non-numeric leaf differs.
pub fn json_max_diff(a: &serde_json::Value, b: &serde_json::Value) -> Option<f64> {
    use serde_json::Value as J;
    match (a, b) {
        (J::Number(x), J::Number(y)) => {
            let (x, y) = (x.as_f64()?, y.as_f64()?);
            Some((x - y).abs() / x.abs().max(y.abs()).max(1.0))
        }
        (J::Array(x), J::Array(y)) if x.len() == y.len() => {
            let mut m: f64 = 0.0;
            for (p, q) in x.iter().zip(y) {
                m = m.max(json_max_diff(p, q)?);
            }
            Some(m)
        }
        (J::Object(x), J::Object(y)) if x.len() == y.len() => {
            let mut m: f64 = 0.0;
            for (k, v) in x {
                m = m.max(json_max_diff(v, y.get(k)?)?);
            }
            Some(m)
        }
        _ => (a == b).then_some(0.0),
    }
}

/// Removes `keys` everywhere in `v`.
pub fn strip_keys(v: &mut serde_json::Value, keys: &[&str]) {
    match v {
        serde_json::Value::Object(o) => {
            for k in keys {
                o.remove(*k);
            }
            for (_, x) in o.iter_mut() {
                strip_keys(x, keys);
            }
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(|x| strip_keys(x, keys)),
        _ => {}
    }
}

/// Document as JSON without byte offsets and handles. The ASCII and binary samples are
/// separate saves of one drawing, so their handle numbering differs.
pub fn comparable_json(doc: &Document) -> serde_json::Value {
    let mut v = serde_json::to_value(doc).expect("serialize");
    strip_keys(
        &mut v,
        &[
            "offset",
            "id",
            "object",
            "dxf.layout.block_record",
            "dxf.imagedef",
            "dxf.codes",
            // Per-save values of the header.
            "dxf.$HANDSEED",
            "dxf.$VERSIONGUID",
            "dxf.$FINGERPRINTGUID",
            "dxf.$TDCREATE",
            "dxf.$TDUCREATE",
            "dxf.$TDUPDATE",
            "dxf.$TDUUPDATE",
            "dxf.$TDINDWG",
            "dxf.$TDUSRTIMER",
        ],
    );
    v
}

pub const TOL: f64 = 1e-9;

pub fn p3(p: &Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

pub fn v3(v: &Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// Angle difference modulo a full turn.
pub fn angle_close(a: f64, b: f64) -> bool {
    let d = (a - b).rem_euclid(std::f64::consts::TAU);
    d < TOL || (std::f64::consts::TAU - d) < TOL
}

pub fn close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| (x - y).abs() <= TOL * x.abs().max(y.abs()).max(1.0))
}

/// Numbers, angles and strings describing an entity.
pub fn numbers(e: &Entity, keep_measure: bool) -> (Vec<f64>, Vec<f64>, Vec<String>) {
    let mut n: Vec<f64> = Vec::new();
    let mut ang: Vec<f64> = Vec::new();
    let mut s: Vec<String> = Vec::new();
    let pts = |n: &mut Vec<f64>, ps: &[Point3]| ps.iter().for_each(|p| n.extend(p3(p)));
    match &e.kind {
        EntityKind::Point { position } => n.extend(p3(position)),
        EntityKind::Line { start, end } => {
            n.extend(p3(start));
            n.extend(p3(end));
        }
        EntityKind::Circle {
            center,
            radius,
            normal,
        } => {
            n.extend(p3(center));
            n.push(*radius);
            n.extend(v3(normal));
        }
        EntityKind::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            normal,
        } => {
            n.extend(p3(center));
            n.push(*radius);
            n.extend(v3(normal));
            ang.extend([*start_angle, *end_angle]);
        }
        EntityKind::Ellipse {
            center,
            major_axis,
            ratio,
            start_param,
            end_param,
            normal,
        } => {
            n.extend(p3(center));
            n.extend(v3(major_axis));
            n.push(*ratio);
            n.extend(v3(normal));
            ang.extend([*start_param, *end_param]);
        }
        EntityKind::Polyline {
            vertices,
            closed,
            normal,
        } => {
            n.push(f64::from(*closed));
            n.extend(v3(normal));
            for v in vertices {
                n.extend(p3(&v.position));
                n.extend([v.bulge, v.start_width, v.end_width]);
            }
        }
        EntityKind::Spline {
            degree,
            knots,
            control_points,
            weights,
            fit_points,
            closed,
        } => {
            n.push(f64::from(*degree));
            n.push(f64::from(*closed));
            n.extend(knots);
            n.extend(weights);
            pts(&mut n, control_points);
            pts(&mut n, fit_points);
        }
        EntityKind::Text {
            position,
            end_point,
            height,
            rotation,
            width_factor,
            oblique,
            value,
            style,
            halign,
            valign,
            normal,
        } => {
            n.extend(p3(position));
            if let Some(e) = end_point {
                n.extend(p3(e));
            }
            n.extend([*height, *width_factor]);
            n.extend(v3(normal));
            ang.extend([*rotation, *oblique]);
            s.push(format!("{value}|{style:?}|{halign:?}|{valign:?}"));
        }
        EntityKind::MText {
            position,
            height,
            width,
            rotation,
            value,
            plain,
            style,
            attachment,
            line_spacing,
            normal,
        } => {
            n.extend(p3(position));
            n.extend([*height, *width, *line_spacing]);
            n.extend(v3(normal));
            ang.push(*rotation);
            // The writer spells raw line feeds as `\P`.
            s.push(format!(
                "{}|{plain}|{style:?}|{attachment:?}",
                value.replace('\n', "\\P")
            ));
        }
        EntityKind::Insert {
            block,
            position,
            scale,
            rotation,
            normal,
            columns,
            rows,
            column_spacing,
            row_spacing,
        } => {
            n.extend(p3(position));
            n.extend(v3(scale));
            n.extend(v3(normal));
            n.extend([
                f64::from(*columns),
                f64::from(*rows),
                *column_spacing,
                *row_spacing,
            ]);
            ang.push(*rotation);
            s.push(block.clone());
            for a in &e.attributes {
                s.push(format!("{}={:?}", a.tag, a.value));
            }
        }
        EntityKind::Hatch {
            loops,
            solid,
            pattern,
            pattern_scale,
            pattern_angle,
            normal,
        } => {
            n.extend([f64::from(*solid), *pattern_scale]);
            n.extend(v3(normal));
            ang.push(*pattern_angle);
            s.push(format!("{pattern:?}"));
            for l in loops {
                n.push(f64::from(l.external));
                n.push(l.edges.len() as f64);
                for edge in &l.edges {
                    match edge {
                        HatchEdge::Line { start, end } => {
                            n.extend(p3(start));
                            n.extend(p3(end));
                        }
                        HatchEdge::Arc {
                            center,
                            radius,
                            start_angle,
                            end_angle,
                            ccw,
                        } => {
                            n.extend(p3(center));
                            n.extend([*radius, f64::from(*ccw)]);
                            ang.extend([*start_angle, *end_angle]);
                        }
                        HatchEdge::Ellipse {
                            center,
                            major_axis,
                            ratio,
                            start_param,
                            end_param,
                            ccw,
                        } => {
                            n.extend(p3(center));
                            n.extend(v3(major_axis));
                            n.extend([*ratio, f64::from(*ccw)]);
                            ang.extend([*start_param, *end_param]);
                        }
                        HatchEdge::Spline {
                            degree,
                            knots,
                            control_points,
                            weights,
                        } => {
                            n.push(f64::from(*degree));
                            n.extend(knots);
                            n.extend(weights);
                            pts(&mut n, control_points);
                        }
                        HatchEdge::Polyline { vertices, closed } => {
                            n.push(f64::from(*closed));
                            for v in vertices {
                                n.extend(p3(&v.position));
                                n.push(v.bulge);
                            }
                        }
                    }
                }
            }
        }
        EntityKind::Dimension {
            dimension_type,
            points,
            text_position,
            measurement,
            text,
            block,
            style,
        } => {
            pts(&mut n, points);
            if let Some(t) = text_position {
                n.extend(p3(t));
            }
            if keep_measure {
                n.extend(measurement.iter());
            }
            // A missing style means the default one; the writer always names it.
            s.push(format!(
                "{dimension_type:?}|{text:?}|{block:?}|{}",
                style.as_deref().unwrap_or("Standard")
            ));
        }
        EntityKind::Face { points, filled } => {
            n.push(f64::from(*filled));
            pts(&mut n, points);
        }
        EntityKind::Leader {
            vertices,
            arrowhead,
        } => {
            n.push(f64::from(*arrowhead));
            pts(&mut n, vertices);
        }
        EntityKind::Image {
            path,
            position,
            u_vector,
            v_vector,
            size_px,
        } => {
            n.extend(p3(position));
            n.extend(v3(u_vector));
            n.extend(v3(v_vector));
            s.push(format!("{path:?}|{size_px:?}"));
        }
        EntityKind::Viewport {
            center,
            width,
            height,
            view_center,
            view_height,
        } => {
            n.extend(p3(center));
            n.extend([*width, *height]);
            n.extend([view_center.x, view_center.y, *view_height]);
        }
        EntityKind::Mesh { vertices, faces } => {
            pts(&mut n, vertices);
            for f in faces {
                n.push(f.len() as f64);
                n.extend(f.iter().map(|i| f64::from(*i)));
            }
        }
        EntityKind::Group { .. } | EntityKind::Unknown { .. } => {}
    }
    (n, ang, s)
}

pub fn is_unknown(e: &Entity) -> bool {
    matches!(e.kind, EntityKind::Unknown { .. })
}

/// Compares two entity lists (Unknown records are dropped by the writer and ignored here).
pub fn compare_lists(what: &str, a: &[Entity], b: &[Entity], v: DxfVersion) {
    // R2000 keeps only the nearest ACI of a true color; the measurement (group 42) is R2007+.
    let full_color = v != DxfVersion::R2000;
    let keep_measure = !matches!(v, DxfVersion::R2000 | DxfVersion::R2004);
    let a: Vec<Entity> = a.iter().filter(|e| !is_unknown(e)).cloned().collect();
    let b: Vec<Entity> = b.iter().filter(|e| !is_unknown(e)).cloned().collect();
    assert_eq!(histogram(&a), histogram(&b), "{what}: entity histogram");
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        let ctx = format!("{what}[{i}] {}", type_name(x));
        // A missing layer means layer 0.
        assert_eq!(
            x.layer.as_deref().unwrap_or("0"),
            y.layer.as_deref().unwrap_or("0"),
            "{ctx}: layer"
        );
        assert_eq!(x.visible, y.visible, "{ctx}: visible");
        assert_eq!(x.linetype, y.linetype, "{ctx}: linetype");
        assert_eq!(x.lineweight, y.lineweight, "{ctx}: lineweight");
        if full_color || !matches!(x.color, Color::Rgb { .. }) {
            assert_eq!(x.color, y.color, "{ctx}: color");
        }
        let (nx, ax, sx) = numbers(x, keep_measure);
        let (ny, ay, sy) = numbers(y, keep_measure);
        assert!(close(&nx, &ny), "{ctx}: numbers differ\n{nx:?}\n{ny:?}");
        assert_eq!(ax.len(), ay.len(), "{ctx}: angle count");
        for (p, q) in ax.iter().zip(&ay) {
            assert!(angle_close(*p, *q), "{ctx}: angle {p} vs {q}");
        }
        assert_eq!(sx, sy, "{ctx}: strings");
    }
}
