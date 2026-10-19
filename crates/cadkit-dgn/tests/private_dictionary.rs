//! Dictionary page layout on the private corpus (`corpus/private/**/*.dgn`, discovered by
//! walking the directory): every plan file, used as a seed for a one-line model, keeps its
//! dictionary (`Dgn^Nm`) pages, and no table or complex element spans pages. Skips
//! silently when the corpus is absent and asserts only counts and structural invariants
//! (AGENTS.md section 3): no names, values, paths or coordinates.
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

mod common;

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read};
use std::path::Component;

use cadkit_core::{Entity, EntityKind, Point3, ReadOptions};
use cadkit_dgn::native::v8::{self, V8File};

/// The layout rules every observed MicroStation file follows: no dictionary page starts
/// with a complex component, every complex table header counts the components after it on
/// its page, and every `Dgn^NmA/$n` XAttribute belongs to an element of `Dgn^Nm/$n`.
fn layout_holds(native: &V8File) -> bool {
    let pages_ok = native.named_pages.iter().all(|page| {
        page.complete
            && page
                .elements
                .first()
                .is_none_or(|e| !e.is_complex_component())
            && page.elements.iter().enumerate().all(|(i, e)| {
                !(e.type_code() == 96 && e.is_complex_header())
                    || u32::from_le_bytes(e.bytes[0x20..0x24].try_into().unwrap()) as usize
                        == page.elements[i + 1..]
                            .iter()
                            .take_while(|c| c.is_complex_component())
                            .count()
            })
    });
    let ids: HashMap<u64, u32> = native
        .named_pages
        .iter()
        .flat_map(|p| p.elements.iter().filter_map(|e| Some((e.id()?, p.number))))
        .collect();
    pages_ok
        && native.named_aux.iter().all(|aux| {
            aux.records
                .iter()
                .all(|r| ids.get(&r.element_id) == Some(&aux.number))
        })
}

/// Level table records: the header (type 96) and entries (type 95) of table 1.
fn is_level_record(e: &v8::RawElement) -> bool {
    matches!(e.type_code(), 95 | 96) && e.level() == Some(1)
}

/// Every stream under `storage` (`Dgn^Nm`), by its path joined with `/`.
fn streams(bytes: &[u8], storage: &str) -> BTreeMap<String, Vec<u8>> {
    let mut cf = cfb::CompoundFile::open(Cursor::new(bytes)).unwrap();
    let paths: Vec<_> = cf
        .walk()
        .filter(|e| e.is_stream())
        .map(|e| e.path().to_path_buf())
        .collect();
    let mut out = BTreeMap::new();
    for path in paths {
        let key = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/");
        if key.starts_with(&format!("{storage}/")) {
            let mut data = vec![];
            cf.open_stream(&path)
                .unwrap()
                .read_to_end(&mut data)
                .unwrap();
            out.insert(key, data);
        }
    }
    out
}

#[test]
fn private_plan_seeds_keep_their_dictionary_pages() {
    let read = ReadOptions::default();
    let mut seeds = 0usize;
    let mut kept_byte_for_byte = 0usize;
    let mut merged_level_tables = 0usize;
    for path in common::private_dgn_paths() {
        // Plan files only; their `_3d` companions are CityMax output.
        if path
            .file_stem()
            .is_some_and(|s| s.to_string_lossy().to_ascii_lowercase().ends_with("_3d"))
        {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let before = v8::read(&bytes, &read).unwrap();
        assert!(
            layout_holds(&before),
            "a corpus seed breaks the layout rules"
        );
        let mut doc = cadkit_dgn::read(&bytes, &read).unwrap();
        assert_eq!(doc.models.len(), 1);
        let mut line = Entity::new(EntityKind::Line {
            start: Point3::new(0.0, 0.0, 0.0),
            end: Point3::new(1.0, 1.0, 0.0),
        });
        line.layer = doc.layers.first().map(|l| l.name.clone());
        doc.models[0].entities = vec![line];
        // As Cady writes: the seed model is cleared.
        let options = cadkit_dgn::WriteOptions {
            clear_seed_model: true,
            ..Default::default()
        };
        let output = cadkit_dgn::write_v8(&doc, &bytes, &options).unwrap();
        let after = v8::read(&output, &read).unwrap();
        seeds += 1;
        assert!(
            layout_holds(&after),
            "a written file breaks the layout rules"
        );
        // Every seed page keeps its number; no page is added.
        let numbers = |f: &V8File| f.named_pages.iter().map(|p| p.number).collect::<Vec<_>>();
        assert_eq!(numbers(&before), numbers(&after));
        let level_tables = before
            .named_pages
            .iter()
            .flat_map(|p| &p.elements)
            .filter(|e| e.type_code() == 96 && e.level() == Some(1))
            .count();
        if level_tables == 1 {
            // Nothing changed: every dictionary stream is the seed's, byte for byte.
            assert_eq!(streams(&output, "Dgn^Nm"), streams(&bytes, "Dgn^Nm"));
            assert_eq!(streams(&output, "Dgn^NmA"), streams(&bytes, "Dgn^NmA"));
            kept_byte_for_byte += 1;
        } else {
            // Further level tables join the first one (0.3.2); every other record keeps
            // its page and order.
            merged_level_tables += 1;
            for (a, b) in before.named_pages.iter().zip(&after.named_pages) {
                let others = |p: &v8::Page| {
                    p.elements
                        .iter()
                        .filter(|e| !is_level_record(e))
                        .map(|e| e.bytes.clone())
                        .collect::<Vec<_>>()
                };
                assert!(others(a) == others(b), "a non-level record moved");
            }
            let headers = after
                .named_pages
                .iter()
                .flat_map(|p| &p.elements)
                .filter(|e| e.type_code() == 96 && e.level() == Some(1))
                .count();
            assert_eq!(headers, 1);
        }
    }
    if seeds > 0 {
        // Structural counts only.
        eprintln!(
            "dictionary layout: {seeds} plan seeds, {kept_byte_for_byte} kept byte for byte, \
             {merged_level_tables} with merged level tables"
        );
    }
}
