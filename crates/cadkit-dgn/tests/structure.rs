//! Structural reports ([`cadkit_dgn::check_structure`]) of the public V8 samples, of what the
//! writer makes of them and of the private corpus (`corpus/private/**/*.dgn`, counts and
//! invariants only, AGENTS.md section 3). Skips silently when the files are absent.
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

mod common;

use cadkit_core::ReadOptions;
#[cfg(feature = "write")]
use cadkit_core::{Document, Entity, EntityKind, Value};
use cadkit_dgn::{StructureReport, check_structure};
use common::corpus;

const PUBLIC: [&str; 3] = [
    "public/gdal/test_dgnv8.dgn",
    "public/safe/TreeTextNodeLabelsWithTags.dgn",
    "public/safe/Water_distribution_mains.dgn",
];

fn codes(r: &StructureReport) -> Vec<&str> {
    r.warnings.iter().map(|w| w.code.as_str()).collect()
}

/// Storages, red-black trees, top-down trees, irregular free slots, pages, numbered pages
/// and warning codes of a report.
type Expected<'a> = (u32, u32, u32, u32, u32, u32, &'a [&'a str]);

/// Expected report of each public sample, from its own bytes (the ODA-written test_dgnv8
/// and the Water file leave stale bytes in unused directory slots; TreeText, saved by
/// MicroStation 8.11, does not; all three store page numbers and populations in some page
/// headers, and none of them uses the writer's slot order).
#[test]
fn public_samples_report_their_structure() {
    let expected: [Expected; 3] = [
        (9, 9, 9, 3, 4, 3, &["dgn.cfb_free_entry"]),
        (8, 8, 8, 0, 4, 1, &[]),
        (10, 10, 8, 1, 138, 137, &["dgn.cfb_free_entry"]),
    ];
    for (rel, want) in PUBLIC.iter().zip(expected) {
        let Some(bytes) = corpus(rel) else { continue };
        let r = check_structure(&bytes, &ReadOptions::default()).unwrap();
        let found = codes(&r);
        let got = (
            r.storages,
            r.red_black_trees,
            r.top_down_trees,
            r.irregular_free_entries,
            r.pages,
            r.pages_with_number_or_population,
            found.as_slice(),
        );
        assert_eq!(got, want, "{rel}");
        assert_eq!(r.cfb_version, 3);
        assert!(!r.writer_slot_order);
        assert_eq!(r.model_records_without_time, 0);
        assert!(r.split_dictionary_pages.is_empty());
    }
}

/// Repacked and written public files: ordered top-down trees, free slots in the MS-CFB form,
/// the writer's slot order, no warning. Unchanged seed pages keep their headers.
#[cfg(feature = "write")]
#[test]
fn written_public_files_report_clean_containers() {
    let read = ReadOptions::default();
    for rel in PUBLIC {
        let Some(seed) = corpus(rel) else { continue };
        let source = check_structure(&seed, &read).unwrap();
        let repacked = cadkit_dgn::repack_v8(&seed, &read).unwrap();
        let r = check_structure(&repacked, &read).unwrap();
        assert!(r.is_clean(), "{rel}: {:?}", r.warnings);
        assert_eq!(
            (r.red_black_trees, r.top_down_trees),
            (r.storages, r.storages)
        );
        assert!(r.writer_slot_order);
        // Repacking changes no stream.
        assert_eq!(
            r.pages_with_number_or_population,
            source.pages_with_number_or_population
        );

        let is_3d = cadkit_dgn::read(&seed, &read).unwrap().models[0].is_3d;
        let mut doc = Document::default();
        doc.models.push(
            cadkit_core::Model::default()
                .with_is_3d(is_3d)
                .with_entities(vec![Entity::new(EntityKind::Line {
                    start: cadkit_core::Point3::new(0.0, 0.0, 0.0),
                    end: cadkit_core::Point3::new(1.0, 2.0, 0.0),
                })]),
        );
        let options = cadkit_dgn::WriteOptions::default()
            .with_clear_seed_model(true)
            .with_timestamp_ms(1.7e12);
        let written = cadkit_dgn::write_v8(&doc, &seed, &options).unwrap();
        let w = check_structure(&written, &read).unwrap();
        assert!(w.is_clean(), "{rel}: {:?}", w.warnings);
        assert_eq!(
            (w.red_black_trees, w.top_down_trees),
            (w.storages, w.storages)
        );
        assert_eq!(w.irregular_free_entries, 0);
        assert!(w.writer_slot_order);
        // The model header's first prefix dword counts the graphic and control pages that
        // hold records.
        assert_eq!(
            model_page_counter(&written),
            filled_pages(&written),
            "{rel}"
        );
        // The output is a seed again.
        let again = cadkit_dgn::write_v8(&doc, &written, &options).unwrap();
        assert!(check_structure(&again, &read).unwrap().is_clean());
    }
}

/// First dword of the first model's header stream (`Dgn~Mh`, zlib from offset 0).
#[cfg(feature = "write")]
fn model_page_counter(bytes: &[u8]) -> u32 {
    use std::io::Read;
    let mut cf = cfb::CompoundFile::open(std::io::Cursor::new(bytes)).unwrap();
    let mut raw = Vec::new();
    cf.open_stream("Dgn-Md/#000000/Dgn~Mh")
        .unwrap()
        .read_to_end(&mut raw)
        .unwrap();
    let mut inflated = Vec::new();
    flate2::read::ZlibDecoder::new(raw.as_slice())
        .read_to_end(&mut inflated)
        .unwrap();
    u32::from_le_bytes(inflated[..4].try_into().unwrap())
}

/// Graphic and control pages of the first model that hold records.
#[cfg(feature = "write")]
fn filled_pages(bytes: &[u8]) -> u32 {
    let native = cadkit_dgn::native::v8::read(bytes, &ReadOptions::default()).unwrap();
    let model = &native.models[0];
    model
        .graphic_pages
        .iter()
        .chain(&model.control_pages)
        .filter(|p| p.header.record_count > 0)
        .count() as u32
}

/// Structural invariants of the private corpus; with the 567 files measured for
/// FORMAT_NOTES ("Compound file") the totals are asserted as well.
#[test]
fn private_corpus_structure() {
    let read = ReadOptions::default();
    let mut files = 0u32;
    let mut totals = [0u32; 6];
    let mut unbalanced_files = 0u32;
    let mut free_entry_files = 0u32;
    for path in common::private_dgn_paths() {
        let bytes = std::fs::read(&path).unwrap();
        let r = check_structure(&bytes, &read).unwrap();
        files += 1;
        assert!(r.red_black_trees <= r.storages && r.top_down_trees <= r.storages);
        // No observed file has an unordered tree, a numbered page header, a record without
        // a time, a split dictionary page or a model page counter outside its range.
        for code in codes(&r) {
            match code {
                "dgn.cfb_tree_unbalanced" => unbalanced_files += 1,
                "dgn.cfb_free_entry" => free_entry_files += 1,
                other => panic!("unexpected warning {other}"),
            }
        }
        assert_eq!(r.pages_with_number_or_population, 0);
        assert!(!r.writer_slot_order);
        for (total, n) in totals.iter_mut().zip([
            r.storages,
            r.red_black_trees,
            r.top_down_trees,
            u32::from(r.top_down_trees == r.storages),
            r.irregular_free_entries,
            r.pages,
        ]) {
            *total += n;
        }
    }
    if files == 567 {
        assert_eq!(totals, [5_372, 5_363, 5_323, 524, 6, 8_905]);
        assert_eq!((unbalanced_files, free_entry_files), (9, 5));
    }
    if files > 0 {
        eprintln!(
            "structure: {files} files, totals {totals:?}, {unbalanced_files} with unbalanced trees, \
             {free_entry_files} with irregular free slots"
        );
    }
}

/// Elements and tags whose `dgn.color_index` or `dgn.weight` is the level sentinel.
#[cfg(feature = "write")]
fn level_sentinels(doc: &Document) -> usize {
    let sentinel = Value::Int(0xffff_ffff);
    let mut todo: Vec<&Entity> = doc.models.iter().flat_map(|m| &m.entities).collect();
    let mut n = 0;
    while let Some(e) = todo.pop() {
        if e.props.get("dgn.color_index") == Some(&sentinel)
            || e.props.get("dgn.weight") == Some(&sentinel)
        {
            n += 1;
        }
        if let EntityKind::Group { children, .. } = &e.kind {
            todo.extend(children);
        }
    }
    n
}

/// Every private file holding the level sentinels is read and written back into itself;
/// no write fails on a sentinel, and each output holds as many sentinel elements. (Before,
/// 51 of the 54 such files failed with "extended DGN color table entries".)
#[cfg(feature = "write")]
#[test]
fn private_level_sentinels_are_written_back() {
    let read = ReadOptions::default();
    let options = cadkit_dgn::WriteOptions::default().with_clear_seed_model(true);
    let (mut files, mut written) = (0u32, 0u32);
    for path in common::private_dgn_paths() {
        let bytes = std::fs::read(&path).unwrap();
        let doc = cadkit_dgn::read(&bytes, &read).unwrap();
        let count = level_sentinels(&doc);
        if count == 0 {
            continue;
        }
        files += 1;
        match cadkit_dgn::write_v8(&doc, &bytes, &options) {
            Ok(out) => {
                written += 1;
                let back = cadkit_dgn::read(&out, &read).unwrap();
                assert_eq!(level_sentinels(&back), count);
                assert!(check_structure(&out, &read).unwrap().is_clean());
            }
            Err(e) => {
                let message = e.to_string();
                assert!(
                    !message.contains("extended DGN color") && !message.contains("above 31"),
                    "a level sentinel was rejected"
                );
            }
        }
    }
    if files > 0 {
        assert!(written > 0);
        eprintln!("level sentinels: {written} of {files} files written back");
    }
}
