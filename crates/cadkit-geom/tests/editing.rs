//! Synthetic regression tests for editing operations.
#![cfg(test)]

use cadkit_geom::*;
fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}
fn t() -> GeometryTolerance {
    GeometryTolerance::default()
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}
fn path() -> Polyline {
    Polyline {
        points: vec![p(0., 0.), p(10., 0.), p(10., 10.)],
        closed: false,
    }
}
#[test]
fn line_trim_extend_offset_match_analytic_coordinates() {
    let line = LineSegment {
        start: p(1., 2.),
        end: p(4., 6.),
    };
    let trim = line.trim(0.2, 0.8, &t()).unwrap();
    assert_eq!(trim.start, p(1.6, 2.8));
    assert_eq!(trim.end, p(3.4000000000000004, 5.2));
    close(trim.length(), 3.);
    let extended = line.extend(5., 10., &t()).unwrap();
    assert!(extended.start.distance(p(-2., -2.)) < 1e-12);
    assert!(extended.end.distance(p(10., 14.)) < 1e-12);
    close(extended.length(), 20.);
    let offset = line.offset(5., &t()).unwrap();
    assert!(offset.start.distance(p(-3., 5.)) < 1e-12);
    assert!(offset.end.distance(p(0., 9.)) < 1e-12);
    close(offset.length(), 5.);
    assert!(line.trim(0.8, 0.2, &t()).is_err());
    assert!(line.extend(-1., 0., &t()).is_err());
}
#[test]
fn polyline_trim_uses_path_length_and_keeps_interior_vertices() {
    let result = path().trim(5., 15., &t()).unwrap();
    assert_eq!(result.points, vec![p(5., 0.), p(10., 0.), p(10., 5.)]);
    assert!(!result.closed);
    let on_vertex = path().trim(10., 20., &t()).unwrap();
    assert_eq!(on_vertex.points, vec![p(10., 0.), p(10., 10.)]);
    let extended = path().extend(2., 3., &t()).unwrap();
    assert_eq!(extended.points, vec![p(-2., 0.), p(10., 0.), p(10., 13.)]);
}
#[test]
fn polyline_offsets_distinguish_inner_miter_and_outer_bevel() {
    let inner = path()
        .offset(1., OffsetJoin::Miter { limit: 4. }, &t())
        .unwrap();
    assert_eq!(inner.points, vec![p(0., 1.), p(9., 1.), p(9., 10.)]);
    let outer = path().offset(-1., OffsetJoin::Bevel, &t()).unwrap();
    assert_eq!(
        outer.points,
        vec![p(0., -1.), p(10., -1.), p(11., 0.), p(11., 10.)]
    );
    let inner_bevel = path().offset(1., OffsetJoin::Bevel, &t()).unwrap();
    assert_eq!(inner_bevel, inner);
    let error = path()
        .offset(1., OffsetJoin::Miter { limit: 1.1 }, &t())
        .unwrap_err();
    assert_eq!(
        error.diagnostic().reason,
        GeometryReason::MiterLimitExceeded
    );
}
#[test]
fn closed_offset_collapse_is_rejected_before_false_regrowth() {
    let square = Polyline {
        points: vec![p(0., 0.), p(10., 0.), p(10., 10.), p(0., 10.)],
        closed: true,
    };
    let inner = square
        .offset(1., OffsetJoin::Miter { limit: 4. }, &t())
        .unwrap();
    assert_eq!(
        inner.points,
        vec![p(1., 1.), p(9., 1.), p(9., 9.), p(1., 9.)]
    );
    close(Curve::Polyline(inner).enclosed_area(&t()).unwrap(), 64.);
    let outer = square
        .offset(-1., OffsetJoin::Miter { limit: 4. }, &t())
        .unwrap();
    close(Curve::Polyline(outer).enclosed_area(&t()).unwrap(), 144.);
    for distance in [5., 6., 10.] {
        assert_eq!(
            square
                .offset(distance, OffsetJoin::Miter { limit: 4. }, &t())
                .unwrap_err()
                .diagnostic()
                .reason,
            GeometryReason::OffsetCollapsed
        );
    }
    assert_eq!(
        square.trim(1., 2., &t()).unwrap_err().diagnostic().reason,
        GeometryReason::EditingNotSupported
    );
}
#[test]
fn offset_preserves_rotation_and_rejects_crossing_input() {
    let original = path();
    let transform = Transform2::rotation(0.31).then(Transform2::translation(20., 30.));
    let Curve::Polyline(transformed) = Curve::Polyline(original.clone())
        .transformed(transform, &t())
        .unwrap()
    else {
        panic!()
    };
    let a = transformed
        .offset(1.5, OffsetJoin::Miter { limit: 4. }, &t())
        .unwrap();
    let b = original
        .offset(1.5, OffsetJoin::Miter { limit: 4. }, &t())
        .unwrap();
    for (left, right) in a.points.iter().zip(&b.points) {
        assert!(left.distance(transform.apply_point(*right)) < 1e-10);
    }
    let crossing = Polyline {
        points: vec![p(0., 0.), p(2., 2.), p(0., 2.), p(2., 0.)],
        closed: false,
    };
    assert_eq!(
        crossing
            .offset(0.2, OffsetJoin::Bevel, &t())
            .unwrap_err()
            .diagnostic()
            .reason,
        GeometryReason::InvalidTopology
    );
}
#[test]
fn straight_polyline_offset_keeps_all_authored_vertices() {
    let path = Polyline {
        points: vec![p(0., 0.), p(2., 0.), p(5., 0.)],
        closed: false,
    };
    let result = path.offset(1., OffsetJoin::Bevel, &t()).unwrap();
    assert_eq!(result.points, vec![p(0., 1.), p(2., 1.), p(5., 1.)]);
    let encoded = serde_json::to_value(OffsetJoin::Miter { limit: 4. }).unwrap();
    assert_eq!(encoded["type"], "miter");
    assert_eq!(encoded["data"]["limit"], 4.);
}

#[test]
fn sub_resolution_edits_never_return_unchanged_success() {
    let line = LineSegment {
        start: p(1e16, 1e16),
        end: p(1e16 + 100., 1e16),
    };
    for error in [
        line.offset(0.1, &t()).unwrap_err(),
        line.extend(0.1, 0., &t()).unwrap_err(),
        line.trim(0.001, 0.5, &t()).unwrap_err(),
    ] {
        assert_eq!(
            error.diagnostic().reason,
            GeometryReason::NumericalErrorBudgetExceeded
        );
    }
    let path = Polyline {
        points: vec![line.start, line.end],
        closed: false,
    };
    assert_eq!(
        path.offset(0.1, OffsetJoin::Bevel, &t())
            .unwrap_err()
            .diagnostic()
            .reason,
        GeometryReason::NumericalErrorBudgetExceeded
    );
}
