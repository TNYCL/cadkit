//! New/edit/save/reopen scenarios for a direct library consumer.
use super::*;
use cadkit_core::{HAlign, VAlign};

#[test]
fn application_must_match_seed_units_and_dimension_without_implicit_conversion() {
    let blank = seed(false);
    let mut doc = document(false, vec![line()]);
    doc.units.unit = cadkit_core::LengthUnit::Meter;
    assert!(write_v8(&doc, &blank, &WriteOptions::default()).is_ok());
    doc.units.unit = cadkit_core::LengthUnit::Millimeter;
    assert!(write_v8(&doc, &blank, &WriteOptions::default()).is_err());
    doc.units.unit = cadkit_core::LengthUnit::Meter;
    doc.models[0].is_3d = true;
    assert!(write_v8(&doc, &blank, &WriteOptions::default()).is_err());
    doc.models[0].is_3d = false;
    if let EntityKind::Line { end, .. } = &mut doc.models[0].entities[0].kind {
        end.z = 0.5;
    }
    assert!(write_v8(&doc, &blank, &WriteOptions::default()).is_err());
}

#[test]
fn new_then_edited_geometry_unicode_labels_and_identifier_types() {
    for three in [false, true] {
        let mut line = line();
        line.layer = Some("Çizim".into());
        line.attributes.push(Attribute {
            tag: "SyntheticIdentifier".into(),
            value: Value::Text("00009007199254740993".into()),
            set: Some("Identifiers".into()),
            position: None,
            invisible: true,
        });
        let mut label = Entity::new(EntityKind::Text {
            position: Point3::xy(3., 4.),
            end_point: None,
            height: 0.3,
            rotation: 0.25,
            width_factor: 1.,
            oblique: 0.,
            value: "Bağımsız bölüm – Çığ".into(),
            style: None,
            halign: HAlign::Center,
            valign: VAlign::Baseline,
            normal: Vec3::Z,
        });
        label
            .props
            .insert("dgn.text_length".into(), Value::Float(4.2));
        label.layer = Some("Etiket".into());
        let source = document(three, vec![line, label]);
        let blank = seed(three);
        let bytes = write_v8(&source, &blank, &WriteOptions::default()).unwrap();
        let mut edited = crate::read(&bytes, &ReadOptions::default()).unwrap();
        let EntityKind::Line { end, .. } = &mut edited.models[0].entities[0].kind else {
            panic!("line");
        };
        *end = Point3::new(7., 8., if three { 2. } else { 0. });
        edited.models[0].entities[0].attributes[0].value =
            Value::Text("00009007199254740994".into());
        let label = &mut edited.models[0].entities[1];
        let EntityKind::Text {
            value, position, ..
        } = &mut label.kind
        else {
            panic!("text");
        };
        *value = "Yeni bölüm – Şişli".into();
        position.x = 8.;
        label
            .props
            .insert("dgn.text_length".into(), Value::Float(3.1));
        let saved = write_v8(
            &edited,
            &bytes,
            &WriteOptions {
                clear_seed_model: true,
                ..Default::default()
            },
        )
        .unwrap();
        ::cfb::CompoundFile::open_strict(Cursor::new(&saved)).unwrap();
        let reopened = crate::read(&saved, &ReadOptions::default()).unwrap();
        assert_eq!(reopened.models[0].entities.len(), 2);
        assert_eq!(
            reopened.models[0].entities[0].kind,
            edited.models[0].entities[0].kind
        );
        assert_eq!(
            reopened.models[0].entities[0].attributes,
            edited.models[0].entities[0].attributes
        );
        assert_eq!(
            reopened.models[0].entities[1].layer,
            edited.models[0].entities[1].layer
        );
        let EntityKind::Text {
            value, position, ..
        } = &reopened.models[0].entities[1].kind
        else {
            panic!("text");
        };
        assert_eq!(value, "Yeni bölüm – Şişli");
        assert!((position.x - 8.).abs() < 1e-8);
        assert!((position.y - 4.).abs() < 1e-8);
    }
}
