//! Byte layouts MicroStation reads: element ranges, string payloads, displayed tags and
//! retained seed control records. Expected values are computed from the inputs, never
//! from the writer's own helpers.

use super::*;
use cadkit_core::{AttributeDisplay, HAlign, VAlign};

/// Graphic records of the written model, decoded by the native reader.
fn graphics(bytes: &[u8]) -> Vec<RawElement> {
    let native = v8::read(bytes, &ReadOptions::default()).unwrap();
    native.models[0]
        .graphic_pages
        .iter()
        .flat_map(|p| p.elements.iter().cloned())
        .collect()
}

fn slots(b: &[u8]) -> ([i64; 3], [i64; 3]) {
    let low = [0, 1, 2].map(|i| le::i64_at(b, 0x38 + i * 8).unwrap());
    let extent = [0, 1, 2].map(|i| le::i64_at(b, 0x50 + i * 8).unwrap());
    (low, extent)
}

/// Seed of either dimension with a font table (numbers and names as given).
fn seed_with_fonts(three: bool, fonts: &[(u32, &str)]) -> Vec<u8> {
    let mut records = vec![];
    for (index, (number, name)) in fonts.iter().enumerate() {
        let name: Vec<_> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut b = prefix(95, 0x2e + name.len(), index as u64 + 100, 2, 0.).unwrap();
        u32_at(&mut b, 0x28, *number).unwrap();
        put(&mut b, 0x2c, &(name.len() as u16).to_le_bytes()).unwrap();
        put(&mut b, 0x2e, &name).unwrap();
        records.push(finish(b, &[]).unwrap());
    }
    let mut file = ::cfb::CompoundFile::open(Cursor::new(seed(three))).unwrap();
    file.create_storage_all("Dgn^Nm").unwrap();
    file.create_stream("Dgn^Nm/$1")
        .unwrap()
        .write_all(&page(&records, 3, 1).unwrap())
        .unwrap();
    file.flush().unwrap();
    file.into_inner().into_inner()
}

#[test]
fn graphic_ranges_store_the_low_corner_and_a_nonnegative_extent() {
    // Every kind below y = 0, where an absolute high corner reads as a negative extent.
    let points = [
        Point3::new(-12.5, -40.0, 0.0),
        Point3::new(3.25, -25.5, 0.0),
        Point3::new(7.0, -31.0, 0.0),
    ];
    for three in [false, true] {
        let z = if three { 2.75 } else { 0.0 };
        let lift = |p: Point3| Point3::new(p.x, p.y, z);
        let ring: Vec<Point3> = points.iter().copied().map(lift).collect();
        let entities = vec![
            Entity::new(EntityKind::Line {
                start: ring[0],
                end: ring[1],
            }),
            Entity::new(EntityKind::Polyline {
                vertices: ring.iter().copied().map(cadkit_core::Vertex::at).collect(),
                closed: false,
                normal: Vec3::Z,
            }),
            Entity::new(EntityKind::Polyline {
                vertices: ring.iter().copied().map(cadkit_core::Vertex::at).collect(),
                closed: true,
                normal: Vec3::Z,
            }),
            Entity::new(EntityKind::Polygon {
                exterior: vec![
                    lift(Point3::new(-20.0, -60.0, 0.0)),
                    lift(Point3::new(-5.0, -60.0, 0.0)),
                    lift(Point3::new(-5.0, -45.0, 0.0)),
                    lift(Point3::new(-20.0, -45.0, 0.0)),
                    lift(Point3::new(-20.0, -60.0, 0.0)),
                ],
                interiors: vec![vec![
                    lift(Point3::new(-15.0, -55.0, 0.0)),
                    lift(Point3::new(-15.0, -50.0, 0.0)),
                    lift(Point3::new(-10.0, -50.0, 0.0)),
                    lift(Point3::new(-10.0, -55.0, 0.0)),
                    lift(Point3::new(-15.0, -55.0, 0.0)),
                ]],
            }),
            Entity::new(EntityKind::Circle {
                center: lift(Point3::new(-30.0, -70.0, 0.0)),
                radius: 1.5,
                normal: Vec3::Z,
            }),
        ];
        // Expected absolute bounds per entity in UOR (seed: 1000 UOR/m, origin x + 5).
        let uor = |v: f64, axis: usize| v * 1000.0 + if axis == 0 { 5.0 } else { 0.0 };
        let bounds = |pts: &[Point3]| {
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            for p in pts {
                for (axis, v) in [p.x, p.y, p.z].into_iter().enumerate() {
                    lo[axis] = lo[axis].min(uor(v, axis));
                    hi[axis] = hi[axis].max(uor(v, axis));
                }
            }
            (lo, hi)
        };
        let expected = [
            bounds(&ring[..2]),
            bounds(&ring),
            bounds(&ring),
            bounds(&[
                lift(Point3::new(-20.0, -60.0, 0.0)),
                lift(Point3::new(-5.0, -45.0, 0.0)),
            ]),
            bounds(&[
                lift(Point3::new(-31.5, -71.5, 0.0)),
                lift(Point3::new(-28.5, -68.5, 0.0)),
            ]),
        ];
        let bytes = write_v8(
            &document(three, entities),
            &seed(three),
            &WriteOptions::default(),
        )
        .unwrap();
        let records = graphics(&bytes);
        let tops: Vec<_> = records
            .iter()
            .filter(|r| !r.is_complex_component())
            .collect();
        assert_eq!(tops.len(), expected.len());
        let mut union_lo = [i64::MAX; 3];
        let mut union_hi = [i64::MIN; 3];
        for (record, (lo, hi)) in tops.iter().zip(expected) {
            let (low, extent) = slots(&record.bytes);
            for axis in 0..if three { 3 } else { 2 } {
                assert!(extent[axis] >= 0, "extent, not an absolute corner");
                assert!(low[axis] as f64 <= lo[axis] + 1e-6);
                assert!((low[axis] + extent[axis]) as f64 >= hi[axis] - 1e-6);
                assert!((low[axis] + extent[axis]) as f64 - hi[axis] <= 1.0 + 1e-6);
                union_lo[axis] = union_lo[axis].min(low[axis]);
                union_hi[axis] = union_hi[axis].max(low[axis] + extent[axis]);
            }
        }
        // Cell components carry their own low corner and extent too.
        for record in records.iter().filter(|r| r.is_complex_component()) {
            let (_, extent) = slots(&record.bytes);
            assert!(extent.iter().all(|e| *e >= 0));
        }
        // The model header alone keeps absolute low/high corners.
        let native = v8::read(&bytes, &ReadOptions::default()).unwrap();
        let extents = native.models[0].header.as_ref().unwrap().extents;
        for axis in 0..if three { 3 } else { 2 } {
            assert_eq!(extents[axis], union_lo[axis]);
            assert_eq!(extents[axis + 3], union_hi[axis]);
        }
        // And the reader returns the same absolute bounds.
        let read = crate::read(&bytes, &ReadOptions::default()).unwrap();
        assert_eq!(read.models[0].entities.len(), expected.len());
    }
}

#[test]
fn text_payloads_and_string_linkages_count_no_terminator() {
    let value = "Dükkân 40,25 m²";
    let units = value.encode_utf16().count();
    for codepage in [None, Some("windows-1254")] {
        let mut text = Entity::new(EntityKind::Text {
            position: Point3::new(1.0, -2.0, 0.0),
            height: 0.25,
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            value: value.into(),
            style: None,
            halign: HAlign::Left,
            valign: VAlign::Baseline,
            normal: Vec3::Z,
            end_point: None,
        });
        text.layer = Some("Oda Adı".into());
        let options = WriteOptions {
            codepage: codepage.map(Into::into),
            ..WriteOptions::default()
        };
        let bytes = write_v8(&document(false, vec![text]), &seed(false), &options).unwrap();
        let record = graphics(&bytes)
            .into_iter()
            .find(|r| r.type_code() == 17)
            .unwrap();
        let b = &record.bytes;
        let length = usize::from(le::u16_at(b, 0x6e).unwrap());
        let payload = &b[0xaa..0xaa + length];
        match codepage {
            None => {
                assert_eq!(length, 2 + 2 * units);
                assert_eq!(&payload[..2], &[0xff, 0xfd]);
                assert_ne!(&payload[length - 2..], &[0, 0], "no counted NUL unit");
            }
            Some(_) => {
                let (expected, _, _) = encoding_rs::WINDOWS_1254.encode(value);
                assert_eq!(payload, &*expected);
                // The code page linkage exactly as MicroStation writes it (1254 = 0x04e6).
                let links = &b[usize::try_from(le::u32_at(b, 8).unwrap()).unwrap() * 2..];
                assert_eq!(
                    &links[..16],
                    &[
                        7, 0x10, 0xd4, 0x80, 0, 2, 0, 0, 0, 0, 0, 0, 0xe6, 0x04, 0, 0
                    ]
                );
            }
        }
        let read = crate::read(&bytes, &ReadOptions::default()).unwrap();
        let EntityKind::Text { value: back, .. } = &read.models[0].entities[0].kind else {
            panic!("text")
        };
        assert_eq!(back, value);
        // The new level's name linkage: `ff fd` + UTF-16, length without terminator.
        let native = v8::read(&bytes, &ReadOptions::default()).unwrap();
        let name: Vec<u8> = "Oda Adı"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let level = native
            .named_pages
            .iter()
            .flat_map(|p| &p.elements)
            .find(|r| r.bytes.windows(name.len()).any(|w| w == name.as_slice()))
            .unwrap();
        let at = level
            .bytes
            .windows(4)
            .position(|w| w == [0xd2, 0x56, 1, 0])
            .unwrap()
            - 2;
        let stored = le::u32_at(&level.bytes, at + 8).unwrap() as usize;
        assert_eq!(stored, 2 + name.len());
    }
}

#[test]
fn text_without_a_style_or_number_uses_font_zero() {
    let seed = seed_with_fonts(false, &[(1025, "AdobeHeitiStd-Regular"), (1042, "Arial")]);
    let text = Entity::new(EntityKind::Text {
        position: Point3::new(0.0, 0.0, 0.0),
        height: 0.25,
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        value: "N.O.A".into(),
        style: None,
        halign: HAlign::Left,
        valign: VAlign::Baseline,
        normal: Vec3::Z,
        end_point: None,
    });
    let bytes = write_v8(
        &document(false, vec![text]),
        &seed,
        &WriteOptions::default(),
    )
    .unwrap();
    let record = graphics(&bytes)
        .into_iter()
        .find(|r| r.type_code() == 17)
        .unwrap();
    assert_eq!(le::u32_at(&record.bytes, 0x68), Some(0));
}

/// Room outline with a displayed name tag on its own level, as the company files hold it.
fn room_with_name(display: bool) -> Entity {
    let mut room = Entity::new(EntityKind::Polyline {
        vertices: [(2.0, -8.0), (6.0, -8.0), (6.0, -5.0), (2.0, -5.0)]
            .into_iter()
            .map(|(x, y)| cadkit_core::Vertex::at(Point3::new(x, y, 3.0)))
            .collect(),
        closed: true,
        normal: Vec3::Z,
    });
    room.layer = Some("OdaAlani".into());
    room.props.insert("dgn.color_index".into(), Value::Int(159));
    room.props.insert("dgn.weight".into(), Value::Int(1));
    let mut props = cadkit_core::Props::new();
    props.insert("dgn.color_index".into(), Value::Int(159));
    props.insert("dgn.weight".into(), Value::Int(1));
    props.insert("dgn.properties".into(), Value::Int(0x0e00));
    props.insert("dgn.type_flags".into(), Value::Int(0x00c0));
    props.insert("dgn.graphic_group".into(), Value::Int(7));
    room.attributes.push(Attribute {
        tag: "Oda Adı".into(),
        value: Value::Text("DÜKKAN".into()),
        set: Some("Oda Bilgisi".into()),
        position: Some(Point3::new(4.0, -6.5, 3.0)),
        invisible: !display,
        layer: Some("OdaAdi".into()),
        display: Some(AttributeDisplay {
            height: 0.25,
            width: 0.25,
            style: Some("Arial".into()),
            halign: HAlign::Center,
            valign: VAlign::Middle,
            rotation: 0.0,
        }),
        props,
    });
    room
}

#[test]
fn displayed_tags_follow_the_microstation_layout() {
    let seed = seed_with_fonts(true, &[(1025, "AdobeHeitiStd-Regular"), (1042, "Arial")]);
    let options = WriteOptions {
        codepage: Some("windows-1254".into()),
        ..WriteOptions::default()
    };
    let bytes = write_v8(&document(true, vec![room_with_name(true)]), &seed, &options).unwrap();
    let records = graphics(&bytes);
    let host = records.iter().find(|r| r.type_code() == 6).unwrap();
    let tag = records.iter().find(|r| r.type_code() == 37).unwrap();
    let b = &tag.bytes;
    let uor = 1000.0;
    // Type word flags, own level, symbology, MicroStation property bits, graphic group.
    assert_eq!(le::u32_at(b, 0), Some(0x10c0_0025));
    assert_ne!(le::u32_at(b, 0x0c), le::u32_at(&host.bytes, 0x0c));
    assert_eq!(le::u32_at(b, 0x20), Some(7));
    assert_eq!(le::u32_at(b, 0x28), Some(0x0e00));
    assert_eq!(le::u32_at(b, 0x30), Some(1));
    assert_eq!(le::u32_at(b, 0x34), Some(159));
    // Display constants, origin at the host's first vertex and offset to the anchor.
    assert_eq!(le::u16_at(b, 0x98), Some(3));
    assert_eq!(le::u16_at(b, 0x9a), Some(11));
    let origin = [2.0 * uor + 5.0, -8.0 * uor, 3.0 * uor];
    let anchor = [4.0 * uor + 5.0, -6.5 * uor, 3.0 * uor];
    for axis in 0..3 {
        let o = le::f64_at(b, 0xa0 + axis * 8).unwrap();
        let d = le::f64_at(b, 0xb8 + axis * 8).unwrap();
        assert!((o - origin[axis]).abs() < 1e-6);
        assert!((o + d - anchor[axis]).abs() < 1e-6);
    }
    assert_eq!(le::u16_at(b, 0xd2), Some(1), "text value");
    assert_eq!(le::u32_at(b, 0xd4), Some(0x0200_0000));
    for o in [0xf0, 0xf8] {
        assert!((le::f64_at(b, o).unwrap() - 0.25 * uor / 0.006).abs() < 1e-6);
    }
    let q = [0, 1, 2, 3].map(|i| le::f64_at(b, 0x100 + i * 8).unwrap());
    assert_eq!(q, [-1.0, 0.0, 0.0, 0.0]);
    assert_eq!(le::u32_at(b, 0x12c), Some(1042), "Arial by name");
    assert_eq!(le::u32_at(b, 0x130), Some(7), "center-middle");
    // Value: Windows-1254 bytes and a counted NUL; linkages 10 bytes after, 2-aligned.
    let value = [0x44, 0xdc, 0x4b, 0x4b, 0x41, 0x4e, 0x00];
    assert_eq!(le::u32_at(b, 0x138), Some(value.len() as u32));
    assert_eq!(&b[0x140..0x140 + value.len()], &value);
    let links = usize::try_from(le::u32_at(b, 8).unwrap()).unwrap() * 2;
    assert_eq!(links, (0x14a + value.len() + 1) & !1);
    let set = &b[links..links + 24];
    assert_eq!(
        &set[..12],
        &[0x0b, 0x10, 0xd0, 0x56, 0x17, 0x27, 0, 0, 0, 2, 1, 0]
    );
    let target = &b[links + 24..links + 80];
    assert_eq!(
        &target[..20],
        &[
            0x1b, 0x10, 0xd0, 0x56, 0x10, 0x27, 1, 0, 2, 9, 1, 0, 1, 0, 0, 0, 0, 0, 1, 0
        ]
    );
    assert_eq!(le::u64_at(target, 20), host.id());
    assert!(target[28..].iter().all(|v| *v == 0));
    assert_eq!(b.len(), links + 80);
    // Range: the text box around the anchor, two text heights tall.
    let (low, extent) = slots(b);
    assert_eq!(extent[1], (2.0 * 0.25 * uor) as i64);
    assert_eq!(extent[0], (6.0 * 0.25 * uor) as i64);
    assert_eq!(low[1], (anchor[1] - 0.25 * uor) as i64);
    assert_eq!(extent[2], 0);

    // The reader returns the tag on its host with its own level and presentation.
    let read = crate::read(&bytes, &ReadOptions::default()).unwrap();
    assert!(
        read.warnings
            .iter()
            .all(|w| w.code != "dgn.tag_target_missing")
    );
    let room = &read.models[0].entities[0];
    assert_eq!(room.layer.as_deref(), Some("OdaAlani"));
    let a = &room.attributes[0];
    assert_eq!(a.value, Value::Text("DÜKKAN".into()));
    assert_eq!(a.layer.as_deref(), Some("OdaAdi"));
    assert!(!a.invisible);
    let d = a.display.as_ref().unwrap();
    assert!((d.height - 0.25).abs() < 1e-9 && (d.width - 0.25).abs() < 1e-9);
    assert_eq!((d.halign, d.valign), (HAlign::Center, VAlign::Middle));
    assert_eq!(d.style.as_deref(), Some("Arial"));
    assert_eq!(d.rotation, 0.0);
    let p = a.position.unwrap();
    assert!((p.x - 4.0).abs() < 1e-9 && (p.y + 6.5).abs() < 1e-9);

    // Rewriting what was read reproduces the tag record byte for byte (but ids/time).
    let again = write_v8(&read, &seed, &options).unwrap();
    let tag_again = graphics(&again)
        .into_iter()
        .find(|r| r.type_code() == 37)
        .unwrap();
    assert_eq!(tag_again.bytes.len(), b.len());
    assert_eq!(&tag_again.bytes[0x20..0xa0], &b[0x20..0xa0]);
    assert_eq!(&tag_again.bytes[0xd2..0x138], &b[0xd2..0x138]);
}

#[test]
fn hidden_tags_keep_an_empty_range_and_the_hidden_display_flag() {
    let seed = seed_with_fonts(true, &[(1042, "Arial")]);
    let bytes = write_v8(
        &document(true, vec![room_with_name(false)]),
        &seed,
        &WriteOptions::default(),
    )
    .unwrap();
    let tag = graphics(&bytes)
        .into_iter()
        .find(|r| r.type_code() == 37)
        .unwrap();
    let b = &tag.bytes;
    assert_eq!(le::u32_at(b, 0x28).map(|v| v & 0x80), Some(0x80));
    assert_eq!(le::u16_at(b, 0x9a), Some(3));
    assert_eq!(le::u32_at(b, 0xd4), Some(0));
    assert_eq!(slots(b), ([0; 3], [0; 3]));
}

/// A seed whose model control stream holds a coordinate-system-like record, a settings
/// record and a record that names a seed graphic, with an auxiliary record and counter.
fn seed_with_controls() -> Vec<u8> {
    let mut graphic = header(3, 0x98, 300, 0, 0., &line(), false, 0).unwrap();
    range(&mut graphic, &[[0., 0., 0.], [1000., 1000., 0.]]).unwrap();
    let graphic = finish(graphic, &[]).unwrap();
    let mut gcs = prefix(66, 0x60, 400, 20, 0.).unwrap();
    put(&mut gcs, 0x28, b"opaque synthetic coordinate system").unwrap();
    let gcs = finish(gcs, &[]).unwrap();
    let mut settings = prefix(57, 0x40, 401, 5, 0.).unwrap();
    put(&mut settings, 0x20, b"opaque settings").unwrap();
    let settings = finish(settings, &[]).unwrap();
    let mut pointing = prefix(66, 0x40, 402, 21, 0.).unwrap();
    u64_at(&mut pointing, 0x30, 300).unwrap();
    let pointing = finish(pointing, &[]).unwrap();
    let mut aux = vec![0; 28];
    u32_at(&mut aux, 0, 0xa11b).unwrap();
    u32_at(&mut aux, 4, 4).unwrap();
    u32_at(&mut aux, 8, 0xaa).unwrap();
    u64_at(&mut aux, 16, 400).unwrap();
    aux.extend([9, 8, 7, 6]);
    let mut aux_page = vec![0; 16];
    u32_at(&mut aux_page, 0, 1).unwrap();
    u32_at(&mut aux_page, 4, 3).unwrap();
    u32_at(&mut aux_page, 8, 2).unwrap();
    u32_at(&mut aux_page, 12, 1).unwrap();
    aux_page.extend(compressed(&aux).unwrap());
    let mut file = ::cfb::CompoundFile::open(Cursor::new(seed(false))).unwrap();
    for storage in ["Dgn^C", "Dgn^CA"] {
        file.create_storage_all(format!("Dgn-Md/#000000/{storage}"))
            .unwrap();
    }
    for (path, data) in [
        ("Dgn-Md/#000000/Dgn^G/$1", page(&[graphic], 3, 1).unwrap()),
        (
            "Dgn-Md/#000000/Dgn^C/$1",
            page(&[gcs, settings, pointing], 3, 1).unwrap(),
        ),
        ("Dgn-Md/#000000/Dgn^CA/$2", aux_page),
        ("Dgn-Md/#000000/Dgn^CA/^AH", 3u32.to_le_bytes().to_vec()),
    ] {
        file.create_stream(path).unwrap().write_all(&data).unwrap();
    }
    file.flush().unwrap();
    file.into_inner().into_inner()
}

#[test]
fn seed_control_records_survive_unless_they_name_a_seed_graphic() {
    let seed = seed_with_controls();
    let source = v8::read(&seed, &ReadOptions::default()).unwrap();
    let original: Vec<_> = source.models[0]
        .control_pages
        .iter()
        .flat_map(|p| p.elements.iter().map(|r| r.bytes.clone()))
        .collect();
    assert_eq!(original.len(), 3);
    let clear = WriteOptions {
        clear_seed_model: true,
        ..WriteOptions::default()
    };
    let bytes = write_v8(&document(false, vec![line()]), &seed, &clear).unwrap();
    let native = v8::read(&bytes, &ReadOptions::default()).unwrap();
    let controls: Vec<_> = native.models[0]
        .control_pages
        .iter()
        .flat_map(|p| p.elements.iter().map(|r| r.bytes.clone()))
        .collect();
    assert_eq!(
        controls,
        original[..2],
        "kept byte for byte, dangling one dropped"
    );
    let aux: Vec<_> = native.models[0]
        .control_aux
        .iter()
        .flat_map(|p| &p.records)
        .collect();
    assert_eq!(aux.len(), 1);
    assert_eq!(aux[0].element_id, 400);
    assert_eq!(aux[0].payload, [9, 8, 7, 6]);
    let mut cf = ::cfb::CompoundFile::open(Cursor::new(&bytes)).unwrap();
    let mut counter = vec![];
    cf.open_stream("Dgn-Md/#000000/Dgn^CA/^AH")
        .unwrap()
        .read_to_end(&mut counter)
        .unwrap();
    assert_eq!(counter, 3u32.to_le_bytes());

    // Opting out drops every control record as before.
    let drop = WriteOptions {
        preserve_seed_controls: false,
        ..clear
    };
    let bytes = write_v8(&document(false, vec![line()]), &seed, &drop).unwrap();
    let native = v8::read(&bytes, &ReadOptions::default()).unwrap();
    assert!(
        native.models[0]
            .control_pages
            .iter()
            .all(|p| p.elements.is_empty())
    );
}
