use super::*;
use crate::model::{Block, Layer, Model};

fn line() -> Entity {
    Entity::new(EntityKind::Line {
        start: Point3::xy(0.0, 0.0),
        end: Point3::xy(10.0, 5.0),
    })
}

fn document(entities: Vec<Entity>) -> Document {
    Document {
        models: vec![Model {
            entities,
            ..Model::default()
        }],
        ..Document::default()
    }
}

fn text(value: String) -> Entity {
    Entity::new(EntityKind::Text {
        position: Point3::default(),
        end_point: None,
        height: 1.0,
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        value,
        style: None,
        halign: HAlign::Left,
        valign: VAlign::Baseline,
        normal: Vec3::Z,
    })
}

fn assert_output_limit(result: crate::Result<String>) {
    assert!(matches!(
        result,
        Err(crate::Error::LimitExceeded {
            limit: crate::LimitKind::Output,
            ..
        })
    ));
}

#[test]
fn exact_utf8_budget_includes_markup_and_hidden_layer_names() {
    let mut entity = text("測定<&>🙂".into());
    entity.layer = Some("隠れた<&>🙂".into());
    let mut doc = document(vec![entity]);
    doc.layers.push(Layer {
        name: "隠れた<&>🙂".into(),
        visible: false,
        ..Layer::default()
    });
    let options = SvgOptions::default();
    for hidden in [false, true] {
        let expected = if hidden {
            render_all_layers(&doc, &options)
        } else {
            render(&doc, &options)
        };
        let exact = options.clone().with_max_output_bytes(expected.len());
        assert_eq!(
            render_bounded(&doc, &exact, hidden).expect("exact budget"),
            expected
        );
        assert_output_limit(render_bounded(
            &doc,
            &exact.with_max_output_bytes(expected.len() - 1),
            hidden,
        ));
    }
}

#[test]
fn a_single_path_is_stopped_before_the_next_entity() {
    let polyline = Entity::new(EntityKind::Polyline {
        vertices: (0..20_000)
            .map(|i| Vertex {
                position: Point3::xy(f64::from(i), f64::from(i % 2)),
                ..Vertex::default()
            })
            .collect(),
        closed: false,
        normal: Vec3::Z,
    });
    let options = SvgOptions::default().with_max_output_bytes(1024);
    assert_output_limit(render_bounded(&document(vec![polyline]), &options, true));
}

#[test]
fn escaping_and_background_do_not_allocate_an_oversized_result() {
    let options = SvgOptions::default().with_max_output_bytes(1024);
    // Input fits but XML escaping expands it beyond the cap within one entity.
    assert_output_limit(render_bounded(
        &document(vec![text("&".repeat(600))]),
        &options,
        true,
    ));
    let options = options.with_background(Some("&".repeat(600)));
    assert_output_limit(render_bounded(&Document::default(), &options, true));
    assert_output_limit(render_bounded(
        &Document::default(),
        &SvgOptions::default().with_max_output_bytes(1),
        true,
    ));
}

#[test]
fn many_layers_and_entities_cannot_accumulate_unbounded_output() {
    let doc = document(
        (0..10_000)
            .map(|i| {
                let mut entity = line();
                entity.layer = Some(format!("layer-{i}-{}", "&".repeat(32)));
                entity
            })
            .collect(),
    );
    assert_output_limit(render_bounded(
        &doc,
        &SvgOptions::default().with_max_output_bytes(16_384),
        true,
    ));
}

#[test]
fn work_exhaustion_is_an_error_instead_of_an_empty_or_partial_success() {
    let doc = document(vec![line(); 100]);
    for work in [0, 10] {
        assert!(matches!(
            render_bounded(&doc, &SvgOptions::default().with_max_work(work), true),
            Err(crate::Error::LimitExceeded {
                limit: crate::LimitKind::Work,
                ..
            })
        ));
    }
}

#[test]
fn block_array_expansion_obeys_output_and_work_limits() {
    let insert = Entity::new(EntityKind::Insert {
        block: "lines".into(),
        position: Point3::default(),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        normal: Vec3::Z,
        columns: 100,
        rows: 100,
        column_spacing: 20.0,
        row_spacing: 20.0,
    });
    let mut doc = document(vec![insert]);
    doc.blocks.push(Block {
        name: "lines".into(),
        entities: vec![line()],
        ..Block::default()
    });
    assert_output_limit(render_bounded(
        &doc,
        &SvgOptions::default().with_max_output_bytes(4096),
        true,
    ));
    assert!(matches!(
        render_bounded(&doc, &SvgOptions::default().with_max_work(10), true),
        Err(crate::Error::LimitExceeded {
            limit: crate::LimitKind::Work,
            ..
        })
    ));
    let small = document(vec![line()]);
    assert_eq!(
        render_bounded(&small, &SvgOptions::default(), true).expect("next document"),
        render_all_layers(&small, &SvgOptions::default())
    );
}

#[test]
fn hatch_definitions_are_part_of_the_exact_output_budget() {
    let hatch = Entity::new(EntityKind::Hatch {
        loops: vec![crate::model::HatchLoop {
            edges: vec![HatchEdge::Polyline {
                vertices: vec![
                    Vertex {
                        position: Point3::xy(0.0, 0.0),
                        ..Vertex::default()
                    },
                    Vertex {
                        position: Point3::xy(5.0, 5.0),
                        ..Vertex::default()
                    },
                ],
                closed: true,
            }],
            external: true,
        }],
        solid: false,
        pattern: Some("ANSI37".into()),
        pattern_scale: 1.0,
        pattern_angle: 0.0,
        normal: Vec3::Z,
    });
    let doc = document(vec![hatch]);
    let expected = render_all_layers(&doc, &SvgOptions::default());
    assert!(expected.contains("<defs>"));
    let options = SvgOptions::default().with_max_output_bytes(expected.len());
    assert_eq!(
        render_bounded(&doc, &options, true).expect("exact hatch budget"),
        expected
    );
    assert_output_limit(render_bounded(
        &doc,
        &options.with_max_output_bytes(expected.len() - 1),
        true,
    ));
}
