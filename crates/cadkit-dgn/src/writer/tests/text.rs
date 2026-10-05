//! Text anchors are checked across every documented DGN justification code.

use super::*;

fn text_entity(code: u16, normal: Vec3, three: bool) -> Entity {
    let (halign, valign, _, _) = crate::map::justification(code);
    let mut e = Entity::new(EntityKind::Text {
        position: Point3::new(10., 20., if three { 30. } else { 0. }),
        end_point: None,
        height: 2.,
        rotation: 0.7,
        width_factor: 0.8,
        oblique: 0.,
        value: "Measured text".into(),
        style: None,
        halign,
        valign,
        normal,
    });
    e.props.insert("dgn.text_length".into(), Value::Float(12.5));
    e.props
        .insert("dgn.justification".into(), Value::Int(i64::from(code)));
    e
}

#[test]
fn all_documented_text_anchors_preserve_position_measurement_and_rotation() {
    for (three, normal) in [
        (false, Vec3::Z),
        (true, Vec3::Z),
        (true, Vec3::new(1., 2., 3.).normalized().unwrap()),
    ] {
        for code in 0..=14 {
            let source = text_entity(code, normal, three);
            let bytes = write_v8(
                &document(three, vec![source.clone()]),
                &seed(three),
                &WriteOptions::default(),
            )
            .unwrap();
            let parsed = crate::read(&bytes, &ReadOptions::default()).unwrap();
            let actual = &parsed.models[0].entities[0];
            let EntityKind::Text {
                position: expected,
                rotation: expected_angle,
                halign: expected_h,
                valign: expected_v,
                ..
            } = source.kind
            else {
                panic!("text");
            };
            let EntityKind::Text {
                position,
                rotation,
                halign,
                valign,
                ..
            } = actual.kind
            else {
                panic!("text");
            };
            assert!((position.x - expected.x).abs() < 1e-9);
            assert!((position.y - expected.y).abs() < 1e-9);
            assert!((position.z - expected.z).abs() < 1e-9);
            assert!((rotation - expected_angle).abs() < 1e-9);
            assert_eq!((halign, valign), (expected_h, expected_v));
            assert_eq!(actual.props["dgn.text_length"], Value::Float(12.5));
            assert_eq!(
                actual.props["dgn.justification"],
                Value::Int(i64::from(code))
            );
        }
    }
}

#[test]
fn text_measurement_is_validated_and_stale_justification_is_not_reused() {
    let mut e = text_entity(6, Vec3::Z, false);
    e.props.remove("dgn.text_length");
    assert!(
        write_v8(
            &document(false, vec![e.clone()]),
            &seed(false),
            &WriteOptions::default()
        )
        .is_err()
    );
    for bad in [f64::NAN, f64::INFINITY, -1., 0.] {
        e.props.insert("dgn.text_length".into(), Value::Float(bad));
        assert!(
            write_v8(
                &document(false, vec![e.clone()]),
                &seed(false),
                &WriteOptions::default()
            )
            .is_err()
        );
    }
    e.props.insert("dgn.text_length".into(), Value::Float(12.5));
    e.props.insert("dgn.justification".into(), Value::Int(2));
    let bytes = write_v8(
        &document(false, vec![e]),
        &seed(false),
        &WriteOptions::default(),
    )
    .unwrap();
    let parsed = crate::read(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(
        parsed.models[0].entities[0].props["dgn.justification"],
        Value::Int(6)
    );
}

#[test]
fn text_ranges_use_relative_spans_and_absolute_model_and_cell_bounds() {
    for three in [false, true] {
        let mut text = text_entity(2, Vec3::Z, three);
        if let EntityKind::Text { rotation, .. } = &mut text.kind {
            *rotation = 0.;
        }
        // The synthetic seed deliberately has global_origin = (5, 0, 0) UOR.
        let low = [10005, 20000, if three { 30000 } else { 0 }];
        let high = [22505, 22000, if three { 30000 } else { 0 }];
        let span = [12500, 2000, 0];
        for cell in [false, true] {
            let entity = if cell {
                Entity::new(EntityKind::Group {
                    group_kind: GroupKind::Cell,
                    name: Some("Synthetic text cell".into()),
                    origin: None,
                    children: vec![text.clone()],
                })
            } else {
                text.clone()
            };
            let bytes = write_v8(
                &document(three, vec![entity]),
                &seed(three),
                &WriteOptions::default(),
            )
            .unwrap();
            let native = v8::read(&bytes, &ReadOptions::default()).unwrap();
            let model = &native.models[0];
            let records: Vec<_> = model
                .graphic_pages
                .iter()
                .flat_map(|p| &p.elements)
                .collect();
            let text_record = records.iter().find(|r| r.type_code() == 17).unwrap();
            for axis in 0..3 {
                assert_eq!(
                    le::i64_at(&text_record.bytes, 0x38 + axis * 8),
                    Some(low[axis])
                );
                assert_eq!(
                    le::i64_at(&text_record.bytes, 0x50 + axis * 8),
                    Some(span[axis])
                );
            }
            assert_eq!(absolute_range(&text_record.bytes).unwrap(), [low, high]);
            let expected_extent: Vec<_> = low.into_iter().chain(high).collect();
            assert_eq!(
                model.header.as_ref().unwrap().extents.as_slice(),
                expected_extent
            );
            if cell {
                let cell_record = records.iter().find(|r| r.type_code() == 2).unwrap();
                assert_eq!(absolute_range(&cell_record.bytes).unwrap(), [low, high]);
            }
        }
    }
}

#[test]
fn rotated_and_tilted_text_range_encloses_its_measured_rectangle() {
    for normal in [Vec3::Z, Vec3::new(1., 2., 3.).normalized().unwrap()] {
        for rotation in [0., 0.7, -1.2, std::f64::consts::FRAC_PI_2] {
            for code in 0..=14 {
                let mut source = text_entity(code, normal, true);
                if let EntityKind::Text {
                    rotation: angle, ..
                } = &mut source.kind
                {
                    *angle = rotation;
                }
                let bytes = write_v8(
                    &document(true, vec![source]),
                    &seed(true),
                    &WriteOptions::default(),
                )
                .unwrap();
                let native = v8::read(&bytes, &ReadOptions::default()).unwrap();
                let model = &native.models[0];
                let record = model.graphic_pages[0]
                    .elements
                    .iter()
                    .find(|r| r.type_code() == 17)
                    .unwrap();
                let decoded = crate::native::decode_v8::decode(
                    record,
                    encoding_rs::WINDOWS_1252,
                    &ReadOptions::default().limits,
                );
                let crate::native::element::ElementData::Text(text) = decoded.data else {
                    panic!("text");
                };
                let m = text.rotation.matrix();
                let x = Vec3::new(m[0], m[3], m[6]);
                let y = Vec3::new(m[1], m[4], m[7]);
                let [ox, oy, oz] = text.origin;
                let dx = x.scaled(text.measured_length.unwrap());
                let dy = y.scaled(text.height);
                let corners = [
                    text.origin,
                    [ox + dx.x, oy + dx.y, oz + dx.z],
                    [ox + dy.x, oy + dy.y, oz + dy.z],
                    [ox + dx.x + dy.x, oy + dx.y + dy.y, oz + dx.z + dy.z],
                ];
                let [low, high] = absolute_range(&record.bytes).unwrap();
                for axis in 0..3 {
                    let min = corners
                        .iter()
                        .map(|p| p[axis])
                        .fold(f64::INFINITY, f64::min);
                    let max = corners
                        .iter()
                        .map(|p| p[axis])
                        .fold(f64::NEG_INFINITY, f64::max);
                    assert!(low[axis] as f64 <= min + 1e-8);
                    assert!(high[axis] as f64 >= max - 1e-8);
                    assert!(min - low[axis] as f64 <= 1.000001);
                    assert!(high[axis] as f64 - max <= 1.000001);
                }
                let expected: Vec<_> = low.into_iter().chain(high).collect();
                assert_eq!(model.header.as_ref().unwrap().extents.as_slice(), expected);
            }
        }
    }
}

fn seed_with_fonts(fonts: &[(u32, &str)]) -> Vec<u8> {
    let mut records = vec![];
    for (index, (number, name)) in fonts.iter().enumerate() {
        let name: Vec<_> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut b = prefix(95, 0x2e + name.len(), index as u64 + 100, 2, 0.).unwrap();
        u32_at(&mut b, 0x28, *number).unwrap();
        put(&mut b, 0x2c, &(name.len() as u16).to_le_bytes()).unwrap();
        put(&mut b, 0x2e, &name).unwrap();
        records.push(finish(b, &[]).unwrap());
    }
    let mut file = ::cfb::CompoundFile::open(Cursor::new(seed(false))).unwrap();
    file.create_storage_all("Dgn^Nm").unwrap();
    file.create_stream("Dgn^Nm/$1")
        .unwrap()
        .write_all(&page(&records, 3, 1).unwrap())
        .unwrap();
    file.flush().unwrap();
    file.into_inner().into_inner()
}

#[test]
fn font_numbers_and_styles_must_refer_to_the_selected_seed() {
    let blank = seed_with_fonts(&[(23, "Synthetic font"), (41, "Other font")]);
    let mut source = text_entity(2, Vec3::Z, false);
    let write = |entity: &Entity, seed: &[u8]| {
        write_v8(
            &document(false, vec![entity.clone()]),
            seed,
            &WriteOptions::default(),
        )
    };
    source
        .props
        .insert("dgn.font_number".into(), Value::Int(23));
    assert!(write(&source, &blank).is_ok());
    if let EntityKind::Text { style, .. } = &mut source.kind {
        *style = Some("Synthetic font".into());
    }
    assert!(write(&source, &blank).is_ok());
    source
        .props
        .insert("dgn.font_number".into(), Value::Int(41));
    assert!(write(&source, &blank).is_err());
    source.props.remove("dgn.font_number");
    let bytes = write(&source, &blank).unwrap();
    let parsed = crate::read(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(
        parsed.models[0].entities[0].props["dgn.font_number"],
        Value::Int(23)
    );
    if let EntityKind::Text { style, .. } = &mut source.kind {
        *style = None;
    }
    for invalid in [
        Value::Int(-1),
        Value::Int(999),
        Value::Float(23.),
        Value::Text("23".into()),
    ] {
        source.props.insert("dgn.font_number".into(), invalid);
        assert!(write(&source, &blank).is_err());
    }
    source.props.insert("dgn.font_number".into(), Value::Int(0));
    assert!(write(&source, &blank).is_ok());
    let table_free = seed(false);
    let bytes = write(&source, &table_free).unwrap();
    let parsed = crate::read(&bytes, &ReadOptions::default()).unwrap();
    assert!(write(&parsed.models[0].entities[0], &table_free).is_ok());
    source
        .props
        .insert("dgn.font_number".into(), Value::Int(23));
    assert!(write(&source, &table_free).is_err());
}

#[test]
fn external_font_referenced_by_seed_graphics_is_retained_without_a_named_table() {
    let original = write_v8(
        &document(false, vec![text_entity(2, Vec3::Z, false)]),
        &seed(false),
        &WriteOptions::default(),
    )
    .unwrap();
    let native = v8::read(&original, &ReadOptions::default()).unwrap();
    let mut records: Vec<_> = native.models[0].graphic_pages[0]
        .elements
        .iter()
        .map(|r| r.bytes.clone())
        .collect();
    u32_at(&mut records[0], 0x68, 127).unwrap();
    let mut file = ::cfb::CompoundFile::open(Cursor::new(original)).unwrap();
    file.create_stream("Dgn-Md/#000000/Dgn^G/$1")
        .unwrap()
        .write_all(&page(&records, 3, 1).unwrap())
        .unwrap();
    file.flush().unwrap();
    let referenced = file.into_inner().into_inner();
    let source = crate::read(&referenced, &ReadOptions::default()).unwrap();
    assert_eq!(
        source.models[0].entities[0].props["dgn.font_number"],
        Value::Int(127)
    );
    let bytes = write_v8(
        &source,
        &referenced,
        &WriteOptions {
            clear_seed_model: true,
            ..Default::default()
        },
    )
    .unwrap();
    let parsed = crate::read(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(
        parsed.models[0].entities[0].props["dgn.font_number"],
        Value::Int(127)
    );
    assert!(write_v8(&source, &seed(false), &WriteOptions::default()).is_err());
}
