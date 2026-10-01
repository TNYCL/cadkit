//! Sentetik seed ile yeni V8 dosyası üretimi ve bağımsız CFB denetimi.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
use super::*;
use cadkit_core::{Attribute, Color, Model};
use std::io::{Cursor, Read};

fn seed(three: bool) -> Vec<u8> {
    let mut file =
        ::cfb::CompoundFile::create_with_version(::cfb::Version::V3, Cursor::new(vec![])).unwrap();
    file.create_storage_all("Dgn-Md/#000000/Dgn^G").unwrap();
    let mut h = vec![0u8; 20];
    h.extend(compressed(&vec![0; 1576]).unwrap());
    file.create_stream("Dgn~H").unwrap().write_all(&h).unwrap();
    let header = crate::native::v8::tests::model_header_stream(three, 1000.0, "Synthetic");
    file.create_stream("Dgn-Md/#000000/Dgn~Mh")
        .unwrap()
        .write_all(&header)
        .unwrap();
    file.create_stream("Dgn-Md/#000000/Dgn^G/$1")
        .unwrap()
        .write_all(&page(&[], 3, 1).unwrap())
        .unwrap();
    file.flush().unwrap();
    file.into_inner().into_inner()
}
fn document(three: bool, entities: Vec<Entity>) -> Document {
    let mut d = Document::default();
    d.models.push(Model {
        is_3d: three,
        entities,
        ..Default::default()
    });
    d
}
fn line() -> Entity {
    Entity::new(EntityKind::Line {
        start: Point3::new(1.0, 2.0, 0.0),
        end: Point3::new(4.0, 5.0, 0.0),
    })
}

#[test]
fn new_2d_and_3d_geometry_with_attached_unicode_tags() {
    for three in [false, true] {
        let mut entity = line();
        entity.layer = Some("Ölçüm".into());
        entity.color = Color::Aci { index: 3 };
        entity.attributes.push(Attribute {
            tag: "Açıklama".into(),
            value: Value::Text("İstanbul, çığ, ölçü".into()),
            set: Some("Öznitelikler".into()),
            position: None,
            invisible: true,
        });
        entity.attributes.push(Attribute {
            tag: "Count".into(),
            value: Value::Int(7),
            set: Some("Öznitelikler".into()),
            position: None,
            invisible: true,
        });
        let d = document(three, vec![entity]);
        let bytes = write_v8(&d, &seed(three), &WriteOptions::default()).unwrap();
        let cf = ::cfb::CompoundFile::open_strict(Cursor::new(&bytes)).unwrap();
        assert!(cf.is_stream("Dgn~H"));
        let read = crate::read(&bytes, &ReadOptions::default()).unwrap();
        assert_eq!(read.models.len(), 1);
        assert_eq!(read.models[0].entities.len(), 1);
        let entity = &read.models[0].entities[0];
        assert_eq!(entity.layer.as_deref(), Some("Ölçüm"));
        assert_eq!(entity.kind, d.models[0].entities[0].kind);
        assert_eq!(entity.attributes.len(), 2);
        assert_eq!(entity.color, Color::Rgb { r: 0, g: 255, b: 0 });
        assert_eq!(
            entity.attributes[0].value,
            Value::Text("İstanbul, çığ, ölçü".into())
        );
        let raw = v8::read(&bytes, &ReadOptions::default()).unwrap();
        assert!(raw.models[0].graphic_pages.iter().all(|p| p.complete));
        assert!(raw.named_pages.iter().all(|p| p.complete));
    }
}

#[test]
fn integer_tags_beyond_i32_are_written_as_doubles() {
    let tagged = |n: i64| {
        let mut e = line();
        e.attributes.push(Attribute {
            tag: "Number".into(),
            value: Value::Int(n),
            set: Some("Ids".into()),
            position: None,
            invisible: true,
        });
        e
    };
    // One value needs more than 32 bits, so the whole tag definition becomes a double.
    let d = document(false, vec![tagged(5), tagged(12_345_678_901)]);
    let bytes = write_v8(&d, &seed(false), &WriteOptions::default()).unwrap();
    let read = crate::read(&bytes, &ReadOptions::default()).unwrap();
    let mut values: Vec<_> = read.models[0]
        .entities
        .iter()
        .map(|e| e.attributes[0].value.clone())
        .collect();
    values.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    assert_eq!(values, [Value::Float(12_345_678_901.0), Value::Float(5.0)]);

    let d = document(false, vec![tagged(i64::MAX)]);
    assert!(write_v8(&d, &seed(false), &WriteOptions::default()).is_err());
}

#[test]
fn planarity_tolerance_accepts_rounded_holes() {
    let ring = |a: f64, b: f64, z: f64| {
        vec![
            Point3::new(a, a, z),
            Point3::new(b, a, z),
            Point3::new(b, b, z),
            Point3::new(a, b, z),
            Point3::new(a, a, z),
        ]
    };
    // A hole 0.05 mm off the exterior plane, as 0.1 mm coordinate rounding produces.
    let d = document(
        true,
        vec![Entity::new(EntityKind::Polygon {
            exterior: ring(0.0, 10.0, 0.0),
            interiors: vec![ring(2.0, 3.0, 0.000_05)],
        })],
    );
    assert!(write_v8(&d, &seed(true), &WriteOptions::default()).is_ok());
    let strict = WriteOptions {
        planarity_tolerance: 1e-6,
        ..Default::default()
    };
    assert!(write_v8(&d, &seed(true), &strict).is_err());
    let invalid = WriteOptions {
        planarity_tolerance: f64::NAN,
        ..Default::default()
    };
    assert!(write_v8(&d, &seed(true), &invalid).is_err());
}

#[test]
fn ellipse_complex_and_holes_survive_readback() {
    let ring = |a: f64, b: f64| {
        vec![
            Point3::xy(a, a),
            Point3::xy(b, a),
            Point3::xy(b, b),
            Point3::xy(a, b),
            Point3::xy(a, a),
        ]
    };
    let d = document(
        true,
        vec![
            Entity::new(EntityKind::Circle {
                center: Point3::new(1.0, 2.0, 3.0),
                radius: 4.0,
                normal: Vec3::Z,
            }),
            Entity::new(EntityKind::Polygon {
                exterior: ring(0.0, 10.0),
                interiors: vec![ring(2.0, 3.0)],
            }),
        ],
    );
    let bytes = write_v8(&d, &seed(true), &WriteOptions::default()).unwrap();
    let read = crate::read(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(read.models[0].entities.len(), 2);
    match &read.models[0].entities[1].kind {
        EntityKind::Group { children, .. } => {
            assert_eq!(children.len(), 2);
            assert_eq!(children[1].props.get("dgn.hole"), Some(&Value::Bool(true)));
        }
        _ => panic!("expected hole-bearing cell"),
    }
}

#[test]
fn clear_seed_is_explicit_and_corrupt_seeds_fail() {
    let seed = seed(false);
    let d = document(false, vec![line()]);
    let bytes = write_v8(&d, &seed, &WriteOptions::default()).unwrap();
    assert!(write_v8(&d, &bytes, &WriteOptions::default()).is_err());
    let out = write_v8(
        &d,
        &bytes,
        &WriteOptions {
            clear_seed_model: true,
            ..Default::default()
        },
    )
    .unwrap();
    let read = crate::read(&out, &ReadOptions::default()).unwrap();
    assert_eq!(read.models[0].entities.len(), 1);
    for n in (0..seed.len()).step_by(113) {
        let _ = write_v8(&d, &seed[..n], &WriteOptions::default());
    }
    assert!(
        write_v8(
            &document(true, vec![line()]),
            &seed,
            &WriteOptions::default()
        )
        .is_err()
    );
    let mut options = WriteOptions::default();
    options.limits.max_input_bytes = 64;
    assert!(write_v8(&d, &seed, &options).is_err());
}

#[test]
fn repack_preserves_streams_and_storage_metadata() {
    let seed = seed(true);
    let bytes = repack_v8(&seed, &ReadOptions::default()).unwrap();
    let mut a = ::cfb::CompoundFile::open_strict(Cursor::new(seed)).unwrap();
    let mut b = ::cfb::CompoundFile::open_strict(Cursor::new(bytes)).unwrap();
    let entries: Vec<_> = a.walk().collect();
    for entry in entries {
        let copy = b.entry(entry.path()).unwrap();
        assert_eq!(entry.clsid(), copy.clsid());
        assert_eq!(entry.created(), copy.created());
        assert_eq!(entry.state_bits(), copy.state_bits());
        if entry.is_stream() {
            let mut x = vec![];
            let mut y = vec![];
            a.open_stream(entry.path())
                .unwrap()
                .read_to_end(&mut x)
                .unwrap();
            b.open_stream(entry.path())
                .unwrap()
                .read_to_end(&mut y)
                .unwrap();
            assert_eq!(x, y);
        }
    }
}

#[test]
fn corpus_repack_and_new_geometry() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/public/gdal/test_dgnv8.dgn");
    let Ok(seed) = std::fs::read(path) else {
        return;
    };
    exercise_real_seed(&seed);
}

fn exercise_real_seed(seed: &[u8]) {
    let read_options = ReadOptions {
        fallback_codepage: Some("windows-1254".into()),
        ..Default::default()
    };
    let original = crate::read(seed, &read_options).unwrap();
    let repacked = repack_v8(seed, &read_options).unwrap();
    let parsed = crate::read(&repacked, &read_options).unwrap();
    assert!(original.models == parsed.models);
    assert!(original.layers == parsed.layers);
    let source = document(original.models[0].is_3d, vec![line()]);
    let options = WriteOptions {
        clear_seed_model: true,
        ..Default::default()
    };
    let bytes = write_v8(&source, seed, &options).unwrap();
    assert!(bytes == write_v8(&source, seed, &options).unwrap());
    ::cfb::CompoundFile::open_strict(Cursor::new(&bytes)).unwrap();
    let parsed = crate::read(&bytes, &read_options).unwrap();
    assert_eq!(parsed.models[0].entities.len(), 1);
    assert_eq!(
        parsed.models[0].entities[0].kind,
        source.models[0].entities[0].kind
    );
}

#[test]
fn private_seed_when_supplied() {
    let Some(path) = std::env::var_os("CADKIT_PRIVATE_DGN") else {
        return;
    };
    let seed = std::fs::read(path).unwrap();
    exercise_real_seed(&seed);
}

#[test]
fn text_and_clockwise_wrapped_arc_use_neutral_conventions() {
    for three in [false, true] {
        for codepage in [None, Some("windows-1254".to_owned())] {
            let mut t = Entity::new(EntityKind::Text {
                position: Point3::xy(2., 3.),
                end_point: None,
                height: 1.5,
                rotation: 0.2,
                width_factor: 0.8,
                oblique: 0.0,
                value: "Çizgi ölçüsü".into(),
                style: None,
                halign: cadkit_core::HAlign::Left,
                valign: cadkit_core::VAlign::Baseline,
                normal: Vec3::Z,
            });
            t.attributes.push(Attribute {
                tag: "Etiket".into(),
                value: Value::Text("İğne".into()),
                set: Some("Test".into()),
                position: None,
                invisible: true,
            });
            let source = document(
                three,
                vec![
                    t,
                    Entity::new(EntityKind::Arc {
                        center: Point3::xy(0., 0.),
                        radius: 2.,
                        start_angle: 5.0,
                        end_angle: 1.0,
                        normal: Vec3::Z,
                    }),
                ],
            );
            let bytes = write_v8(
                &source,
                &seed(three),
                &WriteOptions {
                    codepage: codepage.clone(),
                    ..Default::default()
                },
            )
            .unwrap();
            let parsed = crate::read(
                &bytes,
                &ReadOptions {
                    fallback_codepage: codepage,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                parsed.models[0].entities[0].attributes[0].value,
                Value::Text("İğne".into())
            );
            if let EntityKind::Text {
                position,
                height,
                valign,
                value,
                ..
            } = &parsed.models[0].entities[0].kind
            {
                assert!((position.x - 2.).abs() < 1e-8 && (position.y - 3.).abs() < 1e-8);
                assert!((height - 1.5).abs() < 1e-8);
                assert_eq!(*valign, cadkit_core::VAlign::Baseline);
                assert_eq!(value, "Çizgi ölçüsü");
            } else {
                panic!("expected text");
            }
            if let EntityKind::Arc {
                start_angle,
                end_angle,
                ..
            } = parsed.models[0].entities[1].kind
            {
                assert!(
                    (cadkit_core::geom_ops::ccw_sweep(start_angle, end_angle)
                        - cadkit_core::geom_ops::ccw_sweep(5., 1.))
                    .abs()
                        < 1e-8
                );
            } else {
                panic!("expected arc");
            }
        }
    }
}
