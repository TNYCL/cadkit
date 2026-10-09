//! Writer behaviour on a synthetic document that uses every entity kind.
#![allow(clippy::field_reassign_with_default)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::print_stdout
)]
mod common;

use cadkit_core::geom_ops::arbitrary_axes;
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

fn on_layer(mut e: Entity, layer: &str) -> Entity {
    e.layer = Some(layer.to_owned());
    e
}

fn vtx(x: f64, y: f64, z: f64, bulge: f64, w0: f64, w1: f64) -> Vertex {
    Vertex {
        position: p(x, y, z),
        bulge,
        start_width: w0,
        end_width: w1,
    }
}

/// A tilted extrusion direction and the OCS -> WCS mapping of the arbitrary axis algorithm.
fn tilted() -> (Vec3, impl Fn(Point3) -> Point3) {
    let n = Vec3::new(1.0, 1.0, 1.0);
    let (ax, ay, az) = arbitrary_axes(n);
    (n, move |q: Point3| {
        Point3::new(
            ax.x * q.x + ay.x * q.y + az.x * q.z,
            ax.y * q.x + ay.y * q.y + az.y * q.z,
            ax.z * q.x + ay.z * q.y + az.z * q.z,
        )
    })
}

/// A document with every kind of entity; `sample_models()` lists the entities that must
/// survive a round trip unchanged.
fn sample_doc() -> Document {
    let mut doc = Document {
        units: Units {
            unit: LengthUnit::Millimeter,
            meters_per_unit: Some(0.001),
        },
        ..Document::default()
    };
    doc.layers = vec![
        Layer {
            name: "0".into(),
            color: Color::Aci { index: 7 },
            visible: true,
            plottable: true,
            lineweight: Lineweight::Default,
            ..Layer::default()
        },
        Layer {
            name: "Walls".into(),
            color: Color::Aci { index: 1 },
            linetype: Some("DASHED".into()),
            lineweight: Lineweight::Millimeters(0.5),
            visible: true,
            plottable: true,
            ..Layer::default()
        },
        Layer {
            name: "Frozen".into(),
            color: Color::Rgb {
                r: 10,
                g: 200,
                b: 30,
            },
            visible: false,
            frozen: true,
            locked: true,
            plottable: false,
            lineweight: Lineweight::Default,
            ..Layer::default()
        },
        Layer {
            name: "\u{c7}al\u{131}\u{15f}ma".into(),
            color: Color::Aci { index: 3 },
            visible: true,
            plottable: true,
            lineweight: Lineweight::Default,
            ..Layer::default()
        },
    ];
    doc.linetypes = vec![Linetype {
        name: "DASHED".into(),
        description: Some("Dashed __ __".into()),
        pattern: vec![12.5, -6.25],
        props: Props::new(),
    }];
    doc.text_styles = vec![
        TextStyle {
            name: "Standard".into(),
            font: Some("arial.ttf".into()),
            height: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            props: Props::new(),
        },
        TextStyle {
            name: "Narrow".into(),
            font: Some("romans.shx".into()),
            height: 2.5,
            width_factor: 0.8,
            oblique: 0.2,
            props: Props::new(),
        },
    ];

    let mut door = Block {
        name: "Door".into(),
        base_point: p(1.0, 2.0, 0.0),
        ..Block::default()
    };
    door.entities.push(on_layer(
        Entity::new(EntityKind::Line {
            start: p(0.0, 0.0, 0.0),
            end: p(10.0, 0.0, 0.0),
        }),
        "Walls",
    ));
    door.entities.push(Entity::new(EntityKind::Circle {
        center: p(5.0, 0.0, 0.0),
        radius: 2.0,
        normal: Vec3::Z,
    }));
    let mut attdef = Entity::new(EntityKind::Unknown {
        type_name: "dxf.ATTDEF".into(),
    });
    for (k, v) in [
        ("dxf.attdef.tag", Value::Text("TAG".into())),
        ("dxf.attdef.prompt", Value::Text("Enter".into())),
        ("dxf.attdef.default", Value::Text("def".into())),
        (
            "dxf.attdef.position",
            Value::List(vec![
                Value::Float(1.0),
                Value::Float(2.0),
                Value::Float(0.0),
            ]),
        ),
        ("dxf.attdef.height", Value::Float(2.0)),
        ("dxf.attdef.flags", Value::Int(0)),
    ] {
        attdef.props.insert(k.into(), v);
    }
    door.entities.push(attdef);
    doc.blocks.push(door);
    doc.blocks.push(Block {
        name: "*U1".into(),
        entities: vec![Entity::new(EntityKind::Point {
            position: p(1.0, 1.0, 0.0),
        })],
        ..Block::default()
    });

    let model = Model {
        name: "Model".into(),
        kind: ModelKind::Model,
        entities: model_entities(),
        ..Model::default()
    };
    let mut sheet_a = Model {
        name: "Sheet A".into(),
        kind: ModelKind::Layout,
        ..Model::default()
    };
    sheet_a.entities.push(Entity::new(EntityKind::Viewport {
        center: p(148.0, 105.0, 0.0),
        width: 200.0,
        height: 150.0,
        view_center: p(50.0, 40.0, 0.0),
        view_height: 120.0,
    }));
    sheet_a.entities.push(on_layer(
        Entity::new(EntityKind::Text {
            position: p(10.0, 10.0, 0.0),
            end_point: None,
            height: 5.0,
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            value: "Sheet title".into(),
            style: None,
            halign: HAlign::Left,
            valign: VAlign::Baseline,
            normal: Vec3::Z,
        }),
        "Walls",
    ));
    let sheet_b = Model {
        name: "Sheet B".into(),
        kind: ModelKind::Layout,
        entities: vec![Entity::new(EntityKind::Line {
            start: p(0.0, 0.0, 0.0),
            end: p(1.0, 1.0, 0.0),
        })],
        ..Model::default()
    };
    doc.models = vec![model, sheet_a, sheet_b];
    doc
}

fn model_entities() -> Vec<Entity> {
    let (n, tf) = tilted();
    let world = |x: f64, y: f64, z: f64| tf(p(x, y, z));
    let mut v: Vec<Entity> = Vec::new();
    v.push(Entity::new(EntityKind::Point {
        position: p(1.0, 2.0, 3.0),
    }));
    let mut line = on_layer(
        Entity::new(EntityKind::Line {
            start: p(0.0, 0.0, 0.0),
            end: p(100.5, 25.25, -3.0),
        }),
        "Walls",
    );
    line.color = Color::Rgb {
        r: 255,
        g: 128,
        b: 0,
    };
    line.linetype = Some("DASHED".into());
    line.lineweight = Lineweight::Millimeters(0.35);
    v.push(line);
    v.push(Entity::new(EntityKind::Circle {
        center: world(1.0, 2.0, 3.0),
        radius: 4.5,
        normal: Vec3::new(n.x / 3f64.sqrt(), n.y / 3f64.sqrt(), n.z / 3f64.sqrt()),
    }));
    v.push(Entity::new(EntityKind::Arc {
        center: p(-1.0, 2.0, -3.0),
        radius: 2.0,
        start_angle: 0.5,
        end_angle: 2.5,
        normal: Vec3::new(0.0, 0.0, -1.0),
    }));
    v.push(Entity::new(EntityKind::Ellipse {
        center: p(5.0, 5.0, 1.0),
        major_axis: Vec3::new(4.0, 3.0, 0.0),
        ratio: 0.4,
        start_param: 0.3,
        end_param: 4.0,
        normal: Vec3::Z,
    }));
    v.push(Entity::new(EntityKind::Polyline {
        vertices: vec![
            vtx(0.0, 0.0, 5.0, 0.5, 0.0, 0.2),
            vtx(10.0, 0.0, 5.0, 0.0, 0.2, 0.2),
            vtx(10.0, 10.0, 5.0, -0.25, 0.0, 0.0),
        ],
        closed: true,
        normal: Vec3::Z,
    }));
    let tilted_pl = Entity::new(EntityKind::Polyline {
        vertices: vec![
            vtx(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            vtx(0.0, 0.0, 0.0, 0.3, 0.0, 0.0),
            vtx(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
        ]
        .into_iter()
        .zip([(1.0, 1.0, 2.0), (4.0, 1.0, 2.0), (4.0, 3.0, 2.0)])
        .map(|(mut vv, (x, y, z))| {
            vv.position = world(x, y, z);
            vv
        })
        .collect(),
        closed: false,
        normal: Vec3::new(n.x / 3f64.sqrt(), n.y / 3f64.sqrt(), n.z / 3f64.sqrt()),
    });
    v.push(tilted_pl);
    let mut pl3 = Entity::new(EntityKind::Polyline {
        vertices: vec![
            vtx(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            vtx(1.0, 1.0, 1.0, 0.0, 0.0, 0.0),
            vtx(2.0, 0.0, 5.0, 0.0, 0.0, 0.0),
        ],
        closed: false,
        normal: Vec3::Z,
    });
    pl3.props
        .insert("dxf.polyline_3d".into(), Value::Bool(true));
    v.push(pl3);
    v.push(Entity::new(EntityKind::Spline {
        degree: 3,
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0, 2.0],
        control_points: vec![
            p(0.0, 0.0, 0.0),
            p(1.0, 2.0, 0.0),
            p(2.0, 3.0, 1.0),
            p(3.0, 2.0, 0.0),
            p(4.0, 0.0, 0.0),
        ],
        weights: vec![1.0, 0.5, 2.0, 1.0, 1.0],
        fit_points: vec![],
        closed: false,
    }));
    v.push(Entity::new(EntityKind::Spline {
        degree: 3,
        knots: vec![],
        control_points: vec![],
        weights: vec![],
        fit_points: vec![
            p(0.0, 0.0, 0.0),
            p(1.0, 1.0, 0.0),
            p(2.0, 0.0, 0.0),
            p(3.0, 1.0, 0.0),
        ],
        closed: false,
    }));
    let text = |pos: Point3, halign, valign, value: &str, style: Option<&str>| {
        Entity::new(EntityKind::Text {
            position: pos,
            end_point: None,
            height: 2.5,
            rotation: 0.7,
            width_factor: 0.9,
            oblique: 0.1,
            value: value.into(),
            style: style.map(str::to_owned),
            halign,
            valign,
            normal: Vec3::Z,
        })
    };
    v.push(text(
        p(1.0, 1.0, 0.0),
        HAlign::Left,
        VAlign::Baseline,
        "Plain",
        None,
    ));
    v.push(text(
        p(2.0, 2.0, 0.0),
        HAlign::Center,
        VAlign::Middle,
        "\u{c7}al\u{131}\u{15f}ma \u{1F600} ^caret",
        Some("Narrow"),
    ));
    v.push(text(
        p(3.0, 3.0, 0.0),
        HAlign::Right,
        VAlign::Top,
        "Right/Top",
        Some("Standard"),
    ));
    let mut aligned = text(
        p(4.0, 4.0, 0.0),
        HAlign::Aligned,
        VAlign::Baseline,
        "Aligned",
        None,
    );
    if let EntityKind::Text { end_point, .. } = &mut aligned.kind {
        *end_point = Some(p(9.0, 4.0, 0.0));
    }
    v.push(aligned);
    let long = "x".repeat(300) + "\\P\u{15f}" + &"y".repeat(300);
    v.push(Entity::new(EntityKind::MText {
        position: p(0.0, 0.0, 0.0),
        height: 3.0,
        width: 40.0,
        rotation: 0.4,
        value: long.clone(),
        plain: long.replace("\\P", "\n"),
        style: Some("Narrow".into()),
        attachment: Attachment::BottomRight,
        line_spacing: 1.5,
        normal: Vec3::Z,
    }));
    let mut insert = Entity::new(EntityKind::Insert {
        block: "Door".into(),
        position: p(10.0, 20.0, 0.0),
        scale: Vec3::new(2.0, 3.0, 1.0),
        rotation: 0.5,
        normal: Vec3::Z,
        columns: 3,
        rows: 2,
        column_spacing: 15.0,
        row_spacing: 25.0,
    });
    insert.attributes = vec![
        Attribute {
            tag: "TAG".into(),
            value: Value::Text("v\u{e9}".into()),
            set: None,
            position: Some(p(10.0, 20.0, 0.0)),
            invisible: false,
            ..Default::default()
        },
        Attribute {
            tag: "HIDDEN".into(),
            value: Value::Text("2".into()),
            set: None,
            position: Some(p(11.0, 21.0, 0.0)),
            invisible: true,
            ..Default::default()
        },
    ];
    v.push(insert);
    v.push(Entity::new(EntityKind::Hatch {
        loops: vec![
            HatchLoop {
                edges: vec![HatchEdge::Polyline {
                    vertices: vec![
                        vtx(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
                        vtx(10.0, 0.0, 0.0, 0.5, 0.0, 0.0),
                        vtx(10.0, 10.0, 0.0, 0.0, 0.0, 0.0),
                        vtx(0.0, 10.0, 0.0, 0.0, 0.0, 0.0),
                    ],
                    closed: true,
                }],
                external: true,
            },
            HatchLoop {
                edges: vec![HatchEdge::Polyline {
                    vertices: vec![
                        vtx(2.0, 2.0, 0.0, 0.0, 0.0, 0.0),
                        vtx(4.0, 2.0, 0.0, 0.0, 0.0, 0.0),
                        vtx(4.0, 4.0, 0.0, 0.0, 0.0, 0.0),
                    ],
                    closed: true,
                }],
                external: false,
            },
        ],
        solid: true,
        pattern: Some("SOLID".into()),
        pattern_scale: 1.0,
        pattern_angle: 0.0,
        normal: Vec3::Z,
    }));
    v.push(Entity::new(EntityKind::Hatch {
        loops: vec![HatchLoop {
            edges: vec![
                HatchEdge::Line {
                    start: p(0.0, 0.0, 0.0),
                    end: p(10.0, 0.0, 0.0),
                },
                HatchEdge::Arc {
                    center: p(10.0, 5.0, 0.0),
                    radius: 5.0,
                    start_angle: -std::f64::consts::FRAC_PI_2,
                    end_angle: std::f64::consts::FRAC_PI_2,
                    ccw: true,
                },
                HatchEdge::Ellipse {
                    center: p(5.0, 10.0, 0.0),
                    major_axis: Vec3::new(5.0, 0.0, 0.0),
                    ratio: 0.5,
                    start_param: 0.0,
                    end_param: std::f64::consts::PI,
                    ccw: true,
                },
                HatchEdge::Spline {
                    degree: 2,
                    knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    control_points: vec![p(0.0, 10.0, 0.0), p(-2.0, 5.0, 0.0), p(0.0, 0.0, 0.0)],
                    weights: vec![],
                },
            ],
            external: true,
        }],
        solid: false,
        pattern: Some("ANSI31".into()),
        pattern_scale: 1.5,
        pattern_angle: 0.25,
        normal: Vec3::Z,
    }));
    let dim = |ty, pts: Vec<Point3>, block: Option<&str>| {
        Entity::new(EntityKind::Dimension {
            dimension_type: ty,
            points: pts,
            text_position: Some(p(5.0, 5.0, 0.0)),
            measurement: Some(12.5),
            text: Some("<>".into()),
            block: block.map(str::to_owned),
            style: Some("ISO".into()),
        })
    };
    v.push(dim(
        DimensionType::Linear,
        vec![p(5.0, 6.0, 0.0), p(0.0, 0.0, 0.0), p(10.0, 0.0, 0.0)],
        Some("Door"),
    ));
    v.push(dim(
        DimensionType::Aligned,
        vec![p(5.0, 6.0, 0.0), p(0.0, 0.0, 0.0), p(10.0, 3.0, 0.0)],
        None,
    ));
    v.push(dim(
        DimensionType::Radius,
        vec![p(0.0, 0.0, 0.0), p(5.0, 0.0, 0.0)],
        None,
    ));
    v.push(dim(
        DimensionType::Diameter,
        vec![p(0.0, 0.0, 0.0), p(5.0, 0.0, 0.0)],
        None,
    ));
    v.push(dim(
        DimensionType::Angular,
        vec![
            p(0.0, 0.0, 0.0),
            p(1.0, 0.0, 0.0),
            p(2.0, 0.0, 0.0),
            p(0.0, 1.0, 0.0),
            p(0.0, 2.0, 0.0),
        ],
        None,
    ));
    v.push(dim(
        DimensionType::Angular3Point,
        vec![
            p(0.0, 0.0, 0.0),
            p(1.0, 0.0, 0.0),
            p(2.0, 0.0, 0.0),
            p(0.0, 1.0, 0.0),
        ],
        None,
    ));
    v.push(dim(
        DimensionType::Ordinate,
        vec![p(0.0, 0.0, 0.0), p(1.0, 2.0, 0.0), p(3.0, 4.0, 0.0)],
        None,
    ));
    v.push(Entity::new(EntityKind::Face {
        points: vec![
            p(0.0, 0.0, 1.0),
            p(1.0, 0.0, 1.0),
            p(1.0, 1.0, 1.0),
            p(0.0, 1.0, 1.0),
        ],
        filled: true,
    }));
    v.push(Entity::new(EntityKind::Face {
        points: vec![p(0.0, 0.0, 0.0), p(1.0, 0.0, 0.0), p(1.0, 1.0, 2.0)],
        filled: false,
    }));
    v.push(Entity::new(EntityKind::Face {
        points: vec![p(0.0, 0.0, 0.0), p(1.0, 0.0, 0.0), p(1.0, 1.0, 0.0)],
        filled: true,
    }));
    v.push(Entity::new(EntityKind::Leader {
        vertices: vec![p(0.0, 0.0, 0.0), p(2.0, 2.0, 0.0), p(5.0, 2.0, 0.0)],
        arrowhead: true,
    }));
    v.push(Entity::new(EntityKind::Image {
        path: Some("C:\\pics\\plan.png".into()),
        position: p(1.0, 2.0, 0.0),
        u_vector: Vec3::new(100.0, 0.0, 0.0),
        v_vector: Vec3::new(0.0, 50.0, 0.0),
        size_px: Some([200, 100]),
    }));
    v.push(Entity::new(EntityKind::Mesh {
        vertices: vec![
            p(0.0, 0.0, 0.0),
            p(1.0, 0.0, 0.0),
            p(1.0, 1.0, 0.0),
            p(0.0, 1.0, 0.5),
            p(2.0, 0.5, 1.0),
        ],
        faces: vec![vec![0, 1, 2, 3], vec![1, 4, 2]],
    }));
    let mut hidden = on_layer(
        Entity::new(EntityKind::Line {
            start: p(0.0, 0.0, 0.0),
            end: p(1.0, 0.0, 0.0),
        }),
        "Auto-created",
    );
    hidden.visible = false;
    hidden.linetype = Some("FANCY".into());
    v.push(hidden);
    v
}

/// What the writer must drop: unknown records, dangling INSERTs, VIEWPORTs in model space.
fn extras() -> Vec<Entity> {
    vec![
        Entity::new(EntityKind::Unknown {
            type_name: "dxf.WIPEOUT".into(),
        }),
        Entity::new(EntityKind::Unknown {
            type_name: "dgn.type_99".into(),
        }),
        Entity::new(EntityKind::Insert {
            block: "NoSuchBlock".into(),
            position: p(0.0, 0.0, 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            normal: Vec3::Z,
            columns: 1,
            rows: 1,
            column_spacing: 0.0,
            row_spacing: 0.0,
        }),
        Entity::new(EntityKind::Viewport {
            center: p(0.0, 0.0, 0.0),
            width: 1.0,
            height: 1.0,
            view_center: p(0.0, 0.0, 0.0),
            view_height: 1.0,
        }),
        Entity::new(EntityKind::Polyline {
            vertices: vec![],
            closed: false,
            normal: Vec3::Z,
        }),
    ]
}

#[test]
fn every_kind_round_trips_in_every_version() {
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("dxf-out");
    std::fs::create_dir_all(&out).unwrap();
    for v in ALL {
        let doc = sample_doc();
        let mut with_extras = doc.clone();
        with_extras.models[0].entities.extend(extras());
        let (text, warnings) = cadkit_dxf::write_with_report(&with_extras, v).unwrap();
        std::fs::write(out.join(format!("synthetic_{}.dxf", v.acadver())), &text).unwrap();
        let ctx = v.acadver();
        assert!(
            warnings.iter().any(|w| w.message.contains("WIPEOUT")),
            "{ctx}: skipped unknown not reported"
        );
        assert!(
            warnings.iter().any(|w| w.message.contains("NoSuchBlock")),
            "{ctx}: dangling insert not reported"
        );
        assert!(text.contains(&format!("\r\n{}\r\n", v.acadver())));
        assert!(text.ends_with("  0\r\nEOF\r\n"));
        if v == DxfVersion::R12 {
            assert!(text.is_ascii(), "R12 output must be pure ASCII");
        }
        if matches!(v, DxfVersion::R12 | DxfVersion::R2000 | DxfVersion::R2004) {
            assert!(
                text.is_ascii(),
                "{ctx}: pre-2007 output must be pure ASCII (\\U+ escapes)"
            );
        }
        let back = read(text.as_bytes());
        assert_eq!(back.source.version, v.acadver());

        if v == DxfVersion::R12 {
            // Only the entities R12 has natively are compared one by one.
            let kinds = histogram(&back.models[0].entities);
            for (k, n) in [
                ("Point", 1),
                ("Insert", 1),
                ("Dimension", 7),
                ("Face", 3),
                ("Circle", 1),
                ("Arc", 1),
            ] {
                assert!(
                    kinds.get(k).copied().unwrap_or(0) >= n,
                    "{ctx}: {k} missing: {kinds:?}"
                );
            }
            continue;
        }

        assert_eq!(back.units, doc.units, "{ctx}");
        let names: Vec<&str> = back.layers.iter().map(|l| l.name.as_str()).collect();
        for want in [
            "0",
            "Walls",
            "Frozen",
            "\u{c7}al\u{131}\u{15f}ma",
            "Auto-created",
        ] {
            assert!(
                names.contains(&want),
                "{ctx}: layer {want} missing in {names:?}"
            );
        }
        let frozen = back.layers.iter().find(|l| l.name == "Frozen").unwrap();
        assert!(
            frozen.frozen && frozen.locked && !frozen.visible && !frozen.plottable,
            "{ctx}"
        );
        let walls = back.layers.iter().find(|l| l.name == "Walls").unwrap();
        assert_eq!(
            (walls.lineweight, walls.linetype.as_deref()),
            (Lineweight::Millimeters(0.5), Some("DASHED")),
            "{ctx}"
        );
        if v != DxfVersion::R2000 {
            assert_eq!(
                frozen.color,
                Color::Rgb {
                    r: 10,
                    g: 200,
                    b: 30
                },
                "{ctx}"
            );
        }
        assert!(
            back.linetypes
                .iter()
                .any(|l| l.name == "DASHED" && l.pattern == vec![12.5, -6.25]),
            "{ctx}"
        );
        assert!(
            back.linetypes.iter().any(|l| l.name == "FANCY"),
            "{ctx}: placeholder linetype"
        );
        let narrow = back
            .text_styles
            .iter()
            .find(|s| s.name == "Narrow")
            .unwrap();
        assert!(
            near(narrow.width_factor, 0.8)
                && near(narrow.oblique, 0.2)
                && narrow.font.as_deref() == Some("romans.shx"),
            "{ctx}"
        );

        assert_eq!(back.models.len(), 3, "{ctx}");
        let names: Vec<&str> = back.models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Model", "Sheet A", "Sheet B"], "{ctx}");
        for i in 0..3 {
            compare_lists(
                &format!("{ctx} model {i}"),
                &doc.models[i].entities,
                &back.models[i].entities,
                v,
            );
        }
        // Blocks: Door (with its ATTDEF) and the anonymous block.
        assert_eq!(back.blocks.len(), 2, "{ctx}");
        compare_lists(
            &format!("{ctx} block Door"),
            &doc.blocks[0].entities,
            &back.blocks[0].entities,
            v,
        );
        let attdef = back.blocks[0].entities.iter().find(
            |e| matches!(&e.kind, EntityKind::Unknown { type_name } if type_name == "dxf.ATTDEF"),
        );
        assert!(
            attdef
                .is_some_and(|e| e.props.get("dxf.attdef.tag") == Some(&Value::Text("TAG".into()))),
            "{ctx}: ATTDEF"
        );
        assert_eq!(back.blocks[1].name, "*U1");
        assert!(near(back.blocks[0].base_point.x, 1.0) && near(back.blocks[0].base_point.y, 2.0));
        // Image path survives through IMAGEDEF.
        assert!(
            back.models[0].entities.iter().any(|e| matches!(&e.kind, EntityKind::Image { path: Some(p), .. } if p == "C:\\pics\\plan.png")),
            "{ctx}: image path"
        );
        println!(
            "{ctx}: ok, {} bytes, {} writer warnings",
            text.len(),
            warnings.len()
        );
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn groups_are_flattened_with_their_name_in_xdata() {
    let mut doc = Document::default();
    let child1 = Entity::new(EntityKind::Line {
        start: p(0.0, 0.0, 0.0),
        end: p(1.0, 0.0, 0.0),
    });
    let child2 = Entity::new(EntityKind::Circle {
        center: p(0.0, 0.0, 0.0),
        radius: 1.0,
        normal: Vec3::Z,
    });
    let nested = Entity::new(EntityKind::Group {
        group_kind: GroupKind::Cell,
        name: Some("Inner".into()),
        origin: None,
        children: vec![child2.clone()],
    });
    let group = Entity::new(EntityKind::Group {
        group_kind: GroupKind::ComplexChain,
        name: Some("Outer".into()),
        origin: Some(p(0.0, 0.0, 0.0)),
        children: vec![child1.clone(), nested],
    });
    doc.models.push(Model {
        name: "Model".into(),
        kind: ModelKind::Model,
        entities: vec![group],
        ..Model::default()
    });
    for v in ALL {
        let text = cadkit_dxf::write(&doc, v).unwrap();
        let back = read(text.as_bytes());
        let list = &back.models[0].entities;
        assert_eq!(list.len(), 2, "{v:?}");
        assert!(
            matches!(list[0].kind, EntityKind::Line { .. })
                && matches!(list[1].kind, EntityKind::Circle { .. })
        );
        let name = |e: &Entity| match e.props.get("dxf.xdata.CADKIT_GROUP") {
            Some(Value::List(items)) => match items.first() {
                Some(Value::List(pair)) => pair.get(1).cloned(),
                _ => None,
            },
            _ => None,
        };
        assert_eq!(name(&list[0]), Some(Value::Text("Outer".into())), "{v:?}");
        assert_eq!(name(&list[1]), Some(Value::Text("Inner".into())), "{v:?}");
    }
}

#[test]
fn ellipse_with_ratio_above_one_is_normalised() {
    let mut doc = Document::default();
    doc.models.push(Model {
        name: "Model".into(),
        kind: ModelKind::Model,
        entities: vec![Entity::new(EntityKind::Ellipse {
            center: p(1.0, 2.0, 0.0),
            major_axis: Vec3::new(2.0, 0.0, 0.0),
            ratio: 2.0,
            start_param: 0.5,
            end_param: 2.0,
            normal: Vec3::Z,
        })],
        ..Model::default()
    });
    let text = cadkit_dxf::write(&doc, DxfVersion::R2013).unwrap();
    let back = read(text.as_bytes());
    let EntityKind::Ellipse {
        center,
        major_axis,
        ratio,
        start_param,
        end_param,
        ..
    } = back.models[0].entities[0].kind
    else {
        panic!()
    };
    assert!(ratio <= 1.0);
    // Same curve: compare the point at the original parameter 1.0.
    let orig = |t: f64| (1.0 + 2.0 * t.cos(), 2.0 + 4.0 * t.sin());
    let minor = (-major_axis.y * ratio, major_axis.x * ratio);
    let t_new = 1.0 - std::f64::consts::FRAC_PI_2;
    let got = (
        center.x + major_axis.x * t_new.cos() + minor.0 * t_new.sin(),
        center.y + major_axis.y * t_new.cos() + minor.1 * t_new.sin(),
    );
    let want = orig(1.0);
    assert!(
        (got.0 - want.0).abs() < 1e-9 && (got.1 - want.1).abs() < 1e-9,
        "{got:?} vs {want:?}"
    );
    assert!(end_param > start_param);
}

#[test]
fn names_are_cleaned_and_duplicates_dropped() {
    let mut doc = Document::default();
    doc.layers = vec![
        Layer {
            name: "Bad:Name*?".into(),
            color: Color::Aci { index: 2 },
            visible: true,
            plottable: true,
            ..Layer::default()
        },
        Layer {
            name: "bad:name*?".into(),
            color: Color::Aci { index: 5 },
            visible: true,
            plottable: true,
            ..Layer::default()
        },
    ];
    doc.models.push(Model {
        name: "Model".into(),
        kind: ModelKind::Model,
        entities: vec![on_layer(
            Entity::new(EntityKind::Point {
                position: p(0.0, 0.0, 0.0),
            }),
            "Bad:Name*?",
        )],
        ..Model::default()
    });
    doc.models.push(Model {
        name: "Model".into(),
        kind: ModelKind::Layout,
        ..Model::default()
    });
    doc.models.push(Model {
        name: "Model".into(),
        kind: ModelKind::Layout,
        ..Model::default()
    });
    let back = read(
        cadkit_dxf::write(&doc, DxfVersion::R2018)
            .unwrap()
            .as_bytes(),
    );
    let layer_names: Vec<&str> = back.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        layer_names
            .iter()
            .filter(|n| n.starts_with("Bad_Name"))
            .count(),
        1,
        "{layer_names:?}"
    );
    assert_eq!(
        back.models[0].entities[0].layer.as_deref(),
        Some("Bad_Name__")
    );
    let names: Vec<&str> = back.models.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Model", "Model_2", "Model_3"]);
}

#[test]
fn empty_documents_and_all_unknown_do_not_panic() {
    for v in ALL {
        let text = cadkit_dxf::write(&Document::default(), v).unwrap();
        let back = read(text.as_bytes());
        assert!(back.models[0].entities.is_empty());
        let mut doc = Document::default();
        doc.models.push(Model {
            entities: extras(),
            ..Model::default()
        });
        assert!(cadkit_dxf::write(&doc, v).is_ok());
    }
}

#[test]
fn numbers_keep_full_precision() {
    let mut doc = Document::default();
    let x = 123_456.789_012_345_67_f64;
    let tiny = 1.234_567_890_123_456e-9_f64;
    doc.models.push(Model {
        name: "Model".into(),
        kind: ModelKind::Model,
        entities: vec![Entity::new(EntityKind::Line {
            start: p(x, tiny, -0.1),
            end: p(1e300, -1e-300, std::f64::consts::PI),
        })],
        ..Model::default()
    });
    for v in ALL {
        let back = read(cadkit_dxf::write(&doc, v).unwrap().as_bytes());
        let EntityKind::Line { start, end } = back.models[0].entities[0].kind else {
            panic!()
        };
        assert_eq!((start.x, start.y, start.z), (x, tiny, -0.1), "{v:?}");
        assert_eq!(
            (end.x, end.y, end.z),
            (1e300, -1e-300, std::f64::consts::PI),
            "{v:?}"
        );
    }
}

#[test]
fn polygon_interiors_are_hatch_boundaries_or_r12_outlines() {
    let ring = |a, b| {
        vec![
            p(a, a, 3.),
            p(b, a, 3.),
            p(b, b, 3.),
            p(a, b, 3.),
            p(a, a, 3.),
        ]
    };
    let mut doc = Document::default();
    doc.models.push(Model {
        is_3d: true,
        entities: vec![Entity::new(EntityKind::Polygon {
            exterior: ring(0., 10.),
            interiors: vec![ring(2., 3.)],
        })],
        ..Default::default()
    });
    for version in [DxfVersion::R12, DxfVersion::R2018] {
        let output = cadkit_dxf::write(&doc, version).unwrap();
        let read = cadkit_dxf::read(output.as_bytes(), &ReadOptions::default()).unwrap();
        if version == DxfVersion::R12 {
            assert_eq!(read.models[0].entities.len(), 2);
        } else if let EntityKind::Hatch { loops, .. } = &read.models[0].entities[0].kind {
            assert_eq!(loops.len(), 2);
            assert!(loops[0].external);
            assert!(!loops[1].external);
        } else {
            panic!("expected HATCH");
        }
    }
}
