//! Synthetic regression tests for nurbs operations.
#![cfg(test)]

use cadkit_geom::*;

fn tolerance() -> GeometryTolerance {
    GeometryTolerance::default()
}
fn bezier(points: &[[f64; 2]], weights: &[f64]) -> NurbsCurve {
    let degree = points.len() - 1;
    NurbsCurve {
        degree: degree as u8,
        control_points: points.iter().map(|p| Point2::new(p[0], p[1])).collect(),
        weights: weights.to_vec(),
        knots: std::iter::repeat_n(0., degree + 1)
            .chain(std::iter::repeat_n(1., degree + 1))
            .collect(),
        periodic: false,
    }
}
fn quarter_circle() -> NurbsCurve {
    bezier(&[[1., 0.], [1., 1.], [0., 1.]], &[1., 0.5_f64.sqrt(), 1.])
}
fn close(actual: Point2, expected: Point2, error: f64) {
    assert!(
        actual.distance(expected) <= error,
        "actual={actual:?} expected={expected:?} error={error}"
    );
}

// Cox-de Boor basis summation, independent of the implementation's knot insertion/de Casteljau
// path.
fn basis(index: usize, degree: usize, knots: &[f64], u: f64) -> f64 {
    if degree == 0 {
        return if knots[index] <= u && u < knots[index + 1] {
            1.
        } else {
            0.
        };
    }
    let a = knots[index + degree] - knots[index];
    let b = knots[index + degree + 1] - knots[index + 1];
    (if a == 0. {
        0.
    } else {
        (u - knots[index]) / a * basis(index, degree - 1, knots, u)
    }) + (if b == 0. {
        0.
    } else {
        (knots[index + degree + 1] - u) / b * basis(index + 1, degree - 1, knots, u)
    })
}
fn reference(curve: &NurbsCurve, parameter: f64) -> Point2 {
    if parameter == 1. {
        return *curve.control_points.last().unwrap();
    }
    let start = curve.knots[curve.degree as usize];
    let u = start + (curve.knots[curve.control_points.len()] - start) * parameter;
    let mut denominator = 0.;
    let mut x = 0.;
    let mut y = 0.;
    for (i, p) in curve.control_points.iter().enumerate() {
        let b = basis(i, curve.degree as usize, &curve.knots, u) * curve.weights[i];
        denominator += b;
        x += p.x * b;
        y += p.y * b;
    }
    Point2::new(x / denominator, y / denominator)
}

#[test]
fn rational_quarter_circle_has_exact_endpoints_and_known_midpoint_tangent() {
    let curve = quarter_circle();
    let t = tolerance();
    assert_eq!(curve.point_at(0., &t).unwrap(), Point2::new(1., 0.));
    assert_eq!(curve.point_at(1., &t).unwrap(), Point2::new(0., 1.));
    close(
        curve.point_at(0.5, &t).unwrap(),
        Point2::new(0.5_f64.sqrt(), 0.5_f64.sqrt()),
        1e-13,
    );
    let tangent = curve.derivative_at(0., &t).unwrap();
    assert!(tangent.x.abs() < 1e-13);
    assert!((tangent.y - 2_f64.sqrt()).abs() < 1e-13);
    for i in 0..=100 {
        let p = curve.point_at(i as f64 / 100., &t).unwrap();
        assert!((p.x.hypot(p.y) - 1.).abs() < 1e-13);
    }
}

#[test]
fn polynomial_quadratic_and_cubic_match_independent_power_basis() {
    let q = bezier(&[[0., 0.], [1., 2.], [3., 1.]], &[1.; 3]);
    let c = bezier(&[[0., 1.], [1., -1.], [2., 3.], [4., 2.]], &[1.; 4]);
    for i in 0..=100 {
        let s = i as f64 / 100.;
        close(
            q.point_at(s, &tolerance()).unwrap(),
            Point2::new(2. * s + s * s, 4. * s - 3. * s * s),
            1e-12,
        );
        let d = q.derivative_at(s, &tolerance()).unwrap();
        close(
            Point2::new(d.x, d.y),
            Point2::new(2. + 2. * s, 4. - 6. * s),
            1e-12,
        );
        close(
            c.point_at(s, &tolerance()).unwrap(),
            Point2::new(
                3. * s + s * s * s,
                1. - 6. * s + 18. * s * s - 11. * s * s * s,
            ),
            1e-12,
        );
        let d = c.derivative_at(s, &tolerance()).unwrap();
        close(
            Point2::new(d.x, d.y),
            Point2::new(3. + 3. * s * s, -6. + 36. * s - 33. * s * s),
            1e-12,
        );
    }
}

#[test]
fn nonuniform_repeated_knots_match_independent_bspline_basis() {
    for (degree, knots, weights) in [
        (
            2,
            vec![10., 10., 10., 11., 13., 13., 15., 15., 15.],
            vec![1., 2., 0.7, 1.2, 3., 1.],
        ),
        (
            3,
            vec![-4., -4., -4., -4., -1., -1., 2., 2., 2., 2.],
            vec![1., 1.2, 0.8, 3., 2., 1.],
        ),
    ] {
        let curve = NurbsCurve {
            degree,
            control_points: vec![
                Point2::new(0., 0.),
                Point2::new(1., 2.),
                Point2::new(2., -1.),
                Point2::new(3., 3.),
                Point2::new(4., 1.),
                Point2::new(5., 0.),
            ],
            weights,
            knots,
            periodic: false,
        };
        curve.validate(&tolerance()).unwrap();
        for i in 0..=200 {
            let s = i as f64 / 200.;
            close(
                curve.point_at(s, &tolerance()).unwrap(),
                reference(&curve, s),
                1e-11,
            );
        }
        let result = curve
            .tessellate_with_options(NurbsTessellationOptions::with_error(1e-3), &tolerance())
            .unwrap();
        assert_eq!(result.bezier_spans, if degree == 2 { 3 } else { 2 });
        assert!(result.max_error_bound <= 1e-3);
    }
}

#[test]
fn c0_knots_preserve_corner_and_reject_ambiguous_derivative() {
    let curve = NurbsCurve {
        degree: 2,
        control_points: vec![
            Point2::new(0., 0.),
            Point2::new(1., 0.),
            Point2::new(2., 0.),
            Point2::new(2., 1.),
            Point2::new(2., 2.),
        ],
        weights: vec![1.; 5],
        knots: vec![0., 0., 0., 0.5, 0.5, 1., 1., 1.],
        periodic: false,
    };
    close(
        curve.point_at(0.5, &tolerance()).unwrap(),
        Point2::new(2., 0.),
        1e-13,
    );
    assert_eq!(
        curve
            .derivative_at(0.5, &tolerance())
            .unwrap_err()
            .diagnostic()
            .reason,
        GeometryReason::InvalidParameterRange
    );
    close(
        curve.point_at(0.5 - 1e-5, &tolerance()).unwrap(),
        reference(&curve, 0.5 - 1e-5),
        1e-12,
    );
    let points = curve.tessellate(1e-6, &tolerance()).unwrap();
    assert!(
        points
            .iter()
            .any(|p| p.distance(Point2::new(2., 0.)) < 1e-12)
    );
}

#[test]
fn smooth_interior_knot_derivative_matches_finite_difference_oracle() {
    let curve = NurbsCurve {
        degree: 3,
        control_points: vec![
            Point2::new(0., 0.),
            Point2::new(1., 1.),
            Point2::new(2., -2.),
            Point2::new(3., 2.),
            Point2::new(4., 0.),
        ],
        weights: vec![1.; 5],
        knots: vec![0., 0., 0., 0., 0.5, 1., 1., 1., 1.],
        periodic: false,
    };
    let a = reference(&curve, 0.5 - 1e-5);
    let b = reference(&curve, 0.5 + 1e-5);
    let d = curve.derivative_at(0.5, &tolerance()).unwrap();
    close(
        Point2::new(d.x, d.y),
        Point2::new((b.x - a.x) / 2e-5, (b.y - a.y) / 2e-5),
        1e-7,
    );
}

#[test]
fn bounds_are_explicit_conservative_control_hull() {
    let curve = quarter_circle();
    let bounds = curve.bounds(&tolerance()).unwrap();
    assert_eq!(bounds.min, Point2::new(0., 0.));
    assert_eq!(bounds.max, Point2::new(1., 1.));
    for i in 0..=100 {
        let p = reference(&curve, i as f64 / 100.);
        assert!(
            p.x >= bounds.min.x
                && p.x <= bounds.max.x
                && p.y >= bounds.min.y
                && p.y <= bounds.max.y
        );
    }
}

fn segment_distance(point: Point2, a: Point2, b: Point2) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let square = dx * dx + dy * dy;
    let fraction = if square == 0. {
        0.
    } else {
        ((point.x - a.x) * dx + (point.y - a.y) * dy) / square
    }
    .clamp(0., 1.);
    point.distance(Point2::new(a.x + fraction * dx, a.y + fraction * dy))
}
#[test]
fn certified_tessellation_encloses_independent_rational_samples() {
    for curve in [
        quarter_circle(),
        bezier(
            &[[0., 0.], [0., 5.], [5., -5.], [5., 0.]],
            &[1., 0.1, 10., 1.],
        ),
    ] {
        let result = curve
            .tessellate_with_options(NurbsTessellationOptions::with_error(2e-4), &tolerance())
            .unwrap();
        assert!(result.max_error_bound > 0. && result.max_error_bound <= 2e-4);
        assert!(result.subdivisions > 0);
        for i in 0..=1000 {
            let point = reference(&curve, i as f64 / 1000.);
            let nearest = result
                .points
                .windows(2)
                .map(|p| segment_distance(point, p[0], p[1]))
                .fold(f64::INFINITY, f64::min);
            assert!(
                nearest <= result.max_error_bound,
                "distance={nearest} bound={}",
                result.max_error_bound
            );
        }
    }
}

#[test]
fn degree_one_positive_weights_trace_the_same_segment() {
    let curve = bezier(&[[2., 3.], [6., 7.]], &[1., 3.]);
    close(
        curve.point_at(0.5, &tolerance()).unwrap(),
        Point2::new(5., 6.),
        1e-12,
    );
    assert_eq!(curve.tessellate(1e-8, &tolerance()).unwrap().len(), 2);
}

#[test]
fn large_coordinate_interval_budget_is_enforced_without_silent_rounding() {
    let mut curve = quarter_circle();
    for point in &mut curve.control_points {
        point.x += 1e12;
        point.y -= 1e12;
    }
    let broad = GeometryTolerance {
        linear: 0.01,
        ..tolerance()
    };
    let point = curve.point_at(0.5, &broad).unwrap();
    close(
        point,
        Point2::new(1e12 + 0.5_f64.sqrt(), -1e12 + 0.5_f64.sqrt()),
        0.001,
    );
    let result = curve
        .tessellate_with_options(NurbsTessellationOptions::with_error(0.01), &broad)
        .unwrap();
    assert!(result.max_error_bound <= 0.01);
    assert_eq!(
        curve
            .point_at(0.5, &tolerance())
            .unwrap_err()
            .diagnostic()
            .reason,
        GeometryReason::NumericalErrorBudgetExceeded
    );
    assert_eq!(
        curve
            .tessellate(1e-8, &tolerance())
            .unwrap_err()
            .diagnostic()
            .reason,
        GeometryReason::NumericalErrorBudgetExceeded
    );
}

#[test]
fn unsupported_profiles_and_malformed_data_have_typed_diagnostics() {
    let good = quarter_circle();
    let mut cases = Vec::new();
    let mut curve = good.clone();
    curve.periodic = true;
    cases.push((curve, "periodic", GeometryReason::InvalidTopology));
    let mut curve = good.clone();
    curve.degree = 4;
    cases.push((curve, "degree", GeometryReason::UnsupportedDegree));
    let mut curve = good.clone();
    curve.weights[1] = 0.;
    cases.push((curve, "weights", GeometryReason::NonPositiveValue));
    let mut curve = good.clone();
    curve.weights[1] = -1.;
    cases.push((curve, "weights", GeometryReason::NonPositiveValue));
    let mut curve = good.clone();
    curve.weights[1] = f64::INFINITY;
    cases.push((curve, "weights", GeometryReason::NonFiniteNumber));
    let mut curve = good.clone();
    curve.control_points[1].x = f64::NAN;
    cases.push((curve, "controlPoints", GeometryReason::NonFiniteNumber));
    let mut curve = good.clone();
    curve.knots[0] = -1.;
    cases.push((curve, "knots", GeometryReason::InvalidTopology));
    let mut curve = good.clone();
    curve.knots.fill(0.);
    cases.push((curve, "knots", GeometryReason::InvalidParameterRange));
    let mut curve = good.clone();
    curve.knots[2] = 2.;
    cases.push((curve, "knots", GeometryReason::InvalidParameterRange));
    let mut curve = good.clone();
    curve.weights.pop();
    cases.push((curve, "weightsOrKnots", GeometryReason::InvalidTopology));
    for (curve, field, reason) in cases {
        let error = curve.validate(&tolerance()).unwrap_err();
        assert_eq!(error.diagnostic().reason, reason);
        assert_eq!(error.diagnostic().field, field);
        assert_eq!(error.diagnostic().operation, "nurbs.validate");
    }
    for parameter in [-0.1, 1.1, f64::NAN] {
        assert!(good.point_at(parameter, &tolerance()).is_err());
    }
}

#[test]
fn explicit_resource_and_iteration_limits_never_return_partial_success() {
    let curve = quarter_circle();
    for options in [
        NurbsTessellationOptions {
            max_subdivisions: 0,
            ..NurbsTessellationOptions::with_error(1e-6)
        },
        NurbsTessellationOptions {
            max_depth: 0,
            ..NurbsTessellationOptions::with_error(1e-6)
        },
    ] {
        assert_eq!(
            curve
                .tessellate_with_options(options, &tolerance())
                .unwrap_err()
                .diagnostic()
                .reason,
            GeometryReason::ApproximationLimit
        );
    }
    assert_eq!(
        curve
            .tessellate_with_options(
                NurbsTessellationOptions {
                    max_segments: 1,
                    ..NurbsTessellationOptions::with_error(1e-4)
                },
                &tolerance()
            )
            .unwrap_err()
            .diagnostic()
            .reason,
        GeometryReason::ResourceLimit
    );
    let mut large = curve.clone();
    large.control_points = vec![Point2::new(0., 0.); 4097];
    assert_eq!(
        large
            .validate(&tolerance())
            .unwrap_err()
            .diagnostic()
            .reason,
        GeometryReason::ResourceLimit
    );
    let mut extreme = curve;
    extreme.weights[1] = f64::from_bits(1);
    extreme.weights[0] = f64::MAX;
    assert_eq!(
        extreme
            .tessellate(1e-4, &tolerance())
            .unwrap_err()
            .diagnostic()
            .reason,
        GeometryReason::NumericalErrorBudgetExceeded
    );
}

#[test]
fn strict_serialization_and_schema_preserve_nurbs_curve_payload() {
    let curve = quarter_circle();
    let json = serde_json::to_value(&curve).unwrap();
    assert!(json.get("controlPoints").is_some());
    assert_eq!(
        serde_json::from_value::<NurbsCurve>(json.clone()).unwrap(),
        curve
    );
    let mut bad = json;
    bad["extra"] = serde_json::json!(true);
    assert!(serde_json::from_value::<NurbsCurve>(bad).is_err());
    #[cfg(feature = "schemars")]
    assert!(schemars::schema_for!(NurbsCurve).as_value()["properties"]["weights"].is_object());
    let error = curve.derivative_at(-1., &tolerance()).unwrap_err();
    let json = serde_json::to_value(&error).unwrap();
    assert_eq!(json["details"]["reason"], "INVALID_PARAMETER_RANGE");
    assert_eq!(
        serde_json::from_value::<Curve>(serde_json::json!({"type":"nurbs","data":curve.clone()}))
            .unwrap(),
        Curve::Nurbs(curve)
    );
}

#[test]
fn normalized_derivative_is_invariant_under_knot_domain_affine_change() {
    let mut curve = bezier(&[[0., 0.], [1., 2.], [3., 1.]], &[1.; 3]);
    for knot in &mut curve.knots {
        *knot = 10. + 7. * *knot;
    }
    for parameter in [0., 0.1, 0.5, 0.9, 1.] {
        let d = curve.derivative_at(parameter, &tolerance()).unwrap();
        close(
            Point2::new(d.x, d.y),
            Point2::new(2. + 2. * parameter, 4. - 6. * parameter),
            1e-11,
        );
    }
}

#[test]
fn multiple_span_extractions_match_basis_oracle_for_all_supported_degrees() {
    for degree in 1..=3u8 {
        for variant in 0..9usize {
            let mut knots = vec![0.; degree as usize + 1];
            for (i, knot) in [0.07, 0.2, 0.61, 0.83].into_iter().enumerate() {
                knots.extend(std::iter::repeat_n(
                    knot,
                    1 + (i + variant) % degree as usize,
                ));
            }
            knots.extend(std::iter::repeat_n(1., degree as usize + 1));
            let count = knots.len() - degree as usize - 1;
            let curve = NurbsCurve {
                degree,
                control_points: (0..count)
                    .map(|i| Point2::new(i as f64, ((i * 7 + variant * 3) % 11) as f64 - 5.))
                    .collect(),
                weights: (0..count)
                    .map(|i| 0.25 + ((i * 11 + variant) % 9) as f64 / 3.)
                    .collect(),
                knots,
                periodic: false,
            };
            for i in 0..=60 {
                let parameter = i as f64 / 60.;
                close(
                    curve.point_at(parameter, &tolerance()).unwrap(),
                    reference(&curve, parameter),
                    1e-10,
                );
            }
            let result = curve
                .tessellate_with_options(NurbsTessellationOptions::with_error(0.005), &tolerance())
                .unwrap();
            assert_eq!(result.bezier_spans, 5);
            for i in 0..=120 {
                let point = reference(&curve, i as f64 / 120.);
                let nearest = result
                    .points
                    .windows(2)
                    .map(|p| segment_distance(point, p[0], p[1]))
                    .fold(f64::INFINITY, f64::min);
                assert!(nearest <= result.max_error_bound);
            }
        }
    }
}

#[test]
fn discontinuous_interior_knots_are_rejected_before_tessellation() {
    let curve = NurbsCurve {
        degree: 2,
        control_points: vec![
            Point2::new(0., 0.),
            Point2::new(1., 0.),
            Point2::new(2., 0.),
            Point2::new(5., 0.),
            Point2::new(6., 0.),
            Point2::new(7., 0.),
        ],
        weights: vec![1.; 6],
        knots: vec![0., 0., 0., 0.5, 0.5, 0.5, 1., 1., 1.],
        periodic: false,
    };
    let error = curve.tessellate(0.01, &tolerance()).unwrap_err();
    assert!(matches!(error, GeometryError::Unsupported(_)));
    assert_eq!(error.diagnostic().reason, GeometryReason::InvalidTopology);
    assert_eq!(error.diagnostic().field, "knots");
}

#[test]
fn common_weight_scale_preserves_shape_and_coincident_controls_fail() {
    let reference_curve = quarter_circle();
    for scale in [1e-200, 1e200] {
        let mut curve = reference_curve.clone();
        for weight in &mut curve.weights {
            *weight *= scale;
        }
        close(
            curve.point_at(0.37, &tolerance()).unwrap(),
            reference(&reference_curve, 0.37),
            1e-12,
        );
        assert!(curve.tessellate(0.001, &tolerance()).is_ok());
    }
    let mut curve = reference_curve;
    curve.control_points.fill(Point2::new(2., 3.));
    assert_eq!(
        curve
            .validate(&tolerance())
            .unwrap_err()
            .diagnostic()
            .reason,
        GeometryReason::CoincidentControlPoints
    );
}
