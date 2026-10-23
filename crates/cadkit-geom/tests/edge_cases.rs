//! Synthetic extreme-input coverage across the public geometry operations.
#![cfg(test)]

use cadkit_geom::*;

fn point(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn square(scale: f64) -> Polygon {
    Polygon {
        exterior: vec![
            point(0., 0.),
            point(scale, 0.),
            point(scale, scale),
            point(0., scale),
        ],
        holes: vec![],
    }
}

fn nurbs() -> NurbsCurve {
    NurbsCurve {
        degree: 2,
        control_points: vec![point(0., 0.), point(1., 2.), point(3., 0.)],
        weights: vec![1., 1., 1.],
        knots: vec![0., 0., 0., 1., 1., 1.],
        periodic: false,
    }
}

#[test]
fn nonfinite_coordinates_fail_with_typed_errors_across_operations() {
    let t = GeometryTolerance::default();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let line = LineSegment {
            start: point(value, 0.),
            end: point(2., 1.),
        };
        let good = LineSegment {
            start: point(0., 0.),
            end: point(2., 1.),
        };
        let mut polygon = square(4.);
        polygon.exterior[0].x = value;
        let mut curve = nurbs();
        curve.control_points[1].y = value;
        let controls = [
            ControlPoint {
                source: point(value, 0.),
                target: point(0., 0.),
            },
            ControlPoint {
                source: point(1., 0.),
                target: point(1., 0.),
            },
            ControlPoint {
                source: point(0., 1.),
                target: point(0., 1.),
            },
        ];
        let errors = [
            line.intersection(&good, &t).unwrap_err(),
            line.offset(1., &t).unwrap_err(),
            line.trim(0.1, 0.9, &t).unwrap_err(),
            Curve::Line(line)
                .nearest_point(point(1., 1.), &t)
                .unwrap_err(),
            polygon
                .union(&square(2.), PolygonBooleanOptions::default(), &t)
                .unwrap_err(),
            curve.point_at(0.5, &t).unwrap_err(),
            curve.tessellate(0.01, &t).unwrap_err(),
            fit_similarity(&controls, &t).unwrap_err(),
            fit_affine(&controls, &t).unwrap_err(),
        ];
        for error in errors {
            assert_eq!(error.diagnostic().reason, GeometryReason::NonFiniteNumber);
        }
    }
}

#[test]
fn invalid_tolerances_never_panic_or_succeed() {
    let line = LineSegment {
        start: point(0., 0.),
        end: point(2., 1.),
    };
    let controls = [
        ControlPoint {
            source: point(0., 0.),
            target: point(0., 0.),
        },
        ControlPoint {
            source: point(1., 0.),
            target: point(1., 0.),
        },
        ControlPoint {
            source: point(0., 1.),
            target: point(0., 1.),
        },
    ];
    for value in [0., -1., f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for field in 0..3 {
            let mut t = GeometryTolerance::default();
            match field {
                0 => t.linear = value,
                1 => t.angular = value,
                _ => t.area = value,
            }
            assert!(line.intersection(&line, &t).is_err());
            assert!(line.trim(0.1, 0.9, &t).is_err());
            assert!(line.offset(1., &t).is_err());
            assert!(Curve::Line(line).nearest_point(point(1., 1.), &t).is_err());
            assert!(
                square(4.)
                    .union(&square(2.), PolygonBooleanOptions::default(), &t)
                    .is_err()
            );
            assert!(nurbs().point_at(0.5, &t).is_err());
            assert!(
                nurbs()
                    .nearest_point_with_options(
                        point(1., 1.),
                        NearestPointOptions::for_tolerance(&t),
                        &t
                    )
                    .is_err()
            );
            assert!(fit_similarity(&controls, &t).is_err());
            assert!(fit_affine(&controls, &t).is_err());
        }
    }
}

#[test]
fn empty_singleton_crossing_and_degenerate_rings_are_rejected() {
    let t = GeometryTolerance::default();
    for ring in [
        vec![],
        vec![point(0., 0.)],
        vec![point(0., 0.), point(1., 0.)],
        vec![point(0., 0.); 4],
        vec![point(0., 0.), point(1., 0.), point(2., 0.)],
        vec![point(0., 0.), point(2., 2.), point(0., 2.), point(2., 0.)],
    ] {
        let polygon = Polygon {
            exterior: ring.clone(),
            holes: vec![],
        };
        assert!(
            polygon
                .union(&square(4.), PolygonBooleanOptions::default(), &t)
                .is_err()
        );
        assert!(polygon_slice(&[polygon], &[], PolygonBooleanOptions::default(), &t).is_err());
        let polyline = Polyline {
            points: ring,
            closed: true,
        };
        assert!(polyline.offset(0.1, OffsetJoin::Bevel, &t).is_err());
    }
    let empty = polygon_boolean(
        &[],
        &[],
        PolygonBooleanOperation::Union,
        PolygonBooleanOptions::default(),
        &t,
    )
    .unwrap();
    assert!(empty.geometry.polygons.is_empty());
    assert_eq!(empty.area, 0.);
}

#[test]
fn zero_length_segments_and_empty_calibration_fail_explicitly() {
    let t = GeometryTolerance::default();
    let line = LineSegment {
        start: point(2., 3.),
        end: point(2., 3.),
    };
    assert!(line.intersection(&line, &t).is_err());
    assert!(line.offset(1., &t).is_err());
    assert!(line.trim(0., 1., &t).is_err());
    assert!(line.nearest_point(point(0., 0.), &t).is_err());
    assert!(fit_similarity(&[], &t).is_err());
    assert!(fit_affine(&[], &t).is_err());
    assert!(evaluate_calibration(Transform2::identity(), &[]).is_err());
    let one = [ControlPoint {
        source: point(0., 0.),
        target: point(1., 1.),
    }];
    assert!(fit_similarity(&one, &t).is_err());
    assert!(fit_affine(&one, &t).is_err());
}

#[test]
fn extreme_finite_coordinates_return_results_without_panicking() {
    let t = GeometryTolerance::default();
    for scale in [f64::from_bits(1), 1e-200, 1e-12, 1e12, 1e150, 1e308] {
        let outcome = std::panic::catch_unwind(|| {
            let a = square(scale);
            let _ = a.union(&square(2.), PolygonBooleanOptions::default(), &t);
            let line = LineSegment {
                start: point(-scale, 0.),
                end: point(scale, scale),
            };
            let _ = line.intersection(
                &LineSegment {
                    start: point(0., -scale),
                    end: point(0., scale),
                },
                &t,
            );
            let _ = line.offset(scale, &t);
            let _ = line.trim(0.25, 0.75, &t);
            let _ = Curve::Line(line).nearest_point(point(0., scale), &t);
            let polyline = Polyline {
                points: a.exterior.clone(),
                closed: true,
            };
            let _ = polyline.offset(scale / 8., OffsetJoin::Miter { limit: 4. }, &t);
            let controls: Vec<_> = a
                .exterior
                .iter()
                .map(|p| ControlPoint {
                    source: *p,
                    target: *p,
                })
                .collect();
            let _ = fit_affine(&controls, &t);
            let _ = fit_similarity(&controls, &t);
            let mut curve = nurbs();
            for p in &mut curve.control_points {
                p.x *= scale;
                p.y *= scale;
            }
            let _ = curve.point_at(0.5, &t);
            let _ = curve.tessellate_with_options(
                NurbsTessellationOptions {
                    max_segments: 128,
                    max_subdivisions: 128,
                    max_depth: 8,
                    max_error: 0.01,
                },
                &t,
            );
            let _ = curve.nearest_point_with_options(
                point(0., scale),
                NearestPointOptions {
                    max_distance_error: 0.01,
                    max_iterations: 16,
                },
                &t,
            );
        });
        assert!(outcome.is_ok(), "panic at scale {scale}");
    }
}

#[test]
fn malformed_nurbs_degree_knot_weight_and_count_combinations_are_errors() {
    let t = GeometryTolerance::default();
    let mut cases = Vec::new();
    for degree in [0, 4, u8::MAX] {
        let mut c = nurbs();
        c.degree = degree;
        cases.push(c);
    }
    for count in [0, 1, 2, 4, 4097] {
        let mut c = nurbs();
        c.control_points.resize(count, point(3., 2.));
        cases.push(c);
    }
    for value in [0., -1., f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut c = nurbs();
        c.weights[1] = value;
        cases.push(c);
    }
    for knots in [
        vec![],
        vec![0.],
        vec![0.; 6],
        vec![0., 0., 1., 0., 1., 1.],
        vec![f64::NAN; 6],
        vec![f64::INFINITY; 6],
        vec![f64::NEG_INFINITY; 6],
    ] {
        let mut c = nurbs();
        c.knots = knots;
        cases.push(c);
    }
    let mut c = nurbs();
    c.weights.clear();
    cases.push(c);
    for (index, curve) in cases.iter().enumerate() {
        assert!(curve.validate(&t).is_err(), "case {index}");
        assert!(curve.point_at(0.5, &t).is_err(), "case {index}");
        assert!(curve.derivative_at(0.5, &t).is_err(), "case {index}");
        assert!(curve.tessellate(0.01, &t).is_err(), "case {index}");
        assert!(
            curve
                .nearest_point_with_options(
                    point(1., 1.),
                    NearestPointOptions::for_tolerance(&t),
                    &t
                )
                .is_err(),
            "case {index}"
        );
    }
}

#[test]
fn nonfinite_edit_parameters_and_approximation_budgets_are_errors() {
    let t = GeometryTolerance::default();
    let line = LineSegment {
        start: point(0., 0.),
        end: point(2., 1.),
    };
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(line.offset(value, &t).is_err());
        assert!(line.trim(value, 0.8, &t).is_err());
        assert!(line.trim(0.2, value, &t).is_err());
        assert!(line.extend(value, 0., &t).is_err());
        assert!(nurbs().point_at(value, &t).is_err());
    }
    for value in [0., -1., f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(nurbs().tessellate(value, &t).is_err());
        assert!(
            nurbs()
                .nearest_point_with_options(
                    point(1., 1.),
                    NearestPointOptions {
                        max_distance_error: value,
                        max_iterations: 16
                    },
                    &t
                )
                .is_err()
        );
    }
}
