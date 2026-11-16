//! The contract a host relies on when it writes documents into its own seed, in one place:
//! levels by exact name, tag sets reused by name, native symbology props, write and seed
//! times, untouched dictionary pages, deterministic output, the native read API and the
//! container invariants. The seed is built here with invented names; a first write adds the
//! tag set "Set A" (text "Name", integer "Count") and a raster attachment, and its output is
//! the seed of every check.

use super::dictionary::{level, streams, table};
use super::raster_template::{image, template_seed};
use super::*;
use crate::native::decode_v8;
use crate::native::element::ElementData;
use cadkit_core::Limits;

/// Last-saved time of the seed's file header (`Dgn~H` `0x130`).
const HEADER_TIME: f64 = 1.65e12;
/// Time of the first write, which adds "Set A".
const FIRST_TIME: f64 = 1.7e12;
/// Time of the writes under test.
const WRITE_TIME: f64 = 1.75e12;

/// Seed palette: entry `i` is `[i, 3i, 200]`, unique and unlike the default palette.
fn palette() -> [[u8; 3]; 256] {
    std::array::from_fn(|i| [i as u8, (i * 3) as u8, 200])
}

/// A color table (type 5 in level slot 1): screen flag, then 256 triples, color 255 first.
fn color_table(id: u64) -> Vec<u8> {
    let mut b = prefix(5, 0x22 + 768 + 2, id, 1, 0.).unwrap();
    let p = palette();
    for (k, i) in std::iter::once(255).chain(0..255).enumerate() {
        b[0x22 + 3 * k..0x25 + 3 * k].copy_from_slice(&p[i]);
    }
    finish(b, &[]).unwrap()
}

/// A font table entry (type 95, table 2).
fn font(id: u64, number: u32, name: &str) -> Vec<u8> {
    let name: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut b = prefix(95, 0x2e + name.len(), id, 2, 0.).unwrap();
    u32_at(&mut b, 0x28, number).unwrap();
    put(&mut b, 0x2c, &(name.len() as u16).to_le_bytes()).unwrap();
    put(&mut b, 0x2e, &name).unwrap();
    finish(b, &[]).unwrap()
}

/// The raster template seed (3D) with a file header time, a palette and two fonts on
/// `Dgn^Nm/$1` and a five-level table on `Dgn^Nm/$2`.
fn host_seed() -> Vec<u8> {
    let mut file = ::cfb::CompoundFile::open(Cursor::new(template_seed(true))).unwrap();
    let mut data = vec![0u8; 1576];
    data[0x130..0x138].copy_from_slice(&HEADER_TIME.to_le_bytes());
    let mut header = vec![0u8; 20];
    header.extend(compressed(&data).unwrap());
    file.create_stream("Dgn~H")
        .unwrap()
        .write_all(&header)
        .unwrap();
    file.create_storage_all("Dgn^Nm").unwrap();
    let first = [
        color_table(200),
        font(201, 1024, "Font A"),
        font(202, 1042, "Font B"),
    ];
    let levels = [
        table(210, 1, 5),
        level(211, 64, 0, "Default"),
        level(212, 1, 1, "Alpha"),
        level(213, 2, 2, "alpha"),
        level(214, 3, 3, "Beta"),
        level(215, 4, 4, "Tag Level"),
    ];
    for (n, records) in [(1, &first[..]), (2, &levels[..])] {
        file.create_stream(format!("Dgn^Nm/${n}"))
            .unwrap()
            .write_all(&page(records, 3, n).unwrap())
            .unwrap();
    }
    file.flush().unwrap();
    file.into_inner().into_inner()
}

fn tagged_line(layer: &str, set: &str, tags: &[(&str, Value)]) -> Entity {
    let mut e = line();
    e.layer = Some(layer.into());
    for (tag, value) in tags {
        e.attributes.push(
            Attribute::new(*tag, value.clone())
                .with_set(set)
                .with_invisible(true),
        );
    }
    e
}

fn set_a(name: &str, count: i64) -> Vec<(&'static str, Value)> {
    vec![
        ("Name", Value::Text(name.into())),
        ("Count", Value::Int(count)),
    ]
}

fn attachment() -> Entity {
    image(
        "plan.tif",
        Point3::new(1.0, 2.0, 0.0),
        Vec3::new(30.0, 0.0, 0.0),
        Vec3::new(0.0, 15.0, 0.0),
        [3000, 1500],
    )
}

fn options(time: f64) -> WriteOptions {
    WriteOptions::default()
        .with_clear_seed_model(true)
        .with_timestamp_ms(time)
}

/// The host's seed: [`host_seed`] after a first write that adds "Set A" and an attachment.
fn stage_seed() -> Vec<u8> {
    let doc = document(
        true,
        vec![
            tagged_line("Beta", "Set A", &set_a("first", 1)),
            attachment(),
        ],
    );
    write_v8(&doc, &host_seed(), &options(FIRST_TIME)).unwrap()
}

fn write(seed: &[u8], entities: Vec<Entity>, time: f64) -> Vec<u8> {
    write_v8(&document(true, entities), seed, &options(time)).unwrap()
}

fn named(bytes: &[u8]) -> Vec<RawElement> {
    v8::read(bytes, &ReadOptions::default())
        .unwrap()
        .named_pages
        .iter()
        .flat_map(|p| p.elements.iter().cloned())
        .collect()
}

fn model_records(bytes: &[u8]) -> (Vec<RawElement>, Vec<RawElement>) {
    let native = v8::read(bytes, &ReadOptions::default()).unwrap();
    let model = &native.models[0];
    let all = |pages: &[v8::Page]| -> Vec<RawElement> {
        pages
            .iter()
            .flat_map(|p| p.elements.iter().cloned())
            .collect()
    };
    (all(&model.graphic_pages), all(&model.control_pages))
}

fn decoded(raw: &RawElement) -> ElementData {
    decode_v8::decode(raw, encoding_rs::WINDOWS_1252, &Limits::default()).data
}

fn time(raw: &RawElement) -> f64 {
    le::f64_at(&raw.bytes, 0x18).unwrap()
}

fn level_entries(bytes: &[u8]) -> usize {
    named(bytes)
        .iter()
        .filter(|r| matches!(decoded(r), ElementData::Level(_)))
        .count()
}

#[test]
fn levels_resolve_by_exact_name_without_new_levels() {
    let seed = stage_seed();
    let mut tag_line = line();
    tag_line.layer = Some("Alpha".into());
    tag_line.attributes.push(
        Attribute::new("Name", Value::Text("on its own level".into()))
            .with_set("Set A")
            .with_layer("Tag Level")
            .with_invisible(true),
    );
    let out = write(
        &seed,
        vec![
            tagged_line("alpha", "Set A", &[]),
            tagged_line("Alpha", "Set A", &[]),
            tag_line,
        ],
        WRITE_TIME,
    );
    let (graphics, _) = model_records(&out);
    let levels: Vec<(u16, Option<u32>)> = graphics
        .iter()
        .map(|r| (r.type_code(), r.level()))
        .collect();
    assert_eq!(
        levels,
        [(3, Some(2)), (3, Some(1)), (3, Some(1)), (37, Some(4))]
    );
    assert_eq!(level_entries(&out), 5);
    let doc = crate::read(&out, &ReadOptions::default()).unwrap();
    let layers: Vec<Option<&str>> = doc.models[0]
        .entities
        .iter()
        .map(|e| e.layer.as_deref())
        .collect();
    assert_eq!(layers, [Some("alpha"), Some("Alpha"), Some("Alpha")]);
    assert_eq!(
        doc.models[0].entities[2].attributes[0].layer.as_deref(),
        Some("Tag Level")
    );
    // Nothing in the dictionary changed, so every page keeps its bytes.
    assert_eq!(streams(&out, "Dgn^Nm"), streams(&seed, "Dgn^Nm"));
}

#[test]
fn tag_sets_are_reused_by_name() {
    let seed = stage_seed();
    let set_id = |bytes: &[u8]| -> Vec<u64> {
        named(bytes)
            .iter()
            .filter(|r| r.type_code() == 39)
            .map(|r| r.id().unwrap())
            .collect()
    };
    let seed_sets = set_id(&seed);
    assert_eq!(seed_sets.len(), 1);
    let out = write(
        &seed,
        vec![
            tagged_line("Beta", "Set A", &set_a("second", 2)),
            tagged_line("Beta", "Set A", &set_a("third", 3)),
        ],
        WRITE_TIME,
    );
    assert_eq!(set_id(&out), seed_sets, "one type-39 record, the seed's");
    let (graphics, _) = model_records(&out);
    let tags: Vec<_> = graphics
        .iter()
        .filter_map(|r| match decoded(r) {
            ElementData::Tag(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(tags.len(), 4);
    assert!(tags.iter().all(|t| t.set_id == Some(seed_sets[0])));
    assert_eq!(streams(&out, "Dgn^Nm"), streams(&seed, "Dgn^Nm"));
    let doc = crate::read(&out, &ReadOptions::default()).unwrap();
    let values: Vec<(&str, &Value)> = doc.models[0]
        .entities
        .iter()
        .flat_map(|e| &e.attributes)
        .map(|a| (a.tag.as_str(), &a.value))
        .collect();
    assert_eq!(
        values,
        [
            ("Name", &Value::Text("second".into())),
            ("Count", &Value::Int(2)),
            ("Name", &Value::Text("third".into())),
            ("Count", &Value::Int(3)),
        ]
    );
    assert!(
        doc.models[0]
            .entities
            .iter()
            .flat_map(|e| &e.attributes)
            .all(|a| a.set.as_deref() == Some("Set A"))
    );
}

#[test]
fn native_symbology_props_reach_the_record() {
    let seed = stage_seed();
    let parsed = V8Seed::from_bytes(&seed).unwrap();
    assert_eq!(parsed.palette(), &palette());
    assert_eq!(
        parsed.font_entries().collect::<Vec<_>>(),
        [("Font A", 1024), ("Font B", 1042)]
    );
    assert_eq!(parsed.font_number("Font B").unwrap(), 1042);
    assert_eq!(parsed.font_number(" font b ").unwrap(), 1042);
    assert!(parsed.font_number("Font C").is_err());
    let rgb = palette()[123];
    assert_eq!(
        crate::palette::exact_index(parsed.palette(), rgb),
        Some(123)
    );

    let mut native = line();
    for (key, value) in [("dgn.color_index", 77), ("dgn.weight", 5), ("dgn.style", 3)] {
        native.props.insert(key.into(), Value::Int(value));
    }
    let mut custom_style = line();
    custom_style
        .props
        .insert("dgn.style".into(), Value::Int(0x7fff_fff0));
    let typed = line().with_color(cadkit_core::Color::Rgb {
        r: rgb[0],
        g: rgb[1],
        b: rgb[2],
    });
    let text = |style: Option<&str>, number: Option<i64>| {
        let mut t = Entity::new(EntityKind::Text {
            position: Point3::new(1.0, 1.0, 0.0),
            end_point: None,
            height: 1.0,
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            value: "Label".into(),
            style: style.map(Into::into),
            halign: cadkit_core::HAlign::Left,
            valign: cadkit_core::VAlign::Baseline,
            normal: Vec3::Z,
        });
        if let Some(n) = number {
            t.props.insert("dgn.font_number".into(), Value::Int(n));
        }
        t
    };
    let out = write(
        &seed,
        vec![
            native,
            custom_style,
            typed,
            text(Some("Font B"), None),
            text(None, Some(1024)),
        ],
        WRITE_TIME,
    );
    let (graphics, _) = model_records(&out);
    let symbology = |r: &RawElement| [0x2c, 0x30, 0x34].map(|o| le::u32_at(&r.bytes, o).unwrap());
    assert_eq!(symbology(&graphics[0]), [3, 5, 77]);
    assert_eq!(symbology(&graphics[1])[0], 0x7fff_fff0);
    assert_eq!(symbology(&graphics[2])[2], 123);
    assert_eq!(le::u32_at(&graphics[3].bytes, 0x68), Some(1042));
    assert_eq!(le::u32_at(&graphics[4].bytes, 0x68), Some(1024));

    let fails =
        |e: Entity| write_v8(&document(true, vec![e]), &seed, &options(WRITE_TIME)).is_err();
    let mut float_style = line();
    float_style
        .props
        .insert("dgn.style".into(), Value::Float(3.0));
    assert!(fails(float_style));
    assert!(fails(line().with_color(cadkit_core::Color::Rgb {
        r: 1,
        g: 2,
        b: 3
    })));
    assert!(fails(text(Some("Font C"), None)));
}

#[test]
fn new_records_carry_the_write_time_and_seed_records_keep_theirs() {
    let seed = stage_seed();
    let seed_named = named(&seed);
    let out = write(
        &seed,
        vec![
            tagged_line("Gamma", "Set B", &[("Label", Value::Text("new".into()))]),
            tagged_line("Beta", "Set A", &set_a("again", 4)),
            attachment(),
        ],
        WRITE_TIME,
    );
    let (graphics, controls) = model_records(&out);
    assert!(graphics.iter().any(|r| r.type_code() == 94));
    assert!(graphics.iter().any(|r| r.type_code() == 37));
    assert!(!controls.is_empty());
    for r in graphics.iter().chain(&controls) {
        assert_eq!(time(r), WRITE_TIME, "type {}", r.type_code());
    }
    let out_named = named(&out);
    let level_time = out_named
        .iter()
        .find_map(|r| match decoded(r) {
            ElementData::Level(l) if l.name.as_deref() == Some("Gamma") => Some(time(r)),
            _ => None,
        })
        .unwrap();
    assert_eq!(level_time, WRITE_TIME);
    let set_time = |bytes: &[RawElement], name: &str| {
        bytes.iter().find_map(|r| match decoded(r) {
            ElementData::TagSet(s) if s.name.as_deref() == Some(name) => Some(time(r)),
            _ => None,
        })
    };
    assert_eq!(set_time(&out_named, "Set B"), Some(WRITE_TIME));
    assert_eq!(set_time(&out_named, "Set A"), Some(FIRST_TIME));
    // Seed records nothing changed keep their bytes, their time included.
    for id in [200, 201, 202, 211, 212] {
        let find = |records: &[RawElement]| records.iter().find(|r| r.id() == Some(id)).cloned();
        assert_eq!(
            find(&out_named).unwrap().bytes,
            find(&seed_named).unwrap().bytes
        );
    }

    // Without a time of its own the write takes the seed's file header time.
    let parsed = V8Seed::from_bytes(&seed).unwrap();
    assert_eq!(parsed.timestamp_ms(), HEADER_TIME);
    let default_time = write_v8(
        &document(true, vec![line()]),
        &seed,
        &WriteOptions::default().with_clear_seed_model(true),
    )
    .unwrap();
    let (graphics, _) = model_records(&default_time);
    assert_eq!(time(&graphics[0]), HEADER_TIME);
}

#[test]
fn a_new_level_changes_only_the_level_table_page() {
    let seed = stage_seed();
    let out = write(&seed, vec![tagged_line("Gamma", "Set A", &[])], WRITE_TIME);
    let before = streams(&seed, "Dgn^Nm");
    let after = streams(&out, "Dgn^Nm");
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    let changed: Vec<&String> = before
        .iter()
        .filter(|(k, v)| after.get(*k) != Some(*v))
        .map(|(k, _)| k)
        .collect();
    assert_eq!(changed, ["Dgn^Nm/$2"]);
    assert_eq!(level_entries(&out), 6);
}

#[test]
fn output_is_deterministic_and_independent_of_seed_parsing() {
    let seed = stage_seed();
    let doc = document(
        true,
        vec![
            tagged_line("Gamma", "Set B", &[("Label", Value::Text("x".into()))]),
            tagged_line("alpha", "Set A", &set_a("y", 5)),
            attachment(),
        ],
    );
    let first = write_v8(&doc, &seed, &options(WRITE_TIME)).unwrap();
    assert_eq!(first, write_v8(&doc, &seed, &options(WRITE_TIME)).unwrap());
    let parsed = V8Seed::from_bytes(&seed).unwrap();
    assert_eq!(
        first,
        write_v8_with_seed(&doc, &parsed, &options(WRITE_TIME)).unwrap()
    );
}

#[test]
fn the_native_api_exposes_what_hosts_read() {
    let seed = stage_seed();
    let out = write(
        &seed,
        vec![tagged_line("Beta", "Set A", &set_a("z", 6))],
        WRITE_TIME,
    );
    let native = v8::read(&out, &ReadOptions::default()).unwrap();
    let model = &native.models[0];
    for p in model
        .graphic_pages
        .iter()
        .chain(&model.control_pages)
        .chain(&native.named_pages)
    {
        assert!(p.complete, "{}", p.path);
        assert_eq!((p.header.page_number, p.header.population), (0, 0));
    }
    let records = named(&out);
    let colors = records
        .iter()
        .find_map(|r| match decoded(r) {
            ElementData::ColorTable { colors } if r.type_code() == 5 => Some(colors),
            _ => None,
        })
        .unwrap();
    assert_eq!(colors, palette().to_vec());
    let fonts: Vec<(u32, String)> = records
        .iter()
        .filter_map(|r| match decoded(r) {
            ElementData::Font { number, name } => Some((number, name)),
            _ => None,
        })
        .collect();
    assert_eq!(fonts, [(1024, "Font A".into()), (1042, "Font B".into())]);
    let level_table = records.iter().find(|r| r.type_code() == 96).unwrap();
    assert!(level_table.is_complex_header() && !level_table.is_complex_component());
    assert_eq!(level_table.id(), Some(210));
    let entries: Vec<&RawElement> = records
        .iter()
        .filter(|r| r.type_code() == 95 && r.level() == Some(1))
        .collect();
    assert_eq!(entries.len(), 5);
    assert!(entries.iter().all(|r| r.is_complex_component()));
}

/// Entry paths by directory slot (1, 2, ...) of the staged seed and of a write into it (the
/// same), the order of FORMAT_NOTES ("Compound file") spelled out.
const WRITER_SLOTS: [&str; 15] = [
    "Dgn~H",
    "Dgn-Md",
    "Dgn-Md/#000000",
    "Dgn-Md/#000000/Dgn^C",
    "Dgn-Md/#000000/Dgn^C/$1",
    "Dgn-Md/#000000/Dgn^G",
    "Dgn-Md/#000000/Dgn~Mh",
    "Dgn^Nm",
    "Dgn^Nm/$1",
    "Dgn^Nm/$2",
    "Dgn^Nm/$3",
    "Dgn-Md/#000000/Dgn^GA",
    "Dgn-Md/#000000/Dgn^G/$1",
    "Dgn-Md/#000000/Dgn^GA/^AH",
    "Dgn-Md/#000000/Dgn^GA/$1",
];

/// Checks every sibling tree from the raw directory bytes, without the writer's tree code:
/// a binary search tree by (UTF-16 length, ASCII upper case) — the names here are ASCII —
/// with colours 0/1, a black tree root, no red node with a red child and one black height.
/// Returns the number of storages with a child tree.
fn independent_red_black_check(bytes: &[u8]) -> usize {
    let offsets = crate::cfb_tree::directory_entry_offsets(bytes).unwrap();
    let u32_at = |off: usize| u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
    let key = |slot: u32| {
        let off = offsets[slot as usize];
        let len = usize::from(u16::from_le_bytes([bytes[off + 0x40], bytes[off + 0x41]])) - 2;
        let name: Vec<u16> = (0..len / 2)
            .map(|i| u16::from_le_bytes([bytes[off + 2 * i], bytes[off + 2 * i + 1]]))
            .collect();
        assert!(name.iter().all(|u| *u < 0x80), "ASCII names only");
        let upper: Vec<u16> = name
            .iter()
            .map(|u| u8::try_from(*u).unwrap().to_ascii_uppercase().into())
            .collect();
        (name.len(), upper)
    };
    // (black height, in-order keys) of the subtree at `slot`.
    fn walk(
        slot: u32,
        bytes: &[u8],
        offsets: &[usize],
        key: &dyn Fn(u32) -> (usize, Vec<u16>),
        u32_at: &dyn Fn(usize) -> u32,
        keys: &mut Vec<(usize, Vec<u16>)>,
    ) -> usize {
        if slot == u32::MAX {
            return 1;
        }
        let off = offsets[slot as usize];
        let red = match bytes[off + 0x43] {
            0 => true,
            1 => false,
            other => panic!("colour {other}"),
        };
        let (left, right) = (u32_at(off + 0x44), u32_at(off + 0x48));
        for child in [left, right] {
            if red && child != u32::MAX {
                assert_eq!(bytes[offsets[child as usize] + 0x43], 1, "red child of red");
            }
        }
        let l = walk(left, bytes, offsets, key, u32_at, keys);
        keys.push(key(slot));
        let r = walk(right, bytes, offsets, key, u32_at, keys);
        assert_eq!(l, r, "black height");
        l + usize::from(!red)
    }
    let mut storages = 0;
    let mut todo = vec![0u32];
    while let Some(storage) = todo.pop() {
        let off = offsets[storage as usize];
        let child = u32_at(off + 0x4c);
        if child == u32::MAX {
            continue;
        }
        storages += 1;
        assert_eq!(bytes[offsets[child as usize] + 0x43], 1, "black tree root");
        let mut keys = Vec::new();
        walk(child, bytes, &offsets, &key, &u32_at, &mut keys);
        assert!(keys.windows(2).all(|w| w[0] < w[1]), "{keys:?}");
        // Storages among the children: an in-order walk again, collecting slots.
        let mut stack = vec![child];
        while let Some(s) = stack.pop() {
            if s == u32::MAX {
                continue;
            }
            let o = offsets[s as usize];
            stack.extend([u32_at(o + 0x44), u32_at(o + 0x48)]);
            if bytes[o + 0x42] == 1 {
                todo.push(s);
            }
        }
    }
    storages
}

#[test]
fn outputs_hold_the_container_invariants() {
    let seed = stage_seed();
    for bytes in [
        seed.clone(),
        write(
            &seed,
            vec![tagged_line("Gamma", "Set B", &[]), attachment()],
            WRITE_TIME,
        ),
    ] {
        let r = crate::check_structure(&bytes, &ReadOptions::default()).unwrap();
        assert!(r.is_clean(), "{:?}", r.warnings);
        assert_eq!(
            (
                r.red_black_trees,
                r.top_down_trees,
                r.irregular_free_entries
            ),
            (r.storages, r.storages, 0)
        );
        assert!(r.writer_slot_order);
        // Checks that share no code with the writer: the cfb crate's strict open, a red-black
        // check from the raw directory, and the slot order as the reader lists it.
        ::cfb::CompoundFile::open_strict(Cursor::new(&bytes)).unwrap();
        assert_eq!(independent_red_black_check(&bytes), r.storages as usize);
        let cf = crate::cfb::CompoundFile::parse(&bytes).unwrap();
        let mut slots: Vec<(usize, String)> = cf
            .paths()
            .iter()
            .map(|p| (p.index, p.path.clone()))
            .collect();
        slots.sort();
        let order: Vec<&str> = slots.iter().map(|(_, p)| p.as_str()).collect();
        assert_eq!(order, WRITER_SLOTS);
        assert_eq!(
            slots.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            (1..=order.len()).collect::<Vec<_>>()
        );
        assert_eq!(r.pages_with_number_or_population, 0);
        let mh = container::stream(&bytes, "Dgn-Md/#000000/Dgn~Mh", u64::MAX).unwrap();
        let counter = le::u32_at(&crate::zlib::inflate(&mh, 1 << 20).data, 0).unwrap();
        let native = v8::read(&bytes, &ReadOptions::default()).unwrap();
        let model = &native.models[0];
        let filled = model
            .graphic_pages
            .iter()
            .chain(&model.control_pages)
            .filter(|p| !p.elements.is_empty())
            .count() as u32;
        assert_eq!(counter, filled);
        assert_eq!(filled, 2, "one graphic and one control page");
    }
}
