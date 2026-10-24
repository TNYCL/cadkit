//! JSON Schema structure remains compatible when public variants are documented.
#![cfg(all(test, feature = "schemars"))]
use cadkit_geom::*;

#[test]
fn unit_enum_schemas_keep_compact_string_enumerations() {
    {
        let schema = schemars::schema_for!(GeometryReason);
        let schema = schema.as_value();
        assert_eq!(schema["type"], "string");
        assert!(schema.get("oneOf").is_none());
        let expected = serde_json::json!([
            GeometryReason::NonFiniteNumber,
            GeometryReason::NonPositiveValue,
            GeometryReason::InsufficientPoints,
            GeometryReason::BelowTolerance,
            GeometryReason::InvalidParameterRange,
            GeometryReason::UnsupportedDegree,
            GeometryReason::DegenerateAxes,
            GeometryReason::CoincidentControlPoints,
            GeometryReason::InvalidTopology,
            GeometryReason::HoleOutsideBoundary,
            GeometryReason::IntersectingHoles,
            GeometryReason::SingularTransform,
            GeometryReason::SimilarityRequired,
            GeometryReason::AmbiguousNearestPoint,
            GeometryReason::NearestNotSupported,
            GeometryReason::MeasurementNotSupported,
            GeometryReason::IntersectionNotSupported,
            GeometryReason::AmbiguousIntersection,
            GeometryReason::EditingNotSupported,
            GeometryReason::MiterLimitExceeded,
            GeometryReason::OffsetCollapsed,
            GeometryReason::BooleanAmbiguity,
            GeometryReason::BooleanKernelFailure,
            GeometryReason::QuantizationRangeExceeded,
            GeometryReason::ResourceLimit,
            GeometryReason::NotClosed,
            GeometryReason::ApproximationLimit,
            GeometryReason::IntegrationNotConverged,
            GeometryReason::NumericalErrorBudgetExceeded,
            GeometryReason::CollinearControlPoints,
            GeometryReason::ResidualLimitExceeded
        ]);
        assert_eq!(schema["enum"], expected);
    }
    {
        let schema = schemars::schema_for!(SnapKind);
        let schema = schema.as_value();
        assert_eq!(schema["type"], "string");
        assert!(schema.get("oneOf").is_none());
        let expected = serde_json::json!([
            SnapKind::Endpoint,
            SnapKind::Midpoint,
            SnapKind::Center,
            SnapKind::Intersection,
            SnapKind::Nearest,
            SnapKind::Perpendicular,
            SnapKind::Tangent,
            SnapKind::Grid
        ]);
        assert_eq!(schema["enum"], expected);
    }
    {
        let schema = schemars::schema_for!(IntersectionKind);
        let schema = schema.as_value();
        assert_eq!(schema["type"], "string");
        assert!(schema.get("oneOf").is_none());
        let expected = serde_json::json!([
            IntersectionKind::Crossing,
            IntersectionKind::Tangent,
            IntersectionKind::Touch
        ]);
        assert_eq!(schema["enum"], expected);
    }
    {
        let schema = schemars::schema_for!(PointLocation);
        let schema = schema.as_value();
        assert_eq!(schema["type"], "string");
        assert!(schema.get("oneOf").is_none());
        let expected = serde_json::json!([
            PointLocation::Outside,
            PointLocation::Boundary,
            PointLocation::Inside
        ]);
        assert_eq!(schema["enum"], expected);
    }
    {
        let schema = schemars::schema_for!(PolygonBooleanOperation);
        let schema = schema.as_value();
        assert_eq!(schema["type"], "string");
        assert!(schema.get("oneOf").is_none());
        let expected = serde_json::json!([
            PolygonBooleanOperation::Union,
            PolygonBooleanOperation::Intersection,
            PolygonBooleanOperation::Difference
        ]);
        assert_eq!(schema["enum"], expected);
    }
    {
        let schema = schemars::schema_for!(BooleanOperand);
        let schema = schema.as_value();
        assert_eq!(schema["type"], "string");
        assert!(schema.get("oneOf").is_none());
        let expected = serde_json::json!([BooleanOperand::Subject, BooleanOperand::Clip]);
        assert_eq!(schema["enum"], expected);
    }
}
