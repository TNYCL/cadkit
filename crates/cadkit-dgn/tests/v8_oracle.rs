//! `corpus/public/gdal/test_dgnv8.dgn` against GDAL's ODA-driver output
//! (`test_dgnv8_ref.csv`), used as a black-box oracle. Skips when the corpus is absent.
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

mod common;

use cadkit_core::geom_ops::nurbs_eval;
use cadkit_core::{
    Entity, EntityKind, GroupKind, HAlign, LengthUnit, Point3, ReadOptions, VAlign, Value, Vec3,
};
use common::{corpus, csv, dist, prop_int, prop_point, walk, wkt_sequences};

const TOL: f64 = 1e-6;

/// Distance from `p` to the curve or outline of `e` (`None` for kinds without one).
fn curve_distance(e: &Entity, p: Point3) -> Option<f64> {
    let seg = |a: Point3, b: Point3| {
        let ab = a.vector_to(b);
        let len2 = ab.dot(ab);
        let t = if len2 > 0.0 {
            (a.vector_to(p).dot(ab) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        dist(p, a.translated(ab.scaled(t)))
    };
    let in_sweep = |angle: f64, s: f64, e: f64| {
        let tau = std::f64::consts::TAU;
        let span = (e - s).rem_euclid(tau);
        let span = if span < 1e-12 { tau } else { span };
        (angle - s).rem_euclid(tau) <= span + 1e-9 || (angle - s).rem_euclid(tau) >= tau - 1e-9
    };
    match &e.kind {
        EntityKind::Line { start, end } => Some(seg(*start, *end)),
        EntityKind::Polyline {
            vertices, closed, ..
        } => {
            let mut v: Vec<Point3> = vertices.iter().map(|v| v.position).collect();
            if *closed {
                v.push(v[0]);
            }
            v.windows(2).map(|w| seg(w[0], w[1])).reduce(f64::min)
        }
        EntityKind::Circle {
            center,
            radius,
            normal,
        } => {
            let d = center.vector_to(p);
            Some((d.length() - radius).abs() + d.dot(*normal).abs())
        }
        EntityKind::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            normal,
        } => {
            let (ax, ay, _) = cadkit_core::geom_ops::arbitrary_axes(*normal);
            let d = center.vector_to(p);
            let a = d.dot(ay).atan2(d.dot(ax));
            in_sweep(a, *start_angle, *end_angle)
                .then(|| (d.length() - radius).abs() + d.dot(*normal).abs())
        }
        EntityKind::Ellipse {
            center,
            major_axis,
            ratio,
            start_param,
            end_param,
            normal,
        } => {
            let r = major_axis.length();
            let mu = major_axis.scaled(1.0 / r);
            let nu = normal.cross(mu);
            let d = center.vector_to(p);
            let (u, v) = (d.dot(mu) / r, d.dot(nu) / (r * ratio));
            let t = v.atan2(u);
            in_sweep(t, *start_param, *end_param)
                .then(|| ((u * u + v * v).sqrt() - 1.0).abs() * r + d.dot(*normal).abs())
        }
        EntityKind::Group { children, .. } => children
            .iter()
            .filter_map(|c| curve_distance(c, p))
            .reduce(f64::min),
        _ => None,
    }
}

/// The point the oracle reports: positions, and for text the stored (lower-left) origin.
fn position(e: &Entity) -> Option<Point3> {
    match &e.kind {
        EntityKind::Point { position } | EntityKind::Insert { position, .. } => Some(*position),
        EntityKind::Text { .. } => Some(prop_point(e, "dgn.origin")),
        _ => None,
    }
}

#[test]
fn test_dgnv8_matches_gdal_oracle() {
    let (Some(bytes), Some(reference)) = (
        corpus("public/gdal/test_dgnv8.dgn"),
        corpus("public/gdal/test_dgnv8_ref.csv"),
    ) else {
        return;
    };
    assert!(cadkit_dgn::sniff(&bytes));
    let doc = cadkit_dgn::read(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(doc.source.version, "V8");
    assert_eq!(doc.units.unit, LengthUnit::Meter);
    assert_eq!(doc.models.len(), 1);
    let model = &doc.models[0];
    assert_eq!(model.name, "my_model");
    assert!(model.is_3d);

    let rows = csv(&String::from_utf8_lossy(&reference));
    let header = &rows[0];
    let col = |name: &str| header.iter().position(|h| h == name).unwrap();
    let (c_wkt, c_type, c_level, c_gg, c_color, c_weight, c_style, c_text) = (
        col("WKT"),
        col("Type"),
        col("Level"),
        col("GraphicGroup"),
        col("ColorIndex"),
        col("Weight"),
        col("Style"),
        col("Text"),
    );
    let rows: Vec<&Vec<String>> = rows[1..].iter().filter(|r| r.len() > c_text).collect();
    assert_eq!(rows.len(), 34);

    // The oracle lists every drawn element; the one unattached tag is extra here.
    let entities: Vec<&Entity> = model
        .entities
        .iter()
        .filter(
            |e| !matches!(&e.kind, EntityKind::Unknown { type_name } if type_name == "dgn.type_37"),
        )
        .collect();
    assert_eq!(entities.len(), rows.len());

    let mut checked_geometry = 0;
    for (row, e) in rows.iter().zip(entities.iter()) {
        // The oracle reports a text node through its text component.
        let e = match &e.kind {
            EntityKind::Group {
                group_kind: GroupKind::TextNode,
                children,
                ..
            } => {
                assert_eq!(children.len(), 1);
                &children[0]
            }
            _ => *e,
        };
        let int = |c: usize| row[c].parse::<i64>().unwrap();
        let ty = int(c_type);
        assert_eq!(prop_int(e, "dgn.type"), Some(ty), "type of {row:?}");
        assert_eq!(
            prop_int(e, "dgn.level_id"),
            Some(int(c_level)),
            "level of {row:?}"
        );
        assert_eq!(
            prop_int(e, "dgn.color_index"),
            Some(int(c_color)),
            "color of {row:?}"
        );
        assert_eq!(
            prop_int(e, "dgn.weight"),
            Some(int(c_weight)),
            "weight of {row:?}"
        );
        assert_eq!(
            prop_int(e, "dgn.style"),
            Some(int(c_style)),
            "style of {row:?}"
        );
        assert_eq!(
            prop_int(e, "dgn.graphic_group").unwrap_or(0),
            int(c_gg),
            "graphic group of {row:?}"
        );

        let wkt = &row[c_wkt];
        let seqs = wkt_sequences(wkt);
        let pts: Vec<Point3> = seqs.iter().flatten().copied().collect();
        match ty {
            // Points, text origins and shared cell origins.
            _ if wkt.starts_with("POINT") => {
                let p = position(e).unwrap_or_else(|| panic!("no position for {row:?}"));
                assert!(dist(p, pts[0]) < TOL, "{p:?} vs {wkt}");
            }
            3 | 4 | 6 | 12 | 14 | 15 | 16 | 2 => {
                for p in &pts {
                    let d = curve_distance(e, *p).unwrap_or(f64::MAX);
                    assert!(d < TOL, "{wkt} point {p:?} is {d} off {:?}", e.kind);
                }
            }
            22 => {
                let EntityKind::Group { children, .. } = &e.kind else {
                    panic!()
                };
                let got: Vec<Point3> = children.iter().filter_map(position).collect();
                assert_eq!(got.len(), pts.len());
                for (a, b) in got.iter().zip(pts.iter()) {
                    assert!(dist(*a, *b) < TOL);
                }
            }
            11 => {
                let EntityKind::Spline { fit_points, .. } = &e.kind else {
                    panic!()
                };
                assert!(dist(fit_points[0], pts[0]) < TOL);
                assert!(dist(*fit_points.last().unwrap(), *pts.last().unwrap()) < TOL);
            }
            27 => {
                let EntityKind::Spline {
                    degree,
                    knots,
                    control_points,
                    weights,
                    ..
                } = &e.kind
                else {
                    panic!()
                };
                let d = *degree as usize;
                let (t0, t1) = (knots[d], knots[control_points.len()]);
                let start = nurbs_eval(d, knots, control_points, weights, t0).unwrap();
                let end = nurbs_eval(d, knots, control_points, weights, t1).unwrap();
                assert!(dist(start, pts[0]) < TOL, "{start:?}");
                assert!(dist(end, *pts.last().unwrap()) < TOL, "{end:?}");
                // A point in the middle of the oracle polyline lies on the curve.
                let mid = pts[pts.len() / 2];
                let near = (0..=20_000)
                    .map(|i| t0 + (t1 - t0) * f64::from(i) / 20_000.0)
                    .map(|t| {
                        dist(
                            nurbs_eval(d, knots, control_points, weights, t).unwrap(),
                            mid,
                        )
                    })
                    .fold(f64::MAX, f64::min);
                assert!(near < 1e-3, "{near}");
            }
            36 => {} // multi-line: not modelled (Unknown), see FORMAT_NOTES.
            _ => panic!("unexpected oracle type {ty}"),
        }
        if !wkt.starts_with("POINT") && ty != 36 {
            checked_geometry += 1;
        }
        if ty == 2 {
            // Cell with a hole: the second shape carries the hole flag.
            let EntityKind::Group {
                group_kind,
                children,
                ..
            } = &e.kind
            else {
                panic!()
            };
            assert_eq!(*group_kind, GroupKind::Cell);
            assert_eq!(children.len(), 2);
            assert_eq!(children[1].props.get("dgn.hole"), Some(&Value::Bool(true)));
            assert_eq!(seqs.len(), 2);
        }
        if ty == 17 {
            let EntityKind::Text {
                value,
                height,
                rotation,
                position,
                halign,
                valign,
                ..
            } = &e.kind
            else {
                panic!()
            };
            // All oracle texts are justification 0 (left-top, OGR anchor p:7): the anchor is
            // one text height above the lower-left origin, along the text's Y axis.
            assert_eq!(prop_int(e, "dgn.justification"), Some(0));
            assert_eq!((*halign, *valign), (HAlign::Left, VAlign::Top));
            let o = prop_point(e, "dgn.origin");
            let up = Point3::new(
                o.x - rotation.sin() * height,
                o.y + rotation.cos() * height,
                o.z,
            );
            assert!(dist(*position, up) < 1e-9, "{position:?} vs {up:?}");
            assert_eq!(value, &row[c_text]);
            assert!(
                (height - 1.0).abs() < 1e-9,
                "label size s:1.0 in the oracle"
            );
            if value == "myTéxt" {
                assert!(
                    (rotation.to_degrees() + 45.0).abs() < 1e-9,
                    "label angle a:-45 in the oracle"
                );
            }
        }
        if ty == 35 {
            let EntityKind::Insert { block, .. } = &e.kind else {
                panic!()
            };
            assert_eq!(block, &row[c_text]);
            assert!(doc.blocks.iter().any(|b| &b.name == block));
        }
    }
    assert!(checked_geometry >= 20);
}

#[test]
fn test_dgnv8_structure() {
    let Some(bytes) = corpus("public/gdal/test_dgnv8.dgn") else {
        return;
    };
    let native = cadkit_dgn::native::v8::read(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(native.cfb_version, 3);
    assert_eq!(native.streams.len(), 15);
    assert_eq!(native.models.len(), 1);
    let m = &native.models[0];
    assert_eq!(m.storage, "#000000");
    let header = m.header.as_ref().unwrap();
    assert_eq!(header.uor_per_master, 10_000.0);
    assert_eq!(header.master.label.as_deref(), Some("m"));
    assert_eq!(header.sub.label.as_deref(), Some("mm"));
    assert_eq!(header.sub.meters(), Some(0.001));
    // Every page walks exactly; record counts match the page headers.
    let graphic: usize = m.graphic_pages.iter().map(|p| p.elements.len()).sum();
    assert_eq!(graphic, 58);
    assert!(m.graphic_pages.iter().all(|p| p.complete));
    let named: usize = native.named_pages.iter().map(|p| p.elements.len()).sum();
    assert_eq!(named, 25);
    assert!(native.warnings.is_empty(), "{:?}", native.warnings);

    let doc = cadkit_dgn::read(
        &bytes,
        &ReadOptions {
            keep_raw: true,
            ..ReadOptions::default()
        },
    )
    .unwrap();
    // Levels from the level table plus referenced level ids.
    let default = doc.layers.iter().find(|l| l.name == "Default").unwrap();
    assert_eq!(default.id, Some(64));
    // Fonts from the font table become text styles.
    assert!(
        doc.text_styles
            .iter()
            .any(|s| s.font.as_deref() == Some("Arial"))
    );
    // The shared cell definition is a block with its ellipse.
    let block = doc
        .blocks
        .iter()
        .find(|b| b.name == "Named definition")
        .unwrap();
    assert_eq!(block.entities.len(), 1);
    assert!(matches!(block.entities[0].kind, EntityKind::Circle { .. }));
    // keep_raw keeps the exact element bytes.
    let mut all = Vec::new();
    for e in &doc.models[0].entities {
        walk(e, &mut all);
    }
    let with_id: Vec<&&Entity> = all.iter().filter(|e| e.id.is_some()).collect();
    assert!(
        with_id
            .iter()
            .all(|e| e.raw.as_ref().is_some_and(|r| !r.bytes.is_empty()))
    );
    // Planar 2D arc: normal +Z.
    assert!(
        all.iter()
            .any(|e| matches!(e.kind, EntityKind::Arc { normal, .. } if normal == Vec3::Z))
    );
    // The text with an extended color index (256) has no palette color.
    let text = all
        .iter()
        .find(|e| prop_int(e, "dgn.color_index") == Some(256))
        .unwrap();
    assert_eq!(text.color, cadkit_core::Color::ByLayer);
}
