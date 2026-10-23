use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// A source-to-target Cartesian control pair, expressed in the units of the respective coordinate
/// systems.
pub struct ControlPoint {
    /// Source point in the source coordinate system's units.
    pub source: Point2,
    /// Corresponding point in the target coordinate system's units.
    pub target: Point2,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Least-squares transform and residuals in target coordinate units. Fitting and residual
/// acceptance are separate operations.
pub struct CalibrationFit {
    /// Mapping from source coordinates to target coordinates, without automatic residual
    /// acceptance.
    pub transform: Transform2,
    /// Euclidean residuals in target coordinate units, in control-pair order.
    pub residuals: Vec<f64>,
    /// Largest residual in target coordinate units.
    pub max_residual: f64,
    /// Root-mean-square residual in target coordinate units.
    pub rms_residual: f64,
}

impl CalibrationFit {
    /// Evaluate independent check points in the target coordinate units, separately from
    /// fitting.
    pub fn check_points(&self, points: &[ControlPoint]) -> GeometryResult<CalibrationFit> {
        evaluate_calibration(self.transform, points)
    }
    /// Require maximum residual at most the positive limit in target coordinate units; rejects
    /// nonfinite or negative stored maxima.
    pub fn require_max_residual(&self, max_residual: f64) -> GeometryResult<()> {
        positive(max_residual, "Residual limit")
            .map_err(|error| error.at("limit", "calibration.require_max_residual"))?;
        finite(self.max_residual)
            .map_err(|error| error.at("maxResidual", "calibration.require_max_residual"))?;
        if self.max_residual < 0. {
            return Err(GeometryError::invalid(
                GeometryReason::InvalidParameterRange,
                "maxResidual",
                "calibration.require_max_residual",
                "Residual must not be negative",
            ));
        }
        if self.max_residual > max_residual {
            return Err(GeometryError::invalid(
                GeometryReason::ResidualLimitExceeded,
                "maxResidual",
                "calibration.require_max_residual",
                format!("Calibration residual exceeds the limit of {max_residual}"),
            ));
        }
        Ok(())
    }
}

/// Fit an orientation-preserving least-squares similarity in caller coordinates. Use affine
/// fitting when reflection is required; tolerances use source units and radians.
pub fn fit_similarity(
    points: &[ControlPoint],
    t: &GeometryTolerance,
) -> GeometryResult<CalibrationFit> {
    validate_controls(points, 2, t)?;
    let (source, target) = centers(points)?;
    let spread = points
        .iter()
        .map(|p| p.source.distance(source))
        .fold(0., f64::max);
    if spread <= t.linear {
        return Err(GeometryError::invalid(
            GeometryReason::CoincidentControlPoints,
            "controlPoints.source",
            "fit_similarity",
            "Similarity fitting requires distinct source points",
        ));
    }
    let mut denominator = 0.;
    let mut dot = 0.;
    let mut cross = 0.;
    for p in points {
        let s = source.vector_to(p.source).scaled(1. / spread);
        let d = target.vector_to(p.target);
        denominator += s.dot(s);
        dot += s.dot(d);
        cross += s.cross(d);
    }
    let a = dot / denominator / spread;
    let b = cross / denominator / spread;
    let mut transform = Transform2 {
        m11: a,
        m12: -b,
        m21: b,
        m22: a,
        tx: 0.,
        ty: 0.,
    };
    transform.tx = target.x - (a * source.x - b * source.y);
    transform.ty = target.y - (b * source.x + a * source.y);
    transform.inverse(t)?;
    evaluate_calibration(transform, points)
}

/// Fit an affine least-squares transform to at least three independent source pairs. Residuals
/// use target coordinate units; angular tolerance controls conditioning.
pub fn fit_affine(
    points: &[ControlPoint],
    t: &GeometryTolerance,
) -> GeometryResult<CalibrationFit> {
    validate_controls(points, 3, t)?;
    let (source, target) = centers(points)?;
    let spread = points
        .iter()
        .map(|p| p.source.distance(source))
        .fold(0., f64::max);
    if spread <= t.linear {
        return Err(GeometryError::invalid(
            GeometryReason::CoincidentControlPoints,
            "controlPoints.source",
            "fit_affine",
            "Affine source points coincide",
        ));
    }
    let (mut xx, mut xy, mut yy, mut ux, mut uy, mut vx, mut vy) = (0., 0., 0., 0., 0., 0., 0.);
    for p in points {
        let s = source.vector_to(p.source).scaled(1. / spread);
        let d = target.vector_to(p.target);
        xx += s.x * s.x;
        xy += s.x * s.y;
        yy += s.y * s.y;
        ux += s.x * d.x;
        uy += s.y * d.x;
        vx += s.x * d.y;
        vy += s.y * d.y;
    }
    let det = xx * yy - xy * xy;
    let trace = xx + yy;
    // The dimensionless conditioning threshold rejects collinear or numerically
    // indistinguishable controls.
    if !det.is_finite() || det <= trace * trace * (t.angular * t.angular).max(64. * f64::EPSILON) {
        return Err(GeometryError::invalid(
            GeometryReason::CollinearControlPoints,
            "controlPoints.source",
            "fit_affine",
            "Affine control points are collinear or numerically unstable",
        ));
    }
    let mut transform = Transform2 {
        m11: (ux * yy - uy * xy) / det / spread,
        m12: (uy * xx - ux * xy) / det / spread,
        m21: (vx * yy - vy * xy) / det / spread,
        m22: (vy * xx - vx * xy) / det / spread,
        tx: 0.,
        ty: 0.,
    };
    transform.tx = target.x - (transform.m11 * source.x + transform.m12 * source.y);
    transform.ty = target.y - (transform.m21 * source.x + transform.m22 * source.y);
    transform.inverse(t)?;
    evaluate_calibration(transform, points)
}

/// Evaluate a nonsingular transform on nonempty control pairs; residuals use target units. Does not
/// accept or reject a residual threshold automatically.
pub fn evaluate_calibration(
    transform: Transform2,
    points: &[ControlPoint],
) -> GeometryResult<CalibrationFit> {
    transform.inverse(&crate::curves::strict_tolerance())?;
    if points.is_empty() {
        return Err(GeometryError::invalid(
            GeometryReason::InsufficientPoints,
            "controlPoints",
            "evaluate_calibration",
            "No calibration control points",
        ));
    }
    let mut residuals = Vec::with_capacity(points.len());
    let mut maximum = 0_f64;
    for p in points {
        p.source.validate()?;
        p.target.validate()?;
        let mapped = transform.apply_point(p.source);
        mapped.validate()?;
        let error = mapped.distance(p.target);
        finite(error)?;
        maximum = maximum.max(error);
        residuals.push(error);
    }
    let rms = if maximum == 0. {
        0.
    } else {
        maximum
            * (residuals.iter().map(|e| (e / maximum).powi(2)).sum::<f64>()
                / residuals.len() as f64)
                .sqrt()
    };
    finite(rms)?;
    Ok(CalibrationFit {
        transform,
        residuals,
        max_residual: maximum,
        rms_residual: rms,
    })
}
fn validate_controls(
    points: &[ControlPoint],
    minimum: usize,
    t: &GeometryTolerance,
) -> GeometryResult<()> {
    t.validate()?;
    if points.len() < minimum {
        return Err(GeometryError::invalid(
            GeometryReason::InsufficientPoints,
            "controlPoints",
            "calibration.validate_controls",
            format!("At least {minimum} control point pairs are required"),
        ));
    }
    for p in points {
        p.source.validate()?;
        p.target.validate()?;
    }
    Ok(())
}
fn centers(points: &[ControlPoint]) -> GeometryResult<(Point2, Point2)> {
    let first = *at(points, 0)?;
    let (mut sx, mut sy, mut tx, mut ty) = (0., 0., 0., 0.);
    let n = points.len() as f64;
    // Use local differences instead of summing large common coordinates.
    for p in points {
        sx += (p.source.x - first.source.x) / n;
        sy += (p.source.y - first.source.y) / n;
        tx += (p.target.x - first.target.x) / n;
        ty += (p.target.y - first.target.y) / n;
    }
    let source = first.source.translated(Vector2 { x: sx, y: sy });
    let target = first.target.translated(Vector2 { x: tx, y: ty });
    source.validate()?;
    target.validate()?;
    Ok((source, target))
}
