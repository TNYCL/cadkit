//! The CityGML sample of the website, built into a document the way cadkit-gml maps it (a
//! group per building with the solid's polygons, and a group per boundary surface holding the
//! same polygons again), then meshed.
//!
//! cadkit-scene does not depend on cadkit-gml, so this test reads the polygon coordinates
//! from the file with a minimal scan and arranges them itself. It skips when the sample is
//! missing.
#![cfg(feature = "document")]

use cadkit_core::{Color, Document, Entity, EntityKind, GroupKind, Layer, Point3};
use cadkit_scene::document::{SurfaceOptions, surface_mesh};

const SAMPLE: &str = "../../site/public/samples/cadkit-sample-buildings.gml";

/// Text between `open` (up to the end of its tag) and `close`, for each occurrence.
fn elements<'a>(text: &'a str, open: &str, close: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(after) = rest.find(open).and_then(|start| rest.get(start..)) {
        let Some(body) = after.find('>').and_then(|i| after.get(i + 1..)) else {
            break;
        };
        let Some((inner, tail)) = body
            .find(close)
            .and_then(|end| Some((body.get(..end)?, body.get(end..)?)))
        else {
            break;
        };
        out.push(inner);
        rest = tail;
    }
    out
}

fn polygons(text: &str) -> Vec<Vec<Point3>> {
    elements(text, "<gml:posList", "</gml:posList>")
        .into_iter()
        .map(|list| {
            let values: Vec<f64> = list
                .split_whitespace()
                .filter_map(|v| v.parse().ok())
                .collect();
            let (points, _) = values.as_chunks::<3>();
            points
                .iter()
                .map(|[x, y, z]| Point3::new(*x, *y, *z))
                .collect()
        })
        .collect()
}

fn group(layer: &str, children: Vec<Entity>) -> Entity {
    Entity::new(EntityKind::Group {
        group_kind: GroupKind::Other,
        name: None,
        origin: None,
        children,
    })
    .on_layer(layer)
}

fn polygon(layer: &str, exterior: Vec<Point3>) -> Entity {
    Entity::new(EntityKind::Polygon {
        exterior,
        interiors: vec![],
    })
    .on_layer(layer)
}

fn sample() -> Option<Document> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(SAMPLE);
    let text = std::fs::read_to_string(path).ok()?;
    let mut doc = Document::new();
    doc.model_space_mut().is_3d = true;
    for name in ["Building", "GroundSurface", "RoofSurface", "WallSurface"] {
        doc.layers
            .push(Layer::new(name).with_color(Color::rgb(180, 186, 196)));
    }
    for building in elements(&text, "<bldg:Building ", "</bldg:Building>") {
        let mut solid = Vec::new();
        let mut surfaces = Vec::new();
        for bounded in elements(building, "<bldg:boundedBy>", "</bldg:boundedBy>") {
            let Some(kind) = ["GroundSurface", "RoofSurface", "WallSurface"]
                .into_iter()
                .find(|k| bounded.contains(&format!("<bldg:{k}")))
            else {
                continue;
            };
            let rings = polygons(bounded);
            // The solid refers to the same polygons by xlink; the reader copies them in.
            solid.extend(rings.iter().cloned().map(|r| polygon("Building", r)));
            surfaces.push(group(
                kind,
                rings.into_iter().map(|r| polygon(kind, r)).collect(),
            ));
        }
        solid.extend(surfaces);
        doc.model_space_mut()
            .entities
            .push(group("Building", solid));
    }
    Some(doc)
}

#[test]
fn the_citygml_sample_gives_13_faces_on_three_surface_layers() {
    let Some(doc) = sample() else {
        return;
    };
    let mesh = surface_mesh(&doc, &SurfaceOptions::new()).unwrap();
    let report = &mesh.report;
    assert_eq!((report.faces, report.duplicates), (13, 13));
    assert!(!report.truncated);
    assert_eq!(report.degenerate, 0);
    let drawn: Vec<&str> = mesh
        .layers
        .iter()
        .filter(|l| l.triangle_count > 0)
        .map(|l| l.name.as_str())
        .collect();
    assert_eq!(drawn, ["GroundSurface", "RoofSurface", "WallSurface"]);
    // Positions relative to an origin at UTM magnitudes keep sub-millimetre precision.
    assert!(mesh.origin[1] > 5_000_000.);
    assert!(mesh.positions.iter().all(|v| v.abs() < 20.));
    assert!(mesh.max_error < 1e-5);
    for item in 0..mesh.items.len() {
        let path = mesh.items.path_indices(item).unwrap();
        assert_eq!(path.len(), 3);
        let entity = doc.entity_at(0, &path).unwrap();
        assert!(matches!(entity.kind, EntityKind::Polygon { .. }));
    }
}
