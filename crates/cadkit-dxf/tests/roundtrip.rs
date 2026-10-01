//! Corpus round trip: read sample -> write every version -> read back -> compare.
//!
//! Also leaves the written files in `target/tmp/dxf-out` for
//! `tests/validate_with_ezdxf.py` (independent validator, dev-time only).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::print_stdout
)]
mod common;

use std::collections::BTreeMap;

use cadkit_core::*;
use cadkit_dxf::DxfVersion;
use common::*;

const ALL: [DxfVersion; 7] = [
    DxfVersion::R12,
    DxfVersion::R2000,
    DxfVersion::R2004,
    DxfVersion::R2007,
    DxfVersion::R2010,
    DxfVersion::R2013,
    DxfVersion::R2018,
];

fn counts(doc: &Document) -> BTreeMap<String, usize> {
    let all: Vec<Entity> = doc
        .models
        .iter()
        .flat_map(|m| m.entities.iter().cloned())
        .collect();
    histogram(&all)
}

#[test]
fn round_trip_every_sample_and_version() {
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("dxf-out");
    std::fs::create_dir_all(&out).unwrap();
    let mut n = 0;
    for (ver, _) in VERSIONS {
        let Some(bytes) = corpus_file(&format!("sample_{ver}_ascii.dxf")) else {
            continue;
        };
        let doc = read(&bytes);
        for v in ALL {
            let (text, warnings) = cadkit_dxf::write_with_report(&doc, v).unwrap();
            let name = format!("from_{ver}_to_{}.dxf", v.acadver());
            std::fs::write(out.join(&name), &text).unwrap();
            let back = read(text.as_bytes());
            let ctx = format!("{ver} -> {}", v.acadver());
            assert_eq!(back.source.version, v.acadver(), "{ctx}");
            // R12 has no $INSUNITS.
            if v != DxfVersion::R12 {
                assert_eq!(back.units, doc.units, "{ctx}: units");
            }
            if v != DxfVersion::R12 {
                // Same layers (the writer may add layer 0 when it was only implied).
                let names = |d: &Document| {
                    let mut l: Vec<String> =
                        d.layers.iter().map(|l| l.name.to_lowercase()).collect();
                    l.sort();
                    l
                };
                let (la, lb) = (names(&doc), names(&back));
                assert!(
                    la.iter().all(|x| lb.contains(x)),
                    "{ctx}: layers lost: {la:?} vs {lb:?}"
                );
                assert!(
                    lb.iter().all(|x| la.contains(x) || x == "0"),
                    "{ctx}: layers added: {la:?} vs {lb:?}"
                );
                for l in &doc.layers {
                    let m = back
                        .layers
                        .iter()
                        .find(|m| m.name.eq_ignore_ascii_case(&l.name))
                        .unwrap();
                    assert_eq!(
                        (l.frozen, l.locked, l.visible, l.plottable),
                        (m.frozen, m.locked, m.visible, m.plottable),
                        "{ctx}: layer {}",
                        l.name
                    );
                    assert_eq!(l.lineweight, m.lineweight, "{ctx}: layer lineweight");
                }
                assert_eq!(doc.models.len(), back.models.len(), "{ctx}: model count");
                for (ma, mb) in doc.models.iter().zip(&back.models) {
                    assert_eq!(ma.name, mb.name, "{ctx}: model name");
                    compare_lists(
                        &format!("{ctx} model {}", ma.name),
                        &ma.entities,
                        &mb.entities,
                        v,
                    );
                }
                assert_eq!(doc.blocks.len(), back.blocks.len(), "{ctx}: block count");
                for (ba, bb) in doc.blocks.iter().zip(&back.blocks) {
                    assert_eq!(ba.name, bb.name, "{ctx}: block name");
                    assert!(
                        close(&p3(&ba.base_point), &p3(&bb.base_point)),
                        "{ctx}: base point"
                    );
                    compare_lists(
                        &format!("{ctx} block {}", ba.name),
                        &ba.entities,
                        &bb.entities,
                        v,
                    );
                }
                for lt in &doc.linetypes {
                    let b = back
                        .linetypes
                        .iter()
                        .find(|x| x.name.eq_ignore_ascii_case(&lt.name))
                        .unwrap_or_else(|| panic!("{ctx}: linetype {} lost", lt.name));
                    assert!(
                        close(&lt.pattern, &b.pattern),
                        "{ctx}: pattern of {}",
                        lt.name
                    );
                }
            } else {
                // R12 has no native ELLIPSE/SPLINE/HATCH/LEADER/IMAGE/MTEXT: those turn into
                // polylines / lines / text, everything else must survive.
                let (ca, cb) = (counts(&doc), counts(&back));
                for k in ["Insert", "Dimension", "Face", "Viewport", "Mesh", "Point"] {
                    assert_eq!(ca.get(k), cb.get(k), "{ctx}: {k} count");
                }
                assert_eq!(doc.blocks.len(), back.blocks.len(), "{ctx}: block count");
            }
            n += 1;
            println!(
                "{ctx}: ok ({} bytes, {} writer warnings)",
                text.len(),
                warnings.len()
            );
        }
    }
    println!("{n} round trips, files in {}", out.display());
    if corpus_file("sample_AC1009_ascii.dxf").is_some() {
        assert_eq!(n, 49);
    }
}
