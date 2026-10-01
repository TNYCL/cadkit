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
