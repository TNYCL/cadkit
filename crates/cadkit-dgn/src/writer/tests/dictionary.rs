//! Dictionary (`Dgn^Nm`) pages keep the seed's layout: unchanged pages keep their bytes,
//! changed pages are rewritten in place with page number and population 0, new tables and
//! tag sets go to a new page, and no table or complex element spans pages, whatever its
//! size. Seed pages are built here the way MicroStation stores them (complex header and
//! component flags, `{count, version, 0, 0}` page headers).

use super::*;
use std::collections::BTreeMap;
use std::path::Component;

/// Type word flags of MicroStation table headers and of their entries.
const HEADER: u32 = 0x2440_0000;
const ENTRY: u32 = 0x4440_0000;

/// A level entry with a 240-byte fixed part: level id at `0x20`, code at `0x5C`, name in
/// string linkage 1.
fn level(id: u64, level: u32, code: u32, name: &str) -> Vec<u8> {
    let mut b = prefix(ENTRY | 95, 0xf0, id, 1, 0.).unwrap();
    u32_at(&mut b, 0x20, level).unwrap();
    u32_at(&mut b, 0x24, u32::MAX).unwrap();
    u32_at(&mut b, 0x5c, code).unwrap();
    finish(b, &string_link(1, name).unwrap()).unwrap()
}

/// A table header (type 96) of `table` with `count` entries.
fn table(id: u64, table: u32, count: u32) -> Vec<u8> {
    let mut b = prefix(HEADER | 96, 0x28, id, table, 0.).unwrap();
    u32_at(&mut b, 0x20, count).unwrap();
    finish(b, &[]).unwrap()
}

/// An entry of a table other than the level and font tables.
fn entry(id: u64, table: u32, size: usize) -> Vec<u8> {
    finish(prefix(ENTRY | 95, size, id, table, 0.).unwrap(), &[]).unwrap()
}

/// A page as MicroStation writes it: page number and population 0, an empty page is the
/// header alone.
fn ms_page(records: &[Vec<u8>], version: u32) -> Vec<u8> {
    if records.is_empty() {
        let mut out = vec![0; 16];
        out[4..8].copy_from_slice(&version.to_le_bytes());
        return out;
    }
    let mut out = page(records, version, 0).unwrap();
    out[12..16].copy_from_slice(&0u32.to_le_bytes());
    out
}

/// An XAttribute page holding one record per element id.
fn aux(ids: &[u64]) -> Vec<u8> {
    let mut data = vec![];
    for id in ids {
        let payload = id.to_le_bytes();
        let mut head = vec![0; 28];
        u32_at(&mut head, 0, 0xa11b).unwrap();
        u32_at(&mut head, 4, payload.len() as u32).unwrap();
        u32_at(&mut head, 8, 170).unwrap();
        u64_at(&mut head, 16, *id).unwrap();
        data.extend(head);
        data.extend(payload);
    }
    let mut out = vec![0; 16];
    u32_at(&mut out, 0, ids.len() as u32).unwrap();
    u32_at(&mut out, 4, 2).unwrap();
    out.extend(compressed(&data).unwrap());
    out
}

/// The synthetic 2D seed with the given `Dgn^Nm` and `Dgn^NmA` pages (`$n`, stream bytes).
fn seed_with(named: &[(u32, Vec<u8>)], named_aux: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut file = ::cfb::CompoundFile::open(Cursor::new(seed(false))).unwrap();
    file.create_storage_all("Dgn^Nm").unwrap();
    for (n, bytes) in named {
        file.create_stream(format!("Dgn^Nm/${n}"))
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }
    if !named_aux.is_empty() {
        file.create_storage_all("Dgn^NmA").unwrap();
        for (n, bytes) in named_aux {
            file.create_stream(format!("Dgn^NmA/${n}"))
                .unwrap()
                .write_all(bytes)
                .unwrap();
        }
    }
    file.flush().unwrap();
    file.into_inner().into_inner()
}

/// Every stream under `storage` (`Dgn^Nm`), by its path joined with `/`.
fn streams(bytes: &[u8], storage: &str) -> BTreeMap<String, Vec<u8>> {
    let mut cf = ::cfb::CompoundFile::open(Cursor::new(bytes)).unwrap();
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

/// The page header fields `{count, version, number, population}` of a stream.
fn header(stream: &[u8]) -> [u32; 4] {
    [0, 4, 8, 12].map(|o| le::u32_at(stream, o).unwrap())
}

/// Asserts the layout rules on a written file and returns its native view: no dictionary
/// page starts with a complex component, every complex table header counts the components
/// that follow it on its page, and every `Dgn^NmA/$n` XAttribute belongs to an element of
/// `Dgn^Nm/$n`.
fn check_layout(bytes: &[u8]) -> v8::V8File {
    let native = v8::read(bytes, &ReadOptions::default()).unwrap();
    for page in &native.named_pages {
        assert!(page.complete, "{}", page.path);
        assert!(
            page.elements
                .first()
                .is_none_or(|e| !e.is_complex_component()),
            "{} starts inside a complex element",
            page.path
        );
        for (i, e) in page.elements.iter().enumerate() {
            if e.type_code() == 96 && e.is_complex_header() {
                let components = page.elements[i + 1..]
                    .iter()
                    .take_while(|c| c.is_complex_component())
                    .count();
                assert_eq!(le::u32_at(&e.bytes, 0x20), Some(components as u32));
            }
        }
    }
    for aux in &native.named_aux {
        let page = native
            .named_pages
            .iter()
            .find(|p| p.number == aux.number)
            .unwrap();
        for r in &aux.records {
            assert!(
                page.elements.iter().any(|e| e.id() == Some(r.element_id)),
                "XAttribute of {} outside its element's page",
                r.element_id
            );
        }
    }
    native
}

/// Element ids of every dictionary page, by page number.
fn ids(native: &v8::V8File) -> BTreeMap<u32, Vec<u64>> {
    native
        .named_pages
        .iter()
        .map(|p| (p.number, p.elements.iter().filter_map(|e| e.id()).collect()))
        .collect()
}

/// The seed's document with one line on each named level.
fn lines_on(seed: &[u8], levels: &[&str]) -> Document {
    let mut doc = crate::read(seed, &ReadOptions::default()).unwrap();
    doc.models[0].entities = levels
        .iter()
        .map(|name| {
            let mut e = line();
            e.layer = Some((*name).into());
            e
        })
        .collect();
    doc
}

/// Three seed levels and two other tables on four pages, one of them empty and one with
/// the page number and population cadkit wrote before 0.3.5; XAttributes on two pages.
fn layered_seed() -> Vec<u8> {
    seed_with(
        &[
            (
                1,
                ms_page(
                    &[table(10, 19, 2), entry(11, 19, 0x40), entry(12, 19, 0x40)],
                    2,
                ),
            ),
            (2, ms_page(&[], 2)),
            (
                3,
                ms_page(
                    &[
                        table(20, 1, 3),
                        level(21, 1, 100, "Frame"),
                        level(22, 2, 112, "Area"),
                        level(23, 64, 0, "Default"),
                    ],
                    3,
                ),
            ),
            (
                4,
                page(&[table(30, 7, 1), entry(31, 7, 0x30)], 2, 4).unwrap(),
            ),
        ],
        &[(1, aux(&[10])), (3, aux(&[21, 22]))],
    )
}

#[test]
fn unchanged_dictionary_streams_keep_their_bytes() {
    let seed = layered_seed();
    let bytes = write_v8(
        &lines_on(&seed, &["Frame", "Area"]),
        &seed,
        &WriteOptions::default(),
    )
    .unwrap();
    assert_eq!(streams(&bytes, "Dgn^Nm"), streams(&seed, "Dgn^Nm"));
    assert_eq!(streams(&bytes, "Dgn^NmA"), streams(&seed, "Dgn^NmA"));
    check_layout(&bytes);
    let parsed = crate::read(&bytes, &ReadOptions::default()).unwrap();
    let layers: Vec<_> = parsed.models[0]
        .entities
        .iter()
        .map(|e| e.layer.clone().unwrap())
        .collect();
    assert_eq!(layers, ["Frame", "Area"]);
}

#[test]
fn a_new_level_joins_its_table_page_with_a_fresh_code() {
    let seed = layered_seed();
    let bytes = write_v8(
        &lines_on(&seed, &["Frame", "Wall"]),
        &seed,
        &WriteOptions::default(),
    )
    .unwrap();
    let (before, after) = (streams(&seed, "Dgn^Nm"), streams(&bytes, "Dgn^Nm"));
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "no page is added or removed"
    );
    for n in [1, 2, 4] {
        let key = format!("Dgn^Nm/${n}");
        assert_eq!(before[&key], after[&key], "{key} is unchanged");
    }
    assert_eq!(streams(&bytes, "Dgn^NmA"), streams(&seed, "Dgn^NmA"));
    // The level page is rewritten in place: its own version, number and population 0.
    assert_eq!(header(&after["Dgn^Nm/$3"]), [5, 3, 0, 0]);
    let native = check_layout(&bytes);
    let page = native.named_pages.iter().find(|p| p.number == 3).unwrap();
    assert_eq!(
        page.elements[..4]
            .iter()
            .map(|e| e.id())
            .collect::<Vec<_>>(),
        [Some(20), Some(21), Some(22), Some(23)]
    );
    let new = &page.elements[4];
    assert_eq!(new.type_word, ENTRY | 95, "flags of its siblings");
    assert_eq!(le::u32_at(&new.bytes, 0x5c), Some(113), "next free code");
    assert_eq!(le::u32_at(&page.elements[0].bytes, 0x20), Some(4));
    let parsed = crate::read(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(parsed.models[0].entities[1].layer.as_deref(), Some("Wall"));
}

/// A level table larger than the 256 KiB graphic page size, grown further by new levels.
#[test]
fn a_table_past_256_kib_stays_on_one_page_up_to_the_decompression_limit() {
    let mut records = vec![table(10, 1, 900)];
    for i in 0..900u32 {
        records.push(level(
            100 + u64::from(i),
            i + 1,
            1000 + i,
            &format!("Seed {i:03}"),
        ));
    }
    let seed = seed_with(
        &[
            (1, ms_page(&[table(5, 19, 1), entry(6, 19, 0x40)], 2)),
            (2, ms_page(&records, 3)),
        ],
        &[],
    );
    let seed_len: u64 = v8::read(&seed, &ReadOptions::default())
        .unwrap()
        .named_pages
        .iter()
        .map(|p| p.inflated_len as u64)
        .sum();
    let names: Vec<String> = (0..1000).map(|i| format!("New {i:04}")).collect();
    let mut doc = lines_on(&seed, &["Seed 000"]);
    doc.layers.extend(names.iter().map(|n| {
        let mut layer = cadkit_core::Layer::new(n.clone());
        layer.visible = false;
        layer.plottable = false;
        layer
    }));
    doc.models[0].entities[0].layer = Some(names[999].clone());

    let bytes = write_v8(&doc, &seed, &WriteOptions::default()).unwrap();
    let native = check_layout(&bytes);
    assert_eq!(
        native
            .named_pages
            .iter()
            .map(|p| p.number)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    let page = &native.named_pages[1];
    assert!(page.inflated_len > 256 * 1024);
    assert_eq!(page.elements.len(), 1901);
    assert_eq!(le::u32_at(&page.elements[0].bytes, 0x20), Some(1900));
    let codes: BTreeSet<_> = page.elements[1..]
        .iter()
        .map(|e| le::u32_at(&e.bytes, 0x5c).unwrap())
        .collect();
    assert_eq!(codes.len(), 1900, "level codes stay unique");
    assert_eq!(
        streams(&bytes, "Dgn^Nm")["Dgn^Nm/$1"],
        streams(&seed, "Dgn^Nm")["Dgn^Nm/$1"]
    );
    let parsed = crate::read(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(
        parsed.models[0].entities[0].layer.as_deref(),
        Some("New 0999")
    );

    // Only the decompression limit bounds a page: past it the write fails, nothing splits.
    let options = WriteOptions {
        limits: Limits::default().with_max_decompressed_bytes(2 * seed_len),
        ..Default::default()
    };
    assert!(seed_len < 256 * 1024 && 2 * seed_len < (page.inflated_len as u64));
    assert!(matches!(
        write_v8(&doc, &seed, &options),
        Err(Error::LimitExceeded(what)) if what == "DGN dictionary records"
    ));
}

#[test]
fn new_tables_and_tag_sets_go_to_one_new_page() {
    // Seed with dictionary pages but no level table and no tag set.
    let seed = seed_with(
        &[
            (1, ms_page(&[table(5, 19, 1), entry(6, 19, 0x40)], 2)),
            (2, ms_page(&[], 2)),
        ],
        &[],
    );
    let mut e = line();
    e.layer = Some("Rooms".into());
    e.attributes.push(
        Attribute::new("Name", Value::Text("Hall".into()))
            .with_set("Room")
            .with_invisible(true),
    );
    let bytes = write_v8(&document(false, vec![e]), &seed, &WriteOptions::default()).unwrap();
    let (before, after) = (streams(&seed, "Dgn^Nm"), streams(&bytes, "Dgn^Nm"));
    for key in before.keys() {
        assert_eq!(before[key], after[key], "{key} is unchanged");
    }
    assert_eq!(after.len(), 3);
    assert_eq!(header(&after["Dgn^Nm/$3"])[2..], [0, 0]);
    let native = check_layout(&bytes);
    let added: Vec<_> = native.named_pages[2]
        .elements
        .iter()
        .map(|e| (e.type_code(), le::u32_at(&e.bytes, 12).unwrap()))
        .collect();
    // Level table header and its entry, then the tag set.
    assert_eq!(added, [(96, 1), (95, 1), (39, 0)]);
    let parsed = crate::read(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(parsed.models[0].entities[0].layer.as_deref(), Some("Rooms"));
    assert_eq!(parsed.models[0].entities[0].attributes.len(), 1);
}

#[test]
fn a_seed_without_dictionary_pages_gets_one_page_past_256_kib() {
    let layers: Vec<String> = (0..4000).map(|i| format!("Level name {i:05}")).collect();
    let entities = layers
        .iter()
        .step_by(1000)
        .map(|name| {
            let mut e = line();
            e.layer = Some(name.clone());
            e
        })
        .collect();
    let mut doc = document(false, entities);
    doc.layers = layers
        .iter()
        .map(|n| {
            let mut layer = cadkit_core::Layer::new(n.clone());
            layer.visible = false;
            layer.plottable = false;
            layer
        })
        .collect();
    let bytes = write_v8(&doc, &seed(false), &WriteOptions::default()).unwrap();
    let native = check_layout(&bytes);
    assert_eq!(native.named_pages.len(), 1);
    let page = &native.named_pages[0];
    assert!(page.inflated_len > 256 * 1024);
    assert_eq!(page.elements.len(), 4001);
    assert_eq!(page.elements[0].type_code(), 96);
    assert_eq!(le::u32_at(&page.elements[0].bytes, 0x20), Some(4000));
    assert_eq!(header(&streams(&bytes, "Dgn^Nm")["Dgn^Nm/$1"])[2..], [0, 0]);
    let parsed = crate::read(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(parsed.layers.len(), 4000);
}

/// A seed written by a writer that split a level table and a cell table over pages (as
/// cadkit did before 0.3.5): the components rejoin their header's page, and their
/// XAttributes follow them.
#[test]
fn split_seed_tables_rejoin_their_header_page() {
    let seed = seed_with(
        &[
            (
                1,
                page(
                    &[
                        table(10, 19, 1),
                        entry(11, 19, 0x40),
                        table(20, 1, 4),
                        level(21, 1, 100, "A"),
                        level(22, 2, 101, "B"),
                    ],
                    2,
                    1,
                )
                .unwrap(),
            ),
            (
                2,
                page(
                    &[
                        level(23, 3, 102, "C"),
                        level(24, 64, 0, "Default"),
                        table(30, 8, 3),
                        entry(31, 8, 0x40),
                    ],
                    2,
                    2,
                )
                .unwrap(),
            ),
            (
                3,
                page(
                    &[entry(32, 8, 0x40), entry(33, 8, 0x40), table(40, 6, 0)],
                    2,
                    3,
                )
                .unwrap(),
            ),
        ],
        &[(2, aux(&[23, 30])), (3, aux(&[32, 40]))],
    );
    let bytes = write_v8(
        &lines_on(&seed, &["A", "C"]),
        &seed,
        &WriteOptions::default(),
    )
    .unwrap();
    let native = check_layout(&bytes);
    assert_eq!(
        ids(&native),
        BTreeMap::from([
            (1, vec![10, 11, 20, 21, 22, 23, 24]),
            (2, vec![30, 31, 32, 33]),
            (3, vec![40]),
        ])
    );
    let xattributes: BTreeMap<u32, Vec<u64>> = native
        .named_aux
        .iter()
        .map(|p| (p.number, p.records.iter().map(|r| r.element_id).collect()))
        .collect();
    assert_eq!(
        xattributes,
        BTreeMap::from([(1, vec![23]), (2, vec![30, 32]), (3, vec![40])])
    );
    let after = streams(&bytes, "Dgn^Nm");
    for n in 1..=3 {
        assert_eq!(header(&after[&format!("Dgn^Nm/${n}")])[2..], [0, 0]);
    }
    let parsed = crate::read(&bytes, &ReadOptions::default()).unwrap();
    let layers: Vec<_> = parsed.models[0]
        .entities
        .iter()
        .map(|e| e.layer.clone().unwrap())
        .collect();
    assert_eq!(layers, ["A", "C"]);
}

/// The second level table of a post-processed seed (0.3.2) joins the first one after its
/// last entry, on its page; its header and that header's XAttribute are dropped.
#[test]
fn a_second_level_table_joins_the_first_on_its_page() {
    let seed = seed_with(
        &[
            (
                1,
                ms_page(
                    &[
                        table(90, 1, 3),
                        level(100, 1, 100, "Frame"),
                        level(101, 2, 101, "Area"),
                        level(102, 64, 0, "Default"),
                        table(110, 19, 0),
                    ],
                    3,
                ),
            ),
            (
                2,
                ms_page(
                    &[
                        table(200, 1, 2),
                        level(201, 1, 199, "Temp"),
                        level(202, 2, 142, "Surface"),
                    ],
                    3,
                ),
            ),
        ],
        &[(2, aux(&[200, 201]))],
    );
    let bytes = write_v8(
        &lines_on(&seed, &["Frame", "Area"]),
        &seed,
        &WriteOptions::default(),
    )
    .unwrap();
    let native = check_layout(&bytes);
    assert_eq!(
        ids(&native),
        BTreeMap::from([(1, vec![90, 100, 101, 102, 201, 202, 110]), (2, vec![])])
    );
    assert_eq!(
        header(&streams(&bytes, "Dgn^Nm")["Dgn^Nm/$2"]),
        [0, 3, 0, 0]
    );
    let xattributes: BTreeMap<u32, Vec<u64>> = native
        .named_aux
        .iter()
        .map(|p| (p.number, p.records.iter().map(|r| r.element_id).collect()))
        .collect();
    assert_eq!(xattributes, BTreeMap::from([(1, vec![201]), (2, vec![])]));
    let parsed = crate::read(&bytes, &ReadOptions::default()).unwrap();
    let layers: Vec<_> = parsed.models[0]
        .entities
        .iter()
        .map(|e| e.layer.clone().unwrap())
        .collect();
    assert_eq!(layers, ["Frame", "Area"]);
}

/// The public GDAL sample, used as a seed without new levels or tag sets, keeps its
/// dictionary streams byte for byte.
#[test]
fn public_seed_dictionary_is_kept_byte_for_byte() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/public/gdal/test_dgnv8.dgn");
    let Ok(seed) = std::fs::read(path) else {
        return;
    };
    let mut doc = crate::read(&seed, &ReadOptions::default()).unwrap();
    let mut e = line();
    e.layer = doc.layers.first().map(|l| l.name.clone());
    // The reader also infers placeholder layers for undeclared level references in the
    // original graphics. This fixture replaces those graphics and requests no new levels:
    // retain only the chosen seed-defined level, rather than asking the writer to add them.
    doc.layers
        .retain(|layer| Some(layer.name.as_str()) == e.layer.as_deref());
    assert_eq!(doc.layers.len(), 1);
    doc.models[0].entities = vec![e];
    let options = WriteOptions {
        clear_seed_model: true,
        ..Default::default()
    };
    let bytes = write_v8(&doc, &seed, &options).unwrap();
    assert_eq!(streams(&bytes, "Dgn^Nm"), streams(&seed, "Dgn^Nm"));
    assert_eq!(streams(&bytes, "Dgn^NmA"), streams(&seed, "Dgn^NmA"));
}
