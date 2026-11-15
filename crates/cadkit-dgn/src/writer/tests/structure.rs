//! Container invariants of written and repacked files, as [`crate::check_structure`] reports
//! them: ordered top-down red-black sibling trees, unused slots in the MS-CFB 2.6.3 free form,
//! the writer's slot order, record-less pages of 16 bytes and the model header's page count.

use super::raster_template::{image, template_seed};
use super::*;
use crate::StructureReport;
use crate::cfb_tree::{self, CHILD, COLOR, LEFT, NOSTREAM, RIGHT};

/// Records need a time: a synthetic seed has none, so these writes give one.
fn timed() -> WriteOptions {
    WriteOptions::default().with_timestamp_ms(1.7e12)
}

fn report(bytes: &[u8]) -> StructureReport {
    crate::check_structure(bytes, &ReadOptions::default()).unwrap()
}

/// `bytes` is a clean writer output that opens strictly and serves as a seed again.
fn assert_clean(bytes: &[u8]) -> StructureReport {
    let r = report(bytes);
    assert!(r.is_clean(), "{:?}", r.warnings);
    assert!(r.storages > 0);
    assert_eq!(r.red_black_trees, r.storages);
    assert_eq!(r.top_down_trees, r.storages);
    assert_eq!(r.irregular_free_entries, 0);
    assert!(r.writer_slot_order);
    ::cfb::CompoundFile::open_strict(Cursor::new(bytes)).unwrap();
    crate::read(bytes, &ReadOptions::default()).unwrap();
    V8Seed::from_bytes(bytes).unwrap();
    r
}

/// Inflated model header stream of the written model.
fn model_header(bytes: &[u8]) -> Vec<u8> {
    let raw = container::stream(bytes, "Dgn-Md/#000000/Dgn~Mh", u64::MAX).unwrap();
    crate::zlib::inflate(&raw, 1 << 24).data
}

/// Graphic and control pages of the written model that hold records.
fn filled_pages(bytes: &[u8]) -> u32 {
    let native = v8::read(bytes, &ReadOptions::default()).unwrap();
    let model = &native.models[0];
    model
        .graphic_pages
        .iter()
        .chain(&model.control_pages)
        .filter(|p| p.header.record_count > 0)
        .count() as u32
}

/// A seed whose model header prefix holds 7, 0x1111 and 0x2222 in its first three dwords.
fn seed_with_counters(three: bool) -> Vec<u8> {
    let mut stream = crate::zlib::inflate(
        &v8::tests::model_header_stream(three, 1000.0, "Synthetic"),
        1 << 20,
    )
    .data;
    for (at, value) in [(0, 7u32), (4, 0x1111), (8, 0x2222)] {
        stream[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut file = ::cfb::CompoundFile::open(Cursor::new(seed(three))).unwrap();
    file.create_stream("Dgn-Md/#000000/Dgn~Mh")
        .unwrap()
        .write_all(&compressed(&stream).unwrap())
        .unwrap();
    file.flush().unwrap();
    file.into_inner().into_inner()
}

#[test]
fn written_and_repacked_files_are_clean_containers() {
    for three in [false, true] {
        let seed = seed(three);
        assert_clean(&repack_v8(&seed, &ReadOptions::default()).unwrap());
        let one = write_v8(&document(three, vec![line()]), &seed, &timed()).unwrap();
        assert_eq!(assert_clean(&one).pages_with_number_or_population, 0);
        // Written into its own output again.
        let again = write_v8(
            &document(three, vec![line(), line()]),
            &one,
            &timed().with_clear_seed_model(true),
        )
        .unwrap();
        assert_clean(&again);
        let rasters = write_v8(
            &document(
                three,
                vec![image(
                    "plan.tif",
                    Point3::new(1.0, 2.0, 0.0),
                    Vec3::new(30.0, 0.0, 0.0),
                    Vec3::new(0.0, 15.0, 0.0),
                    [3000, 1500],
                )],
            ),
            &template_seed(three),
            &timed().with_clear_seed_model(true),
        )
        .unwrap();
        assert_clean(&rasters);
    }
}

#[test]
fn a_record_less_page_is_its_header_and_counts_no_page() {
    let empty = write_v8(
        &document(false, vec![]),
        &seed_with_counters(false),
        &timed(),
    )
    .unwrap();
    assert_clean(&empty);
    let page = container::stream(&empty, "Dgn-Md/#000000/Dgn^G/$1", u64::MAX).unwrap();
    assert_eq!(page.len(), 16);
    assert_eq!(&page[..4], &[0; 4]);
    let header = model_header(&empty);
    assert_eq!(le::u32_at(&header, 0), Some(0));
    // The other prefix dwords keep the seed's values.
    assert_eq!(le::u32_at(&header, 4), Some(0x1111));
    assert_eq!(le::u32_at(&header, 8), Some(0x2222));
}

#[test]
fn model_header_counts_the_graphic_and_control_pages_holding_records() {
    // Over 256 KiB of lines fill two graphic pages; a new raster attachment adds a control
    // page: three pages hold records.
    let mut entities: Vec<Entity> = (0..2500).map(|_| line()).collect();
    entities.push(image(
        "plan.tif",
        Point3::new(1.0, 2.0, 0.0),
        Vec3::new(30.0, 0.0, 0.0),
        Vec3::new(0.0, 15.0, 0.0),
        [3000, 1500],
    ));
    let mut seed = template_seed(false);
    {
        // The template's model header with distinct values after the counter.
        let raw = container::stream(&seed, "Dgn-Md/#000000/Dgn~Mh", u64::MAX).unwrap();
        let mut stream = crate::zlib::inflate(&raw, 1 << 20).data;
        stream[4..8].copy_from_slice(&0x1111u32.to_le_bytes());
        let mut file = ::cfb::CompoundFile::open(Cursor::new(seed)).unwrap();
        file.create_stream("Dgn-Md/#000000/Dgn~Mh")
            .unwrap()
            .write_all(&compressed(&stream).unwrap())
            .unwrap();
        file.flush().unwrap();
        seed = file.into_inner().into_inner();
    }
    let out = write_v8(
        &document(false, entities),
        &seed,
        &timed().with_clear_seed_model(true),
    )
    .unwrap();
    assert_clean(&out);
    let native = v8::read(&out, &ReadOptions::default()).unwrap();
    assert_eq!(native.models[0].graphic_pages.len(), 2);
    assert_eq!(native.models[0].control_pages.len(), 1);
    assert_eq!(filled_pages(&out), 3);
    let header = model_header(&out);
    assert_eq!(le::u32_at(&header, 0), Some(3));
    assert_eq!(le::u32_at(&header, 4), Some(0x1111));
    // Only the counter differs from the seed's prefix; the element after it is the
    // written model header.
    let source = crate::zlib::inflate(
        &container::stream(&seed, "Dgn-Md/#000000/Dgn~Mh", u64::MAX).unwrap(),
        1 << 20,
    )
    .data;
    assert_eq!(header[4..0x1004], source[4..0x1004]);
}

#[test]
fn the_report_flags_damaged_outputs() {
    let out = write_v8(&document(false, vec![line()]), &seed(false), &timed()).unwrap();
    let offsets = cfb_tree::directory_entry_offsets(&out).unwrap();
    let reachable = cfb_tree::reachable_slots(&out, &offsets);
    let root_tree = le::u32_at(&out, offsets[0] + CHILD).unwrap();
    let codes = |bytes: &[u8]| -> Vec<String> {
        report(bytes).warnings.into_iter().map(|w| w.code).collect()
    };
    // A red tree root: still ordered, no longer red-black.
    let mut red = out.clone();
    red[offsets[root_tree as usize] + COLOR] = 0;
    assert_eq!(codes(&red), ["dgn.cfb_tree_unbalanced"]);
    // Swapped children of the tree root: names no longer ascend.
    let mut swapped = out.clone();
    let at = offsets[root_tree as usize];
    let (l, r) = (
        le::u32_at(&out, at + LEFT).unwrap(),
        le::u32_at(&out, at + RIGHT).unwrap(),
    );
    assert!(
        l != NOSTREAM && r != NOSTREAM,
        "the root tree has two children"
    );
    swapped[at + LEFT..at + LEFT + 4].copy_from_slice(&r.to_le_bytes());
    swapped[at + RIGHT..at + RIGHT + 4].copy_from_slice(&l.to_le_bytes());
    assert!(codes(&swapped).contains(&"dgn.cfb_tree_unordered".to_owned()));
    // A stale byte in an unused slot.
    let unused = (0..offsets.len() as u32)
        .find(|s| !reachable.contains(s))
        .expect("an unused slot");
    let mut stale = out.clone();
    stale[offsets[unused as usize] + 2] = b'x';
    assert_eq!(codes(&stale), ["dgn.cfb_free_entry"]);
    assert_eq!(report(&stale).irregular_free_entries, 1);
    // Without a time from the options or the seed, records are written with time 0.
    let untimed = write_v8(
        &document(false, vec![line()]),
        &seed(false),
        &WriteOptions::default(),
    )
    .unwrap();
    assert_eq!(codes(&untimed), ["dgn.record_time"]);
    assert_eq!(report(&untimed).model_records_without_time, 1);
}
