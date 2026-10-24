//! Synthetic regression tests for geometry operations.
#![cfg(test)]

use cadkit_geom::*;
use std::f64::consts::{FRAC_PI_2, PI, TAU};
fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}
fn tolerance() -> GeometryTolerance {
    GeometryTolerance::default()
}
fn close(actual: f64, expected: f64, error: f64) {
    assert!((actual - expected).abs() <= error, "{actual} != {expected}");
}
fn rectangle(x: f64, y: f64, width: f64, height: f64) -> Vec<Point2> {
    vec![
        p(x, y),
        p(x + width, y),
        p(x + width, y + height),
        p(x, y + height),
    ]
}

#[test]
fn length_and_area_use_analytic_oracles() {
    let t = tolerance();
    close(
        Curve::Line(LineSegment {
            start: p(2., 3.),
            end: p(5., 7.),
        })
        .length(&t)
        .unwrap(),
        5.,
        1e-12,
    );
    let circle = Curve::Circle(Circle {
        center: p(10., -20.),
        radius: 3.,
    });
    close(circle.length(&t).unwrap(), 6. * PI, 1e-12);
    close(circle.enclosed_area(&t).unwrap(), 9. * PI, 1e-12);
    let arc = Curve::Arc(Arc {
        center: p(0., 0.),
        radius: 4.,
        start_angle: 0.,
        sweep_angle: -FRAC_PI_2,
    });
    close(arc.length(&t).unwrap(), 2. * PI, 1e-12);
    let polygon = Polygon {
        exterior: rectangle(1e9, -1e9, 10., 6.),
        holes: vec![rectangle(1e9 + 1., -1e9 + 2., 2., 3.)],
    };
    close(polygon.area(&t).unwrap(), 54., 1e-12);
    close(polygon.perimeter(&t).unwrap(), 42., 1e-12);
    close(
        signed_ring_area(&rectangle(1e9, -1e9, 10., 6.)).unwrap(),
        60.,
        1e-12,
    );
}
#[test]
fn polygon_holes_and_winding_are_explicit() {
    let t = tolerance();
    let mut polygon = Polygon {
        exterior: rectangle(0., 0., 10., 10.),
        holes: vec![rectangle(2., 2., 3., 3.)],
    };
    assert_eq!(
        polygon.locate(p(1., 1.), &t).unwrap(),
        PointLocation::Inside
    );
    assert_eq!(
        polygon.locate(p(3., 3.), &t).unwrap(),
        PointLocation::Outside
    );
    assert_eq!(
        polygon.locate(p(2., 3.), &t).unwrap(),
        PointLocation::Boundary
    );
    assert_eq!(
        polygon.locate(p(11., 3.), &t).unwrap(),
        PointLocation::Outside
    );
    polygon.exterior.reverse();
    polygon.holes[0].reverse();
    close(polygon.area(&t).unwrap(), 91., 1e-12);
}
#[test]
fn invalid_polygon_topology_is_never_repaired() {
    let t = tolerance();
    for exterior in [
        vec![p(0., 0.), p(3., 3.), p(0., 3.), p(3., 0.)],
        vec![p(0., 0.), p(3., 0.), p(1., 0.), p(1., 3.)],
        vec![p(0., 0.), p(3., 0.), p(3., 3.), p(0., 0.)],
    ] {
        assert!(
            Polygon {
                exterior,
                holes: vec![]
            }
            .validate(&t)
            .is_err()
        );
    }
    for holes in [
        vec![rectangle(9., 1., 2., 2.)],
        vec![rectangle(0., 1., 2., 2.)],
        vec![rectangle(1., 1., 5., 5.), rectangle(2., 2., 1., 1.)],
        vec![rectangle(1., 1., 3., 3.), rectangle(3., 3., 2., 2.)],
    ] {
        assert!(
            Polygon {
                exterior: rectangle(0., 0., 10., 10.),
                holes
            }
            .validate(&t)
            .is_err()
        );
    }
}
#[test]
fn segment_crossing_touch_and_overlap_have_distinct_results() {
    let t = tolerance();
    let a = LineSegment {
        start: p(0., 0.),
        end: p(10., 0.),
    };
    assert_eq!(
        a.intersection(
            &LineSegment {
                start: p(4., -2.),
                end: p(4., 2.)
            },
            &t
        )
        .unwrap(),
        SegmentIntersection::Point(p(4., 0.))
    );
    assert_eq!(
        a.intersection(
            &LineSegment {
                start: p(10., 0.),
                end: p(20., 0.)
            },
            &t
        )
        .unwrap(),
        SegmentIntersection::Point(p(10., 0.))
    );
    assert_eq!(
        a.intersection(
            &LineSegment {
                start: p(8., 0.),
                end: p(2., 0.)
            },
            &t
        )
        .unwrap(),
        SegmentIntersection::Overlap(LineSegment {
            start: p(2., 0.),
            end: p(8., 0.)
        })
    );
    assert_eq!(
        a.intersection(
            &LineSegment {
                start: p(0., 1.),
                end: p(10., 1.)
            },
            &t
        )
        .unwrap(),
        SegmentIntersection::None
    );
    let near_parallel = LineSegment {
        start: p(0., -1e-7),
        end: p(10_000., 1e-7),
    };
    let long = LineSegment {
        start: p(0., 0.),
        end: p(10_000., 0.),
    };
    match long.intersection(&near_parallel, &t).unwrap() {
        SegmentIntersection::Point(q) => close(q.x, 5000., 1e-6),
        result => panic!("{result:?}"),
    }
}
#[test]
fn exact_bounds_include_arc_and_bezier_interior_extrema() {
    let arc = Curve::Arc(Arc {
        center: p(2., 3.),
        radius: 4.,
        start_angle: PI / 4.,
        sweep_angle: PI,
    });
    let bounds = arc.bounds().unwrap();
    close(bounds.min.x, -2., 1e-12);
    close(bounds.max.y, 7., 1e-12);
    let curve = Curve::Bezier(Bezier {
        control_points: vec![p(0., 0.), p(1., 2.), p(2., 0.)],
    });
    let bounds = curve.bounds().unwrap();
    close(bounds.max.y, 1., 1e-12);
    close(bounds.max.x, 2., 1e-12);
    let curve = Curve::Bezier(Bezier {
        control_points: vec![p(0., 0.), p(0., 3.), p(3., 3.), p(3., 0.)],
    });
    close(curve.bounds().unwrap().max.y, 2.25, 1e-12);
}
#[test]
fn ellipse_general_affine_representation_preserves_area_and_bounds() {
    let t = tolerance();
    let circle = Curve::Circle(Circle {
        center: p(1., 2.),
        radius: 2.,
    });
    let transform = Transform2 {
        m11: 3.,
        m12: 1.,
        m21: 0.,
        m22: 2.,
        tx: 7.,
        ty: -3.,
    };
    let ellipse = circle.transformed(transform, &t).unwrap();
    assert!(matches!(ellipse, Curve::Ellipse(_)));
    close(ellipse.enclosed_area(&t).unwrap(), 24. * PI, 1e-10);
    let bounds = ellipse.bounds().unwrap();
    let center = transform.apply_point(p(1., 2.));
    close(bounds.max.x - center.x, 40_f64.sqrt(), 1e-12);
    close(bounds.max.y - center.y, 4., 1e-12);
    let points = ellipse.tessellate(0.001, &t).unwrap();
    for point in points {
        assert!(
            point.x <= bounds.max.x + 1e-12
                && point.x >= bounds.min.x - 1e-12
                && point.y <= bounds.max.y + 1e-12
                && point.y >= bounds.min.y - 1e-12
        );
    }
}
#[test]
fn numerical_lengths_match_independent_closed_form_references() {
    let t = tolerance();
    // Analytic arc length of the quadratic curve x=2t, y=4t(1-t).
    let parabola = Curve::Bezier(Bezier {
        control_points: vec![p(0., 0.), p(1., 2.), p(2., 0.)],
    });
    let expected = 5_f64.sqrt() + 0.5 * 2_f64.asinh();
    close(parabola.length(&t).unwrap(), expected, 1e-8);
    let circle_as_ellipse = Curve::Ellipse(Ellipse {
        center: p(1., 2.),
        axis_u: Vector2 { x: 3., y: 0. },
        axis_v: Vector2 { x: 0., y: 3. },
    });
    close(circle_as_ellipse.length(&t).unwrap(), 6. * PI, 1e-8);
    // High-precision complete elliptic integral reference for a=2, b=1.
    let ellipse = Curve::Ellipse(Ellipse {
        center: p(0., 0.),
        axis_u: Vector2 { x: 2., y: 0. },
        axis_v: Vector2 { x: 0., y: 1. },
    });
    close(ellipse.length(&t).unwrap(), 9.688_448_220_547_675, 1e-8);
}
#[test]
fn transform_order_inverse_and_area_scale_are_consistent() {
    let t = tolerance();
    let shape = Polygon {
        exterior: rectangle(0., 0., 3., 4.),
        holes: vec![rectangle(1., 1., 1., 1.)],
    };
    for index in 1..100 {
        let angle = index as f64 * 0.173;
        let transform = Transform2::scale(0.5 + index as f64 / 100., -2.)
            .then(Transform2::rotation(angle))
            .then(Transform2::translation(200., -300.));
        let inverse = transform.inverse(&t).unwrap();
        let q = p(17., -9.);
        assert!(inverse.apply_point(transform.apply_point(q)).distance(q) < 1e-10);
        close(
            shape.transformed(transform, &t).unwrap().area(&t).unwrap(),
            11. * transform.determinant().abs(),
            1e-10,
        );
    }
    let q = Transform2::translation(3., 4.)
        .then(Transform2::scale(2., 3.))
        .apply_point(p(1., 1.));
    assert_eq!(q, p(8., 15.));
}
#[test]
fn reflection_reverses_arc_sweep_but_preserves_endpoints() {
    let t = tolerance();
    let arc = Arc {
        center: p(2., 3.),
        radius: 4.,
        start_angle: 0.3,
        sweep_angle: 1.2,
    };
    let transform = Transform2::scale(-2., 2.);
    let Curve::Arc(mapped) = Curve::Arc(arc).transformed(transform, &t).unwrap() else {
        panic!()
    };
    assert_eq!(mapped.sweep_angle, -1.2);
    assert!(
        mapped
            .point_at(0.)
            .distance(transform.apply_point(arc.point_at(0.)))
            < 1e-12
    );
    assert!(
        mapped
            .point_at(1.)
            .distance(transform.apply_point(arc.point_at(1.)))
            < 1e-12
    );
    assert!(matches!(
        Curve::Arc(arc).transformed(Transform2::scale(2., 3.), &t),
        Err(GeometryError::Unsupported(_))
    ));
}
#[test]
fn tessellation_respects_circle_sagitta_and_keeps_measurement_independent() {
    let t = tolerance();
    let circle = Curve::Circle(Circle {
        center: p(0., 0.),
        radius: 50.,
    });
    let error = 0.003;
    let points = circle.tessellate(error, &t).unwrap();
    for pair in points.windows(2) {
        let midpoint = pair[0].lerp(pair[1], 0.5);
        assert!(50. - midpoint.distance(p(0., 0.)) <= error + 1e-12);
    }
    assert!(points[0].distance(*points.last().unwrap()) < 1e-12);
    close(circle.length(&t).unwrap(), 100. * PI, 1e-12);
    assert!(circle.tessellate(1e-30, &t).is_err());
}
#[test]
fn bezier_render_chords_stay_within_requested_distance() {
    let t = tolerance();
    let bezier = Bezier {
        control_points: vec![p(0., 0.), p(10., 20.), p(-10., 20.), p(0., 0.)],
    };
    let error = 0.005;
    let points = Curve::Bezier(bezier.clone()).tessellate(error, &t).unwrap();
    for i in 0..=1000 {
        let q = bezier.point_at(i as f64 / 1000.).unwrap();
        let distance = points
            .windows(2)
            .map(|w| {
                LineSegment {
                    start: w[0],
                    end: w[1],
                }
                .nearest_point(q, &t)
                .unwrap()
                .distance(q)
            })
            .fold(f64::INFINITY, f64::min);
        assert!(distance <= error);
    }
}
#[test]
fn nearest_and_tangent_snap_match_world_geometry() {
    let t = tolerance();
    let line = Curve::Line(LineSegment {
        start: p(0., 0.),
        end: p(5., 0.),
    });
    assert_eq!(line.nearest_point(p(2., 3.), &t).unwrap(), p(2., 0.));
    assert_eq!(line.nearest_point(p(8., 1.), &t).unwrap(), p(5., 0.));
    let circle = Curve::Circle(Circle {
        center: p(0., 0.),
        radius: 2.,
    });
    let query = p(4., 0.);
    let candidates = circle.snap_candidates(query, 10., &t).unwrap();
    let tangents: Vec<_> = candidates
        .iter()
        .filter(|c| c.kind == SnapKind::Tangent)
        .collect();
    assert_eq!(tangents.len(), 2);
    for candidate in tangents {
        close(candidate.point.distance(p(0., 0.)), 2., 1e-12);
        close(
            p(0., 0.)
                .vector_to(candidate.point)
                .dot(candidate.point.vector_to(query)),
            0.,
            1e-12,
        );
    }
    assert!(circle.nearest_point(p(0., 0.), &t).is_err());
    assert_eq!(
        grid_snap(p(3.1, 4.9), p(1., 1.), 2.).unwrap().point,
        p(3., 5.)
    );
}
#[test]
fn similarity_and_affine_calibration_recover_known_coordinates() {
    let t = tolerance();
    let sources = [
        p(1e6, 2e6),
        p(1e6 + 100., 2e6),
        p(1e6, 2e6 + 200.),
        p(1e6 + 30., 2e6 + 70.),
    ];
    for transform in [
        Transform2::scale(0.01, 0.01)
            .then(Transform2::rotation(0.37))
            .then(Transform2::translation(7., 11.)),
        Transform2 {
            m11: 0.02,
            m12: 0.004,
            m21: -0.001,
            m22: -0.03,
            tx: 7.,
            ty: 11.,
        },
    ] {
        let points: Vec<_> = sources
            .iter()
            .map(|source| ControlPoint {
                source: *source,
                target: transform.apply_point(*source),
            })
            .collect();
        let fit = fit_affine(&points, &t).unwrap();
        fit.require_max_residual(1e-8).unwrap();
        let independent = ControlPoint {
            source: p(1e6 + 150., 2e6 + 230.),
            target: transform.apply_point(p(1e6 + 150., 2e6 + 230.)),
        };
        fit.check_points(&[independent])
            .unwrap()
            .require_max_residual(1e-8)
            .unwrap();
        if transform.determinant() > 0. {
            fit_similarity(&points, &t)
                .unwrap()
                .require_max_residual(1e-8)
                .unwrap();
        }
    }
}
#[test]
fn calibration_degeneracy_and_conflicting_controls_are_visible() {
    let t = tolerance();
    let same = [ControlPoint {
        source: p(0., 0.),
        target: p(1., 1.),
    }; 3];
    assert!(fit_similarity(&same, &t).is_err());
    assert!(fit_affine(&same, &t).is_err());
    let collinear = [
        ControlPoint {
            source: p(0., 0.),
            target: p(0., 0.),
        },
        ControlPoint {
            source: p(1., 0.),
            target: p(1., 0.),
        },
        ControlPoint {
            source: p(2., 0.),
            target: p(2., 0.),
        },
    ];
    assert!(fit_affine(&collinear, &t).is_err());
    let mut points: Vec<_> = rectangle(0., 0., 10., 10.)
        .iter()
        .map(|p| ControlPoint {
            source: *p,
            target: *p,
        })
        .collect();
    points[3].target.x += 2.;
    let fit = fit_affine(&points, &t).unwrap();
    assert!(fit.max_residual > 0.4);
    assert!(fit.require_max_residual(0.01).is_err());
}
#[test]
fn nonfinite_degenerate_and_unknown_protocol_fields_fail() {
    let t = tolerance();
    for radius in [f64::NAN, f64::INFINITY, 0., -1., 1e-10] {
        assert!(
            Curve::Circle(Circle {
                center: p(0., 0.),
                radius
            })
            .validate(&t)
            .is_err()
        );
    }
    assert!(
        Curve::Line(LineSegment {
            start: p(0., 0.),
            end: p(0., 0.)
        })
        .validate(&t)
        .is_err()
    );
    assert!(Transform2::scale(0., 1.).inverse(&t).is_err());
    assert!(
        GeometryTolerance {
            linear: f64::NAN,
            ..t
        }
        .validate()
        .is_err()
    );
    assert!(GeometryTolerance { angular: 0., ..t }.validate().is_err());
    let encoded = serde_json::to_value(Curve::Arc(Arc {
        center: p(0., 0.),
        radius: 2.,
        start_angle: 0.,
        sweep_angle: TAU,
    }))
    .unwrap();
    assert_eq!(encoded["type"], "arc");
    assert_eq!(encoded["data"]["sweepAngle"], TAU);
    let mut invalid = encoded.clone();
    invalid["data"]["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Curve>(invalid).is_err());
    let mut invalid = encoded;
    invalid["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Curve>(invalid).is_err());
}

#[test]
fn errors_have_stable_typed_reasons_fields_and_operations() {
    let t = tolerance();
    let cases = [
        (
            Curve::Circle(Circle {
                center: p(0., 0.),
                radius: f64::NAN,
            })
            .validate(&t)
            .unwrap_err(),
            GeometryReason::NonFiniteNumber,
            "radius",
            "curve.validate",
        ),
        (
            Curve::Line(LineSegment {
                start: p(0., 0.),
                end: p(0., 0.),
            })
            .validate(&t)
            .unwrap_err(),
            GeometryReason::BelowTolerance,
            "length",
            "line.validate",
        ),
        (
            Transform2::scale(0., 1.).inverse(&t).unwrap_err(),
            GeometryReason::SingularTransform,
            "transform",
            "inverse",
        ),
        (
            Curve::Ellipse(Ellipse {
                center: p(0., 0.),
                axis_u: Vector2 { x: 2., y: 0. },
                axis_v: Vector2 { x: 0., y: 1. },
            })
            .intersections(
                &Curve::Circle(Circle {
                    center: p(0., 0.),
                    radius: 1.,
                }),
                &t,
            )
            .unwrap_err(),
            GeometryReason::IntersectionNotSupported,
            "curves",
            "intersections",
        ),
        (
            GeometryTolerance { linear: 0., ..t }
                .validate()
                .unwrap_err(),
            GeometryReason::NonPositiveValue,
            "linear",
            "tolerance.validate",
        ),
    ];
    for (error, reason, field, operation) in cases {
        assert_eq!(error.diagnostic().reason, reason);
        assert_eq!(error.diagnostic().field, field);
        assert_eq!(error.diagnostic().operation, operation);
        let encoded = serde_json::to_value(&error).unwrap();
        assert!(encoded["code"].is_string());
        assert!(encoded["details"]["reason"].is_string());
        assert!(encoded["details"]["message"].is_string());
        assert_eq!(encoded["details"]["field"], field);
        assert_eq!(
            serde_json::from_value::<GeometryError>(encoded.clone()).unwrap(),
            error
        );
        let mut invalid = encoded.clone();
        invalid["details"]["reason"] = serde_json::json!("UNKNOWN_REASON");
        assert!(serde_json::from_value::<GeometryError>(invalid).is_err());
        let mut invalid = encoded;
        invalid["details"]["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<GeometryError>(invalid).is_err());
    }
}

#[test]
fn malformed_calibration_and_large_shear_cannot_report_success() {
    let bad = CalibrationFit {
        transform: Transform2::identity(),
        residuals: vec![f64::NAN],
        max_residual: f64::NAN,
        rms_residual: f64::NAN,
    };
    assert_eq!(
        bad.require_max_residual(0.01)
            .unwrap_err()
            .diagnostic()
            .reason,
        GeometryReason::NonFiniteNumber
    );
    assert!(
        evaluate_calibration(
            Transform2::scale(0., 1.),
            &[ControlPoint {
                source: p(0., 0.),
                target: p(0., 0.)
            }]
        )
        .is_err()
    );
    let shear = Transform2 {
        m11: 1e200,
        m12: 1e200,
        m21: 0.,
        m22: 1e200,
        tx: 0.,
        ty: 0.,
    };
    assert!(shear.similarity_scale(&tolerance()).is_err());
}

#[test]
fn large_bezier_bounds_cannot_silently_lose_extrema() {
    let curve = Curve::Bezier(Bezier {
        control_points: vec![p(0., 0.), p(8e307, 0.), p(-8e307, 0.), p(0., 0.)],
    });
    let bounds = curve.bounds().unwrap();
    close(bounds.max.x / 8e307, 1. / (3_f64.sqrt() * 2.), 1e-14);
    close(bounds.min.x / 8e307, -1. / (3_f64.sqrt() * 2.), 1e-14);
    let invalid = Curve::Bezier(Bezier {
        control_points: vec![p(0., 0.), p(1e308, 0.), p(-1e308, 0.), p(0., 0.)],
    });
    assert_eq!(
        invalid.bounds().unwrap_err().diagnostic().reason,
        GeometryReason::NonFiniteNumber
    );
}
#[test]
fn huge_arc_start_angle_preserves_sweep_and_mesh_precision_is_explicit() {
    let arc = Arc {
        center: p(0., 0.),
        radius: 2.,
        start_angle: 1e100,
        sweep_angle: FRAC_PI_2,
    };
    let a = arc.point_at(0.);
    let b = arc.point_at(1.);
    close(
        p(0., 0.).vector_to(a).dot(p(0., 0.).vector_to(b)),
        0.,
        1e-12,
    );
    close(a.distance(b), 8_f64.sqrt(), 1e-12);
    let bounds = Curve::Arc(arc).bounds().unwrap();
    for point in Curve::Arc(arc).tessellate(0.001, &tolerance()).unwrap() {
        assert!(
            point.x <= bounds.max.x + 1e-12
                && point.x >= bounds.min.x - 1e-12
                && point.y <= bounds.max.y + 1e-12
                && point.y >= bounds.min.y - 1e-12
        );
    }
    let circle = Curve::Circle(Circle {
        center: p(1e20, 1e20),
        radius: 1.,
    });
    assert_eq!(
        circle
            .tessellate(0.01, &tolerance())
            .unwrap_err()
            .diagnostic()
            .reason,
        GeometryReason::NumericalErrorBudgetExceeded
    );
}
