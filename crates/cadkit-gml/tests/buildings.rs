//! Buildings with thematic boundary surfaces from synthetic closed-polyline models.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
use cadkit_core::{
    Block, Document, Entity, EntityKind, GroupKind, Limits, Model, Point3, ReadOptions,
};
use cadkit_gml::{native::*, *};

#[path = "support/xsd.rs"]
mod xsd;
use xsd::validate_xsd_if_cached;

const SRS: &str = "EPSG:5254";

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

/// The six faces of an axis-aligned box; every other face is wound clockwise.
fn box_faces(o: Point3, size: Point3) -> Vec<Vec<Point3>> {
    let (x0, y0, z0) = (o.x, o.y, o.z);
    let (x1, y1, z1) = (o.x + size.x, o.y + size.y, o.z + size.z);
    let faces = vec![
        vec![p(x0, y0, z0), p(x1, y0, z0), p(x1, y1, z0), p(x0, y1, z0)],
        vec![p(x0, y0, z1), p(x1, y0, z1), p(x1, y1, z1), p(x0, y1, z1)],
        vec![p(x0, y0, z0), p(x1, y0, z0), p(x1, y0, z1), p(x0, y0, z1)],
        vec![p(x1, y0, z0), p(x1, y1, z0), p(x1, y1, z1), p(x1, y0, z1)],
        vec![p(x1, y1, z0), p(x0, y1, z0), p(x0, y1, z1), p(x1, y1, z1)],
        vec![p(x0, y1, z0), p(x0, y0, z0), p(x0, y0, z1), p(x0, y1, z1)],
    ];
    faces
        .into_iter()
        .enumerate()
        .map(|(i, mut f)| {
            if i % 2 == 1 {
                f.reverse();
            }
            f
        })
        .collect()
}

/// A 10 x 6 house with 3 m eaves and a 5 m ridge along x; roof faces wound both ways.
fn gable_faces() -> Vec<Vec<Point3>> {
    vec![
        vec![p(0., 0., 0.), p(0., 6., 0.), p(10., 6., 0.), p(10., 0., 0.)],
        vec![p(0., 0., 0.), p(10., 0., 0.), p(10., 0., 3.), p(0., 0., 3.)],
        vec![p(0., 6., 0.), p(0., 6., 3.), p(10., 6., 3.), p(10., 6., 0.)],
        vec![
            p(0., 0., 0.),
            p(0., 6., 0.),
            p(0., 6., 3.),
            p(0., 3., 5.),
            p(0., 0., 3.),
        ],
        vec![
            p(10., 0., 3.),
            p(10., 3., 5.),
            p(10., 6., 3.),
            p(10., 6., 0.),
            p(10., 0., 0.),
        ],
        vec![p(0., 0., 3.), p(10., 0., 3.), p(10., 3., 5.), p(0., 3., 5.)],
        vec![p(0., 3., 5.), p(10., 3., 5.), p(10., 6., 3.), p(0., 6., 3.)],
    ]
}

fn polylines(faces: Vec<Vec<Point3>>) -> Vec<Entity> {
    faces
        .into_iter()
        .map(|f| Entity::polyline(f, true))
        .collect()
}

fn group(children: Vec<Entity>) -> Entity {
    Entity::new(EntityKind::Group {
        group_kind: GroupKind::Cell,
        name: None,
        origin: None,
        children,
    })
}

fn document(entities: Vec<Entity>) -> Document {
    let mut doc = Document::default();
    doc.models
        .push(Model::default().with_is_3d(true).with_entities(entities));
    doc
}

fn options() -> BuildingOptions {
    BuildingOptions::new(ExportOptions::new(SRS, 2).with_closed_polylines_as_surfaces(true))
}

/// Writes, reads back and validates with the default geometry checks.
fn roundtrip(
    doc: &Document,
    options: &BuildingOptions,
) -> (Vec<u8>, BuildingReport, CityGmlDocument) {
    let (bytes, report) = write_buildings(doc, 0, options).unwrap();
    validate_xsd_if_cached(&bytes);
    let native = read_native(&bytes, &ReadOptions::default()).unwrap();
    assert_schema_order(&native);
    validate(&native, &ValidationOptions::default()).unwrap();
    assert!(
        geometry_issues(&native, &ValidationOptions::default())
            .unwrap()
            .is_empty()
    );
    read(&bytes, &ReadOptions::default()).unwrap();
    (bytes, report, native)
}

/// (surface kind, z of the exterior's unit normal) for every polygon below a boundary surface.
fn surfaces(doc: &CityGmlDocument) -> Vec<(String, f64)> {
    fn visit(e: &Element, kind: Option<&str>, out: &mut Vec<(String, f64)>) {
        let thematic = ["WallSurface", "RoofSurface", "GroundSurface"];
        let kind = if e.name.namespace == BUILDING && thematic.contains(&e.name.local()) {
            Some(e.name.local())
        } else {
            kind
        };
        if let Some(kind) = kind.filter(|_| e.name.is(GML, "posList")) {
            let v: Vec<f64> = e
                .text()
                .split_whitespace()
                .map(|t| t.parse().unwrap())
                .collect();
            let ring: Vec<Point3> = v.chunks(3).map(|c| p(c[0], c[1], c[2])).collect();
            let n = cadkit_core::polygon::normal(&ring).unwrap();
            out.push((kind.to_owned(), n.z));
        }
        for c in e.elements() {
            if !(e.name.is(GML, "interior")) {
                visit(c, kind, out);
            }
        }
    }
    let mut out = vec![];
    visit(&doc.root, None, &mut out);
    out
}

/// Properties a generated `bldg:Building` may hold, in CityGML 2.0 schema order: the
/// feature's `gml:boundedBy`, then from `AbstractBuildingType` in building.xsd
/// `lod2MultiSurface`, `boundedBy`, `lod3MultiSurface`, `lod4MultiSurface`.
fn assert_schema_order(doc: &CityGmlDocument) {
    const ORDER: [(&str, &str); 5] = [
        (GML, "boundedBy"),
        (BUILDING, "lod2MultiSurface"),
        (BUILDING, "boundedBy"),
        (BUILDING, "lod3MultiSurface"),
        (BUILDING, "lod4MultiSurface"),
    ];
    let buildings = doc
        .root
        .elements()
        .flat_map(|m| m.elements())
        .filter(|e| e.name.is(BUILDING, "Building"));
    for building in buildings {
        let ranks: Vec<usize> = building
            .elements()
            .map(|e| {
                ORDER
                    .iter()
                    .position(|&(ns, local)| e.name.is(ns, local))
                    .unwrap_or_else(|| panic!("unexpected property {}", e.name.qualified))
            })
            .collect();
        assert!(ranks.windows(2).all(|w| w[0] <= w[1]), "{ranks:?}");
        assert_eq!(ranks.first(), Some(&0), "the envelope comes first");
    }
}

fn count(surfaces: &[(String, f64)], kind: &str) -> usize {
    surfaces.iter().filter(|(k, _)| k == kind).count()
}

#[test]
fn box_of_closed_polylines_becomes_one_building_with_six_classified_surfaces() {
    let doc = document(polylines(box_faces(p(0., 0., 0.), p(4., 3., 2.5))));
    let (_, report, native) = roundtrip(&doc, &options());
    assert_eq!(report.buildings.len(), 1);
    assert_eq!(
        (
            report.wall_surfaces,
            report.roof_surfaces,
            report.ground_surfaces
        ),
        (4, 1, 1)
    );
    assert_eq!((report.unclassified_faces, report.skipped_faces), (0, 0));
    assert!(report.issues.is_empty());
    let s = surfaces(&native);
    assert_eq!(count(&s, "WallSurface"), 4);
    for (kind, nz) in &s {
        match kind.as_str() {
            "GroundSurface" => assert!(*nz < -0.99, "ground faces point down"),
            "RoofSurface" => assert!(*nz > 0.99, "roof faces point up"),
            _ => assert!(nz.abs() < 1e-9),
        }
    }
    let summary = &report.buildings[0];
    assert_eq!(summary.id, "cadkit_bldg_1");
    assert_eq!(
        (
            summary.wall_surfaces,
            summary.roof_surfaces,
            summary.ground_surfaces
        ),
        (4, 1, 1)
    );
}

#[test]
fn gable_house_in_both_windings_has_two_roofs_four_walls_and_a_ground() {
    for reversed in [false, true] {
        let mut faces = gable_faces();
        if reversed {
            faces.iter_mut().for_each(|f| f.reverse());
        }
        let (_, report, native) = roundtrip(&document(polylines(faces)), &options());
        assert_eq!(
            (
                report.wall_surfaces,
                report.roof_surfaces,
                report.ground_surfaces
            ),
            (4, 2, 1),
            "reversed: {reversed}"
        );
        let s = surfaces(&native);
        assert!(
            s.iter()
                .filter(|(k, _)| k == "RoofSurface")
                .all(|(_, z)| *z > 0.5)
        );
        assert!(
            s.iter()
                .filter(|(k, _)| k == "GroundSurface")
                .all(|(_, z)| *z < -0.99)
        );
    }
}

#[test]
fn closed_polylines_are_ignored_unless_enabled() {
    let doc = document(polylines(box_faces(p(0., 0., 0.), p(1., 1., 1.))));
    let options = BuildingOptions::new(ExportOptions::new(SRS, 2));
    let (bytes, report) = write_buildings(&doc, 0, &options).unwrap();
    assert_eq!(
        (report.buildings.len(), report.closed_polylines_ignored),
        (0, 6)
    );
    let native = read_native(&bytes, &ReadOptions::default()).unwrap();
    assert!(
        !native
            .root
            .elements()
            .any(|e| e.name.is(CORE, "cityObjectMember"))
    );
    for lod in [0, 1, 5] {
        let bad = BuildingOptions::new(
            ExportOptions::new(SRS, lod).with_closed_polylines_as_surfaces(true),
        );
        assert!(buildings_from_document(&doc, 0, &bad).is_err());
    }
    assert!(buildings_from_document(&doc, 1, &self::options()).is_err());
    let unnamed =
        BuildingOptions::new(ExportOptions::new(" ", 2).with_closed_polylines_as_surfaces(true));
    assert!(buildings_from_document(&doc, 0, &unnamed).is_err());
}

#[test]
fn grouping_modes_split_faces_into_buildings() {
    let a: Vec<Entity> = polylines(box_faces(p(0., 0., 0.), p(4., 4., 3.)))
        .into_iter()
        .map(|e| e.on_layer("A"))
        .collect();
    let b: Vec<Entity> = polylines(box_faces(p(10., 0., 0.), p(4., 4., 6.)))
        .into_iter()
        .map(|e| e.on_layer("B"))
        .collect();
    let cells = document(vec![group(a.clone()), group(b.clone())]);
    let loose = document(a.into_iter().chain(b).collect());
    let run = |doc: &Document, grouping| roundtrip(doc, &options().with_grouping(grouping)).1;
    assert_eq!(run(&cells, BuildingGrouping::Single).buildings.len(), 1);
    let by_layer = run(&loose, BuildingGrouping::Layer);
    assert_eq!(by_layer.buildings.len(), 2);
    assert_eq!(by_layer.buildings[1].layer.as_deref(), Some("B"));
    let by_cell = run(&cells, BuildingGrouping::TopLevel);
    assert_eq!(by_cell.buildings.len(), 2);
    assert_eq!(by_cell.buildings[1].top_level, Some(1));
    // Without cells every top-level face is its own building; connectivity rebuilds them.
    assert_eq!(run(&loose, BuildingGrouping::TopLevel).buildings.len(), 12);
    let connected = run(&loose, BuildingGrouping::Connected);
    assert_eq!(connected.buildings.len(), 2);
    for summary in &connected.buildings {
        assert_eq!(
            (
                summary.wall_surfaces,
                summary.roof_surfaces,
                summary.ground_surfaces
            ),
            (4, 1, 1)
        );
    }
    // Ground is per building: the taller box standing on a plinth still has a ground.
    let mut raised = box_faces(p(10., 0., 1.), p(4., 4., 2.));
    raised.extend(box_faces(p(0., 0., 0.), p(4., 4., 3.)));
    let r = run(&document(polylines(raised)), BuildingGrouping::Connected);
    assert!(
        r.buildings
            .iter()
            .all(|b| b.ground_surfaces == 1 && b.roof_surfaces == 1)
    );
    let single = run(
        &document(polylines({
            let mut f = box_faces(p(10., 0., 1.), p(4., 4., 2.));
            f.extend(box_faces(p(0., 0., 0.), p(4., 4., 3.)));
            f
        })),
        BuildingGrouping::Single,
    );
    assert_eq!((single.ground_surfaces, single.roof_surfaces), (1, 3));
}

#[test]
fn connected_grouping_welds_vertices_within_tolerance() {
    let mut faces = box_faces(p(0., 0., 0.), p(2., 2., 2.));
    for v in &mut faces[1] {
        v.x += 0.0004;
    }
    let doc = document(polylines(faces));
    let welded = roundtrip(&doc, &options().with_grouping(BuildingGrouping::Connected)).1;
    assert_eq!(welded.buildings.len(), 1);
    let exact = roundtrip(
        &doc,
        &options()
            .with_grouping(BuildingGrouping::Connected)
            .with_weld_tolerance(0.0),
    )
    .1;
    assert_eq!(exact.buildings.len(), 2);
}

#[test]
fn bad_faces_are_skipped_with_paths_that_resolve() {
    let mut entities = polylines(box_faces(p(0., 0., 0.), p(4., 3., 2.5)));
    // Not planar: one corner 10 cm off the plane of a 4 m face.
    entities.push(Entity::polyline(
        [p(0., 0., 5.), p(4., 0., 5.), p(4., 3., 5.1), p(0., 3., 5.)],
        true,
    ));
    // Degenerate: collinear.
    entities
        .push(Entity::polyline([p(0., 0., 0.), p(1., 1., 1.), p(2., 2., 2.)], true).with_id(77));
    // Self-intersecting bow tie.
    entities.push(Entity::polyline(
        [p(0., 0., 7.), p(4., 2., 7.), p(4., 0., 7.), p(0., 3., 7.)],
        true,
    ));
    let mut curved = Entity::polyline([p(0., 0., 0.), p(1., 0., 0.), p(1., 1., 0.)], true);
    if let EntityKind::Polyline { vertices, .. } = &mut curved.kind {
        vertices[0].bulge = 0.5;
    }
    entities.push(curved);
    entities.push(Entity::polyline([p(0., 0., 0.), p(1., 0., 0.)], false));
    entities.push(Entity::new(EntityKind::Mesh {
        vertices: vec![p(0., 0., 9.), p(1., 0., 9.), p(1., 1., 9.)],
        faces: vec![vec![0, 1, 2], vec![0, 1, 9]],
    }));
    let doc = document(entities.into_iter().map(|e| e.on_layer("L")).collect());
    let (_, report, _) = roundtrip(&doc, &options());
    let codes: Vec<&str> = report.issues.iter().map(|i| i.code.as_str()).collect();
    assert_eq!(
        codes,
        [
            "gml.face_not_planar",
            "gml.face_degenerate",
            "gml.face_self_intersecting",
            "gml.face_curved",
            "gml.face_index_out_of_range",
        ]
    );
    assert_eq!(report.skipped_faces, 5);
    assert_eq!(report.skipped_by_code["gml.face_not_planar"], 1);
    assert_eq!(report.ignored_entities, 1);
    // The mesh's valid triangle is the second horizontal face above the base: a roof.
    assert_eq!((report.roof_surfaces, report.ground_surfaces), (2, 1));
    for issue in &report.issues {
        let e = doc.entity_at(0, &issue.path).unwrap();
        assert_eq!(e.kind.type_name(), issue.entity_type);
        assert_eq!(issue.layer, "L");
        assert_eq!(issue.model_path_len, issue.path.len());
    }
    assert_eq!(report.issues[1].entity_id, Some(77));
    assert_eq!(report.issues[4].mesh_face, Some(1));
}

#[test]
fn holes_are_kept_and_wound_against_the_exterior() {
    let mut faces = box_faces(p(0., 0., 0.), p(10., 10., 3.));
    let roof = faces.remove(1);
    let mut entities = polylines(faces);
    let mut exterior = roof.clone();
    exterior.push(roof[0]);
    // Same winding as the exterior: invalid as GML until reversed.
    let hole: Vec<Point3> = [
        p(4., 4., 3.),
        p(6., 4., 3.),
        p(6., 6., 3.),
        p(4., 6., 3.),
        p(4., 4., 3.),
    ]
    .into_iter()
    .collect();
    let hole = if cadkit_core::polygon::normal(&hole).unwrap().z
        * cadkit_core::polygon::normal(&exterior).unwrap().z
        > 0.0
    {
        hole
    } else {
        hole.into_iter().rev().collect()
    };
    entities.push(Entity::new(EntityKind::Polygon {
        exterior,
        interiors: vec![hole],
    }));
    let (_, report, native) = roundtrip(&document(entities), &options());
    assert_eq!(report.roof_surfaces, 1);
    assert_eq!(
        validate(&native, &ValidationOptions::default())
            .unwrap()
            .interiors,
        1
    );
}

#[test]
fn faces_inside_block_references_are_placed_and_traceable() {
    let mut block_faces = polylines(box_faces(p(0., 0., 0.), p(3., 3., 3.)));
    block_faces.push(Entity::polyline(
        [p(0., 0., 4.), p(3., 0., 4.), p(3., 3., 4.2), p(0., 3., 4.)],
        true,
    ));
    let mut doc = document(vec![
        Entity::line(p(0., 0., 0.), p(1., 0., 0.)),
        Entity::insert("HOUSE", p(100., 0., 0.)),
        Entity::insert("HOUSE", p(200., 0., 0.)),
    ]);
    doc.blocks
        .push(Block::new("HOUSE").with_entities(block_faces));
    let (bytes, report, _) = roundtrip(&doc, &options().with_grouping(BuildingGrouping::TopLevel));
    assert_eq!(report.buildings.len(), 2);
    assert_eq!(report.buildings[0].top_level, Some(1));
    assert_eq!(report.ignored_entities, 1);
    assert_eq!(report.issues.len(), 2);
    for issue in &report.issues {
        assert_eq!(issue.code, "gml.face_not_planar");
        assert_eq!(issue.path[1], 6);
        assert_eq!(issue.model_path_len, 1);
        assert!(matches!(
            doc.entity_at(0, &issue.path).unwrap().kind,
            EntityKind::Polyline { .. }
        ));
        assert!(matches!(
            doc.entity_at(0, &issue.path[..issue.model_path_len])
                .unwrap()
                .kind,
            EntityKind::Insert { .. }
        ));
    }
    assert_eq!(report.issues[0].path, [1, 6]);
    assert_eq!(report.issues[1].path, [2, 6]);
    // Placed in model space by the insert.
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("<gml:lowerCorner>100 0 0</gml:lowerCorner>"));
    assert!(text.contains("<gml:upperCorner>203 3 3</gml:upperCorner>"));
}

#[test]
fn sloped_faces_at_the_base_stay_unclassified_in_the_building_multisurface() {
    // An A-frame: two roof planes from the ground to the ridge, two triangular ends.
    let faces = vec![
        vec![p(0., 0., 0.), p(8., 0., 0.), p(8., 6., 0.), p(0., 6., 0.)],
        vec![p(0., 0., 0.), p(8., 0., 0.), p(8., 3., 5.), p(0., 3., 5.)],
        vec![p(0., 6., 0.), p(0., 3., 5.), p(8., 3., 5.), p(8., 6., 0.)],
        vec![p(0., 0., 0.), p(0., 3., 5.), p(0., 6., 0.)],
        vec![p(8., 0., 0.), p(8., 6., 0.), p(8., 3., 5.)],
    ];
    for lod in [2, 3, 4] {
        let o = BuildingOptions::new(
            ExportOptions::new(SRS, lod).with_closed_polylines_as_surfaces(true),
        );
        let (_, report, native) = roundtrip(&document(polylines(faces.clone())), &o);
        assert_eq!(
            (
                report.wall_surfaces,
                report.ground_surfaces,
                report.unclassified_faces
            ),
            (2, 1, 2)
        );
        assert!(
            report
                .issues
                .iter()
                .all(|i| i.code == "gml.face_sloped_at_base"
                    && i.building_id.as_deref() == Some("cadkit_bldg_1"))
        );
        assert_eq!(
            report.issues[0].polygon_id.as_deref(),
            Some("cadkit_bldg_1_p2")
        );
        let building = native
            .root
            .elements()
            .flat_map(|m| m.elements())
            .find(|e| e.name.is(BUILDING, "Building"))
            .unwrap();
        let order: Vec<&str> = building.elements().map(|e| e.name.local()).collect();
        let multi = format!("lod{lod}MultiSurface");
        let at = order.iter().position(|n| *n == multi).unwrap();
        let bounded = order.iter().position(|n| *n == "boundedBy").unwrap();
        // gml:boundedBy (the envelope) comes first; then CityGML 2.0 schema order.
        assert_eq!(order[0], "boundedBy");
        let thematic = order
            .iter()
            .skip(1)
            .position(|n| *n == "boundedBy")
            .unwrap()
            + 1;
        assert!(bounded == 0);
        assert_eq!(lod == 2, at < thematic);
    }
}

#[test]
fn output_is_deterministic() {
    let mut entities = polylines(gable_faces());
    entities.extend(polylines(box_faces(p(20., 0., 0.), p(3., 3., 3.))));
    let doc = document(entities);
    let o = options().with_grouping(BuildingGrouping::Connected);
    let (a, ra) = write_buildings(&doc, 0, &o).unwrap();
    let (b, rb) = write_buildings(&doc, 0, &o).unwrap();
    assert_eq!(a, b);
    assert_eq!(ra, rb);
    let text = String::from_utf8(a).unwrap();
    assert!(text.contains("gml:id=\"cadkit_bldg_2_s6\""));
    assert!(!text.contains("cadkit_bldg_3"));
}

#[test]
fn tkgm_preflight_on_buildings_names_what_only_the_caller_can_supply() {
    let doc = document(polylines(box_faces(
        p(500_000., 4_400_000., 10.),
        p(4., 3., 2.5),
    )));
    let (_, _, native) = roundtrip(&doc, &options());
    let plain =
        tkgm::preflight(&native, tkgm::Profile::CityModelTender, &Limits::default()).unwrap();
    assert!(
        !serde_json::to_string(&plain)
            .unwrap()
            .contains("cadkit_bldg")
    );
    let report =
        tkgm::preflight_located(&native, tkgm::Profile::CityModelTender, &Limits::default())
            .unwrap();
    assert_eq!(report.issues.len(), plain.issues.len());
    assert_eq!(report.buildings, 1);
    let mut found: Vec<(String, Option<String>)> = report
        .issues
        .iter()
        .map(|i| (i.code.clone(), i.field.clone()))
        .collect();
    found.sort();
    for code in [
        "tkgm.missing_building",
        "tkgm.missing_envelope",
        "tkgm.missing_class",
        "tkgm.crs",
        "tkgm.envelope_dimension",
    ] {
        assert!(!found.iter().any(|(c, _)| c == code), "{code}");
    }
    let properties: Vec<&str> = found
        .iter()
        .filter(|(c, _)| c == "tkgm.missing_property")
        .filter_map(|(_, f)| f.as_deref())
        .collect();
    assert_eq!(
        properties,
        [
            "lod0FootPrint",
            "lod0RoofEdge",
            "lod1Solid",
            "lod2MultiSurface",
            "lod2TerrainIntersection",
            "name",
            "storeysAboveGround",
            "storeysBelowGround",
        ]
    );
    assert!(found.contains(&("tkgm.building_class".into(), Some("class".into()))));
    let attributes = found
        .iter()
        .filter(|(c, _)| c == "tkgm.missing_attribute")
        .count();
    // 15 building fields less the two year-dependent MAKS ones, plus buildingHeight.
    assert_eq!(attributes, 14);
    assert_eq!(found.len(), 8 + 1 + 14);
    for issue in &report.issues {
        assert_eq!(issue.id.as_deref(), Some("cadkit_bldg_1"));
        assert_eq!(issue.building_id.as_deref(), Some("cadkit_bldg_1"));
    }
}

#[test]
fn generic_export_can_write_closed_polylines_as_polygons() {
    let mut entities = polylines(box_faces(p(0., 0., 0.), p(2., 2., 2.)));
    entities.push(Entity::polyline(
        [p(0., 0., 5.), p(4., 0., 5.), p(4., 3., 5.1), p(0., 3., 5.)],
        true,
    ));
    let doc = document(entities);
    let off = ExportOptions::new(SRS, 2);
    let native = from_document(&doc, &off).unwrap();
    assert_eq!(
        validate(&native, &ValidationOptions::default())
            .unwrap()
            .polygons,
        0
    );
    let on = off.clone().with_closed_polylines_as_surfaces(true);
    let (native, report) =
        from_document_with_report(&doc, &on, UnsupportedGeometry::Reject).unwrap();
    assert_eq!(
        validate(&native, &ValidationOptions::default())
            .unwrap()
            .polygons,
        6
    );
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].code, "gml.face_not_planar");
    assert_eq!(report.issues[0].path, [6]);
    assert_eq!(report.metadata_only_entities, 0);
    let bytes = write_document(&doc, &on).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(!text.contains("cadkit.geometry_status"));
    validate(
        &read_native(&bytes, &ReadOptions::default()).unwrap(),
        &ValidationOptions::default(),
    )
    .unwrap();
    // A tighter absolute ceiling rejects even small deviations.
    let mut tilted = polylines(vec![vec![
        p(0., 0., 0.),
        p(4., 0., 0.),
        p(4., 3., 0.003),
        p(0., 3., 0.),
    ]]);
    let doc = document(std::mem::take(&mut tilted));
    let strict = on.clone().with_max_planarity_deviation(0.001);
    assert_eq!(
        from_document_with_report(&doc, &strict, UnsupportedGeometry::Reject)
            .unwrap()
            .1
            .issues
            .len(),
        1
    );
    assert!(
        from_document_with_report(&doc, &on, UnsupportedGeometry::Reject)
            .unwrap()
            .1
            .issues
            .is_empty()
    );
}

#[test]
fn located_geometry_issues_name_polygon_surface_and_building() {
    let mut doc = CityGmlDocument::new();
    let mut building = CityObject::new(ObjectKind::Building, "b1");
    let mut wall = CityObject::new(ObjectKind::WallSurface, "w1");
    let mut lod = Element::new(BUILDING, "bldg:lod2MultiSurface");
    let mut multi = Element::new(GML, "gml:MultiSurface");
    let mut member = Element::new(GML, "gml:surfaceMember");
    member.push(
        Polygon {
            id: Some("p1".into()),
            exterior: vec![
                p(0., 0., 0.),
                p(4., 0., 0.),
                p(4., 3., 1.),
                p(0., 3., 0.),
                p(0., 0., 0.),
            ],
            interiors: vec![],
        }
        .to_element(),
    );
    multi.push(member);
    lod.push(multi);
    wall.properties.push(lod);
    let mut bounded = Element::new(BUILDING, "bldg:boundedBy");
    bounded.push(wall.into_element());
    building.properties.push(bounded);
    doc.add_object(building);
    let issues = located_geometry_issues(&doc, &ValidationOptions::default()).unwrap();
    assert_eq!(issues.len(), 1);
    let issue = &issues[0];
    assert_eq!(issue.code, "gml.polygon_geometry");
    assert_eq!(
        (
            issue.id.as_deref(),
            issue.object_id.as_deref(),
            issue.building_id.as_deref()
        ),
        (Some("p1"), Some("w1"), Some("b1"))
    );
    assert_eq!(
        geometry_issues(&doc, &ValidationOptions::default())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn reader_counts_the_nodes_the_writer_counted() {
    // Each quote of a JSON metadata attribute is written as an entity reference; the
    // reader used to count every reference as a node and refuse its own writer's output.
    let mut doc = CityGmlDocument::new();
    let mut object = CityObject::new(ObjectKind::GenericCityObject, "g1");
    let json = format!(
        "{{{}}}",
        (0..200)
            .map(|i| format!("\"k{i}\":\"v\""))
            .collect::<Vec<_>>()
            .join(",")
    );
    object
        .properties
        .push(generic_attribute("cadkit.entity", &cadkit_core::Value::Text(json)).unwrap());
    doc.add_object(object);
    let mut limits = Limits::default();
    // CityModel, member, object, attribute, value and its text.
    limits.max_objects = 6;
    let bytes = write(&doc, &ValidationOptions::default().with_limits(limits)).unwrap();
    assert!(String::from_utf8_lossy(&bytes).matches("&quot;").count() >= 800);
    let back = read_native(&bytes, &ReadOptions::default().with_limits(limits)).unwrap();
    assert_eq!(back, read_native(&bytes, &ReadOptions::default()).unwrap());
    limits.max_objects = 5;
    assert!(read_native(&bytes, &ReadOptions::default().with_limits(limits)).is_err());
}

#[test]
fn options_deserialize_with_defaults() {
    let export: ExportOptions =
        serde_json::from_str(r#"{"srs_name":"EPSG:5254","lod":2}"#).unwrap();
    assert!(!export.closed_polylines_as_surfaces);
    assert_eq!(
        (export.planarity_tolerance, export.max_planarity_deviation),
        (1e-3, 0.01)
    );
    let options: BuildingOptions = serde_json::from_str(
        r#"{"export":{"srs_name":"EPSG:5254","lod":2,"closed_polylines_as_surfaces":true},"grouping":"connected"}"#,
    )
    .unwrap();
    assert_eq!(options.grouping, BuildingGrouping::Connected);
    assert_eq!(
        (
            options.wall_min_tilt_degrees,
            options.horizontal_max_tilt_degrees
        ),
        (80.0, 10.0)
    );
    assert_eq!(
        (options.ground_tolerance, options.weld_tolerance),
        (0.01, 0.001)
    );
    assert_eq!(options.max_work, 100_000_000);
}

#[test]
fn budgets_bound_the_work() {
    let doc = document(polylines(box_faces(p(0., 0., 0.), p(1., 1., 1.))));
    assert!(buildings_from_document(&doc, 0, &options().with_max_work(10)).is_err());
    let mut limits = Limits::default();
    limits.max_objects = 30;
    let o = BuildingOptions::new(
        ExportOptions::new(SRS, 2)
            .with_closed_polylines_as_surfaces(true)
            .with_limits(limits),
    );
    assert!(matches!(
        buildings_from_document(&doc, 0, &o),
        Err(cadkit_core::Error::LimitExceeded { .. })
    ));
}

/// A box with a holed roof drawn as a cell on layer A, and an A-frame (two faces that
/// stay unclassified) as a cell on layer B, 20 m away.
fn mixed_site() -> Document {
    let mut faces = box_faces(p(0., 0., 0.), p(10., 10., 3.));
    let roof = faces.remove(1);
    let mut house = polylines(faces);
    let mut exterior = roof.clone();
    exterior.push(roof[0]);
    let mut hole = vec![
        p(4., 4., 3.),
        p(6., 4., 3.),
        p(6., 6., 3.),
        p(4., 6., 3.),
        p(4., 4., 3.),
    ];
    if cadkit_core::polygon::normal(&hole).unwrap().z
        * cadkit_core::polygon::normal(&exterior).unwrap().z
        < 0.0
    {
        hole.reverse();
    }
    house.push(Entity::new(EntityKind::Polygon {
        exterior,
        interiors: vec![hole],
    }));
    let frame = polylines(vec![
        vec![
            p(20., 0., 0.),
            p(28., 0., 0.),
            p(28., 6., 0.),
            p(20., 6., 0.),
        ],
        vec![
            p(20., 0., 0.),
            p(28., 0., 0.),
            p(28., 3., 5.),
            p(20., 3., 5.),
        ],
        vec![
            p(20., 6., 0.),
            p(20., 3., 5.),
            p(28., 3., 5.),
            p(28., 6., 0.),
        ],
        vec![p(20., 0., 0.), p(20., 3., 5.), p(20., 6., 0.)],
        vec![p(28., 0., 0.), p(28., 6., 0.), p(28., 3., 5.)],
    ]);
    let on = |entities: Vec<Entity>, layer: &str| -> Vec<Entity> {
        entities.into_iter().map(|e| e.on_layer(layer)).collect()
    };
    document(vec![group(on(house, "A")), group(on(frame, "B"))])
}

#[test]
fn every_lod_and_grouping_writes_schema_ordered_buildings() {
    let groupings = [
        (BuildingGrouping::Single, 1),
        (BuildingGrouping::Layer, 2),
        (BuildingGrouping::TopLevel, 2),
        (BuildingGrouping::Connected, 2),
    ];
    let doc = mixed_site();
    for lod in [2, 3, 4] {
        for (grouping, buildings) in groupings {
            let o = BuildingOptions::new(
                ExportOptions::new(SRS, lod).with_closed_polylines_as_surfaces(true),
            )
            .with_grouping(grouping);
            // roundtrip() runs the official XSD check and the schema order assertion.
            let (bytes, report, native) = roundtrip(&doc, &o);
            let context = format!("LoD {lod}, {grouping:?}");
            assert_eq!(report.buildings.len(), buildings, "{context}");
            assert_eq!(
                (
                    report.wall_surfaces,
                    report.roof_surfaces,
                    report.ground_surfaces,
                    report.unclassified_faces,
                ),
                (6, 1, 2, 2),
                "{context}"
            );
            let text = String::from_utf8(bytes).unwrap();
            for other in [2, 3, 4].into_iter().filter(|&l| l != lod) {
                assert!(!text.contains(&format!("lod{other}")), "{context}");
            }
            // Every polygon, surface and building has a distinct id.
            let ids = validate(&native, &ValidationOptions::default()).unwrap();
            assert_eq!(ids.polygons, 11, "{context}");
            assert_eq!(ids.interiors, 1, "{context}");
        }
    }
}

#[test]
fn classification_at_the_edges() {
    let classes = |doc: &Document, o: &BuildingOptions| {
        let (_, r, _) = roundtrip(doc, o);
        (
            r.wall_surfaces,
            r.roof_surfaces,
            r.ground_surfaces,
            r.unclassified_faces,
        )
    };
    // No ground face: the base is the bottom of the walls, the flat roof stays a roof.
    let mut open = box_faces(p(0., 0., 0.), p(4., 3., 2.5));
    open.remove(0);
    assert_eq!(
        classes(&document(polylines(open)), &options()),
        (4, 1, 0, 0)
    );
    // An intermediate slab is a horizontal face above the base: a roof by geometry.
    let mut storeys = box_faces(p(0., 0., 0.), p(4., 3., 6.));
    storeys.push(vec![
        p(0., 0., 3.),
        p(4., 0., 3.),
        p(4., 3., 3.),
        p(0., 3., 3.),
    ]);
    assert_eq!(
        classes(&document(polylines(storeys)), &options()),
        (4, 2, 1, 0)
    );
    // A face that is its own building has its lowest point at the base: a lone flat
    // roof drawn at the top level becomes a ground surface under TopLevel grouping.
    let roof = vec![vec![
        p(0., 0., 3.),
        p(4., 0., 3.),
        p(4., 3., 3.),
        p(0., 3., 3.),
    ]];
    let top = options().with_grouping(BuildingGrouping::TopLevel);
    assert_eq!(classes(&document(polylines(roof)), &top), (0, 0, 1, 0));
    // A sloped face whose lowest point lies in the base band stays unclassified, one
    // just above the band is a roof; a horizontal face inside the band is ground.
    for (z, expected) in [(0.009, (0, 0, 2, 1)), (0.011, (0, 1, 2, 0))] {
        let faces = vec![
            vec![p(0., 0., 0.), p(4., 0., 0.), p(4., 3., 0.), p(0., 3., 0.)],
            vec![
                p(0., 0., 0.005),
                p(4., 0., 0.005),
                p(4., 3., 0.005),
                p(0., 3., 0.005),
            ],
            vec![
                p(0., 0., z),
                p(4., 0., z),
                p(4., 3., z + 2.),
                p(0., 3., z + 2.),
            ],
        ];
        assert_eq!(
            classes(&document(polylines(faces)), &options()),
            expected,
            "z = {z}"
        );
    }
}

#[test]
fn faces_exactly_at_a_threshold_belong_to_it() {
    // Tilt 45 degrees at the base: |n.z| is 1/sqrt(2) up to rounding, as is
    // cos(45 degrees), so only the threshold decides between wall, ground and none.
    let slope = vec![
        vec![p(0., 0., 0.), p(4., 0., 0.), p(4., 3., 0.), p(0., 3., 0.)],
        vec![
            p(0., 0., 0.),
            p(10., 0., 0.),
            p(10., 10., 10.),
            p(0., 10., 10.),
        ],
    ];
    let doc = document(polylines(slope));
    let wall = options()
        .with_horizontal_max_tilt_degrees(10.)
        .with_wall_min_tilt_degrees(45.);
    assert_eq!(roundtrip(&doc, &wall).1.wall_surfaces, 1);
    let flat = options()
        .with_horizontal_max_tilt_degrees(45.)
        .with_wall_min_tilt_degrees(80.);
    assert_eq!(roundtrip(&doc, &flat).1.ground_surfaces, 2);
    // Walls of a rotated block placed at map coordinates are vertical only up to
    // rounding; they stay walls with the threshold at 90 degrees.
    let mut doc = document(vec![Entity::new(EntityKind::Insert {
        block: "BOX".into(),
        position: p(500_000.123, 4_400_000.456, 50.),
        scale: cadkit_core::Vec3::new(1., 1., 1.),
        rotation: 0.7,
        normal: cadkit_core::Vec3::new(0., 0., 1.),
        columns: 1,
        rows: 1,
        column_spacing: 0.,
        row_spacing: 0.,
    })]);
    let mut walls = box_faces(p(0.3, 0.7, 0.), p(4.1, 3.7, 50.));
    // Split every wall edge so the rings have collinear extra vertices.
    for f in walls.iter_mut().skip(2) {
        let (a, b) = (f[0], f[1]);
        let t = 0.37;
        f.insert(
            1,
            p(
                a.x + (b.x - a.x) * t,
                a.y + (b.y - a.y) * t,
                a.z + (b.z - a.z) * t,
            ),
        );
    }
    doc.blocks
        .push(Block::new("BOX").with_entities(polylines(walls)));
    let strict = options().with_wall_min_tilt_degrees(90.);
    let (_, report, _) = roundtrip(&doc, &strict);
    assert_eq!(report.skipped_faces, 0);
    assert_eq!(report.wall_surfaces, 4);
}

#[test]
fn map_coordinates_and_units_do_not_change_the_classes() {
    for (origin, size) in [
        (p(500_000.123, 4_400_000.456, 50.), p(12.3, 8.7, 50.)),
        (p(-73_456.25, 9_876_543.5, 1_234.5), p(4., 3., 2.5)),
        // Millimetres: a 12 m x 8 m x 3 m box.
        (p(0., 0., 0.), p(12_000., 8_000., 3_000.)),
    ] {
        let mut faces = box_faces(origin, size);
        faces.extend(gable_faces().into_iter().map(|f| {
            f.into_iter()
                .map(|q| p(q.x + origin.x + 2. * size.x, q.y + origin.y, q.z + origin.z))
                .collect()
        }));
        for grouping in [BuildingGrouping::Single, BuildingGrouping::Connected] {
            let (_, r, native) = roundtrip(
                &document(polylines(faces.clone())),
                &options().with_grouping(grouping),
            );
            assert_eq!(
                (
                    r.wall_surfaces,
                    r.roof_surfaces,
                    r.ground_surfaces,
                    r.unclassified_faces,
                    r.skipped_faces
                ),
                (8, 3, 2, 0, 0),
                "{origin:?} {grouping:?}"
            );
            for (kind, nz) in surfaces(&native) {
                match kind.as_str() {
                    "GroundSurface" => assert!(nz < -0.99),
                    "RoofSurface" => assert!(nz > 0.5),
                    _ => assert!(nz.abs() < 1e-9),
                }
            }
        }
    }
}

#[test]
fn connected_grouping_welds_across_grid_cell_borders() {
    // Weld cells are weld_tolerance wide; vertices 2e-7 apart on both sides of a cell
    // border (also below zero, where floor rounds away from zero) are one vertex.
    for border in [1.0, -1.0, 0.0, 4_400_000.0] {
        let left = vec![
            p(border - 1e-7, 0., 0.),
            p(border - 2., 0., 0.),
            p(border - 2., 0., 3.),
        ];
        let right = vec![
            p(border + 1e-7, 0., 0.),
            p(border + 2., 0., 3.),
            p(border + 2., 0., 0.),
        ];
        let o = options().with_grouping(BuildingGrouping::Connected);
        let r = roundtrip(&document(polylines(vec![left.clone(), right.clone()])), &o).1;
        assert_eq!(r.buildings.len(), 1, "border {border}");
        // Two tolerances apart they are separate buildings.
        let far: Vec<Point3> = right.iter().map(|q| p(q.x + 0.002, q.y, q.z)).collect();
        let r = roundtrip(&document(polylines(vec![left, far])), &o).1;
        assert_eq!(r.buildings.len(), 2, "border {border}");
    }
}

#[test]
fn issues_are_truncated_at_the_object_limit_but_still_counted() {
    // Each mesh has 40 collinear faces; the document stays within 50 objects.
    let mesh = || {
        Entity::new(EntityKind::Mesh {
            vertices: vec![p(0., 0., 0.), p(1., 1., 1.), p(2., 2., 2.)],
            faces: vec![vec![0, 1, 2]; 40],
        })
    };
    let mut limits = Limits::default();
    limits.max_objects = 50;
    let o = BuildingOptions::new(
        ExportOptions::new(SRS, 2)
            .with_closed_polylines_as_surfaces(true)
            .with_limits(limits),
    );
    let doc = document(vec![group(vec![mesh()]), mesh()]);
    let (_, report) = buildings_from_document(&doc, 0, &o).unwrap();
    assert_eq!(report.skipped_faces, 80);
    assert_eq!(report.skipped_by_code["gml.face_degenerate"], 80);
    assert_eq!(report.issues.len(), 50);
    assert!(report.issues_truncated);
    assert_eq!(report.issues[0].path, [0, 0]);
    assert_eq!(report.issues[49].path, [1]);
    assert_eq!(report.issues[49].mesh_face, Some(9));
    for issue in &report.issues {
        assert!(matches!(
            doc.entity_at(0, &issue.path).unwrap().kind,
            EntityKind::Mesh { .. }
        ));
    }
}

#[test]
fn the_node_estimate_bounds_the_written_tree() {
    // Holes add nodes: whenever the builder accepts a limit, writing accepts it too.
    let mut faces = box_faces(p(0., 0., 0.), p(10., 10., 3.));
    let roof = faces.remove(1);
    let mut entities = polylines(faces);
    let mut exterior = roof.clone();
    exterior.push(roof[0]);
    let up = cadkit_core::polygon::normal(&exterior).unwrap().z > 0.0;
    let interiors = (0..3)
        .map(|i| {
            let x = 1. + 3. * f64::from(i);
            let mut hole = vec![
                p(x, 4., 3.),
                p(x + 2., 4., 3.),
                p(x + 2., 6., 3.),
                p(x, 6., 3.),
                p(x, 4., 3.),
            ];
            if up {
                hole.reverse();
            }
            hole
        })
        .collect();
    entities.push(Entity::new(EntityKind::Polygon {
        exterior,
        interiors,
    }));
    let doc = document(entities);
    let mut accepted = None;
    for max_objects in 1..2_000 {
        let mut limits = Limits::default();
        limits.max_objects = max_objects;
        let o = BuildingOptions::new(
            ExportOptions::new(SRS, 2)
                .with_closed_polylines_as_surfaces(true)
                .with_limits(limits),
        );
        if buildings_from_document(&doc, 0, &o).is_ok() {
            accepted = Some(max_objects);
            write_buildings(&doc, 0, &o).unwrap();
            break;
        }
    }
    assert!(accepted.is_some());
}

#[test]
fn generic_export_writes_what_its_own_planarity_ceiling_accepted() {
    // A corner 6 cm up puts a 40 m x 30 m face 3 cm off its plane: accepted with a
    // 5 cm ceiling, rejected by the default 1 cm.
    let doc = document(polylines(vec![vec![
        p(0., 0., 0.),
        p(40., 0., 0.),
        p(40., 30., 0.06),
        p(0., 30., 0.),
    ]]));
    let loose = ExportOptions::new(SRS, 2)
        .with_closed_polylines_as_surfaces(true)
        .with_max_planarity_deviation(0.05);
    let (native, report) =
        from_document_with_report(&doc, &loose, UnsupportedGeometry::Reject).unwrap();
    assert!(report.issues.is_empty());
    let mut tolerant = ValidationOptions::default();
    tolerant.planarity_tolerance = 0.05;
    assert_eq!(validate(&native, &tolerant).unwrap().polygons, 1);
    // Default validation, which the writers used to apply, refuses the whole document.
    assert!(validate(&native, &ValidationOptions::default()).is_err());
    let bytes = write_document(&doc, &loose).unwrap();
    validate_xsd_if_cached(&bytes);
    write_document_with_report(&doc, &loose, UnsupportedGeometry::Reject).unwrap();
    // The default ceiling keeps it a line string, and the default write validates it.
    let strict = ExportOptions::new(SRS, 2).with_closed_polylines_as_surfaces(true);
    let (_, report) =
        from_document_with_report(&doc, &strict, UnsupportedGeometry::Reject).unwrap();
    assert_eq!(report.issues[0].code, "gml.face_not_planar");
    write_document(&doc, &strict).unwrap();
}

#[test]
fn preflight_located_adds_ids_and_nothing_else() {
    let (_, _, native) = roundtrip(
        &mixed_site(),
        &options().with_grouping(BuildingGrouping::Layer),
    );
    let plain =
        tkgm::preflight(&native, tkgm::Profile::CityModelTender, &Limits::default()).unwrap();
    let located =
        tkgm::preflight_located(&native, tkgm::Profile::CityModelTender, &Limits::default())
            .unwrap();
    let plain_json = serde_json::to_value(&plain).unwrap();
    assert!(!plain.issues.is_empty());
    for issue in plain_json["issues"].as_array().unwrap() {
        let issue = issue.as_object().unwrap();
        assert!(!issue.contains_key("id") && !issue.contains_key("building_id"));
    }
    let mut stripped = serde_json::to_value(&located).unwrap();
    let mut buildings = std::collections::BTreeSet::new();
    for issue in stripped["issues"].as_array_mut().unwrap() {
        let issue = issue.as_object_mut().unwrap();
        if let Some(b) = issue.remove("building_id") {
            buildings.insert(b.as_str().unwrap().to_owned());
        }
        issue.remove("id");
    }
    assert_eq!(stripped, plain_json);
    assert_eq!(
        buildings.into_iter().collect::<Vec<_>>(),
        ["cadkit_bldg_1", "cadkit_bldg_2"]
    );
    // A located finding names the element it counts.
    let nodes: Vec<&Element> = {
        fn walk<'a>(e: &'a Element, out: &mut Vec<&'a Element>) {
            out.push(e);
            for c in e.elements() {
                walk(c, out);
            }
        }
        let mut out = vec![];
        walk(&native.root, &mut out);
        out
    };
    for issue in &located.issues {
        if let Some(id) = &issue.id {
            assert!(nodes.iter().any(|e| e.attribute(GML, "id") == Some(id)));
        }
    }
}

#[test]
fn reader_bounds_documents_made_of_character_references() {
    let doc = |body: &str| {
        format!(
            "<core:CityModel xmlns:core=\"http://www.opengis.net/citygml/2.0\" \
             xmlns:gen=\"http://www.opengis.net/citygml/generics/2.0\">{body}</core:CityModel>"
        )
    };
    // Two million references in one text node: one string, bounded by max_string_bytes.
    let one = doc(&"&#65;".repeat(2_000_000));
    let mut limits = Limits::default();
    limits.max_string_bytes = 1_000_000;
    assert!(matches!(
        read_native(one.as_bytes(), &ReadOptions::default().with_limits(limits)),
        Err(cadkit_core::Error::LimitExceeded { .. })
    ));
    // The decoded total is bounded too, before the text node grows.
    let mut limits = Limits::default();
    limits.max_decompressed_bytes = 1_000_000;
    assert!(matches!(
        read_native(one.as_bytes(), &ReadOptions::default().with_limits(limits)),
        Err(cadkit_core::Error::LimitExceeded { .. })
    ));
    // References in separate elements are separate nodes.
    let many = doc(&"<gen:x>&#65;</gen:x>".repeat(200_000));
    let mut limits = Limits::default();
    limits.max_objects = 100_000;
    assert!(matches!(
        read_native(many.as_bytes(), &ReadOptions::default().with_limits(limits)),
        Err(cadkit_core::Error::LimitExceeded { .. })
    ));
    // Alternating comments split the text into nodes, each counted.
    let split = doc(&"&#65;<!---->".repeat(60_000));
    assert!(matches!(
        read_native(
            split.as_bytes(),
            &ReadOptions::default().with_limits(limits)
        ),
        Err(cadkit_core::Error::LimitExceeded { .. })
    ));
}

#[test]
fn exact_welding_stays_linear_at_web_mercator_coordinates() {
    // 3,000 wall triangles in a row share their y and z values. With a zero weld
    // tolerance the grid must still tell x = 15,000,000 + 10 i apart.
    let row = |x0: f64| {
        let faces = (0..3_000)
            .map(|i| {
                let x = x0 + 10. * f64::from(i);
                vec![p(x, 0., 0.), p(x + 4., 0., 0.), p(x + 4., 0., 3.)]
            })
            .collect();
        document(polylines(faces))
    };
    let o = options()
        .with_grouping(BuildingGrouping::Connected)
        .with_weld_tolerance(0.)
        .with_max_work(1_000_000);
    for x0 in [0., 15_000_000.] {
        let (_, report) = buildings_from_document(&row(x0), 0, &o).unwrap();
        assert_eq!(report.buildings.len(), 3_000, "x0 = {x0}");
    }
}
