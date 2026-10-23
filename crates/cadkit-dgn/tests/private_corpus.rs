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
        // The transform origin is the range low corner and the frame fills the range.
        assert!((position.x - r[0]).abs() <= tol && (position.y - r[1]).abs() <= tol);
        let far = position.translated(u_vector.plus(*v_vector));
        assert!((far.x - r[3]).abs() <= tol && (far.y - r[4]).abs() <= tol);
    }
    assert!(doc.warnings.iter().all(|w| w.code != "dgn.raster_range_placement"
        && w.code != "dgn.raster_outside_range"));
    // The 8 frames show one raster file at one size, each at its own place.
    let footprints: Vec<_> = images
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Image {
                position,
                u_vector,
                v_vector,
                ..
            } => Some((*position, u_vector.length(), v_vector.length())),
            _ => None,
        })
        .collect();
    let (_, w0, h0) = footprints[0];
    assert!(
        footprints
            .iter()
            .all(|(_, w, h)| (w - w0).abs() < 1e-6 && (h - h0).abs() < 1e-6)
    );
    let mut origins: Vec<_> = footprints
        .iter()
        .map(|(p, _, _)| ((p.x * 1e3) as i64, (p.y * 1e3) as i64))
        .collect();
    origins.sort_unstable();
    origins.dedup();
    assert_eq!(origins.len(), 8);
    // Every tag is displayed: font, size and its own level are read back.
    for a in m.entities.iter().flat_map(|e| e.attributes.iter()) {
        assert!(!a.invisible);
        let d = a.display.as_ref().expect("displayed tag presentation");
        assert!(
            [0.05, 0.1, 0.25, 0.5]
                .iter()
                .any(|h| (d.height - h).abs() < 1e-6)
        );
        assert_eq!(d.halign, cadkit_core::HAlign::Center);
        assert_eq!(d.valign, cadkit_core::VAlign::Middle);
        assert!(a.layer.is_some(), "tag element sits on its own level");
    }
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

/// Every raster frame of the private corpus is placed from its matrix and edge lengths
/// (`0x108`/`0x110` along the image axes) and fits inside its own element range: neither
/// range fallback is needed, and every frame knows its pixel size.
#[test]
fn private_raster_frames_fit_their_ranges() {
    let mut images = 0usize;
    for path in common::private_dgn_paths() {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let doc = cadkit_dgn::read(&bytes, &ReadOptions::default()).unwrap();
        assert!(doc.warnings.iter().all(
            |w| w.code != "dgn.raster_outside_range" && w.code != "dgn.raster_range_placement"
        ));
        for m in &doc.models {
            let mut all = Vec::new();
            for e in &m.entities {
                common::walk(e, &mut all);
            }
            for e in all {
                if let EntityKind::Image { size_px, .. } = &e.kind {
                    images += 1;
                    assert!(size_px.is_some_and(|[w, h]| w > 0 && h > 0));
                    assert!(e.props.contains_key("dgn.raster_dpi"));
                    assert!(e.props.contains_key("dgn.raster_rotation"));
                }
            }
        }
    }
    if !common::private_dgn_paths().is_empty() {
        assert!(images > 0);
    }
}

/// A private seed with raster attachments takes a new attachment cloned from its own
/// group: the new records have the corpus record signature, and the attachment reads back
/// with the placement, pixel size and path of the attachment it was copied from.
#[cfg(feature = "write")]
#[test]
fn private_seed_takes_a_new_raster_attachment() {
    let read = ReadOptions::default();
    let Some((bytes, source)) = common::private_dgn_paths().into_iter().find_map(|p| {
        let bytes = std::fs::read(p).ok()?;
        if !cadkit_dgn::seed_has_raster_template(&bytes, &read).ok()? {
            return None;
        }
        let doc = cadkit_dgn::read(&bytes, &read).ok()?;
        (doc.models.len() == 1).then_some((bytes, doc))
    }) else {
        return;
    };
    let mut image = source.models[0]
        .entities
        .iter()
        .find(|e| matches!(e.kind, EntityKind::Image { .. }))
        .unwrap()
        .clone();
    image.id = None;
    image.attributes.clear();
    let mut doc = source.clone();
    doc.models[0].entities = vec![image.clone()];
    // Application workflow: clear the seed model while keeping its other control records.
    let options = cadkit_dgn::WriteOptions {
        clear_seed_model: true,
        ..Default::default()
    };
    let output = cadkit_dgn::write_v8(&doc, &bytes, &options).unwrap();

    // Record signature against the seed's own groups.
    let before = cadkit_dgn::native::v8::read(&bytes, &read).unwrap();
    let after = cadkit_dgn::native::v8::read(&output, &read).unwrap();
    let signature = |m: &cadkit_dgn::native::v8::V8Model| -> Vec<(u16, usize)> {
        m.control_pages
            .iter()
            .flat_map(|p| &p.elements)
            .filter(|r| (90..=93).contains(&r.type_code()))
            .map(|r| {
                (
                    r.type_code(),
                    if r.type_code() == 90 {
                        0
                    } else {
                        r.bytes.len()
                    },
                )
            })
            .collect()
    };
    let (seed_sig, out_sig) = (signature(&before.models[0]), signature(&after.models[0]));
    assert_eq!(out_sig.len(), 9);
    assert!(seed_sig.windows(9).any(|w| w == out_sig.as_slice()));
    let frames: Vec<_> = after.models[0]
        .graphic_pages
        .iter()
        .flat_map(|p| &p.elements)
        .filter(|r| r.type_code() == 94)
        .collect();
    assert_eq!(frames.len(), 1);
    assert!(
        before.models[0]
            .graphic_pages
            .iter()
            .flat_map(|p| &p.elements)
            .any(|r| r.type_code() == 94 && r.bytes.len() == frames[0].bytes.len())
    );
    let xattributes: Vec<_> = after.models[0]
        .graphic_aux
        .iter()
        .flat_map(|p| &p.records)
        .collect();
    assert_eq!(xattributes.len(), 1);
    assert_eq!(xattributes[0].kind, 170);
    assert_eq!(Some(xattributes[0].element_id), frames[0].id());
    // Eight sub-records: `u16` index, `u16` 0x570B, ..., `u16` length at +8.
    let payload = &xattributes[0].payload;
    let mut offset = 0usize;
    for index in 0..8u16 {
        let head = &payload[offset..offset + 16];
        assert_eq!(u16::from_le_bytes([head[0], head[1]]), index);
        assert_eq!(u16::from_le_bytes([head[2], head[3]]), 0x570b);
        offset += 16 + usize::from(u16::from_le_bytes([head[8], head[9]]));
    }
    assert!(payload.len() - offset <= 16);

    let reread = cadkit_dgn::read(&output, &read).unwrap();
    assert!(
        reread
            .warnings
            .iter()
            .all(|w| !w.code.starts_with("dgn.raster"))
    );
    let got = &reread.models[0].entities[0];
    let (
        EntityKind::Image {
            path,
            position,
            u_vector,
            v_vector,
            size_px,
        },
        EntityKind::Image {
            path: want_path,
            position: want_position,
            u_vector: want_u,
            v_vector: want_v,
            size_px: want_size,
        },
    ) = (&got.kind, &image.kind)
    else {
        panic!()
    };
    assert_eq!(path, want_path);
    assert_eq!(size_px, want_size);
    let tol = 1e-6 * want_u.length().max(want_v.length());
    assert!(common::dist(*position, *want_position) <= tol);
    assert!(u_vector.minus(*want_u).length() <= tol);
    assert!(v_vector.minus(*want_v).length() <= tol);
    for key in ["dgn.raster_file", "dgn.raster_path", "dgn.raster_dpi"] {
        assert_eq!(got.props.get(key), image.props.get(key));
    }
}

/// A private seed without raster attachments takes a new attachment from the built-in
/// group: it reads back with the placement, pixel size, path and resolution it was written
/// with, and the output holds a complete group.
#[cfg(feature = "write")]
#[test]
fn private_seed_without_rasters_takes_the_builtin_group() {
    let read = ReadOptions::default();
    let paths = common::private_dgn_paths();
    let single = |p: &std::path::PathBuf, template: bool| {
        let bytes = std::fs::read(p).ok()?;
        if cadkit_dgn::seed_has_raster_template(&bytes, &read).ok()? != template {
            return None;
        }
        let doc = cadkit_dgn::read(&bytes, &read).ok()?;
        (doc.models.len() == 1).then_some((bytes, doc))
    };
    let Some((_, source)) = paths.iter().find_map(|p| single(p, true)) else {
        return;
    };
    let Some((seed, mut doc)) = paths.iter().find_map(|p| single(p, false)) else {
        return;
    };
    let mut image = source.models[0]
        .entities
        .iter()
        .find(|e| matches!(e.kind, EntityKind::Image { .. }))
        .unwrap()
        .clone();
    image.id = None;
    image.attributes.clear();
    image.layer = None;
    doc.models[0].entities = vec![image.clone()];
    let options = cadkit_dgn::WriteOptions {
        clear_seed_model: true,
        ..Default::default()
    };
    let output = cadkit_dgn::write_v8(&doc, &seed, &options).unwrap();
    assert!(cadkit_dgn::seed_has_raster_template(&output, &read).unwrap());
    let reread = cadkit_dgn::read(&output, &read).unwrap();
    assert!(
        reread
            .warnings
            .iter()
            .all(|w| !w.code.starts_with("dgn.raster"))
    );
    let images: Vec<_> = reread.models[0]
        .entities
        .iter()
        .filter(|e| matches!(e.kind, EntityKind::Image { .. }))
        .collect();
    assert_eq!(images.len(), 1);
    let (
        EntityKind::Image {
            path,
            position,
            u_vector,
            v_vector,
            size_px,
        },
        EntityKind::Image {
            path: want_path,
            position: want_position,
            u_vector: want_u,
            v_vector: want_v,
            size_px: want_size,
        },
    ) = (&images[0].kind, &image.kind)
    else {
        panic!()
    };
    assert_eq!(path, want_path);
    assert_eq!(size_px, want_size);
    let tol = 1e-6 * want_u.length().max(want_v.length());
    assert!(common::dist(*position, *want_position) <= tol);
    assert!(u_vector.minus(*want_u).length() <= tol);
    assert!(v_vector.minus(*want_v).length() <= tol);
    for key in ["dgn.raster_file", "dgn.raster_path", "dgn.raster_dpi"] {
        assert_eq!(images[0].props.get(key), image.props.get(key));
    }
}
