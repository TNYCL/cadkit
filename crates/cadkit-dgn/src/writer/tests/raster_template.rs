//! New raster attachments cloned from a synthetic seed attachment group or the built-in one.
//!
//! The seed group follows the observed layout (FORMAT_NOTES, "Raster attachments"): control
//! records 92, 93, 93, 90 and five 91, a 1024-byte frame 94 and an XAttribute 170 with
//! eight sub-records. Every field the writer does not own is filled with marker bytes so a
//! changed byte outside the documented fields shows up.

use super::*;
use crate::native::linkage::{self, LinkageData};

const FRAME: u64 = 100;
const LINK: u64 = 101;
const OPTION_A: u64 = 102;
const OPTION_B: u64 = 103;
const REFERENCE: u64 = 104;
const SETTINGS: [u64; 5] = [105, 106, 107, 108, 109];
const SUBTYPES: [u32; 5] = [0x2_0007, 0x2_0008, 0x1_0009, 0x1_000b, 0x1_000b];
const SETTING_SIZES: [usize; 5] = [0xb8, 0x60, 0x230, 0x238, 0x238];
const MARK: u8 = 0x5a;

/// Fills `b[from..to]` with marker bytes.
fn mark(b: &mut [u8], from: usize, to: usize) {
    for (i, x) in b[from..to].iter_mut().enumerate() {
        *x = MARK ^ (i as u8);
    }
}

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn xattribute(name: &str, url: &str) -> Vec<u8> {
    let mut out = vec![];
    for i in 0..8u16 {
        let data = match i {
            1 => {
                let mut d = vec![2, 0, 0, 0, 1, 0, 0, 0];
                d.extend(utf16z("(SYN-A)"));
                d
            }
            3 => utf16z(name),
            4 => utf16z(url),
            _ => vec![0; 4],
        };
        let mut head = vec![0; 16];
        put(&mut head, 0, &i.to_le_bytes()).unwrap();
        put(&mut head, 2, &0x570bu16.to_le_bytes()).unwrap();
        put(&mut head, 8, &(data.len() as u16).to_le_bytes()).unwrap();
        put(&mut head, 10, &0x8000u16.to_le_bytes()).unwrap();
        out.extend(head);
        out.extend(data);
    }
    out.extend([1, 0, 0, 0]);
    out
}

fn aux_record(id: u64, payload: &[u8]) -> Vec<u8> {
    let mut a = vec![0; 28];
    u32_at(&mut a, 0, 0xa11b).unwrap();
    u32_at(&mut a, 4, payload.len() as u32).unwrap();
    u32_at(&mut a, 8, 170).unwrap();
    u64_at(&mut a, 16, id).unwrap();
    u32_at(&mut a, 24, 8).unwrap();
    a.extend_from_slice(payload);
    a
}

/// A seed holding one complete attachment of an 800 x 400 pixel raster, 2.5 UOR per
/// pixel, at UOR (1000, 2000), and a line.
fn template_seed(three: bool) -> Vec<u8> {
    let mut frame = prefix(0x10c0_005e, 0x400, FRAME, 7, 1.5e12).unwrap();
    mark(&mut frame, 0x20, 0x38);
    u32_at(&mut frame, 0x28, 0x0a00).unwrap();
    let matrix = [
        2.5, 0., 0., 1000., 0., 2.5, 0., 2000., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    for (i, n) in matrix.into_iter().enumerate() {
        f64_at(&mut frame, 0x78 + i * 8, n).unwrap();
    }
    mark(&mut frame, 0xf8, 0x108);
    f64_at(&mut frame, 0x108, 2000.).unwrap();
    f64_at(&mut frame, 0x110, 1000.).unwrap();
    f64_at(&mut frame, 0x118, 200.).unwrap();
    f64_at(&mut frame, 0x120, 200.).unwrap();
    mark(&mut frame, 0x128, 0x400);
    range(&mut frame, &[[1000., 2000., 0.], [3000., 3000., 0.]]).unwrap();
    let frame = finish(frame, &[]).unwrap();

    let mut link = prefix(0x20c0_005c, 0x78, LINK, 64, 1.5e12).unwrap();
    mark(&mut link, 0x20, 0x30);
    u64_at(&mut link, 0x30, REFERENCE).unwrap();
    let link = finish(link, &dependency(0x271b, FRAME).unwrap()).unwrap();
    let options: Vec<Vec<u8>> = [(OPTION_A, 0x68, 0x1_000c), (OPTION_B, 0x98, 0x1_000d)]
        .into_iter()
        .map(|(id, size, subtype)| {
            let mut b = prefix(0x60c0_005d, size, id, 64, 1.5e12).unwrap();
            mark(&mut b, 0x20, size);
            u32_at(&mut b, 0x30, subtype).unwrap();
            finish(b, &[]).unwrap()
        })
        .collect();
    let mut reference = prefix(0x20c0_005a, 0x78, REFERENCE, 64, 1.5e12).unwrap();
    mark(&mut reference, 0x20, 0x38);
    u64_at(&mut reference, 0x38, LINK).unwrap();
    mark(&mut reference, 0x40, 0x78);
    let mut links = string_link_bytes(3, b"old.tif").unwrap();
    links.extend(string_link_bytes(0x1f, b"file://D:\\old\\old.tif").unwrap());
    links.extend(dependency(0x271b, FRAME).unwrap());
    let reference = finish(reference, &links).unwrap();
    let settings: Vec<Vec<u8>> = SETTINGS
        .iter()
        .zip(SUBTYPES)
        .zip(SETTING_SIZES)
        .map(|((id, subtype), size)| {
            let mut b = prefix(0x40c0_005b, size, *id, 64, 1.5e12).unwrap();
            mark(&mut b, 0x20, size);
            u32_at(&mut b, 0x20, subtype).unwrap();
            finish(b, &[]).unwrap()
        })
        .collect();

    let mut controls = vec![link];
    controls.extend(options);
    controls.push(reference);
    controls.extend(settings);
    let ga_payload = aux_record(FRAME, &xattribute("old.tif", "file://D:\\old\\old.tif"));
    let mut ga = vec![0; 16];
    u32_at(&mut ga, 0, 1).unwrap();
    u32_at(&mut ga, 4, 2).unwrap();
    ga.extend(compressed(&ga_payload).unwrap());

    let mut file = ::cfb::CompoundFile::open(Cursor::new(seed(three))).unwrap();
    for storage in ["Dgn^C", "Dgn^GA"] {
        file.create_storage_all(format!("Dgn-Md/#000000/{storage}"))
            .unwrap();
    }
    let line = header(4, 0x70 + 32, 99, 0, 0., &line(), false, 0).unwrap();
    for (path, data) in [
        (
            "Dgn-Md/#000000/Dgn^G/$1",
            page(&[finish(line, &[]).unwrap(), frame], 2, 1).unwrap(),
        ),
        ("Dgn-Md/#000000/Dgn^C/$1", page(&controls, 3, 1).unwrap()),
        ("Dgn-Md/#000000/Dgn^GA/$1", ga),
        ("Dgn-Md/#000000/Dgn^GA/^AH", 1u32.to_le_bytes().to_vec()),
    ] {
        file.create_stream(path).unwrap().write_all(&data).unwrap();
    }
    file.flush().unwrap();
    file.into_inner().into_inner()
}

fn image(path: &str, position: Point3, u: Vec3, v: Vec3, size: [u32; 2]) -> Entity {
    let mut e = Entity::new(EntityKind::Image {
        path: Some(path.into()),
        position,
        u_vector: u,
        v_vector: v,
        size_px: Some(size),
    });
    e.layer = Some("Default".into());
    e
}

fn clear() -> WriteOptions {
    WriteOptions {
        clear_seed_model: true,
        timestamp_ms: 1.7e12,
        ..Default::default()
    }
}

fn close(a: f64, b: f64, scale: f64) -> bool {
    (a - b).abs() <= 1e-6 * scale.max(1.0)
}

fn same_vec(a: Vec3, b: Vec3) -> bool {
    let s = b.length();
    close(a.x, b.x, s) && close(a.y, b.y, s) && close(a.z, b.z, s)
}

fn text(e: &Entity, key: &str) -> Option<String> {
    match e.props.get(key) {
        Some(Value::Text(s)) => Some(s.clone()),
        _ => None,
    }
}

fn float(e: &Entity, key: &str) -> Option<f64> {
    match e.props.get(key) {
        Some(Value::Float(v)) => Some(*v),
        _ => None,
    }
}

/// Four attachments: two share one raster file (one of them turned by 90 degrees), one is
/// sheared with its own resolution and one has a non-ASCII path.
fn attachments() -> Vec<Entity> {
    let mut skewed = image(
        "file://C:\\plans\\other.tif",
        Point3::new(-4.25, 7.5, 0.0),
        Vec3::new(30.0, 5.0, 0.0),
        Vec3::new(2.0, 15.0, 0.0),
        [600, 300],
    );
    skewed
        .props
        .insert("dgn.raster_dpi".into(), Value::Int(300));
    vec![
        image(
            "C:\\plans\\sheet.tif",
            Point3::new(10.0, 20.0, 0.0),
            Vec3::new(30.0, 0.0, 0.0),
            Vec3::new(0.0, 15.0, 0.0),
            [3000, 1500],
        ),
        image(
            "C:\\plans\\sheet.tif",
            Point3::new(100.0, 20.0, 0.0),
            Vec3::new(0.0, 30.0, 0.0),
            Vec3::new(-15.0, 0.0, 0.0),
            [3000, 1500],
        ),
        skewed,
        image(
            "C:\\Çizimler\\kat planı.tif",
            Point3::new(0.0, -50.0, 0.0),
            Vec3::new(42.0, 0.0, 0.0),
            Vec3::new(0.0, 29.7, 0.0),
            [4961, 3508],
        ),
    ]
}

#[test]
fn new_attachments_read_back_as_written() {
    for three in [false, true] {
        let seed = template_seed(three);
        assert!(crate::seed_has_raster_template(&seed, &ReadOptions::default()).unwrap());
        let written = attachments();
        let output = write_v8(&document(three, written.clone()), &seed, &clear()).unwrap();
        assert_read_back(&output, &written);
    }
}

#[test]
fn seeds_without_an_attachment_use_the_builtin_group() {
    let read = ReadOptions::default();
    for three in [false, true] {
        let plain = seed(three);
        assert!(!crate::seed_has_raster_template(&plain, &read).unwrap());
        let written = attachments();
        let output = write_v8(&document(three, written.clone()), &plain, &clear()).unwrap();
        assert_read_back(&output, &written);
        // The output's groups are the built-in records with the documented fields written.
        let native = v8::read(&output, &read).unwrap();
        let model = &native.models[0];
        let builtin = crate::writer::raster_template::Template::builtin(3).unwrap();
        let captured = crate::writer::raster_template::Template::capture(model).unwrap();
        assert_eq!(
            captured.aux_version,
            model.graphic_pages[0].header.format_version
        );
        assert!(captured.same_constants(&builtin), "{captured:?}");
    }
}

/// `output` holds `written` as new attachments: path, name, placement, resolution, level,
/// no raster warning, and complete groups that can serve as a template again.
fn assert_read_back(output: &[u8], written: &[Entity]) {
    let reread = crate::read(output, &ReadOptions::default()).unwrap();
    assert!(
        reread
            .warnings
            .iter()
            .all(|w| !w.code.starts_with("dgn.raster")),
        "{:?}",
        reread.warnings
    );
    let images = &reread.models[0].entities;
    assert_eq!(
        images.len(),
        written.len(),
        "the seed line and frame are cleared"
    );
    for (got, want) in images.iter().zip(written) {
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
        ) = (&got.kind, &want.kind)
        else {
            panic!("not an image: {:?}", got.kind);
        };
        let want_path = want_path.as_deref().unwrap();
        let url = if want_path.starts_with("file://") {
            want_path.to_owned()
        } else {
            format!("file://{want_path}")
        };
        // The stored path is the `file://` form of the given path.
        assert_eq!(path.as_deref(), Some(url.as_str()));
        assert_eq!(text(got, "dgn.raster_path").as_deref(), Some(url.as_str()));
        let name = want_path.rsplit('\\').next().unwrap();
        assert_eq!(text(got, "dgn.raster_file").as_deref(), Some(name));
        assert_eq!(size_px, want_size);
        let scale = want_u.length().max(want_position.x.abs());
        assert!(close(position.x, want_position.x, scale));
        assert!(close(position.y, want_position.y, scale));
        assert!(close(position.z, want_position.z, scale));
        assert!(same_vec(*u_vector, *want_u), "{u_vector:?} {want_u:?}");
        assert!(same_vec(*v_vector, *want_v), "{v_vector:?} {want_v:?}");
        assert_eq!(got.layer.as_deref(), Some("Default"));
        let dpi = if want.props.contains_key("dgn.raster_dpi") {
            300.0
        } else {
            200.0
        };
        assert_eq!(float(got, "dgn.raster_dpi"), Some(dpi));
        assert!(!got.props.contains_key("dgn.raster_dpi_y"));
        let rotation = want_u.y.atan2(want_u.x);
        assert!(close(
            float(got, "dgn.raster_rotation").unwrap(),
            rotation,
            1.0
        ));
        assert!(got.visible);
    }
    // Y axis angle - X axis angle - 90 degrees: the sheared frame leans left.
    let skew = float(&images[2], "dgn.raster_skew").unwrap();
    assert!(
        close(skew, -2f64.atan2(15.0) - 5f64.atan2(30.0), 1.0),
        "{skew}"
    );
    assert!(close(
        float(&images[1], "dgn.raster_skew").unwrap(),
        0.0,
        1.0
    ));
    // The output holds complete attachment groups again.
    assert!(crate::seed_has_raster_template(output, &ReadOptions::default()).unwrap());
}

#[test]
fn every_attachment_gets_its_own_linked_group() {
    let seed = template_seed(false);
    let output = write_v8(&document(false, attachments()), &seed, &clear()).unwrap();
    let native = v8::read(&output, &ReadOptions::default()).unwrap();
    let model = &native.models[0];
    let controls: Vec<&RawElement> = model
        .control_pages
        .iter()
        .flat_map(|p| &p.elements)
        .collect();
    let types: Vec<u16> = controls.iter().map(|r| r.type_code()).collect();
    assert_eq!(
        types,
        [92, 93, 93, 90, 91, 91, 91, 91, 91].repeat(4),
        "control records in stored order"
    );
    let frames: Vec<&RawElement> = model
        .graphic_pages
        .iter()
        .flat_map(|p| &p.elements)
        .filter(|r| r.type_code() == 94)
        .collect();
    assert_eq!(frames.len(), 4);
    let xattributes: Vec<_> = model.graphic_aux.iter().flat_map(|p| &p.records).collect();
    assert_eq!(xattributes.len(), 4);
    let mut all_ids = BTreeSet::new();
    for (group, frame) in controls.chunks(9).zip(&frames) {
        let frame_id = frame.id().unwrap();
        let ids: Vec<u64> = std::iter::once(frame_id)
            .chain(group.iter().map(|r| r.id().unwrap()))
            .collect();
        // Ten consecutive fresh ids: 94, 92, 93, 93, 90, 91 x 5.
        assert_eq!(ids, (frame_id..frame_id + 10).collect::<Vec<_>>());
        assert!(frame_id > 109, "above every seed id");
        all_ids.extend(ids);
        let (link, reference) = (group[0], group[3]);
        assert_eq!(le::u64_at(&link.bytes, 0x30), reference.id());
        assert_eq!(le::u64_at(&reference.bytes, 0x38), link.id());
        let roots = |r: &RawElement| {
            linkage::dependency_root(
                &linkage::parse(
                    r.attributes(),
                    r.attribute_offset(),
                    true,
                    encoding_rs::UTF_8,
                ),
                0x271b,
            )
        };
        assert_eq!(roots(link), Some(frame_id));
        assert_eq!(roots(reference), Some(frame_id));
        assert_eq!(
            xattributes
                .iter()
                .filter(|a| a.element_id == frame_id && a.kind == 170)
                .count(),
            1
        );
    }
    assert_eq!(all_ids.len(), 40);
    // Two attachments name one raster file; each has its own 90 record.
    let names: Vec<String> = controls
        .iter()
        .filter(|r| r.type_code() == 90)
        .map(|r| {
            linkage::string(
                &linkage::parse(
                    r.attributes(),
                    r.attribute_offset(),
                    true,
                    encoding_rs::UTF_8,
                ),
                3,
            )
            .unwrap()
        })
        .collect();
    assert_eq!(
        names,
        ["sheet.tif", "sheet.tif", "other.tif", "kat planı.tif"]
    );
    // XAttributes sit in the auxiliary page numbered like the frame's graphic page, and
    // the page counter follows the highest page.
    assert!(
        model
            .graphic_aux
            .iter()
            .all(|p| p.path.ends_with("/Dgn^GA/$1"))
    );
    let counter = container::stream(
        &output,
        "Dgn-Md/#000000/Dgn^GA/^AH",
        ReadOptions::default().limits.max_input_bytes,
    )
    .unwrap();
    assert_eq!(counter, 2u32.to_le_bytes());
}

/// Byte offsets where `a` and `b` differ (including a length difference).
fn changed(a: &[u8], b: &[u8]) -> Vec<usize> {
    let mut out: Vec<usize> = (0..a.len().min(b.len()))
        .filter(|&i| a[i] != b[i])
        .collect();
    out.extend(a.len().min(b.len())..a.len().max(b.len()));
    out
}

fn only_in(offsets: &[usize], allowed: &[(usize, usize)], what: &str) {
    for o in offsets {
        assert!(
            allowed.iter().any(|(from, to)| (from..to).contains(&o)),
            "{what}: byte {o:#x} changed outside the documented fields"
        );
    }
}

#[test]
fn cloned_group_differs_from_the_template_only_in_documented_fields() {
    let seed = template_seed(false);
    let doc = document(false, vec![attachments().remove(2)]);
    let output = write_v8(&doc, &seed, &clear()).unwrap();
    let read = ReadOptions::default();
    let before = v8::read(&seed, &read).unwrap();
    let after = v8::read(&output, &read).unwrap();
    let (before, after) = (&before.models[0], &after.models[0]);
    let frame = |m: &crate::native::v8::V8Model| {
        m.graphic_pages
            .iter()
            .flat_map(|p| &p.elements)
            .find(|r| r.type_code() == 94)
            .unwrap()
            .bytes
            .clone()
    };
    const IDENTITY: (usize, usize) = (0x10, 0x20);
    let (old, new) = (frame(before), frame(after));
    assert_eq!(old.len(), new.len());
    only_in(
        &changed(&old, &new),
        &[
            (0x0c, 0x10),
            IDENTITY,
            (0x38, 0x68),
            (0x78, 0xf8),
            (0x108, 0x128),
        ],
        "94",
    );
    let controls = |m: &crate::native::v8::V8Model| -> Vec<Vec<u8>> {
        m.control_pages
            .iter()
            .flat_map(|p| &p.elements)
            .map(|r| r.bytes.clone())
            .collect()
    };
    let (old, new) = (controls(before), controls(after));
    assert_eq!(old.len(), 9);
    assert_eq!(new.len(), 9);
    // 92: the 90 id and the dependency root (linkage at 0x78, root at +12).
    only_in(
        &changed(&old[0], &new[0]),
        &[IDENTITY, (0x30, 0x38), (0x84, 0x8c)],
        "92",
    );
    for i in [1, 2, 5, 6, 7, 8] {
        assert_eq!(old[i].len(), new[i].len());
        only_in(&changed(&old[i], &new[i]), &[IDENTITY], "93/91");
    }
    // 91/0x20007: origin, edge lengths, angles, resolution and both pixel axes.
    only_in(
        &changed(&old[4], &new[4]),
        &[IDENTITY, (0x28, 0x70), (0x88, 0xb8)],
        "91/0x20007",
    );
    // 90: the record length, the 92 id and the linkages (two strings and the root).
    only_in(
        &changed(&old[3][..0x78], &new[3][..0x78]),
        &[(0x04, 0x08), IDENTITY, (0x38, 0x40)],
        "90",
    );
    let links = linkage::parse(&new[3][0x78..], 0x78, true, encoding_rs::UTF_8);
    assert_eq!(links.len(), 3);
    assert!(
        matches!(&links[0].data, LinkageData::String { string_id: 3, text, .. } if text == "other.tif")
    );
    assert!(
        matches!(&links[1].data, LinkageData::String { string_id: 0x1f, text, .. } if text == "file://C:\\plans\\other.tif")
    );
    assert_eq!(links[2].bytes[..12], old[3][old[3].len() - 24..][..12]);
    // XAttribute: only the strings of sub-records 3 and 4 and their lengths change.
    let xa = |m: &crate::native::v8::V8Model| m.graphic_aux[0].records[0].clone();
    let (old, new) = (xa(before), xa(after));
    assert_eq!(
        (old.kind, old.reserved, old.flags),
        (new.kind, new.reserved, new.flags)
    );
    assert_eq!(
        new.payload,
        xattribute("other.tif", "file://C:\\plans\\other.tif")
    );
}

#[test]
fn ascii_and_unicode_paths_use_their_observed_encodings() {
    let seed = template_seed(false);
    let output = write_v8(&document(false, attachments()), &seed, &clear()).unwrap();
    let native = v8::read(&output, &ReadOptions::default()).unwrap();
    let encodings: Vec<Vec<bool>> = native.models[0]
        .control_pages
        .iter()
        .flat_map(|p| &p.elements)
        .filter(|r| r.type_code() == 90)
        .map(|r| {
            linkage::parse(
                r.attributes(),
                r.attribute_offset(),
                true,
                encoding_rs::UTF_8,
            )
            .iter()
            .filter_map(|l| match l.data {
                LinkageData::String { encoding, .. } => {
                    Some(encoding == crate::text::StringEncoding::Utf16)
                }
                _ => None,
            })
            .collect()
        })
        .collect();
    // 8-bit when ASCII, `ff fd` UTF-16 otherwise.
    assert_eq!(
        encodings,
        [
            vec![false, false],
            vec![false, false],
            vec![false, false],
            vec![true, true]
        ]
    );
}

#[test]
fn invalid_attachments_are_reported() {
    let read = ReadOptions::default();
    let seed = template_seed(false);
    let mut no_size = attachments().remove(0);
    if let EntityKind::Image { size_px, .. } = &mut no_size.kind {
        *size_px = None;
    }
    let mut relative = attachments().remove(0);
    if let EntityKind::Image { path, .. } = &mut relative.kind {
        *path = Some("plans\\sheet.tif".into());
    }
    let mut parallel = attachments().remove(0);
    if let EntityKind::Image { v_vector, .. } = &mut parallel.kind {
        *v_vector = Vec3::new(60.0, 0.0, 0.0);
    }
    let mut lifted = attachments().remove(0);
    if let EntityKind::Image { u_vector, .. } = &mut lifted.kind {
        u_vector.z = 1.0;
    }
    for (e, message) in [
        (
            no_size,
            Some("new DGN raster attachments need path and size_px"),
        ),
        (relative, Some("DGN raster path must be absolute")),
        (parallel, None),
        (lifted, Some("nonzero Z cannot be written into a 2D seed")),
    ] {
        match (
            write_v8(&document(false, vec![e]), &seed, &clear()),
            message,
        ) {
            (Err(Error::Unsupported(m)), Some(want)) => assert_eq!(m, want),
            (Err(_), None) => {}
            (other, _) => panic!("{other:?}"),
        }
    }
    // A seed attachment kept by id cannot be mixed with new ones.
    let mut doc = crate::read(&seed, &read).unwrap();
    doc.models[0]
        .entities
        .retain(|e| matches!(e.kind, EntityKind::Image { .. }));
    doc.models[0].entities.push(attachments().remove(0));
    assert!(matches!(
        write_v8(&doc, &seed, &clear()),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn incomplete_groups_are_not_templates() {
    // The seed of the preservation tests holds a 90, 92 and one 91 but no full group.
    let read = ReadOptions::default();
    let seed = super::raster::raster_seed();
    assert!(!crate::seed_has_raster_template(&seed, &read).unwrap());
    // A template seed whose XAttribute lost its path sub-record.
    let native = v8::read(&template_seed(false), &read).unwrap();
    let mut model = native.models[0].clone();
    let payload = &mut model.graphic_aux[0].records[0].payload;
    payload.truncate(payload.len() - 40);
    assert!(crate::writer::raster_template::Template::capture(&model).is_none());
    let mut model = native.models[0].clone();
    model.control_pages[0].elements.remove(5);
    assert!(crate::writer::raster_template::Template::capture(&model).is_none());
    assert!(crate::writer::raster_template::Template::capture(&native.models[0]).is_some());
}
