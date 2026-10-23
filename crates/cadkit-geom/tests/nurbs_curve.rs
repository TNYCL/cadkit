//! Synthetic regression tests for nurbs curve operations.
#![cfg(test)]

use cadkit_geom::*;
fn quarter() -> NurbsCurve {
    NurbsCurve {
        degree: 2,
        control_points: vec![
            Point2::new(1., 0.),
            Point2::new(1., 1.),
            Point2::new(0., 1.),
        ],
        weights: vec![1., 0.5_f64.sqrt(), 1.],
        knots: vec![0., 0., 0., 1., 1., 1.],
        periodic: false,
    }
}
fn options() -> NearestPointOptions {
    NearestPointOptions {
        max_distance_error: 1e-6,
        max_iterations: 100_000,
    }
}
fn verify(result: &NearestPointResult, distance: f64) {
    assert!(result.converged);
    assert!(result.distance_error_bound <= options().max_distance_error);
    assert!(
        (result.distance - distance).abs() <= result.distance_error_bound + 1e-12,
        "actual={} expected={distance} bound={}",
        result.distance,
        result.distance_error_bound
    );
}

#[test]
fn rational_quarter_circle_nearest_matches_radial_and_endpoint_oracles() {
    let curve = Curve::Nurbs(quarter());
    let t = GeometryTolerance::default();
    let result = curve
        .nearest_point_with_options(Point2::new(2., 2.), options(), &t)
        .unwrap();
    verify(&result, 8_f64.sqrt() - 1.);
    assert!(
        result
            .point
            .distance(Point2::new(0.5_f64.sqrt(), 0.5_f64.sqrt()))
            < 0.002
    );
    let result = curve
        .nearest_point_with_options(Point2::new(-1., 2.), options(), &t)
        .unwrap();
    verify(&result, 2_f64.sqrt());
    assert!(result.point.distance(Point2::new(0., 1.)) < 0.002);
    // Default Curve dispatch reaches NURBS search without recursively calling itself.
    let point = curve.nearest_point(Point2::new(2., 2.), &t).unwrap();
    assert!(point.distance(Point2::new(0.5_f64.sqrt(), 0.5_f64.sqrt())) < 0.001);
}

#[test]
fn nonuniform_rational_linear_spans_keep_native_parameter_and_nearest_point() {
    let curve = NurbsCurve {
        degree: 1,
        control_points: vec![
            Point2::new(0., 0.),
            Point2::new(2., 0.),
            Point2::new(2., 3.),
            Point2::new(5., 3.),
        ],
        weights: vec![1., 3., 2., 1.],
        knots: vec![5., 5., 6., 8., 10., 10.],
        periodic: false,
    };
    let result = curve
        .nearest_point_with_options(
            Point2::new(3., 1.),
            options(),
            &GeometryTolerance::default(),
        )
        .unwrap();
    verify(&result, 1.);
    assert!(result.point.distance(Point2::new(2., 1.)) < 0.002);
    assert!((result.parameter - 13. / 35.).abs() < 0.001);
    assert!(
        curve
            .point_at(result.parameter, &GeometryTolerance::default())
            .unwrap()
            .distance(result.point)
            < 1e-10
    );
}

#[test]
fn c0_corner_is_a_nearest_candidate_without_derivative_assumptions() {
    let curve = NurbsCurve {
        degree: 1,
        control_points: vec![
            Point2::new(0., 1.),
            Point2::new(0., 0.),
            Point2::new(1., 0.),
        ],
        weights: vec![1.; 3],
        knots: vec![0., 0., 0.3, 1., 1.],
        periodic: false,
    };
    let result = curve
        .nearest_point_with_options(
            Point2::new(-0.2, -0.3),
            options(),
            &GeometryTolerance::default(),
        )
        .unwrap();
    verify(&result, 0.13_f64.sqrt());
    assert!(result.point.distance(Point2::new(0., 0.)) < 0.002);
    assert!((result.parameter - 0.3).abs() < 0.001);
}

#[test]
fn global_twin_minima_do_not_get_stuck_at_stationary_midpoint() {
    let curve = NurbsCurve {
        degree: 2,
        control_points: vec![
            Point2::new(-2., 4.),
            Point2::new(0., -4.),
            Point2::new(2., 4.),
        ],
        weights: vec![1.; 3],
        knots: vec![0., 0., 0., 1., 1., 1.],
        periodic: false,
    };
    let result = Curve::Nurbs(curve.clone())
        .nearest_point_with_options(
            Point2::new(0., 2.),
            options(),
            &GeometryTolerance::default(),
        )
        .unwrap();
    verify(&result, 1.75_f64.sqrt());
    assert!((result.point.x.abs() - 1.5_f64.sqrt()).abs() < 0.002);
    assert!((result.point.y - 1.5).abs() < 0.005);
    assert!(
        curve
            .point_at(result.parameter, &GeometryTolerance::default())
            .unwrap()
            .distance(result.point)
            < 1e-10
    );
}

#[test]
fn continuum_of_minima_still_requires_global_distance_certificate() {
    let result = Curve::Nurbs(quarter())
        .nearest_point_with_options(
            Point2::new(0., 0.),
            NearestPointOptions {
                max_distance_error: 1e-4,
                max_iterations: 10_000,
            },
            &GeometryTolerance::default(),
        )
        .unwrap();
    assert!((result.distance - 1.).abs() <= result.distance_error_bound + 1e-12);
    assert!(result.distance_error_bound <= 1e-4);
    assert!(result.iterations > 0);
}

#[test]
fn affine_transform_preserves_nurbs_source_parameters_and_rejects_loss() {
    let curve = quarter();
    let t = GeometryTolerance::default();
    let transform = Transform2 {
        m11: 2.,
        m12: 0.5,
        m21: -0.3,
        m22: 1.5,
        tx: 3.,
        ty: -4.,
    };
    let Curve::Nurbs(result) = Curve::Nurbs(curve.clone())
        .transformed(transform, &t)
        .unwrap()
    else {
        panic!("NURBS type must be preserved")
    };
    assert_eq!(result.weights, curve.weights);
    assert_eq!(result.knots, curve.knots);
    assert_eq!(result.degree, curve.degree);
    assert_eq!(result.periodic, curve.periodic);
    for parameter in [0., 0.2, 0.5, 0.9, 1.] {
        assert!(
            result
                .point_at(parameter, &t)
                .unwrap()
                .distance(transform.apply_point(curve.point_at(parameter, &t).unwrap()))
                < 1e-11
        );
    }
    assert_eq!(
        Curve::Nurbs(curve)
            .transformed(Transform2::translation(1e16, 1e16), &t)
            .unwrap_err()
            .diagnostic()
            .reason,
        GeometryReason::NumericalErrorBudgetExceeded
    );
}

#[test]
fn curve_dispatch_bounds_tessellation_and_parameter_snap_preserve_rational_shape() {
    let curve = Curve::Nurbs(quarter());
    let t = GeometryTolerance::default();
    curve.validate(&t).unwrap();
    assert_eq!(curve.bounds().unwrap().max, Point2::new(1., 1.));
    let points = curve.tessellate(1e-3, &t).unwrap();
    assert!(points.len() > 2);
    let snaps = curve
        .snap_candidates(Point2::new(0.7, 0.7), 2., &t)
        .unwrap();
    assert_eq!(
        snaps
            .iter()
            .filter(|s| s.kind == SnapKind::Endpoint)
            .count(),
        2
    );
    let midpoint = snaps.iter().find(|s| s.kind == SnapKind::Midpoint).unwrap();
    assert!(
        midpoint
            .point
            .distance(Point2::new(0.5_f64.sqrt(), 0.5_f64.sqrt()))
            < 1e-12
    );
    assert!(snaps.iter().any(|s| s.kind == SnapKind::Nearest));
    let value = serde_json::to_value(&curve).unwrap();
    assert_eq!(value["type"], "nurbs");
    assert_eq!(serde_json::from_value::<Curve>(value).unwrap(), curve);
}

#[test]
fn unsupported_measurements_and_intersections_return_typed_reasons() {
    let curve = Curve::Nurbs(quarter());
    let t = GeometryTolerance::default();
    for error in [
        curve.length(&t).unwrap_err(),
        curve.enclosed_area(&t).unwrap_err(),
    ] {
        assert!(matches!(error, GeometryError::Unsupported(_)));
        assert_eq!(
            error.diagnostic().reason,
            GeometryReason::MeasurementNotSupported
        );
    }
    let line = Curve::Line(LineSegment {
        start: Point2::new(0., 0.),
        end: Point2::new(2., 2.),
    });
    for (a, b) in [(&curve, &line), (&line, &curve), (&curve, &curve)] {
        assert_eq!(
            a.intersections(b, &t).unwrap_err().diagnostic().reason,
            GeometryReason::IntersectionNotSupported
        );
    }
}

#[test]
fn nearest_iteration_and_invalid_query_budgets_fail_explicitly() {
    let curve = Curve::Nurbs(quarter());
    let t = GeometryTolerance::default();
    let error = curve
        .nearest_point_with_options(
            Point2::new(0., 0.),
            NearestPointOptions {
                max_distance_error: 1e-12,
                max_iterations: 1,
            },
            &t,
        )
        .unwrap_err();
    assert_eq!(
        error.diagnostic().reason,
        GeometryReason::ApproximationLimit
    );
    assert_eq!(error.diagnostic().operation, "nurbs.nearest_point");
    assert_eq!(
        curve
            .nearest_point_with_options(Point2::new(f64::NAN, 0.), options(), &t)
            .unwrap_err()
            .diagnostic()
            .reason,
        GeometryReason::NonFiniteNumber
    );
    assert_eq!(
        curve
            .nearest_point_with_options(
                Point2::new(0., 0.),
                NearestPointOptions {
                    max_iterations: 0,
                    ..options()
                },
                &t
            )
            .unwrap_err()
            .diagnostic()
            .reason,
        GeometryReason::InvalidParameterRange
    );
}

#[test]
fn nonuniform_multiple_spans_and_cubic_degree_elevation_share_the_global_oracle() {
    let multispan = NurbsCurve {
        degree: 2,
        control_points: vec![
            Point2::new(-100., 4.),
            Point2::new(-60., -4.),
            Point2::new(-2., 4.),
            Point2::new(0., -4.),
            Point2::new(2., 4.),
            Point2::new(60., -4.),
            Point2::new(100., 4.),
        ],
        weights: vec![1.; 7],
        knots: vec![0., 0., 0., 0.2, 0.2, 0.8, 0.8, 1., 1., 1.],
        periodic: false,
    };
    let cubic = NurbsCurve {
        degree: 3,
        control_points: vec![
            Point2::new(-2., 4.),
            Point2::new(-2. / 3., -4. / 3.),
            Point2::new(2. / 3., -4. / 3.),
            Point2::new(2., 4.),
        ],
        weights: vec![1.; 4],
        knots: vec![0., 0., 0., 0., 1., 1., 1., 1.],
        periodic: false,
    };
    for curve in [multispan, cubic] {
        let result = curve
            .nearest_point_with_options(
                Point2::new(0., 2.),
                options(),
                &GeometryTolerance::default(),
            )
            .unwrap();
        verify(&result, 1.75_f64.sqrt());
        assert!((result.point.x.abs() - 1.5_f64.sqrt()).abs() < 0.002);
        assert!(
            curve
                .point_at(result.parameter, &GeometryTolerance::default())
                .unwrap()
                .distance(result.point)
                < 1e-10
        );
    }
}
