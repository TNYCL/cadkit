//! Seed stripping ([`cadkit_dgn::strip_seed_v8`]) of the public V8 samples and of the private
//! corpus (`corpus/private/**/*.dgn`, counts and invariants only, AGENTS.md section 3). Skips
//! silently when the files are absent.
#![cfg(feature = "write")]
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

mod common;

use std::collections::BTreeMap;

use cadkit_core::{Document, Entity, EntityKind, Model, Point3, ReadOptions};
use cadkit_dgn::cfb::{CompoundFile, EntryKind};
use cadkit_dgn::{StripSeedOptions, V8Seed, WriteOptions, check_structure, strip_seed_v8};
use common::{corpus, private_dgn_paths};

const PUBLIC: [&str; 3] = [
    "public/gdal/test_dgnv8.dgn",
    "public/safe/TreeTextNodeLabelsWithTags.dgn",
    "public/safe/Water_distribution_mains.dgn",
];

/// Property ids the stripped `\u{5}SummaryInformation` may hold: code page, creation and
/// last-save time, application.
const SUMMARY_IDS: [u32; 4] = [0x01, 0x0c, 0x0d, 0x12];

fn streams(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let cf = CompoundFile::parse(bytes).unwrap();
    cf.paths()
        .iter()
        .filter(|p| p.kind == EntryKind::Stream)
        .map(|p| (p.path.clone(), cf.read_stream(p.index).unwrap()))
        .collect()
}

fn dictionary(all: &BTreeMap<String, Vec<u8>>) -> BTreeMap<&String, &Vec<u8>> {
    all.iter()
        .filter(|(k, _)| k.starts_with("Dgn^Nm"))
        .collect()
}

/// Property ids of the first property set of a `[MS-OLEPS]` stream.
fn property_ids(b: &[u8]) -> Vec<u32> {
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
    let set = u32_at(44);
    (0..u32_at(set + 4))
        .map(|i| u32_at(set + 8 + 8 * i) as u32)
        .collect()
}

fn model_records(bytes: &[u8]) -> (usize, usize, usize) {
    let native = cadkit_dgn::native::v8::read(bytes, &ReadOptions::default()).unwrap();
    let m = &native.models[0];
    let count = |pages: &[cadkit_dgn::native::v8::Page]| -> usize {
        pages.iter().map(|p| p.elements.len()).sum()
    };
    let aux = m
        .graphic_aux
        .iter()
        .chain(&m.control_aux)
        .map(|p| p.records.len())
        .sum();
    (count(&m.graphic_pages), count(&m.control_pages), aux)
}

/// The checks every default strip passes: no model record, two minimal property sets, a
/// clean structure, a seed the writer accepts as empty, the seed's dictionary unchanged
/// unless the report says otherwise.
fn assert_stripped(seed: &[u8], out: &[u8], dictionary_rewritten: u64) {
    let read = ReadOptions::default();
    assert_eq!(model_records(out), (0, 0, 0));
    let after = streams(out);
    let sets: Vec<&String> = after.keys().filter(|k| k.starts_with('\u{5}')).collect();
    assert_eq!(sets.len(), 2);
    let summary = property_ids(&after["\u{5}SummaryInformation"]);
    assert!(
        summary.iter().all(|id| SUMMARY_IDS.contains(id)),
        "{summary:?}"
    );
    assert_eq!(property_ids(&after["\u{5}DocumentSummaryInformation"]), [1]);
    let report = check_structure(out, &read).unwrap();
    assert!(report.is_clean(), "{:?}", report.warnings);
    assert!(!V8Seed::parse(out, &read).unwrap().is_nonempty());
    if dictionary_rewritten == 0 {
        let before = streams(seed);
        assert_eq!(dictionary(&after), dictionary(&before));
    }
}

/// One line in the units and dimension of `seed`.
fn line_document(seed: &[u8]) -> Document {
    let parsed = V8Seed::parse(seed, &ReadOptions::default()).unwrap();
    let mut doc = Document::default();
    doc.units = parsed.units();
    let mut model = Model::default();
    model.is_3d = parsed.is_3d();
    model.entities.push(Entity::new(EntityKind::Line {
        start: Point3::new(1.0, 2.0, 0.0),
        end: Point3::new(3.0, 5.0, 0.0),
    }));
    doc.models.push(model);
    doc
}

#[test]
fn public_samples_strip_to_clean_seeds_the_writer_fills() {
    let read = ReadOptions::default();
    for rel in PUBLIC {
        let Some(seed) = corpus(rel) else { continue };
        let before = streams(&seed);
        let input_ids = property_ids(&before["\u{5}SummaryInformation"]);
        let out = strip_seed_v8(&seed, &StripSeedOptions::default(), &read).unwrap();
        let r = &out.report;
        assert_stripped(&seed, &out.bytes, r.dictionary_pages_rewritten);
        assert_eq!(r.dictionary_pages_rewritten, 0, "{rel}");
        let (graphics, controls, _) = model_records(&seed);
        assert_eq!(r.removed_graphic_records, graphics as u64, "{rel}");
        assert_eq!(r.removed_control_records, controls as u64, "{rel}");
        // Every public sample names a person or holds a thumbnail; none of it stays.
        for id in [0x04, 0x08, 0x11] {
            assert_eq!(
                input_ids.contains(&id),
                r.removed_summary_properties.contains(&id),
                "{rel} {id:#x}"
            );
        }
        assert!(
            r.removed_summary_properties.contains(&0x11),
            "{rel}: thumbnail"
        );
        assert!(
            r.removed_document_summary_properties.contains(&0x0f),
            "{rel}: company"
        );
        assert!(r.levels >= 1, "{rel}");
        assert!(r.unknown_streams >= 2, "{rel}: Dgn~S and Dgn~Mf");
        // Equal input, equal output; stripping again changes nothing.
        let again = strip_seed_v8(&out.bytes, &StripSeedOptions::default(), &read).unwrap();
        assert_eq!(again.bytes, out.bytes, "{rel}");
        assert_eq!(
            strip_seed_v8(&seed, &StripSeedOptions::default(), &read)
                .unwrap()
                .bytes,
            out.bytes
        );
        // The writer fills the result without clearing anything.
        let parsed = V8Seed::parse(&out.bytes, &read).unwrap();
        let written = cadkit_dgn::write_v8(
            &line_document(&out.bytes),
            &out.bytes,
            &WriteOptions::default(),
        )
        .unwrap();
        let back = cadkit_dgn::read(&written, &read).unwrap();
        assert_eq!(back.models[0].entities.len(), 1, "{rel}");
        assert!(
            check_structure(&written, &read).unwrap().is_clean(),
            "{rel}"
        );
        // A renamed model and an explicit time strip deterministically too.
        let options = StripSeedOptions::new()
            .with_model_name(Some("Model"))
            .with_timestamp_ms(1.7e12);
        let renamed = strip_seed_v8(&seed, &options, &read).unwrap();
        assert!(renamed.report.warnings.is_empty(), "{rel}");
        let header = V8Seed::parse(&renamed.bytes, &read).unwrap();
        assert_eq!(header.levels(), parsed.levels());
        let native = cadkit_dgn::native::v8::read(&renamed.bytes, &read).unwrap();
        assert_eq!(
            native.models[0].header.as_ref().unwrap().name.as_deref(),
            Some("Model")
        );
        assert_eq!(native.model_index[0].name, "Model");
        assert_eq!(
            strip_seed_v8(&renamed.bytes, &options, &read)
                .unwrap()
                .bytes,
            renamed.bytes
        );
    }
}

/// Control records of the model, byte for byte.
fn control_bytes(bytes: &[u8]) -> Vec<Vec<u8>> {
    let native = cadkit_dgn::native::v8::read(bytes, &ReadOptions::default()).unwrap();
    native.models[0]
        .control_pages
        .iter()
        .flat_map(|p| p.elements.iter().map(|r| r.bytes.clone()))
        .collect()
}

/// `keep_controls` keeps the one control record of TreeText and of Water (a type 66 model
/// settings record without a dependency linkage, whose body holds many byte windows equal to
/// graphic ids) byte for byte, and so does a default `write_v8` that clears the model;
/// test_dgnv8 has no control record.
#[test]
fn keep_controls_keeps_the_public_model_settings_records() {
    let read = ReadOptions::default();
    for (rel, expected) in [(PUBLIC[0], 0u64), (PUBLIC[1], 1), (PUBLIC[2], 1)] {
        let Some(seed) = corpus(rel) else { continue };
        let original = control_bytes(&seed);
        assert_eq!(original.len() as u64, expected, "{rel}");
        let options = StripSeedOptions::new().with_keep_controls(true);
        let out = strip_seed_v8(&seed, &options, &read).unwrap();
        assert_eq!(out.report.kept_control_records, expected, "{rel}");
        assert_eq!(out.report.removed_control_records, 0, "{rel}");
        if expected > 0 {
            assert_eq!(
                out.report.kept_control_types,
                BTreeMap::from([(66u16, 1u64)]),
                "{rel}"
            );
        }
        assert_eq!(control_bytes(&out.bytes), original, "{rel}");
        assert!(
            check_structure(&out.bytes, &read).unwrap().is_clean(),
            "{rel}"
        );
        let again = strip_seed_v8(&out.bytes, &options, &read).unwrap();
        assert_eq!(again.bytes, out.bytes, "{rel}");
        // The default strip still removes it.
        let default = strip_seed_v8(&seed, &StripSeedOptions::default(), &read).unwrap();
        assert_eq!(default.report.kept_control_records, 0, "{rel}");
        assert_eq!(default.report.removed_control_records, expected, "{rel}");
        // A write that replaces the model keeps it too (`preserve_seed_controls`, default on).
        let written = cadkit_dgn::write_v8(
            &line_document(&seed),
            &seed,
            &WriteOptions::default().with_clear_seed_model(true),
        )
        .unwrap();
        assert_eq!(control_bytes(&written), original, "{rel}");
    }
}

#[test]
fn cell_definitions_go_only_on_request() {
    let read = ReadOptions::default();
    let Some(seed) = corpus("public/gdal/test_dgnv8.dgn") else {
        return;
    };
    let blocks = cadkit_dgn::read(&seed, &read).unwrap().blocks.len();
    assert_eq!(blocks, 1);
    let kept = strip_seed_v8(&seed, &StripSeedOptions::default(), &read).unwrap();
    assert_eq!(kept.report.cell_definitions, 1);
    assert_eq!(
        cadkit_dgn::read(&kept.bytes, &read).unwrap().blocks.len(),
        1
    );
    let options = StripSeedOptions::new().with_keep_cell_definitions(false);
    let out = strip_seed_v8(&seed, &options, &read).unwrap();
    assert_eq!(out.report.removed_cell_definitions, 1);
    assert_eq!(out.report.cell_definitions, 0);
    assert_eq!(out.report.dictionary_pages_rewritten, 1);
    assert!(
        cadkit_dgn::read(&out.bytes, &read)
            .unwrap()
            .blocks
            .is_empty()
    );
    assert_stripped(&seed, &out.bytes, out.report.dictionary_pages_rewritten);
    assert_eq!(
        strip_seed_v8(&out.bytes, &options, &read).unwrap().bytes,
        out.bytes
    );
}

/// Every private V8 file the writer accepts as a seed strips to a clean seed, and every
/// file it rejects is rejected with the same error; counts only.
#[test]
fn private_corpus_strips_like_the_writer_accepts() {
    let read = ReadOptions::default();
    let mut stripped = 0usize;
    let mut rejected = 0usize;
    let mut rewritten = 0usize;
    for (i, path) in private_dgn_paths().into_iter().enumerate() {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if !cadkit_dgn::native::v8::sniff(&bytes) {
            continue;
        }
        let parsed = V8Seed::parse(&bytes, &read);
        let out = strip_seed_v8(&bytes, &StripSeedOptions::default(), &read);
        let out = match (parsed, out) {
            (Ok(_), Ok(out)) => out,
            (Err(a), Err(b)) => {
                assert_eq!(a.to_string(), b.to_string(), "file #{i}");
                rejected += 1;
                continue;
            }
            (a, b) => panic!("file #{i}: seed {:?} / strip {:?}", a.is_ok(), b.is_ok()),
        };
        stripped += 1;
        rewritten += usize::from(out.report.dictionary_pages_rewritten > 0);
        // The seed's own dictionary, byte for byte, also where the writer would repair it.
        assert_eq!(out.report.dictionary_pages_rewritten, 0, "file #{i}");
        assert_stripped(&bytes, &out.bytes, out.report.dictionary_pages_rewritten);
        assert!(
            out.report.removed_summary_properties.contains(&0x04),
            "file #{i}"
        );
        if i % 16 == 0 {
            let again = strip_seed_v8(&out.bytes, &StripSeedOptions::default(), &read).unwrap();
            assert_eq!(again.bytes, out.bytes, "file #{i}");
            let written = cadkit_dgn::write_v8(
                &line_document(&out.bytes),
                &out.bytes,
                &WriteOptions::default(),
            )
            .unwrap();
            let back = cadkit_dgn::read(&written, &read).unwrap();
            assert_eq!(back.models[0].entities.len(), 1, "file #{i}");
            for options in [
                StripSeedOptions::new().with_keep_controls(true),
                StripSeedOptions::new().with_keep_raster_template(true),
            ] {
                let kept = strip_seed_v8(&bytes, &options, &read).unwrap();
                if options.keep_controls {
                    // No non-raster control record of the corpus has a dependency linkage
                    // that names a graphic: all of them stay.
                    let non_raster = control_bytes(&bytes)
                        .iter()
                        .filter(|b| !(90..=93).contains(&u16::from_le_bytes([b[0], b[1]])))
                        .count() as u64;
                    assert_eq!(kept.report.kept_control_records, non_raster, "file #{i}");
                }
                assert!(
                    check_structure(&kept.bytes, &read).unwrap().is_clean(),
                    "file #{i}"
                );
                let again = strip_seed_v8(&kept.bytes, &options, &read).unwrap();
                assert_eq!(again.bytes, kept.bytes, "file #{i}");
            }
        }
    }
    assert!(stripped + rejected == 0 || stripped > 0);
    assert_eq!(rewritten, 0);
}
