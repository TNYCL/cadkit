//! Choosing the source model of a multi-model document, 2D/3D dimension policies and the
//! estimated length of center/right text.

use super::*;
use cadkit_core::ModelKind;

fn models(kinds: &[ModelKind]) -> Document {
    let mut d = Document::default();
    for (i, kind) in kinds.iter().enumerate() {
        let mut e = line();
        // Each model's line starts at x = index, so the written model is recognisable.
        e.kind = EntityKind::Line {
            start: Point3::new(i as f64, 0.0, 0.0),
            end: Point3::new(i as f64 + 1.0, 1.0, 0.0),
        };
        d.models
            .push(Model::new(format!("M{i}"), *kind).with_entities(vec![e]));
    }
    d
}

fn start_x(bytes: &[u8]) -> f64 {
    let read = crate::read(bytes, &ReadOptions::default()).unwrap();
    assert_eq!(read.models.len(), 1);
    assert_eq!(read.models[0].entities.len(), 1);
    match read.models[0].entities[0].kind {
        EntityKind::Line { start, .. } => start.x,
        ref other => panic!("{other:?}"),
    }
}

fn codes(report: &WriteReport) -> Vec<(&str, Option<u64>)> {
    report
        .warnings
        .iter()
        .map(|w| (w.code.as_str(), w.object))
        .collect()
}

#[test]
fn the_only_design_model_beside_layouts_is_written_and_the_layouts_reported() {
    let d = models(&[ModelKind::Layout, ModelKind::Model, ModelKind::Layout]);
    let seed = seed(false);
    let report = write_v8_report(&d, &seed, &WriteOptions::default()).unwrap();
    assert_eq!(report.model_index, 1);
    assert_eq!(
        codes(&report),
        [
            ("dgn.model_not_written", Some(0)),
            ("dgn.model_not_written", Some(2))
        ]
    );
    assert!(
        report.warnings[0]
            .message
            .starts_with("layout 0 (\"M0\", 1 ")
    );
    assert_eq!(start_x(&report.bytes), 1.0);
    assert_eq!(
        write_v8(&d, &seed, &WriteOptions::default()).unwrap(),
        report.bytes
    );
    // A single-model document reports nothing, as before.
    let single = models(&[ModelKind::Model]);
    let report = write_v8_report(&single, &seed, &WriteOptions::default()).unwrap();
    assert_eq!((report.model_index, report.warnings.len()), (0, 0));
}

#[test]
fn other_documents_need_an_explicit_model_index() {
    let seed = seed(false);
    let defaults = WriteOptions::default();
    for kinds in [
        &[ModelKind::Model, ModelKind::Model][..],
        &[ModelKind::Model, ModelKind::Sheet],
        &[ModelKind::Layout, ModelKind::Layout],
        &[],
    ] {
        let err = write_v8(&models(kinds), &seed, &defaults).unwrap_err();
        assert!(
            err.to_string()
                .contains("DGN writer requires exactly one source model"),
            "{err}"
        );
    }
    let d = models(&[ModelKind::Model, ModelKind::Model, ModelKind::Layout]);
    let report = write_v8_report(&d, &seed, &defaults.clone().with_model_index(Some(1))).unwrap();
    assert_eq!(report.model_index, 1);
    assert_eq!(start_x(&report.bytes), 1.0);
    assert_eq!(
        codes(&report),
        [
            ("dgn.model_not_written", Some(0)),
            ("dgn.model_not_written", Some(2))
        ]
    );
    assert!(report.warnings[0].message.starts_with("design model 0"));
    // Out of range, or a layout: rejected.
    assert!(
        write_v8(&d, &seed, &defaults.clone().with_model_index(Some(3)))
            .unwrap_err()
            .to_string()
            .contains("model index 3 out of bounds (3 models)")
    );
    assert!(
        write_v8(&d, &seed, &defaults.clone().with_model_index(Some(2)))
            .unwrap_err()
            .to_string()
            .contains("design models only")
    );
    // The pre-parsed seed path chooses alike.
    let parsed = V8Seed::from_bytes(&seed).unwrap();
    let report =
        write_v8_with_seed_report(&d, &parsed, &defaults.with_model_index(Some(0))).unwrap();
    assert_eq!(start_x(&report.bytes), 0.0);
}

#[test]
fn a_2d_model_is_lifted_into_a_3d_seed_only_when_asked() {
    let d = document(false, vec![line()]);
    let seed = seed(true);
    let err = write_v8(&d, &seed, &WriteOptions::default()).unwrap_err();
    assert!(err.to_string().contains("dimensions differ"), "{err}");
    for policy in [
        DimensionPolicy::Promote2dTo3d,
        DimensionPolicy::ProjectToSeed,
    ] {
        let options = WriteOptions::default().with_dimension_policy(policy);
        let report = write_v8_report(&d, &seed, &options).unwrap();
        assert_eq!(codes(&report), [("dgn.model_dimension_changed", None)]);
        let read = crate::read(&report.bytes, &ReadOptions::default()).unwrap();
        assert!(read.models[0].is_3d);
        assert_eq!(read.models[0].entities[0].kind, line().kind);
    }
}

#[test]
fn a_3d_model_reaches_a_2d_seed_flat_or_projected() {
    let lifted = Entity::new(EntityKind::Line {
        start: Point3::new(1.0, 2.0, 0.5),
        end: Point3::new(4.0, 5.0, 0.5),
    });
    let seed = seed(false);
    let flat = document(true, vec![line()]);
    let raised = document(true, vec![lifted]);
    let promote = WriteOptions::default().with_dimension_policy(DimensionPolicy::Promote2dTo3d);
    let project = WriteOptions::default().with_dimension_policy(DimensionPolicy::ProjectToSeed);
    assert!(write_v8(&flat, &seed, &WriteOptions::default()).is_err());
    let read = crate::read(
        &write_v8(&flat, &seed, &promote).unwrap(),
        &ReadOptions::default(),
    )
    .unwrap();
    assert!(!read.models[0].is_3d);
    assert_eq!(read.models[0].entities[0].kind, line().kind);
    assert!(
        write_v8(&raised, &seed, &promote)
            .unwrap_err()
            .to_string()
            .contains("nonzero Z")
    );
    let report = write_v8_report(&raised, &seed, &project).unwrap();
    assert_eq!(codes(&report), [("dgn.model_dimension_changed", None)]);
    let read = crate::read(&report.bytes, &ReadOptions::default()).unwrap();
    assert_eq!(read.models[0].entities[0].kind, line().kind);
}

#[test]
fn prepared_images_from_other_formats_are_new_attachments() {
    let image = || {
        let mut e = Entity::new(EntityKind::Image {
            path: Some("C:\\plans\\site.tif".into()),
            position: Point3::new(1.0, 2.0, 0.0),
            u_vector: Vec3::new(30.0, 0.0, 0.0),
            v_vector: Vec3::new(0.0, 20.0, 0.0),
            size_px: Some([300, 200]),
        });
        // A DWG handle, not an element id of the seed.
        e.id = Some(0x2f);
        e.props.insert("dgn.raster_dpi".into(), Value::Float(100.0));
        e
    };
    let seed = V8Seed::from_bytes(&seed(true)).unwrap();
    let group = Entity::new(EntityKind::Group {
        group_kind: GroupKind::Other,
        name: None,
        origin: None,
        children: vec![image()],
    });
    for (format, kept) in [
        (cadkit_core::Format::Dwg, false),
        (cadkit_core::Format::Dxf, false),
        (cadkit_core::Format::DgnV8, true),
        (cadkit_core::Format::Unknown, true),
    ] {
        let mut d = document(false, vec![image(), group.clone()]);
        d.source = cadkit_core::SourceInfo::new(format, "");
        let prepared = seed
            .prepare_document(&d, &SeedPrepareOptions::default())
            .unwrap();
        let entities = &prepared.models[0].entities;
        assert_eq!(entities[0].id.is_some(), kept, "{format:?}");
        let EntityKind::Group { children, .. } = &entities[1].kind else {
            panic!("{:?}", entities[1].kind)
        };
        assert_eq!(children[0].id.is_some(), kept, "{format:?}");
    }
    // A DWG document with an image now writes: the image is a new attachment.
    let mut d = document(false, vec![image()]);
    d.source = cadkit_core::SourceInfo::new(cadkit_core::Format::Dwg, "AC1032");
    let prepared = seed
        .prepare_document(&d, &SeedPrepareOptions::default())
        .unwrap();
    let bytes = write_v8_with_seed(&prepared, &seed, &WriteOptions::default()).unwrap();
    let read = crate::read(&bytes, &ReadOptions::default()).unwrap();
    assert!(matches!(
        read.models[0].entities[0].kind,
        EntityKind::Image {
            size_px: Some([300, 200]),
            ..
        }
    ));
}

#[test]
fn unmeasured_center_text_takes_the_estimate_when_asked() {
    let text = |halign| {
        Entity::new(EntityKind::Text {
            position: Point3::new(10., 20., 0.),
            end_point: None,
            height: 2.,
            rotation: 0.3,
            width_factor: 0.5,
            oblique: 0.,
            value: "Centre".into(),
            style: None,
            halign,
            valign: cadkit_core::VAlign::Middle,
            normal: Vec3::Z,
        })
    };
    let d = document(
        false,
        vec![
            text(cadkit_core::HAlign::Center),
            text(cadkit_core::HAlign::Right),
            text(cadkit_core::HAlign::Left),
        ],
    );
    let seed = seed(false);
    let err = write_v8(&d, &seed, &WriteOptions::default()).unwrap_err();
    assert!(err.to_string().contains("dgn.text_length"), "{err}");
    let report = write_v8_report(
        &d,
        &seed,
        &WriteOptions::default().with_estimate_text_length(true),
    )
    .unwrap();
    // Left text has never needed the length: only the two others count.
    assert_eq!(codes(&report), [("dgn.text_length_estimated", None)]);
    assert!(report.warnings[0].message.starts_with("2 "));
    let read = crate::read(&report.bytes, &ReadOptions::default()).unwrap();
    for (got, want) in read.models[0].entities.iter().zip(&d.models[0].entities) {
        let (
            EntityKind::Text { position, .. },
            EntityKind::Text {
                position: expected, ..
            },
        ) = (&got.kind, &want.kind)
        else {
            panic!("{got:?}")
        };
        assert!(position.distance_to(*expected) < 1e-9);
        // 6 characters x height 2 x width factor 0.5, stored as the text's length.
        assert_eq!(got.props["dgn.text_length"], Value::Float(6.0));
    }
}
