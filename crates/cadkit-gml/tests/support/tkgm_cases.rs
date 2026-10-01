//! Public-guide field names with invented values; no private example data.
use super::*;

#[test]
fn tender_and_registration_rules_require_explicit_selection() {
    use tkgm::Profile;
    assert!("tkgm".parse::<Profile>().is_err());
    assert_eq!(
        "city-model-tender".parse::<Profile>().unwrap(),
        Profile::CityModelTender
    );
    assert_eq!(
        "digital-building-registration".parse::<Profile>().unwrap(),
        Profile::DigitalBuildingRegistration
    );
    let mut doc = CityGmlDocument::new();
    let mut building = CityObject::new(ObjectKind::Building, "synthetic_building");
    building
        .properties
        .push(generic_attribute("buildingHeight", &Value::Text("12".into())).unwrap());
    for value in [1001, 1002] {
        building
            .properties
            .push(generic_attribute("geometrySuitability", &Value::Int(value)).unwrap());
    }
    doc.add_object(building);
    let mut unit = CityObject::new(ObjectKind::GenericCityObject, "synthetic_unit");
    unit.properties
        .push(Element::with_text(GENERICS, "gen:class", "BagimsizBolum"));
    unit.properties
        .push(generic_attribute("integrationState", &Value::Int(1001)).unwrap());
    doc.add_object(unit);
    let before = serde_json::to_string(&doc).unwrap();
    let tender = tkgm::preflight(
        &doc,
        Profile::CityModelTender,
        &cadkit_core::Limits::default(),
    )
    .unwrap();
    let registration = tkgm::preflight(
        &doc,
        Profile::DigitalBuildingRegistration,
        &cadkit_core::Limits::default(),
    )
    .unwrap();
    assert_eq!(tender.workflow, "city_model_tender_architectural");
    assert!(!tender.acceptance_verified);
    assert!(
        tender
            .unchecked_rules
            .iter()
            .any(|s| s == "maks_rules_by_tender_year")
    );
    for field in ["registeredQuality", "propertyLot", "maksIdentityNumber"] {
        assert!(
            !tender
                .issues
                .iter()
                .any(|i| i.code == "tkgm.missing_attribute" && i.field.as_deref() == Some(field))
        );
        assert!(
            registration
                .issues
                .iter()
                .any(|i| i.code == "tkgm.missing_attribute" && i.field.as_deref() == Some(field))
        );
    }
    assert!(!tender.issues.iter().any(|i| i.code == "tkgm.fixed_value"));
    assert!(
        registration
            .issues
            .iter()
            .any(|i| i.code == "tkgm.fixed_value")
    );
    assert!(
        tender.issues.iter().any(
            |i| i.code == "tkgm.attribute_type" && i.field.as_deref() == Some("buildingHeight")
        )
    );
    for report in [&tender, &registration] {
        assert!(
            !report
                .issues
                .iter()
                .any(|i| i.code == "tkgm.duplicate_attribute")
        );
    }
    assert_eq!(serde_json::to_string(&doc).unwrap(), before);
}

#[test]
fn storey_can_reference_a_common_area_room() {
    let mut doc = fixture();
    let mut room = CityObject::new(ObjectKind::Room, "synthetic_common_area");
    room.properties
        .push(Element::with_text(BUILDING, "bldg:class", "OrtakAlan"));
    doc.add_object(room);
    let mut floor = CityObject::new(ObjectKind::CityObjectGroup, "synthetic_floor");
    floor
        .properties
        .push(Element::with_text(GROUPS, "grp:class", "Kat"));
    let mut member = Element::new(GROUPS, "grp:groupMember");
    member.set_attribute(XLINK, "xlink:href", "#synthetic_common_area");
    floor.properties.push(member);
    doc.add_object(floor);
    let report = tkgm::preflight(
        &doc,
        tkgm::Profile::CityModelTender,
        &cadkit_core::Limits::default(),
    )
    .unwrap();
    assert!(!report.issues.iter().any(|i| i.code == "tkgm.storey_member"));
}

#[test]
fn zero_unit_storeys_need_review_without_invented_membership() {
    for (count, code, severity) in [
        (0, "tkgm.empty_storey_membership", "review"),
        (1, "tkgm.missing_property", "error"),
    ] {
        let mut doc = fixture();
        let mut floor = CityObject::new(ObjectKind::CityObjectGroup, "synthetic_floor");
        floor
            .properties
            .push(Element::with_text(GROUPS, "grp:class", "Kat"));
        floor
            .properties
            .push(generic_attribute("independentSectionCount", &Value::Int(count)).unwrap());
        doc.add_object(floor);
        let report = tkgm::preflight(
            &doc,
            tkgm::Profile::CityModelTender,
            &cadkit_core::Limits::default(),
        )
        .unwrap();
        let findings: Vec<_> = report
            .issues
            .iter()
            .filter(|i| i.field.as_deref() == Some("groupMember"))
            .collect();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].code, code);
        assert_eq!(findings[0].severity, severity);
    }
}

#[test]
fn generic_cad_geometry_cannot_be_mistaken_for_tkgm_delivery() {
    let mut doc = Document::default();
    doc.models.push(Model {
        entities: vec![Entity::new(EntityKind::Point {
            position: Point3::default(),
        })],
        ..Default::default()
    });
    let native = from_document(
        &doc,
        &ExportOptions {
            srs_name: "EPSG:5254".into(),
            lod: 2,
            limits: cadkit_core::Limits::default(),
        },
    )
    .unwrap();
    let report = tkgm::preflight(
        &native,
        tkgm::Profile::DigitalBuildingRegistration,
        &cadkit_core::Limits::default(),
    )
    .unwrap();
    assert!(!report.acceptance_verified);
    assert_eq!(report.workflow, "digital_building_registration");
    for code in [
        "tkgm.missing_building",
        "tkgm.missing_envelope",
        "tkgm.missing_class",
    ] {
        assert!(report.issues.iter().any(|i| i.code == code));
    }
}

#[test]
fn preflight_detects_wrong_id_type_missing_fields_and_wrong_parent_without_values() {
    let mut doc = fixture();
    let mut unit = CityObject::new(ObjectKind::GenericCityObject, "synthetic_unit");
    unit.properties
        .push(generic_attribute("maksIdentityNumber", &Value::Float(12345678901.)).unwrap());
    unit.properties
        .push(generic_attribute("integrationState", &Value::Int(9999)).unwrap());
    unit.properties
        .push(Element::with_text(GENERICS, "gen:class", "BagimsizBolum"));
    doc.add_object(unit);
    let mut floor = CityObject::new(ObjectKind::CityObjectGroup, "synthetic_floor");
    floor
        .properties
        .push(Element::with_text(GROUPS, "grp:class", "Kat"));
    let mut parent = Element::new(GROUPS, "grp:parent");
    parent.set_attribute(XLINK, "xlink:href", "#synthetic_unit");
    floor.properties.push(parent);
    doc.add_object(floor);
    let report = tkgm::preflight(
        &doc,
        tkgm::Profile::DigitalBuildingRegistration,
        &cadkit_core::Limits::default(),
    )
    .unwrap();
    assert_eq!(
        (
            report.buildings,
            report.storeys,
            report.independent_sections
        ),
        (1, 1, 1)
    );
    assert!(report.issues.iter().any(
        |i| i.code == "tkgm.attribute_type" && i.field.as_deref() == Some("maksIdentityNumber")
    ));
    assert!(report.issues.iter().any(
        |i| i.code == "tkgm.missing_attribute" && i.field.as_deref() == Some("constructionID")
    ));
    assert!(report.issues.iter().any(|i| i.code == "tkgm.fixed_value"));
    assert!(report.issues.iter().any(|i| i.code == "tkgm.storey_parent"));
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("synthetic_unit"));
    assert!(!json.contains("12345678901"));
    assert!(
        report
            .unchecked_rules
            .iter()
            .any(|s| s == "property_lot_type_ambiguity")
    );
}

#[test]
fn preflight_enforces_explicit_turef_envelope_and_limits() {
    // A nested building envelope does not replace the required CityModel envelope.
    let mut nested = CityGmlDocument::new();
    let mut building = CityObject::new(ObjectKind::Building, "synthetic_building");
    let mut bound = Element::new(GML, "gml:boundedBy");
    let mut envelope = Element::new(GML, "gml:Envelope");
    envelope.set_attribute("", "srsName", "EPSG:5254");
    envelope.set_attribute("", "srsDimension", "3");
    bound.push(envelope);
    building.properties.push(bound);
    nested.add_object(building);
    let report = tkgm::preflight(
        &nested,
        tkgm::Profile::CityModelTender,
        &cadkit_core::Limits::default(),
    )
    .unwrap();
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.code == "tkgm.missing_envelope")
    );
    for (srs, dimension, expected_crs, expected_dim) in [
        ("EPSG:5254", "3", false, false),
        ("urn:ogc:def:crs:EPSG::5259", "3", false, false),
        ("EPSG:4326", "2", true, true),
    ] {
        let mut doc = fixture();
        let mut bound = Element::new(GML, "gml:boundedBy");
        let mut env = Element::new(GML, "gml:Envelope");
        env.set_attribute("", "srsName", srs);
        env.set_attribute("", "srsDimension", dimension);
        bound.push(env);
        doc.root.push(bound);
        let report = tkgm::preflight(
            &doc,
            tkgm::Profile::DigitalBuildingRegistration,
            &cadkit_core::Limits::default(),
        )
        .unwrap();
        assert_eq!(
            report.issues.iter().any(|i| i.code == "tkgm.crs"),
            expected_crs
        );
        assert_eq!(
            report
                .issues
                .iter()
                .any(|i| i.code == "tkgm.envelope_dimension"),
            expected_dim
        );
        assert!(!report.acceptance_verified);
        let limits = cadkit_core::Limits {
            max_objects: 1,
            ..Default::default()
        };
        assert!(
            tkgm::preflight(&doc, tkgm::Profile::DigitalBuildingRegistration, &limits).is_err()
        );
    }
}
