//! Public V8 samples from Safe Software (see docs/dgn/FORMAT_NOTES.md, "Test corpus").
//! Skips when the files are absent.
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

mod common;

use cadkit_core::{EntityKind, GroupKind, HAlign, ReadOptions, VAlign, Value};
use common::{corpus, prop_int, prop_point};

#[test]
fn tree_labels_with_tags() {
    let Some(bytes) = corpus("public/safe/TreeTextNodeLabelsWithTags.dgn") else {
        return;
    };
    let doc = cadkit_dgn::read(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(doc.models.len(), 1);
    let m = &doc.models[0];
    assert!(!m.is_3d);
    // 6904 text nodes holding 12623 texts; all 13808 tags attach to the nodes.
    assert_eq!(m.entities.len(), 6904);
    let mut texts = 0;
    let mut attributes = 0;
    for e in &m.entities {
        let EntityKind::Group {
            group_kind: GroupKind::TextNode,
            children,
            ..
        } = &e.kind
        else {
            panic!("{:?}", e.kind)
        };
        texts += children
            .iter()
            .filter(|c| matches!(c.kind, EntityKind::Text { .. }))
            .count();
        attributes += e.attributes.len();
        assert_eq!(e.attributes.len(), 2);
        for a in &e.attributes {
            assert_eq!(a.set.as_deref(), Some("Trees"));
            match a.tag.as_str() {
                "Species" => assert!(matches!(a.value, Value::Text(_))),
                "Tree Count" => assert!(matches!(a.value, Value::Int(_))),
                other => panic!("unexpected tag {other}"),
            }
        }
    }
    assert_eq!(texts, 12_623);
    // Every text is left-bottom justified: the anchor is the stored origin, and the length
    // comes from the file (not estimated).
    for e in &m.entities {
        let EntityKind::Group { children, .. } = &e.kind else {
            continue;
        };
        for t in children {
            let EntityKind::Text {
                position,
                halign,
                valign,
                ..
            } = &t.kind
            else {
                continue;
            };
            assert_eq!(prop_int(t, "dgn.justification"), Some(2));
            assert_eq!((*halign, *valign), (HAlign::Left, VAlign::Baseline));
            assert_eq!(*position, prop_point(t, "dgn.origin"));
            assert!(!t.props.contains_key("dgn.text_length_estimated"));
        }
    }
    // The tag set definition is kept in the document props.
    let Some(Value::List(sets)) = doc.props.get("dgn.tag_sets") else {
        panic!()
    };
    assert_eq!(sets.len(), 1);
    let Value::List(set) = &sets[0] else { panic!() };
    assert_eq!(set[0], Value::Text("Trees".into()));
    let Value::List(tags) = &set[2] else { panic!() };
    let names: Vec<&Value> = tags
        .iter()
        .map(|t| match t {
            Value::List(f) => &f[1],
            _ => panic!(),
        })
        .collect();
    assert_eq!(
        names,
        [
            &Value::Text("Species".into()),
            &Value::Text("Tree Count".into())
        ]
    );
    let Value::List(species) = &tags[0] else {
        panic!()
    };
    assert_eq!(species[3], Value::Int(1));
    assert_eq!(species[4], Value::Text("Default Value".into()));
    assert_eq!(attributes, 13_808);
    assert!(
        doc.warnings
            .iter()
            .all(|w| w.code != "dgn.tag_target_missing")
    );
    assert_eq!(prop_int_doc(&doc, "dgn.tag_set_count"), Some(1));
}

fn prop_int_doc(doc: &cadkit_core::Document, key: &str) -> Option<i64> {
    match doc.props.get(key) {
        Some(Value::Int(v)) => Some(*v),
        _ => None,
    }
}

#[test]
fn water_mains_line_strings() {
    let Some(bytes) = corpus("public/safe/Water_distribution_mains.dgn") else {
        return;
    };
    let doc = cadkit_dgn::read(&bytes, &ReadOptions::default()).unwrap();
    let m = &doc.models[0];
    assert_eq!(m.entities.len(), 66_053);
    let strings = m
        .entities
        .iter()
        .filter(|e| prop_int(e, "dgn.type") == Some(4))
        .count();
    let lines = m
        .entities
        .iter()
        .filter(|e| prop_int(e, "dgn.type") == Some(3))
        .count();
    assert_eq!((strings, lines), (66_035, 18));
    assert!(m.entities.iter().all(|e| e.layer.is_some()));
    assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
}

/// The public V8 samples written back into themselves give the same bytes with and without
/// the layer data the reader takes from 240-byte level entries (their entries are 104 bytes,
/// so the reader adds none; the check guards the writer's use of it).
#[cfg(feature = "write")]
#[test]
fn write_back_ignores_level_colors() {
    let options = cadkit_dgn::WriteOptions::default()
        .with_clear_seed_model(true)
        .with_timestamp_ms(1.7e12);
    for rel in [
        "public/gdal/test_dgnv8.dgn",
        "public/safe/TreeTextNodeLabelsWithTags.dgn",
        "public/safe/Water_distribution_mains.dgn",
    ] {
        let Some(bytes) = corpus(rel) else {
            continue;
        };
        let doc = cadkit_dgn::read(&bytes, &ReadOptions::default()).unwrap();
        assert!(
            doc.layers
                .iter()
                .all(|l| l.color == cadkit_core::Color::ByLayer
                    && !l.props.contains_key("dgn.color_index"))
        );
        let mut without = doc.clone();
        for l in &mut without.layers {
            l.color = cadkit_core::Color::ByLayer;
            for key in [
                "dgn.color_index",
                "dgn.level_weight_raw",
                "dgn.level_style_raw",
            ] {
                l.props.remove(key);
            }
        }
        match (
            cadkit_dgn::write_v8(&doc, &bytes, &options),
            cadkit_dgn::write_v8(&without, &bytes, &options),
        ) {
            (Ok(a), Ok(b)) => assert!(a == b, "{rel}"),
            (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
            _ => panic!("{rel}: level colors changed whether a write-back succeeds"),
        }
    }
}
