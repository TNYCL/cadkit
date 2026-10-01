//! Synthetic application-created drawings; no private corpus data.
use super::*;
use cadkit_core::{Attribute, GroupKind, Vec3, Vertex};

fn options() -> ExportOptions {
    ExportOptions {
        srs_name: "urn:ogc:def:crs:EPSG::5254".into(),
        lod: 2,
        limits: cadkit_core::Limits::default(),
    }
}

fn document(entities: Vec<Entity>) -> Document {
    let mut doc = Document::default();
    doc.models.push(Model {
        is_3d: true,
        entities,
        ..Default::default()
    });
    doc
}

fn image() -> Entity {
    Entity::new(EntityKind::Image {
        path: Some("synthetic.tif".into()),
        position: Point3::default(),
        u_vector: Vec3::new(2.0, 0.0, 0.0),
        v_vector: Vec3::new(0.0, 3.0, 0.0),
        size_px: Some([20, 30]),
    })
}

#[test]
fn closed_cad_paths_are_curves_and_explicit_polygons_stay_strict() {
    let points = vec![
        Point3::xy(0., 0.),
        Point3::xy(4., 4.),
        Point3::xy(0., 4.),
        Point3::xy(4., 0.),
    ];
    let doc = document(vec![Entity::new(EntityKind::Polyline {
        vertices: points.iter().copied().map(Vertex::at).collect(),
        closed: true,
        normal: Vec3::Z,
    })]);
    let bytes = write_document(&doc, &options()).unwrap();
    let native = read_native(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(
        validate(&native, &ValidationOptions::default())
            .unwrap()
            .polygons,
        0
    );
    let read = to_document(&native, &ImportOptions::default(), &ReadOptions::default()).unwrap();
    let mut found = false;
    read.walk(0, 64, |e, _| {
        if let EntityKind::Polyline { vertices, .. } = &e.kind {
            assert_eq!(vertices.len(), 5);
            assert_eq!(
                vertices.first().unwrap().position,
                vertices.last().unwrap().position
            );
            found = true;
        }
    });
    assert!(found);
    validate_xsd_if_cached(&bytes);
    let mut ring = points;
    ring.push(ring[0]);
    let invalid = document(vec![Entity::new(EntityKind::Polygon {
        exterior: ring,
        interiors: vec![],
    })]);
    for policy in [
        UnsupportedGeometry::Reject,
        UnsupportedGeometry::MetadataOnly,
    ] {
        assert!(write_document_with_report(&invalid, &options(), policy).is_err());
    }
}

#[test]
fn exact_circles_survive_tilted_and_negative_normals() {
    for normal in [
        Vec3::Z,
        Vec3::new(0., 0., -1.),
        Vec3::new(1., 2., 3.).normalized().unwrap(),
    ] {
        let center = Point3::new(300_000., 4_000_000., 7.5);
        let doc = document(vec![Entity::new(EntityKind::Circle {
            center,
            radius: 3.25,
            normal,
        })]);
        let bytes = write_document(&doc, &options()).unwrap();
        validate_xsd_if_cached(&bytes);
        let after = read(&bytes, &ReadOptions::default()).unwrap();
        let mut count = 0;
        after.walk(0, 64, |e, _| {
            if let EntityKind::Circle {
                center: c,
                radius,
                normal: n,
            } = e.kind
            {
                assert!(center.vector_to(c).length() < 1e-8);
                assert!((radius - 3.25).abs() < 1e-8);
                assert!(n.minus(normal).length() < 1e-8);
                count += 1;
            }
        });
        assert_eq!(count, 1);
    }
}

#[test]
fn circle_reader_rejects_degenerate_and_oversized_segments_and_survives_mutation() {
    let doc = document(vec![Entity::new(EntityKind::Circle {
        center: Point3::default(),
        radius: 2.,
        normal: Vec3::Z,
    })]);
    let bytes = write_document(&doc, &options()).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    for bad in [
        text.replace("numArc=\"1\"", "numArc=\"2\""),
        text.replace("0 2 0", "2 0 0"),
        text.replace("0 2 0", "NaN 2 0"),
    ] {
        assert!(read_native(bad.as_bytes(), &ReadOptions::default()).is_err());
    }
    let mut read_options = ReadOptions::default();
    read_options.limits.max_vertices = 2;
    read_options.limits.max_total_vertices = 2;
    assert!(read_native(&bytes, &read_options).is_err());
    read_options = ReadOptions::default();
    read_options.limits.max_objects = 500;
    read_options.limits.max_depth = 32;
    for i in (0..bytes.len()).step_by(3) {
        let _ = read_native(&bytes[..i], &read_options);
        let mut changed = bytes.clone();
        changed[i] ^= 0x80;
        if let Ok(doc) = read_native(&changed, &read_options) {
            let _ = to_document(&doc, &ImportOptions::default(), &read_options);
        }
    }
}

#[test]
fn metadata_fallback_is_explicit_reported_and_preserves_nested_entities() {
    let mut annotation = image();
    annotation.attributes.push(Attribute {
        tag: "SyntheticID".into(),
        value: Value::Text("000012345678901234567890".into()),
        set: Some("Ids".into()),
        position: None,
        invisible: true,
    });
    let group = Entity::new(EntityKind::Group {
        group_kind: GroupKind::Other,
        name: None,
        origin: None,
        children: vec![
            Entity::new(EntityKind::Point {
                position: Point3::xy(1., 2.),
            }),
            annotation,
        ],
    });
    let doc = document(vec![group.clone()]);
    assert!(write_document(&doc, &options()).is_err());
    let (bytes, report) =
        write_document_with_report(&doc, &options(), UnsupportedGeometry::MetadataOnly).unwrap();
    assert_eq!(
        (
            report.entities,
            report.geometry_entities,
            report.metadata_only_entities
        ),
        (3, 2, 1)
    );
    assert_eq!(report.issues[0].path, [0, 1]);
    assert_eq!(report.issues[0].entity_type, "image");
    let diagnostics = serde_json::to_string(&report).unwrap();
    assert!(!diagnostics.contains("synthetic.tif"));
    assert!(!diagnostics.contains("000012345678901234567890"));
    let native = read_native(&bytes, &ReadOptions::default()).unwrap();
    let object = native
        .root
        .elements()
        .next()
        .unwrap()
        .elements()
        .next()
        .unwrap();
    let metadata = object
        .elements()
        .find(|e| e.attribute("", "name") == Some("cadkit.entity"))
        .unwrap()
        .elements()
        .next()
        .unwrap()
        .text();
    assert_eq!(serde_json::from_str::<Entity>(&metadata).unwrap(), group);
    assert!(
        String::from_utf8(bytes.clone())
            .unwrap()
            .contains(">partial<")
    );
    validate_xsd_if_cached(&bytes);
}

#[test]
fn wholly_unsupported_objects_have_metadata_and_no_geometry_property() {
    let (bytes, report) = write_document_with_report(
        &document(vec![image()]),
        &options(),
        UnsupportedGeometry::MetadataOnly,
    )
    .unwrap();
    assert_eq!(report.metadata_only_entities, 1);
    assert_eq!(report.geometry_entities, 0);
    assert!(
        !String::from_utf8(bytes.clone())
            .unwrap()
            .contains("lod2Geometry")
    );
    validate_xsd_if_cached(&bytes);
}

#[test]
fn fallback_does_not_hide_bad_numbers_or_resource_limits() {
    let mut image = image();
    if let EntityKind::Image { position, .. } = &mut image.kind {
        position.z = f64::NAN;
    }
    assert!(
        write_document_with_report(
            &document(vec![image]),
            &options(),
            UnsupportedGeometry::MetadataOnly
        )
        .is_err()
    );
    let circle = Entity::new(EntityKind::Circle {
        center: Point3::default(),
        radius: -1.,
        normal: Vec3::Z,
    });
    assert!(
        write_document_with_report(
            &document(vec![circle]),
            &options(),
            UnsupportedGeometry::MetadataOnly
        )
        .is_err()
    );
    let doc = document(vec![self::image()]);
    let mut limited = options();
    limited.limits.max_objects = 1;
    assert!(write_document_with_report(&doc, &limited, UnsupportedGeometry::MetadataOnly).is_err());
    limited = options();
    limited.limits.max_string_bytes = 16;
    assert!(write_document_with_report(&doc, &limited, UnsupportedGeometry::MetadataOnly).is_err());
}

#[test]
fn new_and_edited_native_model_preserves_semantic_links_and_typed_ids() {
    let mut native = fixture();
    let mut unit = CityObject::new(ObjectKind::GenericCityObject, "unit_synthetic");
    unit.properties
        .push(Element::with_text(GML, "gml:name", "Bağımsız bölüm – Çığ"));
    unit.properties
        .push(generic_attribute("syntheticNumericID", &Value::Int(9_007_199_254_740_993)).unwrap());
    unit.properties
        .push(generic_attribute("syntheticTextID", &Value::Text("00001234567890".into())).unwrap());
    native.add_object(unit);
    let mut floor = CityObject::new(ObjectKind::CityObjectGroup, "floor_synthetic");
    let mut member = Element::new(GROUPS, "grp:groupMember");
    member.set_attribute(XLINK, "xlink:href", "#unit_synthetic");
    floor.properties.push(member);
    let mut parent = Element::new(GROUPS, "grp:parent");
    parent.set_attribute(XLINK, "xlink:href", "#building_a");
    floor.properties.push(parent);
    native.add_object(floor);
    let first = write(&native, &ValidationOptions::default()).unwrap();
    validate_xsd_if_cached(&first);
    let mut edited = read_native(&first, &ReadOptions::default()).unwrap();
    fn edit(e: &mut Element) {
        if e.name.is(GML, "pos") && e.text() == "10 10 0" {
            e.children = vec![Node::Text("11 10 0".into())];
        }
        for node in &mut e.children {
            if let Node::Element(child) = node {
                edit(child);
            }
        }
    }
    edit(&mut edited.root);
    let bytes = write(&edited, &ValidationOptions::default()).unwrap();
    assert_ne!(first, bytes);
    validate_xsd_if_cached(&bytes);
    let reread = read_native(&bytes, &ReadOptions::default()).unwrap();
    let report = validate(&reread, &ValidationOptions::default()).unwrap();
    assert_eq!(report.references, 3);
    assert_eq!(
        write(&reread, &ValidationOptions::default()).unwrap(),
        bytes
    );
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("9007199254740993"));
    assert!(text.contains("00001234567890"));
    assert!(text.contains("Bağımsız bölüm – Çığ"));
}
