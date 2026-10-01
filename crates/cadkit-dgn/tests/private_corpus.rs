//! Structural checks on the private corpus (`corpus/private/dgn/*.dgn`, discovered by
//! listing the directory). Skips silently when it is absent and asserts only counts and
//! invariants (AGENTS.md section 3): no names, values, paths or coordinates.
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

mod common;

use std::collections::BTreeMap;

use cadkit_core::{EntityKind, ReadOptions, Value};
use common::{private_files, prop_int};

/// Size of the V8i 3D architectural sample whose element histogram is known.
const KNOWN_SAMPLE_BYTES: usize = 154_112;

#[test]
fn private_files_hold_their_invariants() {
    for bytes in private_files("dgn") {
        assert!(cadkit_dgn::sniff(&bytes));
        let doc = cadkit_dgn::read(&bytes, &ReadOptions::default()).unwrap();
        assert!(!doc.models.is_empty());
        assert!(!doc.layers.is_empty());
        assert!(doc.layers.iter().all(|l| !l.name.is_empty()));
        for m in &doc.models {
            // Every entity resolves to a declared layer.
            assert!(m.entities.iter().all(|e| {
                e.layer
                    .as_deref()
                    .is_none_or(|l| doc.layers.iter().any(|x| x.name == l))
            }));
        }
        if bytes.len() == KNOWN_SAMPLE_BYTES {
            known_sample(&bytes);
        }
    }
}

fn known_sample(bytes: &[u8]) {
    let native = cadkit_dgn::native::v8::read(bytes, &ReadOptions::default()).unwrap();
    assert_eq!(native.models.len(), 1);
    let model = &native.models[0];
    assert_eq!(model.graphic_pages.len(), 7);
    let mut histogram: BTreeMap<u16, usize> = BTreeMap::new();
    for e in model.graphic_pages.iter().flat_map(|p| p.elements.iter()) {
        *histogram.entry(e.type_code()).or_default() += 1;
    }
    assert_eq!(histogram.values().sum::<usize>(), 2032);
    assert_eq!(histogram.get(&6), Some(&1533));
    assert_eq!(histogram.get(&37), Some(&418));
    assert_eq!(histogram.get(&15), Some(&49));
    assert_eq!(histogram.get(&4), Some(&24));
    assert_eq!(histogram.get(&94), Some(&8));
    assert!(model.graphic_pages.iter().all(|p| p.complete));
    assert!(model.header.as_ref().is_some_and(|h| h.is_3d));

    let doc = cadkit_dgn::read(bytes, &ReadOptions::default()).unwrap();
    let m = &doc.models[0];
    assert!(m.is_3d);
    // Every tag is attached to its target with a resolved set and tag name.
    let attributes: usize = m.entities.iter().map(|e| e.attributes.len()).sum();
    assert_eq!(attributes, 418);
    assert!(
        doc.warnings
            .iter()
            .all(|w| w.code != "dgn.tag_target_missing")
    );
    for a in m.entities.iter().flat_map(|e| e.attributes.iter()) {
        assert!(a.set.is_some(), "tag set resolved");
        assert!(!a.tag.starts_with("tag_"), "tag name resolved");
    }
    // Drawn elements: 1533 shapes + 24 line strings + 49 ellipses + 8 raster frames.
    assert_eq!(m.entities.len(), 1533 + 24 + 49 + 8);
    assert!(
        m.entities
            .iter()
            .all(|e| !matches!(&e.kind, EntityKind::Unknown { .. }))
    );
    let images: Vec<_> = m
        .entities
        .iter()
        .filter(|e| matches!(e.kind, EntityKind::Image { .. }))
        .collect();
    assert_eq!(images.len(), 8);
    // Frames are placed from their transform (unrotated here) and every image rectangle lies
    // inside its element range, MicroStation's own bounding box (tolerance: 4 UOR).
    let tol = 4.0 / 10_000.0;
    for e in &images {
        let EntityKind::Image {
            position,
            u_vector,
            v_vector,
            ..
        } = &e.kind
        else {
            panic!()
        };
        assert!(u_vector.x > 0.0 && u_vector.y == 0.0);
        assert!(v_vector.y > 0.0 && v_vector.x == 0.0);
        let Some(Value::List(r)) = e.props.get("dgn.range") else {
            panic!()
        };
        let r: Vec<f64> = r
            .iter()
            .map(|v| match v {
                Value::Float(f) => *f,
                _ => f64::NAN,
            })
            .collect();
        for p in [
            *position,
            position.translated(*u_vector),
            position.translated(*v_vector),
            position.translated(u_vector.plus(*v_vector)),
        ] {
            assert!(p.x >= r[0] - tol && p.x <= r[3] + tol);
            assert!(p.y >= r[1] - tol && p.y <= r[4] + tol);
        }
        // All 8 frames store one extent corner for one file: flagged, see FORMAT_NOTES.
        assert_eq!(
            e.props.get("dgn.raster_extent_shared"),
            Some(&Value::Bool(true))
        );
    }
    assert!(doc.warnings.iter().all(|w| w.code != "dgn.raster_range_placement"
        && w.code != "dgn.raster_outside_range"));
    assert!(
        doc.warnings
            .iter()
            .any(|w| w.code == "dgn.raster_extent_shared")
    );
    // Tag set definitions survive in the document props (8 sets).
    let Some(Value::List(sets)) = doc.props.get("dgn.tag_sets") else {
        panic!()
    };
    assert_eq!(sets.len(), 8);
    assert!(
        images
            .iter()
            .all(|e| matches!(&e.kind, EntityKind::Image { path: Some(_), .. }))
    );
    assert!(
        m.entities
            .iter()
            .all(|e| prop_int(e, "dgn.level_id").is_some())
    );
}
