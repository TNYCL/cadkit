//! Synthetic regression tests for boolean operations.
#![cfg(test)]

use cadkit_geom::*;
fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}
fn rect(x: f64, y: f64, w: f64, h: f64) -> Polygon {
    Polygon {
        exterior: vec![p(x, y), p(x + w, y), p(x + w, y + h), p(x, y + h)],
        holes: vec![],
    }
}
fn t() -> GeometryTolerance {
    GeometryTolerance::default()
}
fn options() -> PolygonBooleanOptions {
    PolygonBooleanOptions::default()
}
fn close(a: f64, b: f64, error: f64) {
    assert!((a - b).abs() <= error, "{a} != {b}");
}
fn valid(result: &PolygonBooleanResult) {
    for polygon in &result.geometry.polygons {
        polygon.validate(&t()).unwrap();
    }
    assert!(result.diagnostics.coordinate_error_bound <= options().max_coordinate_error);
    assert!(result.diagnostics.area_error_bound <= options().max_area_error);
}
#[test]
fn rectangles_match_union_intersection_difference_oracles() {
    let a = rect(0., 0., 10., 10.);
    let b = rect(5., 0., 10., 10.);
    for (result, area) in [
        (a.union(&b, options(), &t()).unwrap(), 150.),
        (a.intersection(&b, options(), &t()).unwrap(), 50.),
        (a.difference(&b, options(), &t()).unwrap(), 50.),
    ] {
        valid(&result);
        close(result.area, area, 1e-8);
        assert_eq!(result.geometry.polygons.len(), 1);
        assert!(!result.boundaries.is_empty());
    }
}

#[test]
fn near_parallel_edges_sharing_an_exact_endpoint_keep_their_contact() {
    let start = p(0.13, 0.27);
    let joint = p(3.731, 4.897);
    let next = p(
        joint.x + (joint.x - start.x) * 1.7 + 1e-10,
        joint.y + (joint.y - start.y) * 1.7,
    );
    let a = Polygon {
        exterior: vec![start, joint, p(start.x - 1., start.y + 1.)],
        holes: vec![],
    };
    let b = Polygon {
        exterior: vec![joint, next, p(joint.x + 1., joint.y - 1.)],
        holes: vec![],
    };
    let result = a.union(&b, options(), &t()).unwrap();
    valid(&result);
    close(
        result.area,
        a.area(&t()).unwrap() + b.area(&t()).unwrap(),
        1e-7,
    );
    assert_eq!(result.geometry.polygons.len(), 2);
}
#[test]
fn partially_outside_overlapping_footprints_are_subtracted_once() {
    let subject = rect(0., 0., 10., 10.);
    let clips = [rect(-2., 2., 6., 4.), rect(2., 3., 5., 4.)];
    // Clipped areas are 16 and 20 with shared area 2*3=6, leaving 70 from the subject area of
    // 100.
    let result = polygon_boolean(
        &[subject],
        &clips,
        PolygonBooleanOperation::Difference,
        options(),
        &t(),
    )
    .unwrap();
    valid(&result);
    close(result.area, 70., 1e-8);
    assert_eq!(result.geometry.polygons.len(), 1);
    assert!(result.geometry.polygons[0].holes.is_empty());
    let clip_sources: Vec<_> = result
        .boundaries
        .iter()
        .flat_map(|b| &b.edges)
        .flat_map(|e| &e.sources)
        .filter(|s| s.source.operand == BooleanOperand::Clip)
        .collect();
    assert!(clip_sources.iter().any(|s| s.source.polygon_index == 0));
    assert!(clip_sources.iter().any(|s| s.source.polygon_index == 1));
}
#[test]
fn l_shape_and_disconnected_difference_have_analytic_areas() {
    let subject = rect(0., 0., 10., 10.);
    let cut = rect(5., 5., 10., 10.);
    let result = subject.difference(&cut, options(), &t()).unwrap();
    close(result.area, 75., 1e-8);
    assert_eq!(result.geometry.polygons.len(), 1);
    assert_eq!(result.geometry.polygons[0].exterior.len(), 6);
    let split = subject
        .difference(&rect(4., -1., 2., 12.), options(), &t())
        .unwrap();
    valid(&split);
    assert_eq!(split.geometry.polygons.len(), 2);
    close(split.area, 80., 1e-8);
}
#[test]
fn holes_nested_islands_and_winding_are_preserved() {
    let mut subject = rect(0., 0., 10., 10.);
    subject.holes.push(rect(2., 2., 6., 6.).exterior);
    let island = rect(4., 4., 2., 2.);
    let result = subject.union(&island, options(), &t()).unwrap();
    valid(&result);
    close(result.area, 68., 1e-8);
    assert_eq!(result.geometry.polygons.len(), 2);
    assert_eq!(
        result
            .geometry
            .polygons
            .iter()
            .map(|p| p.holes.len())
            .sum::<usize>(),
        1
    );
    subject.exterior.reverse();
    subject.holes[0].reverse();
    let reversed = subject.union(&island, options(), &t()).unwrap();
    close(reversed.area, result.area, 1e-8);
    let hole = rect(0., 0., 10., 10.)
        .difference(&rect(3., 3., 4., 4.), options(), &t())
        .unwrap();
    assert_eq!(hole.geometry.polygons[0].holes.len(), 1);
    close(hole.area, 84., 1e-8);
}
#[test]
fn identical_adjacent_disjoint_and_empty_operands_are_explicit() {
    let a = rect(0., 0., 2., 2.);
    let adjacent = rect(2., 0., 2., 2.);
    let distant = rect(4., 0., 2., 2.);
    let empty = a.difference(&a, options(), &t()).unwrap();
    assert!(empty.geometry.polygons.is_empty());
    assert_eq!(empty.area, 0.);
    close(a.union(&adjacent, options(), &t()).unwrap().area, 8., 1e-8);
    assert!(
        a.intersection(&adjacent, options(), &t())
            .unwrap()
            .geometry
            .polygons
            .is_empty()
    );
    assert_eq!(
        a.union(&distant, options(), &t())
            .unwrap()
            .geometry
            .polygons
            .len(),
        2
    );
    let zero = polygon_boolean(&[], &[], PolygonBooleanOperation::Union, options(), &t()).unwrap();
    assert_eq!(zero.area, 0.);
    assert_eq!(zero.diagnostics.input_edge_count, 0);
    assert_eq!(
        polygon_boolean(
            std::slice::from_ref(&a),
            &[],
            PolygonBooleanOperation::Difference,
            options(),
            &t()
        )
        .unwrap()
        .area,
        4.
    );
}
#[test]
fn diagonal_clipping_returns_source_parameter_provenance() {
    let subject = rect(0., 0., 10., 10.);
    let triangle = Polygon {
        exterior: vec![p(5., -5.), p(15., 5.), p(5., 15.)],
        holes: vec![],
    };
    let result = subject.intersection(&triangle, options(), &t()).unwrap();
    valid(&result);
    close(result.area, 50., 1e-7);
    for boundary in &result.boundaries {
        for edge in &boundary.edges {
            assert!(!edge.sources.is_empty());
            for source in &edge.sources {
                assert!((0. ..=1.).contains(&source.start_parameter));
                assert!((0. ..=1.).contains(&source.end_parameter));
                assert!(source.max_deviation <= options().max_coordinate_error);
            }
        }
    }
    assert!(
        result
            .boundaries
            .iter()
            .flat_map(|b| &b.edges)
            .flat_map(|e| &e.sources)
            .any(|s| s.start_parameter > 0. && s.start_parameter < 1.
                || s.end_parameter > 0. && s.end_parameter < 1.)
    );
}
#[test]
fn translation_preserves_area_and_records_local_origin() {
    let tolerance = GeometryTolerance {
        linear: 1e-5,
        angular: 1e-10,
        area: 1e-10,
    };
    let opts = PolygonBooleanOptions::with_error_budget(2e-6, 0.001);
    let a = rect(0., 0., 10., 10.);
    let b = rect(3., 2., 5., 12.);
    let baseline = a.difference(&b, opts, &tolerance).unwrap();
    for offset in [p(1e6, -1e6), p(1e9, 1e9)] {
        let transform = Transform2::translation(offset.x, offset.y);
        let result = a
            .transformed(transform, &tolerance)
            .unwrap()
            .difference(
                &b.transformed(transform, &tolerance).unwrap(),
                opts,
                &tolerance,
            )
            .unwrap();
        close(result.area, baseline.area, 0.001);
        assert!(result.diagnostics.local_origin.distance(offset) < 20.);
        assert!(result.diagnostics.coordinate_error_bound <= opts.max_coordinate_error);
    }
}
#[test]
fn near_coincident_and_under_resolution_inputs_fail_with_typed_errors() {
    let a = rect(0., 0., 2., 2.);
    let near = rect(2. + 1e-10, 0., 2., 2.);
    let error = a.union(&near, options(), &t()).unwrap_err();
    assert_eq!(error.diagnostic().reason, GeometryReason::BooleanAmbiguity);
    let translated = rect(1e16, 1e16, 10., 10.);
    let error = translated.union(&translated, options(), &t()).unwrap_err();
    assert_eq!(
        error.diagnostic().reason,
        GeometryReason::NumericalErrorBudgetExceeded
    );
    let opts = PolygonBooleanOptions::with_error_budget(1e-20, 1e-12);
    let error = a.union(&rect(1., 0., 2., 2.), opts, &t()).unwrap_err();
    assert!(matches!(
        error.diagnostic().reason,
        GeometryReason::QuantizationRangeExceeded | GeometryReason::NumericalErrorBudgetExceeded
    ));
}
#[test]
fn resource_limits_and_protocol_fields_are_parseable() {
    let a = rect(0., 0., 10., 10.);
    let mut opts = options();
    opts.max_input_edges = 4;
    let error = a.union(&a, opts, &t()).unwrap_err();
    assert_eq!(error.diagnostic().reason, GeometryReason::ResourceLimit);
    assert_eq!(error.diagnostic().field, "maxInputEdges");
    opts = options();
    opts.max_pair_checks = 1;
    assert_eq!(
        a.union(&a, opts, &t()).unwrap_err().diagnostic().field,
        "maxPairChecks"
    );
    let result = a
        .difference(&rect(2., 2., 2., 2.), options(), &t())
        .unwrap();
    let encoded = serde_json::to_value(&result).unwrap();
    assert_eq!(encoded["diagnostics"]["profile"], "linear_polygon_i64_v1");
    assert_eq!(
        serde_json::from_value::<PolygonBooleanResult>(encoded.clone()).unwrap(),
        result
    );
    let mut invalid = encoded;
    invalid["diagnostics"]["silentRepair"] = serde_json::json!(true);
    assert!(serde_json::from_value::<PolygonBooleanResult>(invalid).is_err());
}
#[test]
fn duplicate_footprints_do_not_cancel_or_double_count() {
    let subject = rect(0., 0., 10., 10.);
    let cut = rect(2., 2., 3., 4.);
    let result = polygon_boolean(
        &[subject],
        &[cut.clone(), cut],
        PolygonBooleanOperation::Difference,
        options(),
        &t(),
    )
    .unwrap();
    close(result.area, 88., 1e-8);
    assert_eq!(result.geometry.polygons[0].holes.len(), 1);
    let source_sets: Vec<_> = result
        .boundaries
        .iter()
        .flat_map(|b| &b.edges)
        .map(|e| e.sources.len())
        .collect();
    assert!(source_sets.contains(&2));
}

#[test]
fn rational_diagonal_intersection_respects_declared_area_and_boundary_budgets() {
    let triangle = Polygon {
        exterior: vec![p(0., 0.), p(10., 0.), p(0., 3.)],
        holes: vec![],
    };
    let result = triangle
        .intersection(&rect(0., 1., 10., 2.), options(), &t())
        .unwrap();
    valid(&result);
    close(result.area, 20. / 3., result.diagnostics.area_error_bound);
    assert!(
        result.diagnostics.max_output_boundary_deviation
            <= result.diagnostics.coordinate_error_bound
    );
    assert!(result.diagnostics.arithmetic_area_error_bound > 0.);
}
#[test]
fn output_and_generated_edge_caps_fail_without_partial_success() {
    let a = rect(0., 0., 10., 10.);
    let b = rect(2., 2., 3., 3.);
    let mut opts = options();
    opts.max_output_edges = 3;
    assert_eq!(
        a.difference(&b, opts, &t()).unwrap_err().diagnostic().field,
        "maxOutputEdges"
    );
    opts = options();
    opts.max_generated_edges = 3;
    assert_eq!(
        a.difference(&b, opts, &t()).unwrap_err().diagnostic().field,
        "maxGeneratedEdges"
    );
}

#[test]
fn local_origin_cancellation_cannot_hide_a_world_space_gap() {
    let left = Polygon {
        exterior: vec![
            p(-10000., 0.),
            p(-1e-20, 0.),
            p(-1e-20, 10000.),
            p(-10000., 10000.),
        ],
        holes: vec![],
    };
    let right = Polygon {
        exterior: vec![
            p(1e-20, 0.),
            p(10000., 0.),
            p(10000., 10000.),
            p(1e-20, 10000.),
        ],
        holes: vec![],
    };
    let far = rect(1e16, 0., 10000., 10000.);
    let tolerance = GeometryTolerance {
        linear: 2048.,
        angular: 1e-10,
        area: 1e-12,
    };
    let opts = PolygonBooleanOptions::with_error_budget(1024., 1e22);
    let error = polygon_boolean(
        &[left, right, far],
        &[],
        PolygonBooleanOperation::Union,
        opts,
        &tolerance,
    )
    .unwrap_err();
    assert_eq!(error.diagnostic().reason, GeometryReason::BooleanAmbiguity);
}
