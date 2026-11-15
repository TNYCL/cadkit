//! Export regressions using invented geometry and synthetic V8 tables only.

use super::*;
use crate::native::element::Rotation;

fn near_vec(a: Vec3, b: Vec3) {
    assert!((a.x - b.x).abs() < 1e-8, "x components differ");
    assert!((a.y - b.y).abs() < 1e-8, "y components differ");
    assert!((a.z - b.z).abs() < 1e-8, "z components differ");
}

#[test]
fn unnamed_sets_and_standalone_tags_retain_their_identity_and_values() {
    let mut standalone = Entity::new(EntityKind::Unknown {
        type_name: "dgn.type_37".into(),
    });
    standalone.attributes.push(
        Attribute::new("identifier", Value::Int(42))
            .with_position(Point3::xy(2., 3.))
            .with_invisible(true),
    );
    let mut named = line();
    named.attributes.push(
        Attribute::new("identifier", Value::Text("separate definition".into()))
            .with_set("CADKIT")
            .with_invisible(true),
    );
    let output = write_v8(
        &document(false, vec![standalone.clone(), named]),
        &seed(false),
        &WriteOptions::default(),
    )
    .unwrap();
    let parsed = crate::read(&output, &ReadOptions::default()).unwrap();
    assert_eq!(parsed.models[0].entities.len(), 2);
    assert_eq!(parsed.models[0].entities[0].kind, standalone.kind);
    assert_eq!(
        parsed.models[0].entities[0].attributes,
        standalone.attributes
    );
    assert_eq!(
        parsed.models[0].entities[1].attributes[0].set.as_deref(),
        Some("CADKIT")
    );
    assert!(
        parsed
            .warnings
            .iter()
            .all(|w| w.code != "dgn.tag_target_missing")
    );
}

#[test]
fn bulged_polylines_write_exact_arcs_in_both_directions_and_dimensions() {
    for three in [false, true] {
        for bulge in [-1.0, 1.0] {
            let mut start = cadkit_core::Vertex::at(Point3::xy(-1., 0.));
            start.bulge = bulge;
            let mut entity = Entity::new(EntityKind::Polyline {
                vertices: vec![start, cadkit_core::Vertex::at(Point3::xy(1., 0.))],
                closed: true,
                normal: Vec3::Z,
            });
            entity
                .attributes
                .push(Attribute::new("unit", Value::Int(42)).with_invisible(true));
            let output = write_v8(
                &document(three, vec![entity]),
                &seed(three),
                &WriteOptions::default(),
            )
            .unwrap();
            let parsed = crate::read(&output, &ReadOptions::default()).unwrap();
            let parent = &parsed.models[0].entities[0];
            assert_eq!(parent.attributes[0].value, Value::Int(42));
            let EntityKind::Group {
                children,
                group_kind,
                ..
            } = &parent.kind
            else {
                panic!("complex shape");
            };
            assert_eq!(*group_kind, GroupKind::ComplexShape);
            assert_eq!(children.len(), 2);
            let EntityKind::Arc {
                center,
                radius,
                normal,
                start_angle,
                end_angle,
            } = children[0].kind
            else {
                panic!("arc");
            };
            let (x, y, _) = cadkit_core::geom_ops::arbitrary_axes(normal);
            let mid = start_angle + cadkit_core::geom_ops::ccw_sweep(start_angle, end_angle) / 2.;
            let point = center.translated(
                x.scaled(radius * mid.cos())
                    .plus(y.scaled(radius * mid.sin())),
            );
            assert!(point.x.abs() < 1e-9);
            assert!((point.y + bulge).abs() < 1e-9);
            assert!(point.z.abs() < 1e-9);
            assert!((radius - 1.).abs() < 1e-9);
        }
    }
}

#[test]
fn quaternion_encoding_preserves_all_rotation_branches() {
    for axis in [
        Vec3::new(1., 0., 0.),
        Vec3::new(0., 1., 0.),
        Vec3::Z,
        Vec3::new(1., 2., 3.),
    ] {
        let axis = axis.normalized().unwrap();
        for angle in [0.2_f64, 2.5, -2.5] {
            // DGN stores the conjugate of the usual local-to-world quaternion.
            let s = (-angle / 2.).sin();
            let matrix =
                Rotation::Quaternion([(-angle / 2.).cos(), axis.x * s, axis.y * s, axis.z * s])
                    .matrix();
            let x = Vec3::new(matrix[0], matrix[3], matrix[6]);
            let y = Vec3::new(matrix[1], matrix[4], matrix[7]);
            let n = Vec3::new(matrix[2], matrix[5], matrix[8]);
            let mut bytes = vec![0; 32];
            rotation(&mut bytes, 0, x, y, n, true).unwrap();
            let parsed = Rotation::Quaternion(crate::le::f64s::<4>(&bytes, 0).unwrap()).matrix();
            for (actual, expected) in parsed.into_iter().zip(matrix) {
                assert!((actual - expected).abs() < 1e-12);
            }
        }
    }
}

#[test]
fn rotated_text_and_ellipses_keep_their_world_orientation() {
    for normal in [Vec3::Z, Vec3::new(0., 0., -1.), Vec3::new(1., 2., 3.)] {
        let normal = normal.normalized().unwrap();
        let (x, y, _) = cadkit_core::geom_ops::arbitrary_axes(normal);
        let major_axis = x.scaled(2.).plus(y);
        let entities = vec![
            Entity::new(EntityKind::Text {
                position: Point3::new(1., 2., 3.),
                end_point: None,
                height: 1.5,
                rotation: std::f64::consts::FRAC_PI_6,
                width_factor: 0.8,
                oblique: 0.,
                value: "Synthetic rotated text".into(),
                style: None,
                halign: cadkit_core::HAlign::Left,
                valign: cadkit_core::VAlign::Baseline,
                normal,
            }),
            Entity::new(EntityKind::Ellipse {
                center: Point3::new(1., 2., 3.),
                major_axis,
                ratio: 0.5,
                start_param: 0.,
                end_param: std::f64::consts::TAU,
                normal,
            }),
        ];
        let bytes = write_v8(
            &document(true, entities),
            &seed(true),
            &WriteOptions::default(),
        )
        .unwrap();
        let parsed = crate::read(&bytes, &ReadOptions::default()).unwrap();
        let EntityKind::Text {
            rotation,
            normal: actual,
            ..
        } = parsed.models[0].entities[0].kind
        else {
            panic!("expected text");
        };
        assert!((rotation - std::f64::consts::FRAC_PI_6).abs() < 1e-8);
        near_vec(actual, normal);
        let EntityKind::Ellipse {
            major_axis: actual_axis,
            normal: actual_normal,
            ..
        } = parsed.models[0].entities[1].kind
        else {
            panic!("expected ellipse");
        };
        near_vec(actual_axis, major_axis);
        near_vec(actual_normal, normal);
    }
}

fn seed_with_levels(levels: &[(u32, Option<&str>)]) -> Vec<u8> {
    let mut records = vec![];
    for (i, (level, name)) in levels.iter().enumerate() {
        // Type 95, table 1; level ID at +0x20, parent at +0x24, name linkage 1.
        let mut b = prefix(95, 0x30, i as u64 + 100, 1, 0.).unwrap();
        u32_at(&mut b, 0x20, *level).unwrap();
        u32_at(&mut b, 0x24, u32::MAX).unwrap();
        let links = name
            .map(|name| string_link(1, name).unwrap())
            .unwrap_or_default();
        records.push(finish(b, &links).unwrap());
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

fn check_level_roundtrip(levels: &[(u32, Option<&str>)]) {
    let seed = seed_with_levels(levels);
    let mut source = crate::read(&seed, &ReadOptions::default()).unwrap();
    let names: BTreeSet<_> = source.layers.iter().map(|l| &l.name).collect();
    assert_eq!(
        names.len(),
        levels.len(),
        "neutral level names must be unique"
    );
    source.models[0].entities = source
        .layers
        .iter()
        .map(|layer| {
            let mut entity = line();
            entity.layer = Some(layer.name.clone());
            entity
        })
        .collect();
    let bytes = write_v8(&source, &seed, &WriteOptions::default()).unwrap();
    let parsed = crate::read(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(
        parsed.layers, source.layers,
        "seed levels must not be duplicated or renamed"
    );
    for ((before, after), layer) in source.models[0]
        .entities
        .iter()
        .zip(&parsed.models[0].entities)
        .zip(&source.layers)
    {
        assert_eq!(before.layer, after.layer);
        assert_eq!(
            after.props.get("dgn.level_id"),
            Some(&Value::Int(layer.id.unwrap() as i64))
        );
    }
    let native = v8::read(&bytes, &ReadOptions::default()).unwrap();
    let table = native
        .named_pages
        .iter()
        .flat_map(|p| &p.elements)
        .find(|e| {
            crate::le::u32_at(&e.bytes, 0).map(|n| n & 0xffff) == Some(96)
                && crate::le::u32_at(&e.bytes, 12) == Some(1)
        })
        .unwrap();
    assert_eq!(
        crate::le::u32_at(&table.bytes, 0x20),
        Some(levels.len() as u32)
    );
}

#[test]
fn duplicate_and_unnamed_seed_levels_keep_their_ids_and_membership() {
    check_level_roundtrip(&[
        (1, Some("Shared")),
        (2, Some("Shared")),
        (3, Some("")),
        (4, None),
    ]);
}

#[test]
fn literal_names_cannot_collide_with_generated_level_aliases() {
    check_level_roundtrip(&[
        (9, Some("Shared (2)")),
        (1, Some("Shared")),
        (2, Some("Shared")),
    ]);
}

/// A seed whose second level table (as left by a 3D post-processor) repeats the codes of
/// the first one: elements written on a first-table level must read back on that level.
#[test]
fn a_second_level_table_with_repeated_codes_cannot_capture_new_elements() {
    let level = |id: u64, code: u32, name: &str| {
        let mut b = prefix(95, 0x30, id, 1, 0.).unwrap();
        u32_at(&mut b, 0x20, code).unwrap();
        u32_at(&mut b, 0x24, u32::MAX).unwrap();
        finish(b, &string_link(1, name).unwrap()).unwrap()
    };
    let header = |id: u64, count: u32| {
        let mut b = prefix(96, 0x28, id, 1, 0.).unwrap();
        u32_at(&mut b, 0x20, count).unwrap();
        finish(b, &[]).unwrap()
    };
    let first = vec![
        header(90, 3),
        level(100, 1, "Frame"),
        level(101, 2, "Area"),
        level(102, 64, "Default"),
    ];
    let second = vec![
        header(200, 2),
        level(201, 1, "Temp"),
        level(202, 2, "Surface"),
    ];
    let mut file = ::cfb::CompoundFile::open(Cursor::new(seed(false))).unwrap();
    file.create_storage_all("Dgn^Nm").unwrap();
    for (path, records) in [("Dgn^Nm/$1", &first), ("Dgn^Nm/$2", &second)] {
        file.create_stream(path)
            .unwrap()
            .write_all(&page(records, 3, 1).unwrap())
            .unwrap();
    }
    file.flush().unwrap();
    let seed = file.into_inner().into_inner();

    let mut doc = crate::read(&seed, &ReadOptions::default()).unwrap();
    doc.models[0].entities = ["Frame", "Area"]
        .into_iter()
        .map(|name| {
            let mut entity = line();
            entity.layer = Some(name.into());
            entity
        })
        .collect();
    let bytes = write_v8(&doc, &seed, &WriteOptions::default()).unwrap();
    let parsed = crate::read(&bytes, &ReadOptions::default()).unwrap();
    let layers: Vec<_> = parsed.models[0]
        .entities
        .iter()
        .map(|e| e.layer.clone().unwrap())
        .collect();
    assert_eq!(layers, ["Frame", "Area"]);

    let native = v8::read(&bytes, &ReadOptions::default()).unwrap();
    let records: Vec<_> = native
        .named_pages
        .iter()
        .flat_map(|p| &p.elements)
        .collect();
    let headers: Vec<_> = records
        .iter()
        .filter(|e| {
            crate::le::u32_at(&e.bytes, 0).map(|n| n & 0xffff) == Some(96)
                && crate::le::u32_at(&e.bytes, 12) == Some(1)
        })
        .collect();
    assert_eq!(headers.len(), 1, "one level table");
    let codes: Vec<_> = records
        .iter()
        .filter(|e| e.type_code() == 95 && crate::le::u32_at(&e.bytes, 12) == Some(1))
        .filter_map(|e| crate::le::u32_at(&e.bytes, 0x20))
        .collect();
    let unique: BTreeSet<_> = codes.iter().collect();
    assert_eq!(
        unique.len(),
        codes.len(),
        "level codes are unique: {codes:?}"
    );
    assert_eq!(
        crate::le::u32_at(&headers[0].bytes, 0x20),
        Some(codes.len() as u32)
    );
    // The second table's levels are kept under fresh codes.
    assert!(parsed.layers.iter().any(|l| l.name.starts_with("Temp")));
}

/// A file whose graphics carry the level sentinels (color and weight `0xFFFF_FFFF`, style
/// `0x7FFF_FFFF`), as 854 records of the private corpus do for color, reads into a document
/// that writes back into the same seed with the sentinels as stored. Before, the writer
/// rejected the color as an extended color and the weight as an index above 31.
#[test]
fn level_sentinels_from_a_read_file_are_written_back() {
    const COLOR: u32 = 0xffff_ffff;
    const WEIGHT: u32 = 0xffff_ffff;
    const STYLE: u32 = 0x7fff_ffff;
    let mut shape = line();
    shape.kind = EntityKind::Polyline {
        vertices: [(0., 0.), (4., 0.), (4., 3.)]
            .into_iter()
            .map(|(x, y)| cadkit_core::Vertex::at(Point3::xy(x, y)))
            .collect(),
        closed: true,
        normal: Vec3::Z,
    };
    let mut cell = Entity::new(EntityKind::Group {
        group_kind: GroupKind::Cell,
        name: None,
        origin: None,
        children: vec![line()],
    });
    cell.attributes.push(
        Attribute::new("Note", Value::Text("tagged".into()))
            .with_set("Set A")
            .with_invisible(true),
    );
    let options = WriteOptions::default().with_timestamp_ms(1.7e12);
    let written = write_v8(
        &document(false, vec![line(), shape, cell]),
        &seed(false),
        &options,
    )
    .unwrap();
    // Every graphic record of the file takes its symbology from its level.
    let native = v8::read(&written, &ReadOptions::default()).unwrap();
    let records: Vec<Vec<u8>> = native.models[0].graphic_pages[0]
        .elements
        .iter()
        .map(|r| {
            let mut b = r.bytes.clone();
            u32_at(&mut b, 0x2c, STYLE).unwrap();
            u32_at(&mut b, 0x30, WEIGHT).unwrap();
            u32_at(&mut b, 0x34, COLOR).unwrap();
            b
        })
        .collect();
    let mut file = ::cfb::CompoundFile::open(Cursor::new(written)).unwrap();
    file.create_stream("Dgn-Md/#000000/Dgn^G/$1")
        .unwrap()
        .write_all(&page(&records, 3, 1).unwrap())
        .unwrap();
    file.flush().unwrap();
    let source_file = file.into_inner().into_inner();

    let sentinels = |doc: &Document| -> Vec<(Value, Value, Value)> {
        let mut out = vec![];
        let mut todo: Vec<&Entity> = doc.models[0].entities.iter().collect();
        while let Some(e) = todo.pop() {
            out.push((
                e.props["dgn.color_index"].clone(),
                e.props["dgn.weight"].clone(),
                e.props["dgn.style"].clone(),
            ));
            if let EntityKind::Group { children, .. } = &e.kind {
                todo.extend(children);
            }
        }
        out
    };
    let expected = (
        Value::Int(i64::from(COLOR)),
        Value::Int(i64::from(WEIGHT)),
        Value::Int(i64::from(STYLE)),
    );
    let source = crate::read(&source_file, &ReadOptions::default()).unwrap();
    let read = sentinels(&source);
    assert_eq!(read.len(), 4);
    assert!(read.iter().all(|s| *s == expected));
    assert!(
        source.models[0]
            .entities
            .iter()
            .all(|e| e.color == cadkit_core::Color::ByLayer)
    );
    let back = write_v8(
        &source,
        &source_file,
        &options.clone().with_clear_seed_model(true),
    )
    .unwrap();
    let reread = crate::read(&back, &ReadOptions::default()).unwrap();
    assert_eq!(sentinels(&reread), read);
    // Preparing the document for the seed keeps them as well.
    let parsed = V8Seed::from_bytes(&source_file).unwrap();
    let prepared = parsed
        .prepare_document(&source, &SeedPrepareOptions::default())
        .unwrap();
    let back =
        write_v8_with_seed(&prepared, &parsed, &options.with_clear_seed_model(true)).unwrap();
    let reread = crate::read(&back, &ReadOptions::default()).unwrap();
    assert_eq!(sentinels(&reread), read);
    // A level cannot take its weight from itself: the sentinel on a layer is rejected.
    let mut layered = document(false, vec![line().on_layer("Walls")]);
    let mut layer = cadkit_core::Layer::new("Walls");
    layer
        .props
        .insert("dgn.weight".into(), Value::Int(i64::from(WEIGHT)));
    layered.layers.push(layer);
    assert!(write_v8(&layered, &seed(false), &WriteOptions::default()).is_err());
}

/// Seed palette for the level color checks: entry `i` is `[i, 3i, 200]` (a color table,
/// type 5 in level slot 1, stores color 255 first).
fn level_color_seed() -> Vec<u8> {
    use super::dictionary::{level, table};
    let mut colors = prefix(5, 0x22 + 768 + 2, 200, 1, 0.).unwrap();
    for (k, i) in std::iter::once(255usize).chain(0..255).enumerate() {
        colors[0x22 + 3 * k..0x25 + 3 * k].copy_from_slice(&[i as u8, (i * 3) as u8, 200]);
    }
    let colors = finish(colors, &[]).unwrap();
    let mut colored = level(212, 1, 1, "Colored");
    // 0x40 style (weak), 0x44 weight (weak), 0x48 and 0x4C color (equal in every corpus entry).
    for (offset, value) in [(0x40, u32::MAX), (0x44, 2), (0x48, 3), (0x4c, 3)] {
        u32_at(&mut colored, offset, value).unwrap();
    }
    // A 104-byte entry: its fields are not located, so 0x48 must not be read.
    let mut short = prefix(0x4440_0000 | 95, 0x68, 213, 1, 0.).unwrap();
    u32_at(&mut short, 0x20, 2).unwrap();
    u32_at(&mut short, 0x24, u32::MAX).unwrap();
    u32_at(&mut short, 0x48, 5).unwrap();
    let short = finish(short, &string_link(1, "Short").unwrap()).unwrap();
    let levels = [
        table(210, 1, 3),
        level(211, 64, 0, "Default"),
        colored,
        short,
    ];
    let mut file = ::cfb::CompoundFile::open(Cursor::new(seed(false))).unwrap();
    file.create_storage_all("Dgn^Nm").unwrap();
    for (n, records) in [(1, vec![colors]), (2, levels.to_vec())] {
        file.create_stream(format!("Dgn^Nm/${n}"))
            .unwrap()
            .write_all(&page(&records, 3, n).unwrap())
            .unwrap();
    }
    file.flush().unwrap();
    file.into_inner().into_inner()
}

/// The color index of the only graphic record of a written file.
fn written_color(bytes: &[u8]) -> u32 {
    let native = v8::read(bytes, &ReadOptions::default()).unwrap();
    let records: Vec<_> = native.models[0]
        .graphic_pages
        .iter()
        .flat_map(|p| &p.elements)
        .collect();
    assert_eq!(records.len(), 1);
    le::u32_at(&records[0].bytes, 0x34).unwrap()
}

/// A 240-byte level entry's color index (`0x48`) becomes the layer's color through the file
/// palette, with the raw fields in props; 104-byte entries are unchanged. An element with
/// the by-level sentinel stays `ByLayer` and is written back with the sentinel, and the
/// new layer data does not change what an unchanged document writes.
#[test]
fn level_color_comes_from_240_byte_level_entries() {
    const BY_LEVEL: i64 = 0xffff_ffff;
    let options = WriteOptions::default()
        .with_clear_seed_model(true)
        .with_timestamp_ms(1.7e12);
    let mut by_level = line().on_layer("Colored");
    by_level
        .props
        .insert("dgn.color_index".into(), Value::Int(BY_LEVEL));
    let source_file = write_v8(
        &document(false, vec![by_level]),
        &level_color_seed(),
        &options,
    )
    .unwrap();
    assert_eq!(written_color(&source_file), 0xffff_ffff);
    let source = crate::read(&source_file, &ReadOptions::default()).unwrap();
    let layer =
        |doc: &Document, name: &str| doc.layers.iter().find(|l| l.name == name).unwrap().clone();
    let colored = layer(&source, "Colored");
    assert_eq!(colored.color, Color::Rgb { r: 3, g: 9, b: 200 });
    assert_eq!(colored.props["dgn.color_index"], Value::Int(3));
    assert_eq!(colored.props["dgn.level_weight_raw"], Value::Int(2));
    assert_eq!(colored.props["dgn.level_style_raw"], Value::Int(-1));
    // The weak fields set no typed symbology.
    assert_eq!(colored.lineweight, cadkit_core::Lineweight::default());
    assert_eq!(colored.linetype, None);
    assert!(!colored.props.contains_key("dgn.weight"));
    assert!(!colored.props.contains_key("dgn.style"));
    let default = layer(&source, "Default");
    assert_eq!(default.color, Color::Rgb { r: 0, g: 0, b: 200 });
    assert_eq!(default.props["dgn.color_index"], Value::Int(0));
    let short = layer(&source, "Short");
    assert_eq!(short.color, Color::ByLayer);
    for key in [
        "dgn.color_index",
        "dgn.level_weight_raw",
        "dgn.level_style_raw",
    ] {
        assert!(!short.props.contains_key(key), "{key}");
    }
    let element = &source.models[0].entities[0];
    assert_eq!(element.color, Color::ByLayer);
    assert_eq!(element.props["dgn.color_index"], Value::Int(BY_LEVEL));

    // Written back into its own file, the element keeps the sentinel, and the output is the
    // one the document gives without the layer data (what the reader produced before).
    let back = write_v8(&source, &source_file, &options).unwrap();
    assert_eq!(written_color(&back), 0xffff_ffff);
    let mut without = source.clone();
    for l in &mut without.layers {
        l.color = Color::ByLayer;
        for key in [
            "dgn.color_index",
            "dgn.level_weight_raw",
            "dgn.level_style_raw",
        ] {
            l.props.remove(key);
        }
    }
    assert!(back == write_v8(&without, &source_file, &options).unwrap());
    let seed = V8Seed::from_bytes(&source_file).unwrap();
    let prepared = seed
        .prepare_document(&source, &SeedPrepareOptions::default())
        .unwrap();
    let back = write_v8_with_seed(&prepared, &seed, &options).unwrap();
    assert_eq!(written_color(&back), 0xffff_ffff);

    // A new ByLayer entity on the level takes the level's index, even when the layer's RGB
    // is not in the seed palette; without an index the RGB must still match exactly.
    let mut doc = source.clone();
    doc.models[0].entities = vec![line().on_layer("Colored")];
    assert_eq!(
        written_color(&write_v8(&doc, &source_file, &options).unwrap()),
        3
    );
    let colored = doc.layers.iter_mut().find(|l| l.name == "Colored").unwrap();
    colored.color = Color::Rgb { r: 1, g: 2, b: 3 };
    assert_eq!(
        written_color(&write_v8(&doc, &source_file, &options).unwrap()),
        3
    );
    let colored = doc.layers.iter_mut().find(|l| l.name == "Colored").unwrap();
    colored.props.remove("dgn.color_index");
    assert!(write_v8(&doc, &source_file, &options).is_err());
}

/// Mesh faces inherit the mesh's symbology as polygon rings do: level, color, line style,
/// weight, visibility and the native indexes (before, faces kept only level and color).
#[test]
fn mesh_faces_inherit_the_full_symbology() {
    let mut mesh = Entity::new(EntityKind::Mesh {
        vertices: vec![
            Point3::xy(0., 0.),
            Point3::xy(1., 0.),
            Point3::xy(1., 1.),
            Point3::xy(0., 1.),
        ],
        faces: vec![vec![0, 1, 2], vec![0, 2, 3]],
    });
    mesh.layer = Some("Mesh level".into());
    mesh.visible = false;
    for (key, value) in [("dgn.color_index", 5), ("dgn.weight", 4), ("dgn.style", 2)] {
        mesh.props.insert(key.into(), Value::Int(value));
    }
    let bytes = write_v8(
        &document(false, vec![mesh]),
        &seed(false),
        &WriteOptions::default(),
    )
    .unwrap();
    let doc = crate::read(&bytes, &ReadOptions::default()).unwrap();
    let EntityKind::Group { children, .. } = &doc.models[0].entities[0].kind else {
        panic!("a cell")
    };
    assert_eq!(children.len(), 2);
    for face in children {
        assert_eq!(face.layer.as_deref(), Some("Mesh level"));
        assert!(!face.visible);
        for (key, value) in [("dgn.color_index", 5), ("dgn.weight", 4), ("dgn.style", 2)] {
            assert_eq!(face.props[key], Value::Int(value), "{key}");
        }
    }
}
