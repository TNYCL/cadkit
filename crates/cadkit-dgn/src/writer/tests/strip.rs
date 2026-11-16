//! Seed stripping ([`strip_seed_v8`]) of synthetic seeds: what goes, what stays byte for byte,
//! and that the result is a seed the writer fills.

use super::raster_template::{image, template_seed};
use super::*;
use crate::cfb_tree::{directory_entry_offsets, reachable_slots};
use crate::native::v8::tests::{element, page as native_page};

const AUTHOR: &str = "AuthorMarker-7f3a";
const COMPANY: &str = "CompanyMarker-91c2";
const CUSTOM: &[u8] = b"CustomPropertySetMarker-55e1";

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// The synthetic seed filled with lines, a level and a tag set, and property sets naming an
/// author and a company plus a custom property set.
fn dirty_seed() -> (Vec<u8>, usize) {
    let mut lines = vec![];
    for i in 0..3 {
        let mut e = line();
        e.layer = Some("Walls".into());
        e.attributes
            .push(Attribute::new("Room", Value::Text(format!("R{i}"))).with_set("Rooms"));
        lines.push(e);
    }
    let options = WriteOptions::default().with_timestamp_ms(1.6e12);
    let filled = write_v8(&document(true, lines), &seed(true), &options).unwrap();
    let graphics = v8::read(&filled, &ReadOptions::default()).unwrap().models[0]
        .graphic_pages
        .iter()
        .map(|p| p.elements.len())
        .sum();
    let mut sets = BTreeMap::new();
    sets.insert(
        "\u{5}SummaryInformation".to_string(),
        Some(oleps::test_stream(
            false,
            &[(0x04, AUTHOR), (0x08, AUTHOR), (0x12, "Old App")],
        )),
    );
    sets.insert(
        "\u{5}DocumentSummaryInformation".to_string(),
        Some(oleps::test_stream(true, &[(0x0f, COMPANY)])),
    );
    sets.insert("\u{5}CustomSet".to_string(), Some(CUSTOM.to_vec()));
    let dirty = container::rewrite(&filled, &sets, &ReadOptions::default()).unwrap();
    assert!(contains(&dirty, &utf16(AUTHOR)) && contains(&dirty, CUSTOM));
    (dirty, graphics)
}

fn streams(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    container::read_streams(bytes, &ReadOptions::default()).unwrap()
}

fn dictionary(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    streams(bytes)
        .into_iter()
        .filter(|(k, _)| k.starts_with("Dgn^Nm"))
        .collect()
}

fn strip(seed: &[u8], options: &StripSeedOptions) -> StrippedSeed {
    strip_seed_v8(seed, options, &ReadOptions::default()).unwrap()
}

#[test]
fn stripping_removes_graphics_and_identifying_properties() {
    let read = ReadOptions::default();
    let (dirty, graphics) = dirty_seed();
    assert!(graphics >= 6, "lines and their tags");
    let out = strip(&dirty, &StripSeedOptions::default());
    let bytes = &out.bytes;
    // Nothing of the removed property sets survives, in streams or in free space.
    for marker in [
        utf16(AUTHOR),
        utf16(COMPANY),
        CUSTOM.to_vec(),
        utf16("Old App"),
    ] {
        assert!(!contains(bytes, &marker));
    }
    let r = &out.report;
    assert_eq!(r.removed_graphic_records, graphics as u64);
    assert_eq!(r.removed_property_sets, 3);
    assert_eq!(r.removed_summary_properties, vec![0x04, 0x08]);
    assert_eq!(r.removed_document_summary_properties, vec![0x0f]);
    let levels = V8Seed::parse(&dirty, &read).unwrap().levels().len() as u64;
    assert!(levels >= 1);
    assert_eq!((r.levels, r.named_levels, r.tag_sets), (levels, levels, 1));
    assert_eq!((r.kept_control_records, r.kept_raster_attachments), (0, 0));
    assert_eq!(r.dictionary_pages_rewritten, 0);
    assert_eq!(r.unknown_streams, 0);
    assert!(r.model_name_from_seed);
    assert!(r.warnings.is_empty());

    let native = v8::read(bytes, &read).unwrap();
    let model = &native.models[0];
    assert!(model.graphic_pages.iter().all(|p| p.elements.is_empty()));
    assert!(model.control_pages.is_empty() && model.graphic_aux.is_empty());
    assert_eq!(model.header.as_ref().unwrap().extents, [0; 6]);
    assert_eq!(native.summary.codepage, Some(1200));
    assert_eq!(native.summary.application.as_deref(), Some("cadkit"));
    let paths: Vec<&str> = native.streams.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(
        paths.iter().filter(|p| p.starts_with('\u{5}')).count(),
        2,
        "{paths:?}"
    );
    // The dictionary (level table, tag set) is the seed's, byte for byte.
    assert_eq!(dictionary(bytes), dictionary(&dirty));
    assert!(crate::check_structure(bytes, &read).unwrap().is_clean());
    ::cfb::CompoundFile::open_strict(Cursor::new(bytes)).unwrap();

    // A seed the writer fills without clearing anything; the level and tag set are reused.
    let parsed = V8Seed::parse(bytes, &read).unwrap();
    assert!(!parsed.is_nonempty());
    let mut e = line();
    e.layer = Some("Walls".into());
    let written = write_v8(&document(true, vec![e]), bytes, &WriteOptions::default()).unwrap();
    let doc = crate::read(&written, &read).unwrap();
    assert_eq!(doc.models[0].entities.len(), 1);
    assert_eq!(doc.layers.len(), 1);

    // Deterministic and idempotent.
    assert_eq!(strip(&dirty, &StripSeedOptions::default()).bytes, *bytes);
    let again = strip(bytes, &StripSeedOptions::default());
    assert_eq!(again.bytes, *bytes);
    assert_eq!(again.report.removed_graphic_records, 0);
    assert_eq!(again.report.removed_property_sets, 2);
    assert!(again.report.removed_summary_properties.is_empty());
}

#[test]
fn times_and_model_name_follow_the_options() {
    let read = ReadOptions::default();
    let (dirty, _) = dirty_seed();
    // A model index entry: storage 0, the name, a description and a 4-byte tail.
    let mut entry = vec![0u8; 32];
    let (name, description) = (utf16("Seed model"), utf16("Desc"));
    let size = 32 + name.len() + description.len() + 4;
    entry[16..18].copy_from_slice(&(size as u16).to_le_bytes());
    entry[18..20].copy_from_slice(&(name.len() as u16).to_le_bytes());
    entry[20..24].copy_from_slice(&(description.len() as u32).to_le_bytes());
    entry.extend(&name);
    entry.extend(&description);
    entry.extend([9, 9, 9, 9]);
    let mut mix = vec![];
    for v in [0xaa00_ba11u32, 4, 1, 0] {
        mix.extend(v.to_le_bytes());
    }
    mix.extend(&entry);
    let mut add = BTreeMap::new();
    add.insert(
        "Dgn^Ix/Dgn~Mix".to_string(),
        Some(compressed(&mix).unwrap()),
    );
    let indexed = container::rewrite(&dirty, &add, &read).unwrap();

    let options = StripSeedOptions::new()
        .with_timestamp_ms(1.7e12)
        .with_model_name(Some("Blank"))
        .with_application("");
    let out = strip(&indexed, &options);
    assert!(out.report.warnings.is_empty(), "{:?}", out.report.warnings);
    assert_eq!(out.report.timestamp_ms, 1.7e12);
    assert!(!out.report.model_name_from_seed && out.report.model_description);
    let parsed = V8Seed::parse(&out.bytes, &read).unwrap();
    assert_eq!(parsed.seed_timestamp_ms, 1.7e12);
    let header = parsed.model_header().unwrap();
    assert_eq!(header.name.as_deref(), Some("Blank"));
    assert_eq!(le::f64_at(&header.raw, 0x88), Some(1.7e12));
    // Unit labels and every other linkage stay.
    assert_eq!(header.master.label.as_deref(), Some("m"));
    assert_eq!(header.sub.label.as_deref(), Some("mm"));
    let index = &parsed.native.model_index[0];
    assert_eq!(
        (index.name.as_str(), index.description.as_str()),
        ("Blank", "Desc")
    );
    // MS-CFB 2.6.1: storages carry the time, the root its modification time only (its
    // creation time MUST be zero), streams none; the summary holds the code page and the
    // times only.
    let filetime = oleps::filetime(1.7e12).unwrap();
    let offsets = directory_entry_offsets(&out.bytes).unwrap();
    let mut kinds = std::collections::BTreeSet::new();
    for slot in reachable_slots(&out.bytes, &offsets) {
        let off = offsets[slot as usize];
        let kind = out.bytes[off + 0x42];
        kinds.insert(kind);
        let (created, modified) = match kind {
            5 => (0, filetime),
            1 => (filetime, filetime),
            _ => (0, 0),
        };
        assert_eq!(le::u64_at(&out.bytes, off + 0x64), Some(created), "{kind}");
        assert_eq!(le::u64_at(&out.bytes, off + 0x6c), Some(modified), "{kind}");
    }
    assert_eq!(kinds, std::collections::BTreeSet::from([1, 2, 5]));
    ::cfb::CompoundFile::open_strict(Cursor::new(&out.bytes)).unwrap();
    let summary = &streams(&out.bytes)["\u{5}SummaryInformation"];
    assert_eq!(oleps::property_ids(summary), vec![1, 0x0c, 0x0d]);
    assert_eq!(
        v8::read(&out.bytes, &read).unwrap().summary.application,
        None
    );

    // A time older than the kept records (written at 1.6e12) is raised to the newest one.
    let old = strip(&indexed, &StripSeedOptions::new().with_timestamp_ms(1.0));
    assert_eq!(old.report.timestamp_ms, 1.6e12);
    // Without a time, the seed's file header time (here none) is used: zero entry times.
    let plain = strip(&indexed, &StripSeedOptions::new());
    assert_eq!(plain.report.timestamp_ms, 0.0);
    let plain_offsets = directory_entry_offsets(&plain.bytes).unwrap();
    assert_eq!(le::u64_at(&plain.bytes, plain_offsets[0] + 0x64), Some(0));
    assert_eq!(le::u64_at(&plain.bytes, plain_offsets[0] + 0x6c), Some(0));
    // Without a model index, the model header is still renamed and a warning says so.
    let unindexed = strip(
        &dirty,
        &StripSeedOptions::new().with_model_name(Some("Blank")),
    );
    let codes: Vec<&str> = unindexed
        .report
        .warnings
        .iter()
        .map(|w| w.code.as_str())
        .collect();
    assert_eq!(codes, ["dgn.strip_model_index"]);
    let header = V8Seed::parse(&unindexed.bytes, &read).unwrap();
    assert_eq!(
        header.model_header().unwrap().name.as_deref(),
        Some("Blank")
    );
}

#[test]
fn controls_and_raster_attachments_stay_only_on_request() {
    let read = ReadOptions::default();
    let template = template_seed(true);
    assert!(
        V8Seed::parse(&template, &read)
            .unwrap()
            .has_raster_template()
    );
    let out = strip(&template, &StripSeedOptions::default());
    let r = &out.report;
    assert_eq!(
        (r.removed_graphic_records, r.removed_control_records),
        (2, 9)
    );
    assert_eq!(r.removed_auxiliary_records, 1);
    assert_eq!(
        r.removed_control_types,
        BTreeMap::from([(90, 1), (91, 5), (92, 1), (93, 2)])
    );
    assert!(!contains(&out.bytes, b"old.tif") && !contains(&out.bytes, &utf16("old.tif")));
    assert!(
        !V8Seed::parse(&out.bytes, &read)
            .unwrap()
            .has_raster_template()
    );

    let kept = strip(
        &template,
        &StripSeedOptions::new().with_keep_raster_template(true),
    );
    let r = &kept.report;
    assert_eq!(
        (r.kept_raster_attachments, r.raster_file_references),
        (1, 1)
    );
    assert_eq!((r.removed_graphic_records, r.kept_control_records), (1, 9));
    assert!(r.removed_control_types.is_empty());
    let parsed = V8Seed::parse(&kept.bytes, &read).unwrap();
    assert!(parsed.has_raster_template() && parsed.is_nonempty());
    assert!(
        crate::check_structure(&kept.bytes, &read)
            .unwrap()
            .is_clean()
    );
    assert_eq!(
        strip(
            &kept.bytes,
            &StripSeedOptions::new().with_keep_raster_template(true)
        )
        .bytes,
        kept.bytes
    );
    // The kept group is the template of new attachments.
    let picture = image(
        "C:\\maps\\new.tif",
        Point3::new(10.0, 20.0, 0.0),
        Vec3::new(0.5, 0.0, 0.0),
        Vec3::new(0.0, 0.5, 0.0),
        [100, 50],
    );
    let options = WriteOptions::default().with_clear_seed_model(true);
    let written = write_v8(&document(true, vec![picture]), &kept.bytes, &options).unwrap();
    let doc = crate::read(&written, &read).unwrap();
    assert_eq!(doc.models[0].entities.len(), 1);

    // A model setting record (type 66 level 20) without a graphic reference.
    let mut body = vec![0u8; 0x30];
    body[0..4].copy_from_slice(&20u32.to_le_bytes());
    body[4..12].copy_from_slice(&500u64.to_le_bytes());
    let setting = element(66, &body, &[]);
    let mut add = BTreeMap::new();
    add.insert(
        "Dgn-Md/#000000/Dgn^C/$1".to_string(),
        Some(native_page(std::slice::from_ref(&setting))),
    );
    let with_setting = container::rewrite(&seed(true), &add, &read).unwrap();
    let dropped = strip(&with_setting, &StripSeedOptions::default());
    assert_eq!(
        dropped.report.removed_control_types,
        BTreeMap::from([(66, 1)])
    );
    let kept = strip(
        &with_setting,
        &StripSeedOptions::new().with_keep_controls(true),
    );
    assert_eq!(kept.report.kept_control_types, BTreeMap::from([(66, 1)]));
    let native = v8::read(&kept.bytes, &read).unwrap();
    assert_eq!(native.models[0].control_pages[0].elements[0].bytes, setting);
}

#[test]
fn invalid_options_and_seeds_are_rejected() {
    let read = ReadOptions::default();
    let plain = seed(true);
    for options in [
        StripSeedOptions::new().with_timestamp_ms(-1.0),
        StripSeedOptions::new().with_timestamp_ms(f64::NAN),
        StripSeedOptions::new().with_timestamp_ms(2e15),
        StripSeedOptions::new().with_model_name(Some("")),
        StripSeedOptions::new().with_model_name(Some("x".repeat(250))),
        StripSeedOptions::new().with_model_name(Some("a\0b")),
        StripSeedOptions::new().with_application("y".repeat(256)),
    ] {
        assert!(matches!(
            strip_seed_v8(&plain, &options, &read),
            Err(Error::Invalid { .. })
        ));
    }
    assert!(
        strip_seed_v8(
            &plain,
            &StripSeedOptions::new().with_model_name(Some("x".repeat(249))),
            &read
        )
        .is_ok()
    );
    // A second model: the writer's error, unchanged.
    let mut add = BTreeMap::new();
    add.insert(
        "Dgn-Md/#000001/Dgn~Mh".to_string(),
        Some(crate::native::v8::tests::model_header_stream(
            true, 1000.0, "Two",
        )),
    );
    let two = container::rewrite(&plain, &add, &read).unwrap();
    let expected = V8Seed::parse(&two, &read).unwrap_err().to_string();
    let got = strip_seed_v8(&two, &StripSeedOptions::default(), &read).unwrap_err();
    assert!(matches!(got, Error::Unsupported(_)));
    assert_eq!(got.to_string(), expected);
    assert!(strip_seed_v8(b"garbage", &StripSeedOptions::default(), &read).is_err());
}
