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
    standalone.attributes.push(Attribute {
        tag: "identifier".into(),
        value: Value::Int(42),
        set: None,
        position: Some(Point3::xy(2., 3.)),
        invisible: true,
    });
    let mut named = line();
    named.attributes.push(Attribute {
        tag: "identifier".into(),
        value: Value::Text("separate definition".into()),
        set: Some("CADKIT".into()),
        position: None,
        invisible: true,
    });
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
            entity.attributes.push(Attribute {
                tag: "unit".into(),
                value: Value::Int(42),
                set: None,
                position: None,
                invisible: true,
            });
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
