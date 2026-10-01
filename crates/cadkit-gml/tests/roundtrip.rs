//! Sentetik CityGML, kaynak sınırları ve isteğe bağlı özel örnek kontrolleri.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use cadkit_core::{Document, Entity, EntityKind, Error, Model, Point3, ReadOptions, Value};
use cadkit_gml::{native::*, *};

fn ring(a: f64, b: f64) -> Vec<Point3> {
    vec![
        Point3::xy(a, a),
        Point3::xy(b, a),
        Point3::xy(b, b),
        Point3::xy(a, b),
        Point3::xy(a, a),
    ]
}
fn fixture() -> CityGmlDocument {
    let mut doc = CityGmlDocument::new();
    let mut building = CityObject::new(ObjectKind::Building, "building_a");
    building
        .properties
        .push(Element::with_text(GML, "gml:name", "İnceleme & <model>"));
    building
        .properties
        .push(generic_attribute("ölçü", &Value::Float(12.5)).unwrap());
    let mut lod = Element::new(BUILDING, "bldg:lod2MultiSurface");
    let mut multi = Element::new(GML, "gml:MultiSurface");
    let mut member = Element::new(GML, "gml:surfaceMember");
    member.push(
        Polygon {
            id: Some("surface_a".into()),
            exterior: ring(0.0, 10.0),
            interiors: vec![ring(2.0, 3.0).into_iter().rev().collect()],
        }
        .to_element(),
    );
    multi.push(member);
    let mut shared = Element::new(GML, "gml:surfaceMember");
    shared.set_attribute(XLINK, "xlink:href", "#surface_a");
    multi.push(shared);
    lod.push(multi);
    building.properties.push(lod);
    doc.add_object(building);
    doc
}

#[test]
fn native_roundtrip_preserves_ids_references_holes_and_unicode() {
    let source = fixture();
    let bytes = write(&source, &ValidationOptions::default()).unwrap();
    assert!(sniff(&bytes));
    let parsed = read_native(&bytes, &ReadOptions::default()).unwrap();
    let report = validate(&parsed, &ValidationOptions::default()).unwrap();
    assert_eq!(
        (
            report.ids,
            report.references,
            report.polygons,
            report.interiors,
            report.attributes
        ),
        (2, 1, 1, 1, 1)
    );
    let encoded = write(&parsed, &ValidationOptions::default()).unwrap();
    assert_eq!(bytes, encoded);
    let doc = to_document(&parsed, &ImportOptions::default(), &ReadOptions::default()).unwrap();
    let mut polygons = 0;
    doc.walk(0, 64, |e, _| {
        if let EntityKind::Polygon { interiors, .. } = &e.kind {
            assert_eq!(interiors.len(), 1);
            polygons += 1;
        }
    });
    assert_eq!(polygons, 2);
}

#[test]
fn new_neutral_geometry_is_writable() {
    let mut doc = Document::default();
    doc.models.push(Model {
        is_3d: true,
        entities: vec![Entity::new(EntityKind::Polygon {
            exterior: ring(0.0, 5.0),
            interiors: vec![ring(1.0, 2.0).into_iter().rev().collect()],
        })],
        ..Default::default()
    });
    let bytes = write_document(
        &doc,
        &ExportOptions {
            srs_name: "urn:ogc:def:crs:EPSG::4979".into(),
            lod: 1,
            limits: cadkit_core::Limits::default(),
        },
    )
    .unwrap();
    let parsed = read_native(&bytes, &ReadOptions::default()).unwrap();
    let report = validate(&parsed, &ValidationOptions::default()).unwrap();
    assert_eq!(report.interiors, 1);
    assert!(
        write_document(
            &doc,
            &ExportOptions {
                srs_name: String::new(),
                lod: 1,
                limits: cadkit_core::Limits::default(),
            }
        )
        .is_err()
    );
}

#[test]
fn malformed_and_bounded_inputs_are_rejected() {
    let bytes = write(&fixture(), &ValidationOptions::default()).unwrap();
    for n in 0..bytes.len() {
        assert!(read_native(&bytes[..n], &ReadOptions::default()).is_err());
    }
    let xml = String::from_utf8(bytes.clone()).unwrap();
    for bad in [
        xml.replace("#surface_a", "#missing"),
        xml.replace("building_a", "surface_a"),
        xml.replace("12.5", "NaN"),
        format!("<!DOCTYPE x [<!ENTITY x SYSTEM 'file:///unused'>]>{xml}"),
    ] {
        assert!(read_native(bad.as_bytes(), &ReadOptions::default()).is_err());
    }
    let mut options = ReadOptions::default();
    options.limits.max_depth = 2;
    assert!(matches!(
        read_native(&bytes, &options),
        Err(Error::LimitExceeded(_))
    ));
    options = ReadOptions::default();
    options.limits.max_total_vertices = 2;
    assert!(read_native(&bytes, &options).is_err());
    options = ReadOptions::default();
    options.limits.max_string_bytes = 64;
    for content in [
        "x".repeat(65),
        format!("<![CDATA[{}]]>", "x".repeat(65)),
        format!("<!--{}-->", "x".repeat(65)),
    ] {
        let xml = format!(
            "<CityModel xmlns=\"http://www.opengis.net/citygml/2.0\">{content}</CityModel>"
        );
        assert!(matches!(
            read_native(xml.as_bytes(), &options),
            Err(Error::LimitExceeded(_))
        ));
    }
}

#[test]
fn json_import_checks_input_and_model_budgets() {
    let native = serde_json::to_string(&fixture()).unwrap();
    let neutral = serde_json::to_string(&Document::default()).unwrap();
    let mut options = ReadOptions::default();
    options.limits.max_input_bytes = 1;
    assert!(matches!(
        read_native_json(&native, &options),
        Err(Error::LimitExceeded(_))
    ));
    assert!(matches!(
        cadkit_core::export::document_from_json(&neutral, &options.limits),
        Err(Error::LimitExceeded(_))
    ));
    options = ReadOptions::default();
    assert!(read_native_json(&native, &options).is_ok());
    assert!(cadkit_core::export::document_from_json(&neutral, &options.limits).is_ok());
    options.limits.max_objects = 1;
    assert!(matches!(
        read_native_json(&native, &options),
        Err(Error::LimitExceeded(_))
    ));
    assert!(read_native_json("{", &ReadOptions::default()).is_err());
    assert!(cadkit_core::export::document_from_json("{", &options.limits).is_err());
}

#[test]
fn prefix_independence_and_reference_cycles() {
    let xml = format!(
        "<c:CityModel xmlns:c='{CORE}' xmlns:g='{GML}' xmlns:x='{XLINK}'><g:MultiSurface g:id='a'><g:surfaceMember x:href='#a'/></g:MultiSurface></c:CityModel>"
    );
    assert!(sniff(xml.as_bytes()));
    assert!(read_native(xml.as_bytes(), &ReadOptions::default()).is_err());
    let xml = format!(
        "<c:CityModel xmlns:c='{CORE}' xmlns:g='{GML}'><g:Point srsDimension='2'><g:pos>1 2</g:pos></g:Point></c:CityModel>"
    );
    let doc = read(xml.as_bytes(), &ReadOptions::default()).unwrap();
    assert!(matches!(
        doc.models[0].entities[0].kind,
        EntityKind::Point {
            position: Point3 {
                x: 1.0,
                y: 2.0,
                z: 0.0
            }
        }
    ));
}

#[test]
fn private_roundtrip_when_supplied() {
    let Some(path) = std::env::var_os("CADKIT_PRIVATE_GML") else {
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    let options = ReadOptions::default();
    let original = read_native(&bytes, &options).unwrap();
    // Kaynak geometri bozuk olsa da XML ilişkilerinin kayıpsız korunması ayrı sınanır.
    let validation = ValidationOptions {
        geometry: false,
        ..Default::default()
    };
    let issues_before = geometry_issues(&original, &ValidationOptions::default()).unwrap();
    let before = validate(&original, &validation).unwrap();
    let rewritten = write(&original, &validation).unwrap();
    let parsed = read_native(&rewritten, &options).unwrap();
    // Bayt konumları değişebilir; hiçbir özgün değer veya ilişki değişmemelidir.
    assert!(same_content(&original.root, &parsed.root));
    validate_xsd_if_cached(&bytes);
    validate_xsd_if_cached(&rewritten);
    let after = validate(&parsed, &validation).unwrap();
    assert_eq!(before, after);
    let issues_after = geometry_issues(&parsed, &ValidationOptions::default()).unwrap();
    assert_eq!(
        issues_before
            .iter()
            .map(|i| (&i.code, &i.message))
            .collect::<Vec<_>>(),
        issues_after
            .iter()
            .map(|i| (&i.code, &i.message))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        write(&parsed, &ValidationOptions::default()).is_err(),
        !issues_after.is_empty()
    );
    assert!(write(&parsed, &validation).unwrap() == rewritten);
}

fn same_content(a: &Element, b: &Element) -> bool {
    a.name == b.name
        && a.attributes == b.attributes
        && a.children.len() == b.children.len()
        && a.children
            .iter()
            .zip(&b.children)
            .all(|(a, b)| match (a, b) {
                (Node::Element(a), Node::Element(b)) => same_content(a, b),
                (Node::Text(a), Node::Text(b)) | (Node::Comment(a), Node::Comment(b)) => a == b,
                _ => false,
            })
}

#[test]
fn xml_controls_and_attribute_whitespace() {
    let mut doc = fixture();
    doc.root.set_attribute("", "test", "a\t\n\rb");
    let bytes = write(&doc, &ValidationOptions::default()).unwrap();
    let parsed = read_native(&bytes, &ReadOptions::default()).unwrap();
    assert_eq!(parsed.root.attribute("", "test"), Some("a\t\n\rb"));
    for bad in ["&#0;", "&#xFFFF;", "\0"] {
        let xml = format!("<CityModel xmlns='{CORE}'>{bad}</CityModel>");
        assert!(read_native(xml.as_bytes(), &ReadOptions::default()).is_err());
    }
}

#[test]
fn strict_writer_rejects_nonplanar_hole_without_changing_source() {
    let mut hole = ring(1.0, 2.0);
    for point in &mut hole {
        point.z = 0.25;
    }
    let mut doc = CityGmlDocument::new();
    let p = Polygon {
        id: None,
        exterior: ring(0.0, 5.0),
        interiors: vec![hole],
    };
    doc.root.push(p.to_element());
    assert!(write(&doc, &ValidationOptions::default()).is_err());
    let diagnostics = geometry_issues(&doc, &ValidationOptions::default()).unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "gml.polygon_geometry");
    assert!(
        write(
            &doc,
            &ValidationOptions {
                geometry: false,
                ..Default::default()
            }
        )
        .is_ok()
    );
}

#[test]
fn solid_shell_closure_and_orientation() {
    let points = [
        Point3::new(0., 0., 0.),
        Point3::new(1., 0., 0.),
        Point3::new(1., 1., 0.),
        Point3::new(0., 1., 0.),
        Point3::new(0., 0., 1.),
        Point3::new(1., 0., 1.),
        Point3::new(1., 1., 1.),
        Point3::new(0., 1., 1.),
    ];
    let faces = [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ];
    let mut shell = Element::new(GML, "gml:CompositeSurface");
    for face in faces {
        let mut ring: Vec<_> = face.into_iter().map(|i| points[i]).collect();
        ring.push(ring[0]);
        let mut member = Element::new(GML, "gml:surfaceMember");
        member.push(
            Polygon {
                id: None,
                exterior: ring,
                interiors: vec![],
            }
            .to_element(),
        );
        shell.push(member);
    }
    let build = |shell: Element| {
        let mut exterior = Element::new(GML, "gml:exterior");
        exterior.push(shell);
        let mut solid = Element::new(GML, "gml:Solid");
        solid.push(exterior);
        let mut geometry = Element::new(BUILDING, "bldg:lod1Solid");
        geometry.push(solid);
        let mut building = CityObject::new(ObjectKind::Building, "cube");
        building.properties.push(geometry);
        let mut doc = CityGmlDocument::new();
        doc.add_object(building);
        doc
    };
    let doc = build(shell.clone());
    assert!(
        geometry_issues(&doc, &ValidationOptions::default())
            .unwrap()
            .is_empty()
    );
    validate_xsd_if_cached(&write(&doc, &ValidationOptions::default()).unwrap());
    shell.children.pop();
    let issues = geometry_issues(&build(shell), &ValidationOptions::default()).unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].code, "gml.solid_shell");
}

fn validate_xsd_if_cached(bytes: &[u8]) {
    use std::io::Write;
    let cache = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/public/citygml-schemas");
    if !cache.join("profile.xsd").exists() {
        assert!(
            std::env::var_os("CADKIT_REQUIRE_GML_XSD").is_none(),
            "required official CityGML schema cache is missing; run scripts/validate-citygml.py --fetch"
        );
        return;
    }
    let mut child = std::process::Command::new("xmllint")
        .args(["--nonet", "--noout", "--schema"])
        .arg(cache.join("profile.xsd"))
        .arg("-")
        .env("XML_CATALOG_FILES", cache.join("catalog.xml"))
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    // XSD hata metni müşteri değerlerini içerebilir; yalnız sonucu raporlarız.
    assert!(
        output.status.success(),
        "official CityGML XSD validation failed"
    );
}

#[test]
fn official_schema_accepts_native_and_generic_output() {
    validate_xsd_if_cached(&write(&fixture(), &ValidationOptions::default()).unwrap());
    let mut doc = Document::default();
    doc.models.push(Model {
        is_3d: true,
        entities: vec![Entity::new(EntityKind::Line {
            start: Point3::xy(0., 0.),
            end: Point3::xy(1., 1.),
        })],
        ..Default::default()
    });
    validate_xsd_if_cached(
        &write_document(
            &doc,
            &ExportOptions {
                srs_name: "urn:ogc:def:crs:EPSG::4979".into(),
                lod: 1,
                limits: cadkit_core::Limits::default(),
            },
        )
        .unwrap(),
    );
}

#[test]
fn bit_flips_names_and_reference_expansion_remain_bounded() {
    let bytes = write(&fixture(), &ValidationOptions::default()).unwrap();
    let mut options = ReadOptions::default();
    options.limits.max_objects = 1000;
    options.limits.max_depth = 32;
    options.limits.max_total_vertices = 1000;
    for i in (0..bytes.len()).step_by(3) {
        let mut mutated = bytes.clone();
        mutated[i] ^= 0x80;
        let _ = read_native(&mutated, &options);
    }
    for name in ["1bad", "two:colons:name", "bad name"] {
        let xml = format!("<CityModel xmlns='{CORE}'><{name}/></CityModel>");
        assert!(read_native(xml.as_bytes(), &options).is_err());
    }
    let mut native = fixture();
    if let Some(Node::Element(building_member)) = native.root.children.first_mut() {
        if let Some(Node::Element(building)) = building_member.children.first_mut() {
            if let Some(Node::Element(lod)) = building.children.last_mut() {
                if let Some(Node::Element(multi)) = lod.children.first_mut() {
                    let mut reverse = Element::new(GML, "gml:OrientableSurface");
                    reverse.set_attribute("", "orientation", "-");
                    let mut base = Element::new(GML, "gml:baseSurface");
                    base.set_attribute(XLINK, "xlink:href", "#surface_a");
                    reverse.push(base);
                    let mut member = Element::new(GML, "gml:surfaceMember");
                    member.push(reverse);
                    multi.push(member);
                }
            }
        }
    }
    let doc = to_document(&native, &ImportOptions::default(), &options).unwrap();
    let mut normals = vec![];
    doc.walk(0, 32, |e, _| {
        if let EntityKind::Polygon { exterior, .. } = &e.kind {
            normals.push(cadkit_core::polygon::normal(exterior).unwrap().z);
        }
    });
    assert_eq!(normals, vec![1., 1., -1.]);
    options.limits.max_total_vertices = 20;
    assert!(to_document(&native, &ImportOptions::default(), &options).is_err());
}

#[test]
fn holes_reach_svg_without_solid_fill_over_the_interior() {
    let native = fixture();
    let doc = to_document(&native, &ImportOptions::default(), &ReadOptions::default()).unwrap();
    let svg = cadkit_core::svg::render(&doc, &cadkit_core::svg::SvgOptions::default());
    assert!(svg.contains("fill-rule=\"evenodd\""));
}
