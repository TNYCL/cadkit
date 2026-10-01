//! Regression tests for the review findings: hangs, huge allocations, quadratic loops,
//! ownership of sub-entities, OCS of dimension points, CLASS group 91, embedded objects.
#![allow(clippy::field_reassign_with_default)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::print_stdout
)]
mod common;

use cadkit_core::*;
use cadkit_dxf::DxfVersion;
use common::*;

const ALL: [DxfVersion; 7] = [
    DxfVersion::R12,
    DxfVersion::R2000,
    DxfVersion::R2004,
    DxfVersion::R2007,
    DxfVersion::R2010,
    DxfVersion::R2013,
    DxfVersion::R2018,
];

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

fn model_doc(entities: Vec<Entity>) -> Document {
    let mut doc = Document::default();
    doc.models.push(Model {
        name: "Model".into(),
        kind: ModelKind::Model,
        entities,
        ..Model::default()
    });
    doc
}

fn ellipse_with(start: f64, end: f64) -> Entity {
    Entity::new(EntityKind::Ellipse {
        center: p(0.0, 0.0, 0.0),
        major_axis: Vec3::new(2.0, 0.0, 0.0),
        ratio: 0.5,
        start_param: start,
        end_param: end,
        normal: Vec3::Z,
    })
}

#[test]
fn ellipse_parameters_never_hang() {
    let started = std::time::Instant::now();
    for v in ALL {
        let doc = model_doc(vec![
            ellipse_with(f64::NEG_INFINITY, 1.0),
            ellipse_with(f64::NAN, 1.0),
            ellipse_with(-1e300, 1e300),
            ellipse_with(-1e17, -1e17 + 4.0),
            ellipse_with(-7.0, -1.0),
        ]);
        let (text, warnings) = cadkit_dxf::write_with_report(&doc, v).unwrap();
        assert!(
            warnings
                .iter()
                .filter(|w| w.message.contains("ELLIPSE"))
                .count()
                >= 2,
            "{v:?}"
        );
        let back = read(text.as_bytes());
        if v != DxfVersion::R12 {
            for e in &back.models[0].entities {
                let EntityKind::Ellipse { start_param, .. } = e.kind else {
                    panic!()
                };
                assert!((0.0..std::f64::consts::TAU).contains(&start_param));
            }
        }
    }
    assert!(started.elapsed().as_secs() < 20);
}

fn spline(degree: u32) -> Entity {
    Entity::new(EntityKind::Spline {
        degree,
        knots: vec![],
        control_points: vec![p(0.0, 0.0, 0.0), p(1.0, 1.0, 0.0), p(2.0, 0.0, 0.0)],
        weights: vec![],
        fit_points: vec![p(0.0, 0.0, 0.0), p(1.0, 0.0, 0.0)],
        closed: false,
    })
}

fn hatch_with_spline(degree: u32) -> Entity {
    Entity::new(EntityKind::Hatch {
        loops: vec![HatchLoop {
            edges: vec![
                HatchEdge::Line {
                    start: p(0.0, 0.0, 0.0),
                    end: p(1.0, 0.0, 0.0),
                },
                HatchEdge::Spline {
                    degree,
                    knots: vec![],
                    control_points: vec![p(0.0, 0.0, 0.0), p(1.0, 1.0, 0.0), p(2.0, 0.0, 0.0)],
                    weights: vec![],
                },
            ],
            external: true,
        }],
        solid: true,
        pattern: None,
        pattern_scale: 1.0,
        pattern_angle: 0.0,
        normal: Vec3::Z,
    })
}

#[test]
fn absurd_spline_degrees_are_rejected_not_allocated() {
    let started = std::time::Instant::now();
    for v in ALL {
        let doc = model_doc(vec![
            spline(i32::MAX as u32),
            spline(u32::MAX),
            spline(2),
            hatch_with_spline(i32::MAX as u32),
            hatch_with_spline(2),
        ]);
        let (text, warnings) = cadkit_dxf::write_with_report(&doc, v).unwrap();
        assert!(text.len() < 1_000_000, "{v:?}: {} bytes", text.len());
        if v != DxfVersion::R12 {
            assert!(
                warnings.iter().any(|w| w.message.contains("HATCH spline")),
                "{v:?}"
            );
        }
        read(text.as_bytes());
    }
    assert!(started.elapsed().as_secs() < 20);
}

#[test]
fn r12_high_degree_spline_is_fast() {
    let ctrl: Vec<Point3> = (0..70)
        .map(|i| p(f64::from(i), f64::from(i % 5), 0.0))
        .collect();
    let e = Entity::new(EntityKind::Spline {
        degree: 60,
        knots: vec![],
        control_points: ctrl,
        weights: vec![],
        fit_points: vec![],
        closed: false,
    });
    let started = std::time::Instant::now();
    let text = cadkit_dxf::write(&model_doc(vec![e]), DxfVersion::R12).unwrap();
    assert!(started.elapsed().as_secs() < 5);
    assert_eq!(read(text.as_bytes()).models[0].entities.len(), 1);
}

#[test]
fn many_duplicate_layouts_images_and_linetypes_stay_fast() {
    let mut doc = model_doc(vec![]);
    for _ in 0..3000 {
        doc.models.push(Model {
            name: "Same".into(),
            kind: ModelKind::Layout,
            ..Model::default()
        });
    }
    for i in 0..3000 {
        let mut e = Entity::new(EntityKind::Image {
            path: Some(format!("dir{}/same.png", i % 1500)),
            position: p(0.0, 0.0, 0.0),
            u_vector: Vec3::new(1.0, 0.0, 0.0),
            v_vector: Vec3::new(0.0, 1.0, 0.0),
            size_px: Some([1, 1]),
        });
        e.linetype = Some(format!("LT{i}"));
        doc.models[0].entities.push(e);
    }
    let started = std::time::Instant::now();
    let text = cadkit_dxf::write(&doc, DxfVersion::R2018).unwrap();
    assert!(started.elapsed().as_secs() < 20, "{:?}", started.elapsed());
    let back = read(text.as_bytes());
    assert_eq!(back.models.len(), 3001);
    let mut names: Vec<String> = back.models.iter().map(|m| m.name.to_lowercase()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 3001);
}

#[test]
fn sub_entities_are_owned_by_their_parent() {
    let mut insert = Entity::new(EntityKind::Insert {
        block: "B".into(),
        position: p(0.0, 0.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        normal: Vec3::Z,
        columns: 1,
        rows: 1,
        column_spacing: 0.0,
        row_spacing: 0.0,
    });
    insert.attributes = vec![Attribute {
        tag: "T".into(),
        value: Value::Text("v".into()),
        set: None,
        position: None,
        invisible: false,
    }];
    let vtx = |x, y, z| Vertex {
        position: p(x, y, z),
        ..Vertex::default()
    };
    let mut pl3 = Entity::new(EntityKind::Polyline {
        vertices: vec![vtx(0.0, 0.0, 0.0), vtx(1.0, 1.0, 1.0)],
        closed: false,
        normal: Vec3::Z,
    });
    pl3.props
        .insert("dxf.polyline_3d".into(), Value::Bool(true));
    let mesh = Entity::new(EntityKind::Mesh {
        vertices: vec![p(0.0, 0.0, 0.0), p(1.0, 0.0, 0.0), p(1.0, 1.0, 0.0)],
        faces: vec![vec![0, 1, 2]],
    });
    let mut doc = model_doc(vec![insert, pl3, mesh]);
    doc.blocks.push(Block {
        name: "B".into(),
        ..Block::default()
    });
    let text = cadkit_dxf::write(&doc, DxfVersion::R2018).unwrap();
    let section = text.split("ENTITIES").nth(1).unwrap();
    let lines: Vec<&str> = section.lines().skip(1).collect();
    let mut parent: Option<String> = None;
    let mut checked = 0;
    let mut i = 0;
    while i + 1 < lines.len() {
        if lines[i].trim() != "0" {
            i += 2;
            continue;
        }
        let kind = lines[i + 1].trim();
        let (mut handle, mut owner) = (String::new(), String::new());
        let mut j = i + 2;
        while j + 1 < lines.len() && lines[j].trim() != "0" {
            match lines[j].trim() {
                "5" => handle = lines[j + 1].trim().to_owned(),
                "330" => owner = lines[j + 1].trim().to_owned(),
                _ => {}
            }
            j += 2;
        }
        match kind {
            "INSERT" | "POLYLINE" => parent = Some(handle),
            "ATTRIB" | "VERTEX" | "SEQEND" => {
                assert_eq!(Some(&owner), parent.as_ref(), "{kind}");
                checked += 1;
            }
            _ => {}
        }
        i = j;
    }
    // attrib + seqend; 2 vertices + seqend; 3 vertices + 1 face + seqend
    assert_eq!(checked, 2 + 3 + 5);
}

#[test]
fn class_instance_count_is_r2004_plus() {
    for (v, want) in [
        (DxfVersion::R2000, false),
        (DxfVersion::R2004, true),
        (DxfVersion::R2018, true),
    ] {
        let text = cadkit_dxf::write(&model_doc(vec![]), v).unwrap();
        let classes = text
            .split("CLASSES")
            .nth(1)
            .unwrap()
            .split("ENDSEC")
            .next()
            .unwrap();
        let has_91 = classes
            .lines()
            .skip(1)
            .collect::<Vec<_>>()
            .chunks(2)
            .any(|c| c[0].trim() == "91");
        assert_eq!(has_91, want, "{v:?}");
    }
}

fn read_entities(body: &str) -> Vec<Entity> {
    let mut text = String::new();
    for line in format!("0 SECTION\n2 ENTITIES\n{body}\n0 ENDSEC\n0 EOF").lines() {
        let (code, value) = line
            .trim_start()
            .split_once(' ')
            .unwrap_or((line.trim_start(), ""));
        text.push_str(&format!("{code}\n{value}\n"));
    }
    read(text.as_bytes()).models[0].entities.clone()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn dimension_group_16_is_ocs() {
    // N = -Z: OCS (1, 2, 0) -> world (-1, 2, 0). Group 15 is already WCS.
    let v = read_entities(
        "0 DIMENSION\n10 0\n20 0\n30 0\n11 1\n21 2\n31 0\n70 2\n210 0\n220 0\n230 -1\n\
         13 5\n23 6\n33 0\n14 7\n24 8\n34 0\n15 3\n25 4\n35 0\n16 1\n26 2\n36 0",
    );
    let EntityKind::Dimension { points, .. } = &v[0].kind else {
        panic!()
    };
    // Roles in order 10, 13, 14, 15, 16.
    assert!(
        near(points[3].x, 3.0) && near(points[3].y, 4.0),
        "{:?}",
        points[3]
    );
    assert!(
        near(points[4].x, -1.0) && near(points[4].y, 2.0),
        "{:?}",
        points[4]
    );
}

#[test]
fn embedded_object_never_overrides_the_main_part() {
    let v = read_entities(
        "0 MTEXT\n10 5\n20 6\n40 2\n1 hello\n101 Embedded Object\n10 1\n20 0\n11 5\n21 6\n40 9\n1 other",
    );
    let EntityKind::MText {
        position,
        rotation,
        height,
        value,
        ..
    } = &v[0].kind
    else {
        panic!()
    };
    assert!(
        near(position.x, 5.0)
            && near(position.y, 6.0)
            && near(*rotation, 0.0)
            && near(*height, 2.0)
    );
    assert_eq!(value, "hello");
    // Multi-line ATTRIB: the value is the embedded MTEXT; a plain one keeps group 1.
    let v = read_entities(
        "0 INSERT\n66 1\n2 B\n0 ATTRIB\n10 0\n20 0\n1 \n2 T\n101 Embedded Object\n10 0\n20 0\n1 line1\\Pline2\n\
         0 ATTRIB\n10 0\n20 0\n1 plain\n2 U\n0 SEQEND",
    );
    assert_eq!(
        v[0].attributes[0].value,
        Value::Text("line1\\Pline2".into())
    );
    assert_eq!(v[0].attributes[1].value, Value::Text("plain".into()));
}
