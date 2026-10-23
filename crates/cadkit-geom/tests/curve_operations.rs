//! Synthetic regression tests for curve operations operations.
#![cfg(test)]

use cadkit_geom::*;
use std::f64::consts::{PI, TAU};
fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}
fn t() -> GeometryTolerance {
    GeometryTolerance::default()
}
fn circle(x: f64, y: f64, radius: f64) -> Curve {
    Curve::Circle(Circle {
        center: p(x, y),
        radius,
    })
}
fn line(a: Point2, b: Point2) -> Curve {
    Curve::Line(LineSegment { start: a, end: b })
}
fn arc(start: f64, sweep: f64) -> Curve {
    Curve::Arc(Arc {
        center: p(0., 0.),
        radius: 2.,
        start_angle: start,
        sweep_angle: sweep,
    })
}
fn close(a: f64, b: f64, error: f64) {
    assert!((a - b).abs() <= error, "{a} != {b}");
}
fn points(result: CurveIntersection) -> Vec<CurveIntersectionPoint> {
    match result {
        CurveIntersection::Points(points) => points,
        other => panic!("{other:?}"),
    }
}
fn overlap(result: CurveIntersection) -> CurveOverlap {
    match result {
        CurveIntersection::Overlap(value) => value,
        other => panic!("{other:?}"),
    }
}

#[test]
fn line_circle_crossing_tangent_and_near_tangent_are_distinct() {
    let c = circle(0., 0., 1.);
    let result = points(line(p(-2., 0.), p(2., 0.)).intersections(&c, &t()).unwrap());
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].point, p(-1., 0.));
    assert_eq!(result[1].point, p(1., 0.));
    assert_eq!(result[0].kind, IntersectionKind::Crossing);
    close(result[0].first_parameter, 0.25, 1e-12);
    let tangent = points(line(p(-2., 1.), p(2., 1.)).intersections(&c, &t()).unwrap());
    assert_eq!(tangent.len(), 1);
    assert_eq!(tangent[0].point, p(0., 1.));
    assert_eq!(tangent[0].kind, IntersectionKind::Tangent);
    assert_eq!(
        line(p(-2., 1.0001), p(2., 1.0001))
            .intersections(&c, &t())
            .unwrap(),
        CurveIntersection::None
    );
    let near = points(
        line(p(-2., 1. - 1e-10), p(2., 1. - 1e-10))
            .intersections(&c, &t())
            .unwrap(),
    );
    assert_eq!(near.len(), 2);
    assert!(near.iter().all(|p| p.kind == IntersectionKind::Crossing));
    assert_eq!(
        line(p(2., 0.), p(3., 0.)).intersections(&c, &t()).unwrap(),
        CurveIntersection::None
    );
}
#[test]
fn line_arc_filters_circle_support_and_reports_arc_parameters() {
    let top = arc(0., PI);
    let bottom = arc(0., -PI);
    let horizontal = line(p(-3., 1.), p(3., 1.));
    let found = points(horizontal.intersections(&top, &t()).unwrap());
    assert_eq!(found.len(), 2);
    close(found[0].point.x, -3_f64.sqrt(), 1e-12);
    close(found[1].point.x, 3_f64.sqrt(), 1e-12);
    close(found[0].second_parameter, 5. / 6., 1e-12);
    close(found[1].second_parameter, 1. / 6., 1e-12);
    assert_eq!(
        horizontal.intersections(&bottom, &t()).unwrap(),
        CurveIntersection::None
    );
    let reversed = points(top.intersections(&horizontal, &t()).unwrap());
    close(reversed[0].first_parameter, 1. / 6., 1e-12);
}
#[test]
fn two_circles_cross_tangent_contain_and_coincide() {
    let a = circle(0., 0., 5.);
    let result = points(a.intersections(&circle(6., 0., 5.), &t()).unwrap());
    assert_eq!(result.len(), 2);
    for q in result {
        close(q.point.x, 3., 1e-12);
        close(q.point.y.abs(), 4., 1e-12);
        assert_eq!(q.kind, IntersectionKind::Crossing);
    }
    for b in [circle(10., 0., 5.), circle(2., 0., 3.)] {
        let result = points(a.intersections(&b, &t()).unwrap());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].kind, IntersectionKind::Tangent);
        close(result[0].point.x, 5., 1e-12);
    }
    assert_eq!(
        a.intersections(&circle(1., 0., 1.), &t()).unwrap(),
        CurveIntersection::None
    );
    assert_eq!(
        a.intersections(&circle(11., 0., 5.), &t()).unwrap(),
        CurveIntersection::None
    );
    let result = overlap(a.intersections(&a, &t()).unwrap());
    assert_eq!(result.curves, vec![a]);
    assert!(result.isolated_points.is_empty());
}
#[test]
fn coincident_arcs_preserve_disconnected_overlaps_and_seam_contacts() {
    let result = overlap(
        arc(0., 1.5 * PI)
            .intersections(&arc(PI, 1.5 * PI), &t())
            .unwrap(),
    );
    assert_eq!(result.curves.len(), 2);
    close(
        result.curves.iter().map(|c| c.length(&t()).unwrap()).sum(),
        2. * PI,
        1e-12,
    );
    assert!(result.isolated_points.is_empty());
    let a = arc(350_f64.to_radians(), 40_f64.to_radians());
    let b = arc(340_f64.to_radians(), 40_f64.to_radians());
    let result = overlap(a.intersections(&b, &t()).unwrap());
    assert_eq!(result.curves.len(), 1);
    close(
        result.curves[0].length(&t()).unwrap(),
        2. * 30_f64.to_radians(),
        1e-12,
    );
    let contacts = points(arc(0., PI).intersections(&arc(PI, PI), &t()).unwrap());
    assert_eq!(contacts.len(), 2);
    assert!(contacts.iter().all(|q| q.kind == IntersectionKind::Touch));
    assert_eq!(
        arc(0., PI / 4.)
            .intersections(&arc(PI, PI / 4.), &t())
            .unwrap(),
        CurveIntersection::None
    );
}
#[test]
fn arc_circle_and_arc_arc_filter_noncoincident_intersections() {
    let top = arc(0., PI);
    let other = circle(2., 0., 2.);
    let found = points(top.intersections(&other, &t()).unwrap());
    assert_eq!(found.len(), 1);
    close(found[0].point.x, 1., 1e-12);
    close(found[0].point.y, 3_f64.sqrt(), 1e-12);
    let other_top = Curve::Arc(Arc {
        center: p(2., 0.),
        radius: 2.,
        start_angle: 0.,
        sweep_angle: PI,
    });
    assert_eq!(
        points(top.intersections(&other_top, &t()).unwrap()).len(),
        1
    );
}
#[test]
fn intersections_are_invariant_under_rigid_transform_and_operand_swap() {
    let left = line(p(-4., 1.), p(4., 1.));
    let right = circle(0., 0., 3.);
    let original = points(left.intersections(&right, &t()).unwrap());
    for i in 0..30 {
        let transform =
            Transform2::rotation(i as f64 * 0.17).then(Transform2::translation(100., -30.));
        let a = left.transformed(transform, &t()).unwrap();
        let b = right.transformed(transform, &t()).unwrap();
        let found = points(a.intersections(&b, &t()).unwrap());
        let reverse = points(b.intersections(&a, &t()).unwrap());
        for point in &original {
            let expected = transform.apply_point(point.point);
            assert!(found.iter().any(|p| p.point.distance(expected) < 1e-10));
            assert!(reverse.iter().any(|p| p.point.distance(expected) < 1e-10));
        }
    }
}
#[test]
fn ellipse_nearest_matches_axis_and_independent_normal_oracles() {
    let ellipse = Ellipse {
        center: p(0., 0.),
        axis_u: Vector2 { x: 3., y: 0. },
        axis_v: Vector2 { x: 0., y: 2. },
    };
    let curve = Curve::Ellipse(ellipse);
    assert!(
        curve
            .nearest_point(p(10., 0.), &t())
            .unwrap()
            .distance(p(3., 0.))
            < 1e-8
    );
    let result = curve
        .nearest_point_with_options(p(0., 0.), NearestPointOptions::for_tolerance(&t()), &t())
        .unwrap();
    close(result.distance, 2., 1e-8);
    assert!(result.converged);
    assert!(result.distance_error_bound <= t().linear);
    let ellipse = Ellipse {
        center: p(4., -3.),
        axis_u: Vector2 { x: 3., y: 1. },
        axis_v: Vector2 { x: 0.5, y: 2. },
    };
    let angle = 0.7_f64;
    let boundary = ellipse.point_at(angle);
    let (s, c) = angle.sin_cos();
    let tangent = Vector2 {
        x: -ellipse.axis_u.x * s + ellipse.axis_v.x * c,
        y: -ellipse.axis_u.y * s + ellipse.axis_v.y * c,
    };
    let normal = Vector2 {
        x: tangent.y,
        y: -tangent.x,
    }
    .scaled(1. / tangent.length());
    let query = boundary.translated(normal.scaled(1.3));
    let result = Curve::Ellipse(ellipse)
        .nearest_point_with_options(query, NearestPointOptions::for_tolerance(&t()), &t())
        .unwrap();
    close(result.distance, 1.3, 1e-8);
    assert!(result.point.distance(boundary) < 1e-6);
    close(result.parameter, angle / TAU, 1e-7);
    assert!(result.distance_error_bound <= t().linear);
}
#[test]
fn bezier_nearest_finds_global_twin_minima_and_endpoint() {
    let curve = Curve::Bezier(Bezier {
        control_points: vec![p(-1., 1.), p(0., -1.), p(1., 1.)],
    });
    // On y=x^2, the two nearest points to (0,1) have x=+/-sqrt(1/2), y=1/2.
    let result = curve
        .nearest_point_with_options(p(0., 1.), NearestPointOptions::for_tolerance(&t()), &t())
        .unwrap();
    close(result.distance, 0.75_f64.sqrt(), 1e-8);
    close(result.point.x.abs(), 0.5_f64.sqrt(), 1e-6);
    close(result.point.y, 0.5, 1e-6);
    assert!(result.distance_error_bound <= t().linear);
    assert!(
        curve
            .nearest_point(p(0., -2.), &t())
            .unwrap()
            .distance(p(0., 0.))
            < 1e-7
    );
    assert!(
        curve
            .nearest_point(p(10., 1.), &t())
            .unwrap()
            .distance(p(1., 1.))
            < 1e-7
    );
}
#[test]
fn cubic_nearest_beats_independent_dense_polynomial_reference() {
    let curve = Curve::Bezier(Bezier {
        control_points: vec![p(0., 0.), p(8., 10.), p(-8., 10.), p(0., 0.)],
    });
    for query in [p(1., 2.), p(-1., 6.), p(0., 8.)] {
        let result = curve
            .nearest_point_with_options(query, NearestPointOptions::for_tolerance(&t()), &t())
            .unwrap();
        let dense = (0..=100_000)
            .map(|i| {
                let u = i as f64 / 100_000.;
                let q = p(24. * u * (1. - u) * (1. - 2. * u), 30. * u * (1. - u));
                q.distance(query)
            })
            .fold(f64::INFINITY, f64::min);
        assert!(result.distance <= dense + 1e-8);
        assert!(dense - result.distance < 1e-5);
        assert!(result.distance_error_bound <= t().linear);
    }
}
#[test]
fn nearest_budget_exhaustion_and_json_are_explicit() {
    let curve = Curve::Bezier(Bezier {
        control_points: vec![p(0., 0.), p(8., 10.), p(-8., 10.), p(0., 0.)],
    });
    let error = curve
        .nearest_point_with_options(
            p(1., 2.),
            NearestPointOptions {
                max_distance_error: 1e-10,
                max_iterations: 1,
            },
            &t(),
        )
        .unwrap_err();
    assert_eq!(
        error.diagnostic().reason,
        GeometryReason::ApproximationLimit
    );
    let encoded = serde_json::to_value(&error).unwrap();
    assert_eq!(encoded["code"], "UNSUPPORTED");
    assert_eq!(encoded["details"]["operation"], "nearest_point");
    let result = curve
        .nearest_point_with_options(p(1., 2.), NearestPointOptions::for_tolerance(&t()), &t())
        .unwrap();
    assert_eq!(
        serde_json::from_value::<NearestPointResult>(serde_json::to_value(result).unwrap())
            .unwrap(),
        result
    );
    let intersection = circle(0., 0., 1.)
        .intersections(&line(p(-2., 1.), p(2., 1.)), &t())
        .unwrap();
    let mut encoded = serde_json::to_value(&intersection).unwrap();
    assert_eq!(encoded["type"], "points");
    assert_eq!(encoded["data"][0]["kind"], "tangent");
    encoded["extra"] = serde_json::json!(true);
    assert!(serde_json::from_value::<CurveIntersection>(encoded).is_err());
}

#[test]
fn near_coincidence_does_not_fabricate_full_overlap() {
    let error = circle(0., 0., 1.)
        .intersections(&circle(1e-10, 0., 1.), &t())
        .unwrap_err();
    assert_eq!(
        error.diagnostic().reason,
        GeometryReason::AmbiguousIntersection
    );
    let c = Curve::Ellipse(Ellipse {
        center: p(0., 0.),
        axis_u: Vector2 { x: 2., y: 0. },
        axis_v: Vector2 { x: 0., y: 2. },
    });
    let snaps = c.snap_candidates(p(0., 0.), 1., &t()).unwrap();
    assert!(snaps.iter().any(|s| s.kind == SnapKind::Center));
    assert!(!snaps.iter().any(|s| s.kind == SnapKind::Nearest));
}
