//! Reader behaviour on small hand-written DXF fixtures (one feature per test).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::print_stdout
)]
mod common;

use cadkit_core::*;

/// Turns compact `"code value"` lines into DXF text.
fn dxf_text(compact: &str) -> String {
    let mut out = String::new();
    for line in compact.lines() {
        let line = line.trim_start();
        if line.is_empty() {
            continue;
        }
        let (code, value) = line.split_once(' ').unwrap_or((line, ""));
        out.push_str(&format!("{code}\n{value}\n"));
    }
    out
}

fn read_text(compact: &str) -> Document {
    let text = dxf_text(compact);
    cadkit_dxf::read(text.as_bytes(), &ReadOptions::default()).unwrap()
}

/// Reads entities placed in a bare ENTITIES section.
fn entities(body: &str) -> Vec<Entity> {
    let doc = read_text(&format!("0 SECTION\n2 ENTITIES\n{body}\n0 ENDSEC\n0 EOF"));
    doc.models[0].entities.clone()
}

fn one(body: &str) -> Entity {
    let mut v = entities(body);
    assert_eq!(v.len(), 1, "{v:?}");
    v.remove(0)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn pt(p: Point3, x: f64, y: f64, z: f64) -> bool {
    near(p.x, x) && near(p.y, y) && near(p.z, z)
}

#[test]
fn common_properties() {
    let e = one(
        "0 LINE\n5 1F\n8 Walls\n6 DASHED\n62 3\n370 50\n60 1\n10 0\n20 0\n30 0\n11 5\n21 0\n31 2",
    );
    assert_eq!(e.id, Some(0x1F));
    assert_eq!(e.layer.as_deref(), Some("Walls"));
    assert_eq!(e.linetype.as_deref(), Some("DASHED"));
    assert_eq!(e.color, Color::Aci { index: 3 });
    assert_eq!(e.lineweight, Lineweight::Millimeters(0.5));
    assert!(!e.visible);
    assert!(matches!(e.kind, EntityKind::Line { end, .. } if pt(end, 5.0, 0.0, 2.0)));

    let e = one("0 LINE\n62 256\n420 16711935\n6 BYBLOCK\n370 -2");
    assert_eq!(
        e.color,
        Color::Rgb {
            r: 0xFF,
            g: 0x00,
            b: 0xFF
        }
    );
    assert_eq!(e.linetype.as_deref(), Some("ByBlock"));
    assert_eq!(e.lineweight, Lineweight::ByBlock);
    assert_eq!(e.layer.as_deref(), Some("0"));
    assert_eq!(one("0 LINE\n62 0").color, Color::ByBlock);
}

#[test]
fn xdata_and_reactors() {
    let e = one(
        "0 CIRCLE\n5 A\n102 {ACAD_REACTORS\n330 77\n102 }\n330 1F\n10 1\n20 2\n40 3\n1001 MYAPP\n1000 hello\n1070 5\n1010 1.5\n1001 OTHER\n1040 2.5",
    );
    assert!(e.props.contains_key("dxf.xdata.MYAPP"));
    let Some(Value::List(items)) = e.props.get("dxf.xdata.MYAPP") else {
        panic!()
    };
    assert_eq!(items.len(), 3);
    assert_eq!(
        items[0],
        Value::List(vec![Value::Int(1000), Value::Text("hello".into())])
    );
    assert_eq!(items[1], Value::List(vec![Value::Int(1070), Value::Int(5)]));
    assert!(e.props.contains_key("dxf.xdata.OTHER"));
    assert_eq!(e.id, Some(10));
}

#[test]
fn ocs_conversion_for_planar_entities() {
    // Normal -Z: OCS (x, y, z) -> world (-x, y, z); the angle origin flips with it.
    let e = one("0 CIRCLE\n10 1\n20 2\n30 3\n40 4\n210 0\n220 0\n230 -1");
    let EntityKind::Circle {
        center,
        radius,
        normal,
    } = e.kind
    else {
        panic!()
    };
    assert!(pt(center, -1.0, 2.0, -3.0), "{center:?}");
    assert!(near(radius, 4.0) && near(normal.z, -1.0));

    let e = one("0 ARC\n10 0\n20 0\n40 1\n50 30\n51 90\n210 0\n220 0\n230 -1");
    let EntityKind::Arc {
        start_angle,
        end_angle,
        normal,
        ..
    } = e.kind
    else {
        panic!()
    };
    assert!(near(start_angle, 30f64.to_radians()) && near(end_angle, 90f64.to_radians()));
    assert!(near(normal.z, -1.0));

    // A tilted normal goes through the arbitrary axis algorithm; a lightweight polyline
    // keeps the normal in props so the writer can restore its OCS.
    let e = one("0 LWPOLYLINE\n90 2\n70 0\n38 5\n10 1\n20 0\n10 2\n20 0\n210 1\n220 0\n230 0");
    let EntityKind::Polyline { vertices, .. } = &e.kind else {
        panic!()
    };
    // N = +X: Ax = Z x N = (0,1,0), Ay = N x Ax = (0,0,1); OCS (1,0,5) -> world (5,1,0).
    assert!(
        pt(vertices[0].position, 5.0, 1.0, 0.0),
        "{:?}",
        vertices[0].position
    );
    assert!(pt(vertices[1].position, 5.0, 2.0, 0.0));
    assert!(matches!(&e.kind, EntityKind::Polyline { normal, .. } if near(normal.x, 1.0)));

    // Entities stored in WCS are untouched.
    let e = one("0 LINE\n10 1\n20 2\n30 3\n11 4\n21 5\n31 6\n210 0\n220 0\n230 -1");
    assert!(matches!(e.kind, EntityKind::Line { start, .. } if pt(start, 1.0, 2.0, 3.0)));
}

#[test]
fn lwpolyline() {
    let e = one(
        "0 LWPOLYLINE\n90 3\n70 1\n43 0.5\n38 2\n10 0\n20 0\n42 1\n10 1\n20 0\n40 0.1\n41 0.2\n10 1\n20 1",
    );
    let EntityKind::Polyline {
        vertices, closed, ..
    } = &e.kind
    else {
        panic!()
    };
    assert!(*closed);
    assert_eq!(vertices.len(), 3);
    assert!(pt(vertices[0].position, 0.0, 0.0, 2.0));
    assert!(near(vertices[0].bulge, 1.0));
    assert!(near(vertices[0].start_width, 0.5) && near(vertices[0].end_width, 0.5));
    assert!(near(vertices[1].start_width, 0.1) && near(vertices[1].end_width, 0.2));
}

#[test]
fn polylines_2d_3d_polyface_and_mesh() {
    let e = one(
        "0 POLYLINE\n66 1\n10 0\n20 0\n30 4\n70 1\n40 0.3\n41 0.3\n0 VERTEX\n10 0\n20 0\n42 0.5\n0 VERTEX\n10 3\n20 0\n0 VERTEX\n10 3\n20 3\n0 SEQEND",
    );
    let EntityKind::Polyline {
        vertices, closed, ..
    } = &e.kind
    else {
        panic!()
    };
    assert!(*closed && vertices.len() == 3);
    assert!(
        near(vertices[0].position.z, 4.0)
            && near(vertices[0].bulge, 0.5)
            && near(vertices[1].start_width, 0.3)
    );

    let e = one(
        "0 POLYLINE\n66 1\n70 8\n0 VERTEX\n10 0\n20 0\n30 0\n70 32\n0 VERTEX\n10 1\n20 2\n30 3\n70 32\n0 SEQEND",
    );
    assert!(
        matches!(&e.kind, EntityKind::Polyline { vertices, .. } if pt(vertices[1].position, 1.0, 2.0, 3.0))
    );
    assert_eq!(e.props.get("dxf.polyline_3d"), Some(&Value::Bool(true)));

    // Polyface: 4 vertices (flag 192), 2 faces (flag 128); negative index = hidden edge.
    let e = one("0 POLYLINE\n66 1\n70 64\n71 4\n72 2\n\
        0 VERTEX\n10 0\n20 0\n30 0\n70 192\n0 VERTEX\n10 1\n20 0\n30 0\n70 192\n0 VERTEX\n10 1\n20 1\n30 0\n70 192\n0 VERTEX\n10 0\n20 1\n30 1\n70 192\n\
        0 VERTEX\n10 0\n20 0\n30 0\n70 128\n71 1\n72 2\n73 3\n0 VERTEX\n10 0\n20 0\n30 0\n70 128\n71 1\n72 -3\n73 4\n74 9\n0 SEQEND");
    let EntityKind::Mesh { vertices, faces } = &e.kind else {
        panic!()
    };
    assert_eq!(vertices.len(), 4);
    assert_eq!(faces, &vec![vec![0, 1, 2], vec![0, 2, 3]]);

    // Polygon mesh 3 x 2, open: (3-1) x (2-1) quads.
    let mut s = String::from("0 POLYLINE\n66 1\n70 16\n71 3\n72 2\n");
    for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1), (0, 2), (1, 2)] {
        s.push_str(&format!("0 VERTEX\n10 {x}\n20 {y}\n30 0\n70 64\n"));
    }
    s.push_str("0 SEQEND");
    let e = one(&s);
    let EntityKind::Mesh { vertices, faces } = &e.kind else {
        panic!()
    };
    assert_eq!((vertices.len(), faces.len()), (6, 2));
    assert_eq!(faces[0], vec![0, 2, 3, 1]);
}

#[test]
fn spline_and_ellipse() {
    let e = one(
        "0 SPLINE\n70 8\n71 2\n72 6\n73 3\n74 0\n40 0\n40 0\n40 0\n40 1\n40 1\n40 1\n41 1\n41 2\n41 1\n10 0\n20 0\n30 0\n10 1\n20 2\n30 0\n10 2\n20 0\n30 0",
    );
    let EntityKind::Spline {
        degree,
        knots,
        control_points,
        weights,
        closed,
        ..
    } = &e.kind
    else {
        panic!()
    };
    assert_eq!(
        (
            *degree,
            knots.len(),
            control_points.len(),
            weights.len(),
            *closed
        ),
        (2, 6, 3, 3, false)
    );
    assert!(pt(control_points[1], 1.0, 2.0, 0.0));
    let e = one("0 SPLINE\n70 1\n71 3\n11 0\n21 0\n31 0\n11 1\n21 1\n31 0\n11 2\n21 0\n31 0");
    assert!(
        matches!(&e.kind, EntityKind::Spline { fit_points, closed: true, .. } if fit_points.len() == 3)
    );

    let e = one(
        "0 ELLIPSE\n10 1\n20 2\n30 3\n11 4\n21 0\n31 0\n210 0\n220 0\n230 1\n40 0.5\n41 0\n42 3.14159265358979",
    );
    let EntityKind::Ellipse {
        center,
        major_axis,
        ratio,
        end_param,
        ..
    } = &e.kind
    else {
        panic!()
    };
    assert!(
        pt(*center, 1.0, 2.0, 3.0)
            && near(major_axis.x, 4.0)
            && near(*ratio, 0.5)
            && near(*end_param, std::f64::consts::PI)
    );
}

#[test]
fn text_alignment() {
    let e = one("0 TEXT\n10 1\n20 2\n40 2.5\n1 Hello\n50 90\n41 0.8\n51 15\n7 Standard");
    let EntityKind::Text {
        position,
        rotation,
        width_factor,
        oblique,
        value,
        style,
        halign,
        valign,
        ..
    } = &e.kind
    else {
        panic!()
    };
    assert!(pt(*position, 1.0, 2.0, 0.0) && near(*rotation, std::f64::consts::FRAC_PI_2));
    assert!(near(*width_factor, 0.8) && near(*oblique, 15f64.to_radians()));
    assert_eq!(
        (value.as_str(), style.as_deref(), *halign, *valign),
        ("Hello", Some("Standard"), HAlign::Left, VAlign::Baseline)
    );

    // Center / middle alignment: the anchor is the second point.
    let e = one("0 TEXT\n10 1\n20 2\n11 5\n21 6\n40 1\n1 x\n72 1\n73 2");
    let EntityKind::Text {
        position,
        halign,
        valign,
        ..
    } = &e.kind
    else {
        panic!()
    };
    assert!(pt(*position, 5.0, 6.0, 0.0));
    assert_eq!((*halign, *valign), (HAlign::Center, VAlign::Middle));
    assert!(matches!(
        &e.kind,
        EntityKind::Text {
            end_point: None,
            ..
        }
    ));

    // Aligned: both points matter.
    let e = one("0 TEXT\n10 1\n20 2\n11 5\n21 6\n40 1\n1 x\n72 3");
    assert!(
        matches!(&e.kind, EntityKind::Text { position, halign: HAlign::Aligned, .. } if pt(*position, 1.0, 2.0, 0.0))
    );
    assert!(
        matches!(&e.kind, EntityKind::Text { end_point: Some(q), .. } if pt(*q, 5.0, 6.0, 0.0))
    );
}

#[test]
fn mtext() {
    let e = one(
        "0 MTEXT\n10 1\n20 2\n40 2.5\n41 30\n71 5\n3 {\\fArial|b0;Hel\n3 lo}\\P\n1 \\C1;wor\\~ld\n44 1.5\n11 0\n21 1\n31 0\n7 Std",
    );
    let EntityKind::MText {
        position,
        height,
        width,
        rotation,
        value,
        plain,
        style,
        attachment,
        line_spacing,
        ..
    } = &e.kind
    else {
        panic!()
    };
    assert!(pt(*position, 1.0, 2.0, 0.0) && near(*height, 2.5) && near(*width, 30.0));
    assert!(near(*rotation, std::f64::consts::FRAC_PI_2));
    assert_eq!(value, "{\\fArial|b0;Hello}\\P\\C1;wor\\~ld");
    assert_eq!(plain, "Hello\nwor ld");
    assert_eq!(
        (style.as_deref(), *attachment, *line_spacing),
        (Some("Std"), Attachment::MiddleCenter, 1.5)
    );
}

#[test]
fn insert_with_attributes_and_array() {
    let e = one(
        "0 INSERT\n66 1\n2 BLK\n10 1\n20 2\n41 2\n42 3\n43 4\n50 45\n70 3\n71 2\n44 10\n45 20\n\
        0 ATTRIB\n10 1\n20 2\n40 1\n1 value1\n2 TAG1\n70 1\n0 ATTRIB\n10 3\n20 4\n1 value2\n2 TAG2\n70 0\n0 SEQEND",
    );
    let EntityKind::Insert {
        block,
        position,
        scale,
        rotation,
        columns,
        rows,
        column_spacing,
        row_spacing,
        ..
    } = &e.kind
    else {
        panic!()
    };
    assert_eq!(block, "BLK");
    assert!(
        pt(*position, 1.0, 2.0, 0.0)
            && near(scale.x, 2.0)
            && near(scale.y, 3.0)
            && near(scale.z, 4.0)
    );
    assert!(near(*rotation, std::f64::consts::FRAC_PI_4));
    assert_eq!(
        (*columns, *rows, *column_spacing, *row_spacing),
        (3, 2, 10.0, 20.0)
    );
    assert_eq!(e.attributes.len(), 2);
    assert_eq!(e.attributes[0].tag, "TAG1");
    assert_eq!(e.attributes[0].value, Value::Text("value1".into()));
    assert!(e.attributes[0].invisible && !e.attributes[1].invisible);
    assert!(pt(e.attributes[1].position.unwrap(), 3.0, 4.0, 0.0));
}

#[test]
fn hatch_paths() {
    let e = one(
        "0 HATCH\n10 0\n20 0\n30 7\n210 0\n220 0\n230 1\n2 ANSI31\n70 0\n71 0\n91 2\n\
        92 3\n72 1\n73 1\n93 3\n10 0\n20 0\n42 0.5\n10 4\n20 0\n42 0\n10 4\n20 4\n42 0\n97 0\n\
        92 0\n93 4\n72 1\n10 1\n20 1\n11 2\n21 1\n\
        72 2\n10 2\n20 2\n40 1\n50 0\n51 90\n73 1\n\
        72 3\n10 0\n20 0\n11 3\n21 0\n40 0.5\n50 0\n51 180\n73 0\n\
        72 4\n94 2\n73 0\n74 0\n95 4\n96 2\n40 0\n40 0\n40 1\n40 1\n10 0\n20 0\n10 1\n20 1\n97 0\n\
        97 0\n75 0\n76 1\n52 30\n41 2\n77 0\n78 1\n53 45\n43 0\n44 0\n45 -1\n46 1\n79 2\n49 1\n49 -0.5\n47 1\n98 1\n10 0.5\n20 0.5",
    );
    let EntityKind::Hatch {
        loops,
        solid,
        pattern,
        pattern_scale,
        pattern_angle,
        ..
    } = &e.kind
    else {
        panic!()
    };
    assert!(!*solid && pattern.as_deref() == Some("ANSI31"));
    assert!(near(*pattern_scale, 2.0) && near(*pattern_angle, 30f64.to_radians()));
    assert_eq!(loops.len(), 2);
    assert!(loops[0].external && !loops[1].external);
    let HatchEdge::Polyline { vertices, closed } = &loops[0].edges[0] else {
        panic!()
    };
    assert!(
        *closed
            && vertices.len() == 3
            && near(vertices[0].bulge, 0.5)
            && near(vertices[0].position.z, 7.0)
    );
    let edges = &loops[1].edges;
    assert_eq!(edges.len(), 4);
    assert!(matches!(edges[0], HatchEdge::Line { end, .. } if pt(end, 2.0, 1.0, 7.0)));
    assert!(
        matches!(edges[1], HatchEdge::Arc { radius, end_angle, ccw: true, .. } if near(radius, 1.0) && near(end_angle, std::f64::consts::FRAC_PI_2))
    );
    assert!(
        matches!(edges[2], HatchEdge::Ellipse { ratio, ccw: false, major_axis, .. } if near(ratio, 0.5) && near(major_axis.x, 3.0))
    );
    assert!(
        matches!(&edges[3], HatchEdge::Spline { degree: 2, knots, control_points, .. } if knots.len() == 4 && control_points.len() == 2)
    );
    assert!(e.props.contains_key("dxf.hatch.pattern_lines"));
    assert!(e.props.contains_key("dxf.hatch.seeds"));

    let e = one(
        "0 HATCH\n10 0\n20 0\n30 0\n210 0\n220 0\n230 1\n2 SOLID\n70 1\n71 0\n91 0\n75 0\n76 1\n98 0",
    );
    assert!(matches!(&e.kind, EntityKind::Hatch { solid: true, loops, .. } if loops.is_empty()));
}

#[test]
fn dimension() {
    let e = one(
        "0 DIMENSION\n2 *D1\n10 5\n20 6\n30 0\n11 2\n21 3\n31 0\n70 32\n1 <>\n3 ISO\n42 12.5\n13 0\n23 0\n33 0\n14 10\n24 0\n34 0\n50 0\n",
    );
    let EntityKind::Dimension {
        dimension_type,
        points,
        text_position,
        measurement,
        text,
        block,
        style,
    } = &e.kind
    else {
        panic!()
    };
    assert_eq!(*dimension_type, DimensionType::Linear);
    assert_eq!(points.len(), 3);
    assert!(pt(points[0], 5.0, 6.0, 0.0) && pt(points[2], 10.0, 0.0, 0.0));
    assert!(pt(text_position.unwrap(), 2.0, 3.0, 0.0));
    assert_eq!(
        (
            measurement,
            text.as_deref(),
            block.as_deref(),
            style.as_deref()
        ),
        (&Some(12.5), Some("<>"), Some("*D1"), Some("ISO"))
    );
    assert_eq!(e.props.get("dxf.dim.flags"), Some(&Value::Int(32)));
    let kinds: Vec<(i64, DimensionType)> = vec![
        (1, DimensionType::Aligned),
        (2, DimensionType::Angular),
        (3, DimensionType::Diameter),
        (4, DimensionType::Radius),
        (5, DimensionType::Angular3Point),
        (6 + 64, DimensionType::Ordinate),
        (7, DimensionType::ArcLength),
    ];
    for (flag, want) in kinds {
        let e = one(&format!("0 DIMENSION\n10 0\n20 0\n70 {flag}"));
        assert!(
            matches!(e.kind, EntityKind::Dimension { dimension_type, .. } if dimension_type == want),
            "{flag}"
        );
    }
}

#[test]
fn faces_use_outline_order() {
    // SOLID stores corners 3 and 4 swapped: the outline is 1, 2, 4, 3.
    let e = one("0 SOLID\n10 0\n20 0\n11 1\n21 0\n12 0\n22 1\n13 1\n23 1");
    let EntityKind::Face { points, filled } = &e.kind else {
        panic!()
    };
    assert!(*filled);
    assert!(
        pt(points[2], 1.0, 1.0, 0.0) && pt(points[3], 0.0, 1.0, 0.0),
        "{points:?}"
    );
    // Triangles repeat the third corner.
    let e = one(
        "0 3DFACE\n10 0\n20 0\n30 0\n11 1\n21 0\n31 0\n12 0\n22 1\n32 1\n13 0\n23 1\n33 1\n70 5",
    );
    let EntityKind::Face { points, filled } = &e.kind else {
        panic!()
    };
    assert!(!*filled && points.len() == 3);
    assert_eq!(e.props.get("dxf.invisible_edges"), Some(&Value::Int(5)));
    // 3DFACE is in outline order already.
    let e = one("0 3DFACE\n10 0\n20 0\n11 1\n21 0\n12 1\n22 1\n13 0\n23 1");
    assert!(
        matches!(&e.kind, EntityKind::Face { points, .. } if pt(points[2], 1.0, 1.0, 0.0) && pt(points[3], 0.0, 1.0, 0.0))
    );
}

#[test]
fn leader_point_viewport_and_unknown() {
    let e = one("0 LEADER\n71 0\n76 3\n10 0\n20 0\n30 0\n10 1\n20 1\n30 0\n10 2\n20 1\n30 0");
    assert!(
        matches!(&e.kind, EntityKind::Leader { vertices, arrowhead: false } if vertices.len() == 3)
    );
    let e = one("0 VIEWPORT\n10 5\n20 4\n40 10\n41 8\n12 1\n22 2\n45 20\n69 2");
    assert!(
        matches!(&e.kind, EntityKind::Viewport { width, view_height, .. } if near(*width, 10.0) && near(*view_height, 20.0))
    );
    let doc = read_text("0 SECTION\n2 ENTITIES\n0 WIPEOUT\n8 L\n10 1\n0 WIPEOUT\n0 ENDSEC\n0 EOF");
    assert!(
        matches!(&doc.models[0].entities[0].kind, EntityKind::Unknown { type_name } if type_name == "dxf.WIPEOUT")
    );
    assert_eq!(doc.warnings.len(), 1);
    assert!(doc.warnings[0].message.contains("2 occurrence"));
    assert_eq!(doc.warnings[0].code, "dxf.unknown_entity");
}

#[test]
fn tables_header_and_blocks() {
    let doc = read_text(
        "999 written by a test\n0 SECTION\n2 HEADER\n9 $ACADVER\n1 AC1015\n9 $INSUNITS\n70 4\n9 $EXTMIN\n10 -1\n20 -2\n30 0\n9 $DWGCODEPAGE\n3 ANSI_1252\n0 ENDSEC\n\
        0 SECTION\n2 TABLES\n\
        0 TABLE\n2 LTYPE\n70 2\n0 LTYPE\n2 DASHED\n3 dashed line\n73 2\n40 1.5\n49 1\n49 -0.5\n0 ENDTAB\n\
        0 TABLE\n2 LAYER\n70 3\n\
        0 LAYER\n2 A\n70 0\n62 5\n6 Continuous\n\
        0 LAYER\n2 B\n70 5\n62 -1\n290 0\n370 35\n420 255\n6 DASHED\n\
        0 ENDTAB\n\
        0 TABLE\n2 STYLE\n70 1\n0 STYLE\n2 Standard\n40 0\n41 0.9\n50 15\n3 arial.ttf\n0 ENDTAB\n\
        0 TABLE\n2 BLOCK_RECORD\n70 1\n0 BLOCK_RECORD\n5 30\n2 MyBlock\n0 ENDTAB\n\
        0 ENDSEC\n\
        0 SECTION\n2 BLOCKS\n\
        0 BLOCK\n5 30\n8 0\n2 MyBlock\n70 0\n10 1\n20 2\n30 0\n3 MyBlock\n4 a block\n0 LINE\n8 A\n10 0\n20 0\n11 1\n21 1\n0 ENDBLK\n\
        0 BLOCK\n8 0\n2 *Model_Space\n70 0\n10 0\n20 0\n30 0\n0 ENDBLK\n\
        0 BLOCK\n8 0\n2 XR\n70 4\n10 0\n20 0\n30 0\n1 c:\\refs\\x.dwg\n0 ENDBLK\n\
        0 ENDSEC\n0 EOF",
    );
    assert_eq!(doc.source.version, "AC1015");
    assert_eq!(doc.source.codepage.as_deref(), Some("ANSI_1252"));
    assert_eq!(doc.source.application.as_deref(), Some("written by a test"));
    assert_eq!(doc.units.unit, LengthUnit::Millimeter);
    assert_eq!(
        doc.props.get("dxf.$EXTMIN"),
        Some(&Value::List(vec![
            Value::Float(-1.0),
            Value::Float(-2.0),
            Value::Float(0.0)
        ]))
    );
    assert_eq!(doc.linetypes[0].pattern, vec![1.0, -0.5]);
    assert_eq!(doc.linetypes[0].description.as_deref(), Some("dashed line"));
    let (a, b) = (&doc.layers[0], &doc.layers[1]);
    assert_eq!(
        (a.color, a.visible, a.frozen, a.locked, a.plottable),
        (Color::Aci { index: 5 }, true, false, false, true)
    );
    assert_eq!(
        (b.visible, b.frozen, b.locked, b.plottable),
        (false, true, true, false)
    );
    assert_eq!(b.color, Color::Rgb { r: 0, g: 0, b: 255 });
    assert_eq!(b.lineweight, Lineweight::Millimeters(0.35));
    assert_eq!(b.linetype.as_deref(), Some("DASHED"));
    let s = &doc.text_styles[0];
    assert!(
        near(s.width_factor, 0.9)
            && near(s.oblique, 15f64.to_radians())
            && s.font.as_deref() == Some("arial.ttf")
    );
    // *Model_Space is not a block; the xref is.
    assert_eq!(doc.blocks.len(), 2);
    assert_eq!(doc.blocks[0].name, "MyBlock");
    assert_eq!(doc.blocks[0].id, Some(0x30));
    assert!(pt(doc.blocks[0].base_point, 1.0, 2.0, 0.0));
    assert_eq!(doc.blocks[0].description.as_deref(), Some("a block"));
    assert_eq!(doc.blocks[0].entities.len(), 1);
    assert!(doc.blocks[1].is_xref);
    assert_eq!(doc.blocks[1].xref_path.as_deref(), Some("c:\\refs\\x.dwg"));
}

#[test]
fn layouts_and_paper_space() {
    let doc = read_text(
        "0 SECTION\n2 HEADER\n9 $ACADVER\n1 AC1027\n0 ENDSEC\n\
        0 SECTION\n2 TABLES\n0 TABLE\n2 BLOCK_RECORD\n\
        0 BLOCK_RECORD\n5 A1\n2 *Model_Space\n0 BLOCK_RECORD\n5 A2\n2 *Paper_Space\n0 BLOCK_RECORD\n5 A3\n2 *Paper_Space0\n0 ENDTAB\n0 ENDSEC\n\
        0 SECTION\n2 BLOCKS\n0 BLOCK\n8 0\n2 *Paper_Space0\n70 0\n10 0\n20 0\n30 0\n0 LINE\n5 C1\n330 A3\n67 1\n8 0\n10 0\n20 0\n11 1\n21 1\n0 ENDBLK\n0 ENDSEC\n\
        0 SECTION\n2 ENTITIES\n\
        0 CIRCLE\n5 B1\n330 A1\n8 0\n10 0\n20 0\n40 1\n\
        0 POINT\n5 B2\n330 A2\n67 1\n8 0\n10 5\n20 5\n\
        0 ENDSEC\n\
        0 SECTION\n2 OBJECTS\n\
        0 LAYOUT\n5 D2\n100 AcDbPlotSettings\n1 \n100 AcDbLayout\n1 Second\n70 1\n71 2\n330 A3\n\
        0 LAYOUT\n5 D1\n100 AcDbPlotSettings\n1 \n100 AcDbLayout\n1 First\n70 1\n71 1\n330 A2\n\
        0 LAYOUT\n5 D0\n100 AcDbPlotSettings\n1 \n100 AcDbLayout\n1 Model\n70 1\n71 0\n330 A1\n\
        0 ENDSEC\n0 EOF",
    );
    let names: Vec<(&str, ModelKind, usize)> = doc
        .models
        .iter()
        .map(|m| (m.name.as_str(), m.kind, m.entities.len()))
        .collect();
    assert_eq!(
        names,
        vec![
            ("Model", ModelKind::Model, 1),
            ("First", ModelKind::Layout, 1),
            ("Second", ModelKind::Layout, 1)
        ]
    );
    assert!(matches!(
        doc.models[1].entities[0].kind,
        EntityKind::Point { .. }
    ));
    assert!(matches!(
        doc.models[2].entities[0].kind,
        EntityKind::Line { .. }
    ));
    assert_eq!(doc.models[0].id, Some(0xD0));
}

#[test]
fn image_resolves_its_definition() {
    let doc = read_text(
        "0 SECTION\n2 ENTITIES\n0 IMAGE\n8 0\n10 1\n20 2\n11 0.5\n21 0\n31 0\n12 0\n22 0.5\n32 0\n13 200\n23 100\n340 E1\n0 ENDSEC\n\
        0 SECTION\n2 OBJECTS\n0 IMAGEDEF\n5 E1\n1 c:\\pics\\a.png\n0 ENDSEC\n0 EOF",
    );
    let EntityKind::Image {
        path,
        position,
        u_vector,
        v_vector,
        size_px,
    } = &doc.models[0].entities[0].kind
    else {
        panic!()
    };
    assert_eq!(path.as_deref(), Some("c:\\pics\\a.png"));
    assert!(pt(*position, 1.0, 2.0, 0.0));
    // Per-pixel vectors are scaled to the full image edge.
    assert!(near(u_vector.x, 100.0) && near(v_vector.y, 50.0));
    assert_eq!(*size_px, Some([200, 100]));
}

#[test]
fn encodings_and_escapes() {
    // AC1018 + ANSI_1254: byte 0xDC is U with diaeresis, 0xDD is dotted-I-less letter.
    let mut bytes = b"0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1018\n9\n$DWGCODEPAGE\n3\nANSI_1254\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nTEXT\n1\nSTR".to_vec();
    bytes.extend_from_slice(&[0xDC, 0xDD]);
    bytes.extend_from_slice(b"\\U+00E7\n0\nENDSEC\n0\nEOF\n");
    let doc = cadkit_dxf::read(&bytes, &ReadOptions::default()).unwrap();
    let EntityKind::Text { value, .. } = &doc.models[0].entities[0].kind else {
        panic!()
    };
    assert_eq!(value, "STR\u{dc}\u{130}\u{e7}");

    // The caller's fallback code page is used when the file names none.
    let mut bytes = b"0\nSECTION\n2\nENTITIES\n0\nTEXT\n1\n".to_vec();
    bytes.push(0xDD);
    bytes.extend_from_slice(b"\n0\nENDSEC\n0\nEOF\n");
    let opts = ReadOptions {
        fallback_codepage: Some("ANSI_1254".into()),
        ..ReadOptions::default()
    };
    let doc = cadkit_dxf::read(&bytes, &opts).unwrap();
    assert!(
        matches!(&doc.models[0].entities[0].kind, EntityKind::Text { value, .. } if value == "\u{130}")
    );
    let doc = cadkit_dxf::read(&bytes, &ReadOptions::default()).unwrap();
    assert!(
        matches!(&doc.models[0].entities[0].kind, EntityKind::Text { value, .. } if value == "\u{dd}")
    );

    // AC1021 is UTF-8.
    let text = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1021\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nTEXT\n1\n\u{c7}ay \u{1F600}\n0\nENDSEC\n0\nEOF\n";
    let doc = cadkit_dxf::read(text.as_bytes(), &ReadOptions::default()).unwrap();
    assert!(
        matches!(&doc.models[0].entities[0].kind, EntityKind::Text { value, .. } if value == "\u{c7}ay \u{1F600}")
    );

    // Warning for an unknown code page.
    let text = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n9\n$DWGCODEPAGE\n3\nNOT_A_PAGE\n0\nENDSEC\n0\nEOF\n";
    let doc = cadkit_dxf::read(text.as_bytes(), &ReadOptions::default()).unwrap();
    assert!(doc.warnings.iter().any(|w| w.code == "dxf.codepage"));
}

fn binary_pairs(wide: bool, pairs: &[(i32, BinVal)]) -> Vec<u8> {
    let mut out = cadkit_dxf::native::BINARY_SENTINEL.to_vec();
    for (code, v) in pairs {
        if wide {
            out.extend_from_slice(&(*code as u16).to_le_bytes());
        } else if *code >= 255 {
            out.push(255);
            out.extend_from_slice(&(*code as u16).to_le_bytes());
        } else {
            out.push(*code as u8);
        }
        match v {
            BinVal::S(s) => {
                out.extend_from_slice(s.as_bytes());
                out.push(0);
            }
            BinVal::F(f) => out.extend_from_slice(&f.to_le_bytes()),
            BinVal::I16(i) => out.extend_from_slice(&i.to_le_bytes()),
            BinVal::I32(i) => out.extend_from_slice(&i.to_le_bytes()),
        }
    }
    out
}

enum BinVal {
    S(&'static str),
    F(f64),
    I16(i16),
    I32(i32),
}

#[test]
fn binary_dxf_narrow_and_wide_codes() {
    use BinVal::*;
    for wide in [false, true] {
        let pairs = [
            (0, S("SECTION")),
            (2, S("HEADER")),
            (9, S("$ACADVER")),
            (1, S(if wide { "AC1015" } else { "AC1009" })),
            (0, S("ENDSEC")),
            (0, S("SECTION")),
            (2, S("ENTITIES")),
            (0, S("LINE")),
            (8, S("L")),
            (10, F(1.5)),
            (20, F(-2.5)),
            (11, F(3.0)),
            (21, F(4.0)),
            (62, I16(3)),
            (90, I32(7)),
            (0, S("ENDSEC")),
            (0, S("EOF")),
        ];
        let bytes = binary_pairs(wide, &pairs);
        assert!(cadkit_dxf::sniff(&bytes));
        let doc = cadkit_dxf::read(&bytes, &ReadOptions::default()).unwrap();
        let e = &doc.models[0].entities[0];
        assert!(
            matches!(e.kind, EntityKind::Line { start, end } if pt(start, 1.5, -2.5, 0.0) && pt(end, 3.0, 4.0, 0.0))
        );
        assert_eq!(e.color, Color::Aci { index: 3 });
        assert_eq!(e.layer.as_deref(), Some("L"));
    }
}

#[test]
fn ascii_flavours() {
    // CRLF line ends, padded codes, a BOM and lowercase section names are all accepted.
    let text = "\u{feff}  0\r\nSECTION\r\n  2\r\nENTITIES\r\n  0\r\nPOINT\r\n 10\r\n1.0\r\n 20\r\n2.0\r\n 30\r\n3.0\r\n  0\r\nENDSEC\r\n  0\r\nEOF\r\n";
    let doc = cadkit_dxf::read(text.as_bytes(), &ReadOptions::default()).unwrap();
    assert!(
        matches!(doc.models[0].entities[0].kind, EntityKind::Point { position } if pt(position, 1.0, 2.0, 3.0))
    );
    // Missing EOF and trailing garbage are tolerated.
    let doc = read_text("0 SECTION\n2 ENTITIES\n0 POINT\n10 1\n20 2\n");
    assert_eq!(doc.models[0].entities.len(), 1);
}
