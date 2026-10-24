//! Independent analytic geometry using `f64` coordinates in a caller-defined Cartesian plane.
//!
//! Lengths use caller units, areas use squared units, and angles are radians,
//! positive counterclockwise from +x. Rings close implicitly without a repeated
//! first vertex. Tolerances are explicit; display tessellation is not a measurement
//! source. Enable `schemars` for JSON Schema generation. Serde support is always available.
//!
//! ```
//! use cadkit_geom::{GeometryTolerance, LineSegment, Point2};
//! let segment = LineSegment { start: Point2::new(0.0, 0.0), end: Point2::new(4.0, 0.0) };
//! let offset = segment.offset(1.0, &GeometryTolerance::default())?;
//! assert_eq!(offset.start, Point2::new(0.0, 1.0));
//! # Ok::<(), cadkit_geom::GeometryError>(())
//! ```

mod boolean;
mod calibration;
mod contacts;
mod curves;
mod editing;
mod intersections;
mod nearest;
mod nurbs;
mod polygon;
#[cfg(test)]
mod scan_equivalence;

pub use boolean::*;
pub use calibration::*;
pub use contacts::*;
pub use curves::*;
pub use editing::*;
pub use intersections::*;
pub use nearest::*;
pub use nurbs::*;
pub use polygon::*;
use serde::{Deserialize, Serialize};

/// Result of an operation in caller-defined coordinates, carrying a typed diagnostic on failure.
pub type GeometryResult<T> = Result<T, GeometryError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// Stable, unit-independent reason codes; coordinate and tolerance context belongs to the
/// diagnostic's operation.
#[cfg_attr(feature = "schemars", schemars(transform = crate::unit_enum_schema))]
pub enum GeometryReason {
    /// A scalar is NaN or infinite.
    NonFiniteNumber,
    /// A required positive scalar is zero or negative.
    NonPositiveValue,
    /// The input lacks the required number of points.
    InsufficientPoints,
    /// A length or area is at or below the operation's tolerance.
    BelowTolerance,
    /// A parameter or option is outside its supported range.
    InvalidParameterRange,
    /// Polynomial degree is outside the supported profile.
    UnsupportedDegree,
    /// Axes lack sufficient length or angular independence.
    DegenerateAxes,
    /// Control points coincide within the linear tolerance.
    CoincidentControlPoints,
    /// The geometry structure or topology is invalid for the operation.
    InvalidTopology,
    /// A hole is not strictly contained inside the exterior.
    HoleOutsideBoundary,
    /// Holes intersect, touch or contain one another.
    IntersectingHoles,
    /// A transform is singular or numerically unstable.
    SingularTransform,
    /// This operation requires uniform scale and orthogonal axes.
    SimilarityRequired,
    /// A unique nearest point cannot be selected under the current profile.
    AmbiguousNearestPoint,
    /// Nearest-point evaluation is unsupported for this profile.
    NearestNotSupported,
    /// Certified analytic measurement is not implemented for this profile.
    MeasurementNotSupported,
    /// The curve pair has no supported intersection algorithm.
    IntersectionNotSupported,
    /// The tolerance cannot distinguish the intersection topology.
    AmbiguousIntersection,
    /// The editing operation does not support this geometry profile.
    EditingNotSupported,
    /// The dimensionless miter ratio exceeds the selected limit.
    MiterLimitExceeded,
    /// An offset collapses or reverses an edge.
    OffsetCollapsed,
    /// Boolean topology cannot be certified within the requested budgets.
    BooleanAmbiguity,
    /// The boolean dependency failed or produced invalid provenance.
    BooleanKernelFailure,
    /// The requested grid cannot fit the safe integer coordinate range.
    QuantizationRangeExceeded,
    /// An explicit resource count limit was exceeded.
    ResourceLimit,
    /// The geometry does not define an enclosed area.
    NotClosed,
    /// Approximation did not converge within its resource budget.
    ApproximationLimit,
    /// Numerical integration did not converge within its error budget.
    IntegrationNotConverged,
    /// Numerical uncertainty exceeds the requested coordinate or area budget.
    NumericalErrorBudgetExceeded,
    /// Control pairs cannot determine a stable affine fit.
    CollinearControlPoints,
    /// Calibration residual exceeds the caller's target-unit limit.
    ResidualLimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Structured error context. Codes and field paths are stable; messages are English. Numerical
/// units follow the failing operation.
pub struct GeometryDiagnostic {
    /// Stable reason code, independent of units.
    pub reason: GeometryReason,
    /// Machine-readable path to the failing field; coordinate units follow the operation.
    pub field: String,
    /// Stable operation identifier; no coordinate conversion is implied.
    pub operation: String,
    /// Human-readable English diagnostic; not a machine-readable reason code.
    pub message: String,
}
impl std::fmt::Display for GeometryDiagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(
    tag = "code",
    content = "details",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
/// Invalid input or an unsupported operation; diagnostics retain the operation's coordinate and
/// tolerance context.
pub enum GeometryError {
    #[error("Invalid geometry: {0}")]
    /// Invalid geometry or numerical budget failure; units follow the diagnostic operation.
    Invalid(GeometryDiagnostic),
    #[error("Unsupported geometry operation: {0}")]
    /// Unsupported profile or exhausted algorithm/resource budget; units follow the diagnostic
    /// operation.
    Unsupported(GeometryDiagnostic),
}
impl GeometryError {
    /// Construct an invalid-input diagnostic; numerical units follow the supplied operation.
    pub fn invalid(
        reason: GeometryReason,
        field: &str,
        operation: &str,
        message: impl Into<String>,
    ) -> Self {
        Self::Invalid(GeometryDiagnostic {
            reason,
            field: field.into(),
            operation: operation.into(),
            message: message.into(),
        })
    }
    /// Construct an unsupported-operation diagnostic; numerical units follow the supplied
    /// operation.
    pub fn unsupported(
        reason: GeometryReason,
        field: &str,
        operation: &str,
        message: impl Into<String>,
    ) -> Self {
        Self::Unsupported(GeometryDiagnostic {
            reason,
            field: field.into(),
            operation: operation.into(),
            message: message.into(),
        })
    }
    /// Return structured diagnostic context without changing its units or stable reason.
    pub fn diagnostic(&self) -> &GeometryDiagnostic {
        match self {
            Self::Invalid(detail) | Self::Unsupported(detail) => detail,
        }
    }
    /// Attach field and operation context to a numerical diagnostic without changing its
    /// reason or units.
    pub fn at(mut self, field: &str, operation: &str) -> Self {
        let detail = match &mut self {
            Self::Invalid(detail) | Self::Unsupported(detail) => detail,
        };
        detail.field = field.into();
        detail.operation = operation.into();
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Independent positive finite budgets: coordinate units for length, radians for angles, squared
/// units for area. Defaults are numerical starting values, not physical accuracy guarantees.
pub struct GeometryTolerance {
    /// Positive finite length tolerance in caller coordinate units; default 1e-8.
    pub linear: f64,
    /// Positive finite angular tolerance in radians; default 1e-10.
    pub angular: f64,
    /// Positive finite minimum valid area in squared coordinate units; default 1e-12. Boolean total
    /// area error is a separate budget.
    pub area: f64,
}

impl Default for GeometryTolerance {
    fn default() -> Self {
        Self {
            linear: 1e-8,
            angular: 1e-10,
            area: 1e-12,
        }
    }
}

impl GeometryTolerance {
    /// Validate finite values and the supported profile. Where supplied, tolerances use caller
    /// units, radians and squared units; no repair or unit conversion is performed.
    pub fn validate(self) -> GeometryResult<()> {
        for (field, value) in [
            ("linear", self.linear),
            ("angular", self.angular),
            ("area", self.area),
        ] {
            positive(value, "Tolerance").map_err(|error| error.at(field, "tolerance.validate"))?;
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
/// A point in the caller's Cartesian coordinate units. Construction is unchecked; validate before
/// operations requiring finite values.
pub struct Point2 {
    /// Cartesian x component in caller coordinate units.
    pub x: f64,
    /// Cartesian y component in caller coordinate units.
    pub y: f64,
}

impl Point2 {
    /// Construct a Cartesian point in caller units without validation or tolerance adjustment.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    /// Validate finite values and the supported profile. Where supplied, tolerances use caller
    /// units, radians and squared units; no repair or unit conversion is performed.
    pub fn validate(self) -> GeometryResult<()> {
        finite(self.x).map_err(|error| error.at("x", "point.validate"))?;
        finite(self.y).map_err(|error| error.at("y", "point.validate"))
    }
    /// Euclidean distance in caller coordinate units; raw arithmetic applies no tolerance.
    pub fn distance(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
    /// Interpolate with dimensionless fraction t; 0 selects self and 1 selects other. Extrapolation
    /// is permitted; no validation or tolerance is applied.
    pub fn lerp(self, other: Self, t: f64) -> Self {
        Self::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
        )
    }
    /// Translate by a displacement in the same Cartesian units; no validation or tolerance is
    /// applied.
    pub fn translated(self, delta: Vector2) -> Self {
        Self::new(self.x + delta.x, self.y + delta.y)
    }
    /// Displacement from self to other in the same Cartesian units; no tolerance is applied.
    pub fn vector_to(self, other: Self) -> Vector2 {
        Vector2 {
            x: other.x - self.x,
            y: other.y - self.y,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
/// Cartesian displacement in caller units. Raw arithmetic is unchecked and applies no tolerance.
pub struct Vector2 {
    /// Cartesian x component in caller coordinate units.
    pub x: f64,
    /// Cartesian y component in caller coordinate units.
    pub y: f64,
}

impl Vector2 {
    /// Length in caller coordinate units. Where supplied, the tolerance controls validation and
    /// numerical integration; raw segment/vector length applies no tolerance.
    pub fn length(self) -> f64 {
        self.x.hypot(self.y)
    }
    /// Dot product in squared coordinate units; no normalization or tolerance is applied.
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }
    /// Signed 2D cross product in squared units, positive for a counterclockwise turn; no tolerance
    /// is applied.
    pub fn cross(self, other: Self) -> f64 {
        self.x * other.y - self.y * other.x
    }
    /// Multiply by a dimensionless factor, retaining coordinate units; no validation is applied.
    pub fn scaled(self, scale: f64) -> Self {
        Self {
            x: self.x * scale,
            y: self.y * scale,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
/// Inclusive axis-aligned Cartesian bounds in caller units, with no implicit tolerance padding.
pub struct Bounds2 {
    /// Inclusive lower Cartesian corner in caller units.
    pub min: Point2,
    /// Inclusive upper Cartesian corner in caller units.
    pub max: Point2,
}

impl Bounds2 {
    /// Compute inclusive Cartesian bounds of a nonempty finite point set, in its coordinate units,
    /// without tolerance padding.
    pub fn from_points(points: &[Point2]) -> GeometryResult<Self> {
        let first = *points.first().ok_or_else(|| {
            GeometryError::invalid(
                GeometryReason::InsufficientPoints,
                "points",
                "bounds",
                "Empty point set",
            )
        })?;
        first.validate()?;
        let mut bounds = Self {
            min: first,
            max: first,
        };
        for p in points {
            p.validate()?;
            bounds.min.x = bounds.min.x.min(p.x);
            bounds.min.y = bounds.min.y.min(p.y);
            bounds.max.x = bounds.max.x.max(p.x);
            bounds.max.y = bounds.max.y.max(p.y);
        }
        Ok(bounds)
    }
    /// Test inclusive bounds in caller coordinates without tolerance padding or input validation.
    pub fn contains(self, p: Point2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
    /// Test inclusive box overlap in the same coordinate system without tolerance padding.
    pub fn intersects(self, other: Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
/// Affine Cartesian transform: x'=m11*x+m12*y+tx, y'=m21*x+m22*y+ty. Linear coefficients are
/// dimensionless; translation uses target units.
pub struct Transform2 {
    /// Dimensionless coefficient mapping input x to output x.
    pub m11: f64,
    /// Dimensionless coefficient mapping input y to output x.
    pub m12: f64,
    /// Dimensionless coefficient mapping input x to output y.
    pub m21: f64,
    /// Dimensionless coefficient mapping input y to output y.
    pub m22: f64,
    /// Translation along output x, in target coordinate units.
    pub tx: f64,
    /// Translation along output y, in target coordinate units.
    pub ty: f64,
}

impl Default for Transform2 {
    fn default() -> Self {
        Self::identity()
    }
}
impl Transform2 {
    /// Identity transform in the current Cartesian coordinate system, with dimensionless unit
    /// scale.
    pub const fn identity() -> Self {
        Self {
            m11: 1.,
            m12: 0.,
            m21: 0.,
            m22: 1.,
            tx: 0.,
            ty: 0.,
        }
    }
    /// Translation by x and y in target coordinate units; no tolerance or validation is applied.
    pub fn translation(x: f64, y: f64) -> Self {
        Self {
            tx: x,
            ty: y,
            ..Self::identity()
        }
    }
    /// Scale axes by dimensionless factors; negative factors reflect, and zero is allowed at
    /// construction but rejected by inversion.
    pub fn scale(x: f64, y: f64) -> Self {
        Self {
            m11: x,
            m22: y,
            ..Self::identity()
        }
    }
    /// Counterclockwise rotation about the origin by angle radians; no tolerance or validation is
    /// applied.
    pub fn rotation(angle: f64) -> Self {
        let (s, c) = angle.sin_cos();
        Self {
            m11: c,
            m12: -s,
            m21: s,
            m22: c,
            ..Self::identity()
        }
    }
    /// Map a Cartesian point to target coordinates, including translation; no tolerance or
    /// validation is applied.
    pub fn apply_point(self, p: Point2) -> Point2 {
        Point2::new(
            self.m11 * p.x + self.m12 * p.y + self.tx,
            self.m21 * p.x + self.m22 * p.y + self.ty,
        )
    }
    /// Map a displacement to target units using only the linear part; no tolerance or validation is
    /// applied.
    pub fn apply_vector(self, v: Vector2) -> Vector2 {
        Vector2 {
            x: self.m11 * v.x + self.m12 * v.y,
            y: self.m21 * v.x + self.m22 * v.y,
        }
    }
    /// Dimensionless determinant of the linear part; negative values reverse orientation. No
    /// validation is applied.
    pub fn determinant(self) -> f64 {
        self.m11 * self.m22 - self.m12 * self.m21
    }
    /// Validate finite values and the supported profile. Where supplied, tolerances use caller
    /// units, radians and squared units; no repair or unit conversion is performed.
    pub fn validate(self) -> GeometryResult<()> {
        for (field, value) in [
            ("m11", self.m11),
            ("m12", self.m12),
            ("m21", self.m21),
            ("m22", self.m22),
            ("tx", self.tx),
            ("ty", self.ty),
        ] {
            finite(value).map_err(|error| error.at(field, "transform.validate"))?;
        }
        Ok(())
    }
    /// Invert the affine mapping after finite-value and conditioning checks using the supplied
    /// tolerance; returns coordinates in the original source units.
    pub fn inverse(self, tolerance: &GeometryTolerance) -> GeometryResult<Self> {
        self.validate()?;
        tolerance.validate()?;
        let d = self.determinant();
        let scale = self
            .m11
            .abs()
            .max(self.m12.abs())
            .max(self.m21.abs())
            .max(self.m22.abs());
        if scale == 0. || !d.is_finite() || d.abs() <= f64::EPSILON * scale * scale * 16. {
            return Err(GeometryError::invalid(
                GeometryReason::SingularTransform,
                "transform",
                "inverse",
                "Singular or unstable transform",
            ));
        }
        let mut inv = Self {
            m11: self.m22 / d,
            m12: -self.m12 / d,
            m21: -self.m21 / d,
            m22: self.m11 / d,
            tx: 0.,
            ty: 0.,
        };
        inv.tx = -(inv.m11 * self.tx + inv.m12 * self.ty);
        inv.ty = -(inv.m21 * self.tx + inv.m22 * self.ty);
        inv.validate()?;
        Ok(inv)
    }
    /// Compose this transform followed by next; both act in the same coordinate convention.
    pub fn then(self, next: Self) -> Self {
        Self {
            m11: next.m11 * self.m11 + next.m12 * self.m21,
            m12: next.m11 * self.m12 + next.m12 * self.m22,
            m21: next.m21 * self.m11 + next.m22 * self.m21,
            m22: next.m21 * self.m12 + next.m22 * self.m22,
            tx: next.m11 * self.tx + next.m12 * self.ty + next.tx,
            ty: next.m21 * self.tx + next.m22 * self.ty + next.ty,
        }
    }
    /// Return the positive dimensionless scale if axes define a similarity within the supplied
    /// angular tolerance; reflection is permitted.
    pub fn similarity_scale(self, t: &GeometryTolerance) -> GeometryResult<f64> {
        self.validate()?;
        t.validate()?;
        let a = Vector2 {
            x: self.m11,
            y: self.m21,
        };
        let b = Vector2 {
            x: self.m12,
            y: self.m22,
        };
        let la = a.length();
        let lb = b.length();
        positive(la, "Scale")?;
        positive(lb, "Scale")?;
        if (la - lb).abs() > t.angular * la.max(lb)
            || a.scaled(1. / la).dot(b.scaled(1. / lb)).abs() > t.angular
        {
            return Err(GeometryError::unsupported(
                GeometryReason::SimilarityRequired,
                "transform",
                "similarity_scale",
                "This curve requires a similarity transform",
            ));
        }
        Ok(la)
    }
}

/// Reject NaN and infinities without changing the scalar or its units; no tolerance is applied.
pub fn finite(value: f64) -> GeometryResult<()> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(GeometryError::invalid(
            GeometryReason::NonFiniteNumber,
            "value",
            "finite",
            "Number must be finite",
        ))
    }
}
/// Require a finite, strictly positive scalar in its original units. The label appears in the
/// English error message; no tolerance is applied.
pub fn positive(value: f64, label: &str) -> GeometryResult<()> {
    finite(value)?;
    if value > 0. {
        Ok(())
    } else {
        Err(GeometryError::invalid(
            GeometryReason::NonPositiveValue,
            "value",
            "positive",
            format!("{label} must be positive"),
        ))
    }
}

// Internal shape invariants are checked again at access sites so malformed data
// cannot turn an indexing mistake into a panic. Existing public validation runs first.
fn missing_geometry() -> GeometryError {
    GeometryError::invalid(
        GeometryReason::InvalidTopology,
        "structure",
        "geometry.access",
        "Geometry structure is missing a required element",
    )
}
fn at<T, I: std::slice::SliceIndex<[T]>>(values: &[T], index: I) -> GeometryResult<&I::Output> {
    values.get(index).ok_or_else(missing_geometry)
}
fn at_mut<T, I: std::slice::SliceIndex<[T]>>(
    values: &mut [T],
    index: I,
) -> GeometryResult<&mut I::Output> {
    values.get_mut(index).ok_or_else(missing_geometry)
}
fn last<T>(values: &[T]) -> GeometryResult<&T> {
    values.last().ok_or_else(missing_geometry)
}

// Schemars represents documented unit variants as oneOf/const. Keep the original
// compact enum representation while retaining English API documentation. This
// transform is attached only to unit enums; it preserves variant order and values.
#[cfg(feature = "schemars")]
fn unit_enum_schema(schema: &mut schemars::Schema) {
    let values = schema
        .get("oneOf")
        .and_then(|value| value.as_array())
        .and_then(|variants| {
            variants
                .iter()
                .map(|variant| variant.get("const").cloned())
                .collect::<Option<Vec<_>>>()
        });
    if let Some(values) = values {
        schema.remove("oneOf");
        schema.insert("type".into(), "string".into());
        schema.insert("enum".into(), values.into());
    }
}

#[cfg(test)]
mod checked_access_tests {
    use super::*;

    #[test]
    fn missing_internal_elements_return_typed_errors() {
        let mut values = [1_u8, 2];
        for error in [
            at(&values, 2).unwrap_err(),
            at(&values, 1..3).unwrap_err(),
            at_mut(&mut values, 2).unwrap_err(),
            last::<u8>(&[]).unwrap_err(),
        ] {
            assert_eq!(error.diagnostic().reason, GeometryReason::InvalidTopology);
            assert_eq!(error.diagnostic().operation, "geometry.access");
        }
    }
}
