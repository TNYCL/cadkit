//! V7 samples from GDAL. Expected values: GDAL dgnlib output as recorded in ezdgn's
//! `tests/data/dgn/README.md` (smalltest.dgn) and the seed files' design headers.
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

mod common;

use cadkit_core::{EntityKind, HAlign, LengthUnit, Point3, ReadOptions, VAlign};
use common::{corpus, dist, prop_int, prop_point};

#[test]
fn smalltest_matches_published_values() {
    let Some(bytes) = corpus("public/gdal/smalltest.dgn") else {
        return;
    };
    assert!(cadkit_dgn::sniff(&bytes));
    let native = cadkit_dgn::native::v7::read(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(native.records.len(), 15);
    assert!(!native.is_3d);
    let tcb = native.tcb.as_ref().unwrap();
    assert_eq!((tcb.subunits_per_master, tcb.uor_per_subunit), (10, 1000));
    assert_eq!(
        (tcb.master_label.as_str(), tcb.sub_label.as_str()),
        ("mu", "su")
    );

    let doc = cadkit_dgn::read(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(doc.source.version, "V7");
    let m = &doc.models[0];
    let drawn: Vec<_> = m.entities.iter().collect();
    assert_eq!(
        drawn.len(),
        4,
        "{:?}",
        drawn.iter().map(|e| e.kind.type_name()).collect::<Vec<_>>()
    );

    let EntityKind::Text {
        position,
        value,
        halign,
        valign,
        ..
    } = &drawn[0].kind
    else {
        panic!("{:?}", drawn[0].kind)
    };
    assert_eq!(value, "Demo Text");
    // The stored origin (GDAL's published value) is the lower-left corner ...
    assert!(
        dist(
            prop_point(drawn[0], "dgn.origin"),
            Point3::new(0.7365, 4.2198, 0.0)
        ) < 1e-9
    );
    // ... and justification 7 (center-center) puts the anchor at the middle of the text
    // box: the range spans x 0.7365..9.4083, and the box is one text height (1.0) tall.
    assert_eq!((*halign, *valign), (HAlign::Center, VAlign::Middle));
    assert!(
        dist(*position, Point3::new(5.0724, 4.7198, 0.0)) < 1e-6,
        "{position:?}"
    );
    assert_eq!(drawn[0].layer.as_deref(), Some("Level 1"));
    assert_eq!(prop_int(drawn[0], "dgn.justification"), Some(7));

    let EntityKind::Circle { center, radius, .. } = &drawn[1].kind else {
        panic!("{:?}", drawn[1].kind)
    };
    assert!(dist(*center, Point3::new(5.0082, 4.5835, 0.0)) < 1e-9);
    assert!((radius - 4.679_606_584).abs() < 1e-8);
    assert_eq!(drawn[1].layer.as_deref(), Some("Level 2"));

    let EntityKind::Polyline {
        vertices, closed, ..
    } = &drawn[2].kind
    else {
        panic!()
    };
    assert!(*closed);
    let expected = [
        (4.5355, 3.3170),
        (4.3832, 2.6517),
        (4.9441, 2.5235),
        (4.8320, 3.3331),
    ];
    assert_eq!(vertices.len(), expected.len());
    for (v, (x, y)) in vertices.iter().zip(expected) {
        assert!(dist(v.position, Point3::new(x, y, 0.0)) < 1e-9);
    }
    // The shape carries a fill linkage (16 bytes of attribute data at record offset 78).
    assert!(drawn[2].props.contains_key("dgn.fill_color"));

    let EntityKind::Line { start, end } = &drawn[3].kind else {
        panic!()
    };
    assert!(dist(*start, Point3::new(2.5562, 5.7218, 0.0)) < 1e-9);
    assert!(dist(*end, Point3::new(2.5242, 6.0709, 0.0)) < 1e-9);

    let names: Vec<&str> = doc.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Level 1", "Level 2"]);
}

#[test]
fn seed_files_units_and_origin() {
    if let Some(bytes) = corpus("public/gdal/seed_2d.dgn") {
        let f = cadkit_dgn::native::v7::read(&bytes, &ReadOptions::default()).unwrap();
        let tcb = f.tcb.unwrap();
        assert_eq!((tcb.subunits_per_master, tcb.uor_per_subunit), (10, 12));
        // VAX D-float global origin.
        assert_eq!(tcb.origin[0], -249_879_416.0);
        assert_eq!(tcb.origin[1], -669_487_710.0);
        let doc = cadkit_dgn::read(&bytes, &ReadOptions::default()).unwrap();
        assert_eq!(doc.units.unit, LengthUnit::Foot);
        assert!(doc.models[0].entities.is_empty());
    }
    if let Some(bytes) = corpus("public/gdal/seed_3d.dgn") {
        let doc = cadkit_dgn::read(&bytes, &ReadOptions::default()).unwrap();
        assert!(doc.models[0].is_3d);
        assert_eq!(doc.units.unit, LengthUnit::Meter);
    }
}

#[test]
fn knot_oob_is_survived() {
    let Some(bytes) = corpus("public/gdal/knot_oob.dgn") else {
        return;
    };
    let doc = cadkit_dgn::read(&bytes, &ReadOptions::default()).unwrap();
    // The lone knot record stays visible as an Unknown entity.
    assert_eq!(doc.models[0].entities.len(), 1);
    assert!(!doc.warnings.is_empty());
}
