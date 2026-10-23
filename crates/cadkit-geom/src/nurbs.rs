use crate::*;
use std::{cmp::Ordering, collections::BinaryHeap};

const MAX_CONTROL_POINTS: usize = 4096;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Rational degree 1-3 curve in caller coordinates, with positive weights and open clamped knots.
/// Public parameters normalize the active knot interval to 0-1; tolerances are explicit.
pub struct NurbsCurve {
    /// Polynomial degree, supported values 1-3; dimensionless.
    pub degree: u8,
    /// Ordered control points in caller Cartesian units.
    pub control_points: Vec<Point2>,
    /// Positive finite dimensionless rational weights, one per control point.
    pub weights: Vec<f64>,
    /// Nondecreasing native knot parameters; endpoints repeat degree+1 times. Public evaluation
    /// normalizes the active interval to 0-1.
    pub knots: Vec<f64>,
    /// Periodic profile flag; true is currently unsupported, not silently interpreted as an open
    /// curve.
    pub periodic: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// NURBS approximation budget in coordinate units and explicit dimensionless resource limits.
pub struct NurbsTessellationOptions {
    /// Positive finite approximation distance budget in caller units.
    pub max_error: f64,
    /// Maximum output segment count, 1-1000000; dimensionless.
    pub max_segments: u32,
    /// Maximum subdivision count, 0-1000000; dimensionless.
    pub max_subdivisions: u32,
    /// Maximum subdivision depth, 0-64; dimensionless.
    pub max_depth: u8,
}
impl NurbsTessellationOptions {
    /// Construct approximation options with max_error in coordinate units and default resource
    /// caps; validation occurs when used.
    pub fn with_error(max_error: f64) -> Self {
        Self {
            max_error,
            max_segments: 262_144,
            max_subdivisions: 262_144,
            max_depth: 48,
        }
    }
    fn validate(self) -> GeometryResult<()> {
        positive(self.max_error, "NURBS approximation budget")
            .map_err(|e| e.at("maxError", "nurbs.tessellate"))?;
        if self.max_segments == 0
            || self.max_segments > 1_000_000
            || self.max_subdivisions > 1_000_000
            || self.max_depth > 64
        {
            return Err(GeometryError::invalid(
                GeometryReason::InvalidParameterRange,
                "options",
                "nurbs.tessellate",
                "Segment limit must be 1-1000000, subdivision limit 0-1000000 and depth 0-64",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Display polyline in caller coordinates and a certified two-sided approximation bound; not a
/// source for analytic measurement.
pub struct NurbsTessellation {
    /// Ordered vertices in caller Cartesian units.
    pub points: Vec<Point2>,
    /// Two-sided Hausdorff upper bound between the source curve and the polyline, in
    /// coordinate units.
    pub max_error_bound: f64,
    /// Number of subdivisions performed; dimensionless.
    pub subdivisions: u32,
    /// Number of extracted rational Bezier spans; dimensionless.
    pub bezier_spans: u32,
}

fn numeric(field: &str, operation: &str) -> GeometryError {
    GeometryError::invalid(
        GeometryReason::NumericalErrorBudgetExceeded,
        field,
        operation,
        "NURBS numerical range could not be certified within the requested error budget",
    )
}
fn resource(field: &str, operation: &str) -> GeometryError {
    GeometryError::unsupported(
        GeometryReason::ResourceLimit,
        field,
        operation,
        "NURBS computation exceeded the explicit resource limit",
    )
}

// Round every elementary operation outward, including subnormals via next_down/next_up.
// Intervals enclose exact real operations; they are not statistical estimates.
#[derive(Debug, Clone, Copy)]
struct Interval {
    lo: f64,
    hi: f64,
}
impl Interval {
    fn exact(value: f64) -> Self {
        Self {
            lo: value,
            hi: value,
        }
    }
    fn outward(lo: f64, hi: f64) -> Self {
        if !lo.is_finite() || !hi.is_finite() {
            Self {
                lo: f64::NEG_INFINITY,
                hi: f64::INFINITY,
            }
        } else {
            Self {
                lo: lo.next_down(),
                hi: hi.next_up(),
            }
        }
    }
    fn add(self, other: Self) -> Self {
        Self::outward(self.lo + other.lo, self.hi + other.hi)
    }
    fn sub(self, other: Self) -> Self {
        Self::outward(self.lo - other.hi, self.hi - other.lo)
    }
    fn mul(self, other: Self) -> Self {
        let values = [
            self.lo * other.lo,
            self.lo * other.hi,
            self.hi * other.lo,
            self.hi * other.hi,
        ];
        if values.iter().any(|v| !v.is_finite()) {
            return Self {
                lo: f64::NEG_INFINITY,
                hi: f64::INFINITY,
            };
        }
        Self::outward(
            values.into_iter().fold(f64::INFINITY, f64::min),
            values.into_iter().fold(f64::NEG_INFINITY, f64::max),
        )
    }
    fn divide(self, other: Self) -> GeometryResult<Self> {
        if !other.lo.is_finite() || !other.hi.is_finite() || other.lo <= 0. {
            return Err(numeric("weightsOrKnots", "nurbs.interval"));
        }
        let values = [
            self.lo / other.lo,
            self.lo / other.hi,
            self.hi / other.lo,
            self.hi / other.hi,
        ];
        if values.iter().any(|v| !v.is_finite()) {
            return Err(numeric("coordinates", "nurbs.interval"));
        }
        Ok(Self::outward(
            values.into_iter().fold(f64::INFINITY, f64::min),
            values.into_iter().fold(f64::NEG_INFINITY, f64::max),
        ))
    }
    fn hull(self, other: Self) -> Self {
        Self {
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }
    fn magnitude(self) -> f64 {
        self.lo.abs().max(self.hi.abs())
    }
    fn middle(self) -> GeometryResult<f64> {
        let middle = self.lo * 0.5 + self.hi * 0.5;
        if !self.lo.is_finite() || !self.hi.is_finite() || !middle.is_finite() {
            return Err(numeric("coordinates", "nurbs.interval"));
        }
        Ok(middle.clamp(self.lo, self.hi))
    }
    fn unit(self) -> Self {
        Self {
            lo: self.lo.max(0.),
            hi: self.hi.min(1.),
        }
    }
}
#[derive(Clone, Copy)]
struct BoxPoint {
    x: Interval,
    y: Interval,
}
impl BoxPoint {
    fn middle(self) -> GeometryResult<Point2> {
        Ok(Point2::new(self.x.middle()?, self.y.middle()?))
    }
    fn error_from(self, point: Point2) -> f64 {
        (self.x.sub(Interval::exact(point.x)).magnitude()
            + self.y.sub(Interval::exact(point.y)).magnitude())
        .next_up()
    }
    fn hull(self, other: Self) -> Self {
        Self {
            x: self.x.hull(other.x),
            y: self.y.hull(other.y),
        }
    }
}
#[derive(Clone, Copy)]
struct Homogeneous {
    x: Interval,
    y: Interval,
    w: Interval,
}
impl Homogeneous {
    fn mix(self, other: Self, alpha: Interval) -> Self {
        let complement = Interval::exact(1.).sub(alpha);
        Self {
            x: self.x.mul(complement).add(other.x.mul(alpha)),
            y: self.y.mul(complement).add(other.y.mul(alpha)),
            w: self.w.mul(complement).add(other.w.mul(alpha)),
        }
    }
    fn project(self, origin: Point2) -> GeometryResult<BoxPoint> {
        Ok(BoxPoint {
            x: self.x.divide(self.w)?.add(Interval::exact(origin.x)),
            y: self.y.divide(self.w)?.add(Interval::exact(origin.y)),
        })
    }
}
struct Span {
    controls: Vec<Homogeneous>,
    start: f64,
    end: f64,
}
struct Prepared {
    spans: Vec<Span>,
    origin: Point2,
    start: f64,
    end: f64,
}

impl NurbsCurve {
    /// Validate finite values and the supported profile. Where supplied, tolerances use caller
    /// units, radians and squared units; no repair or unit conversion is performed.
    pub fn validate(&self, tolerance: &GeometryTolerance) -> GeometryResult<()> {
        tolerance.validate()?;
        let operation = "nurbs.validate";
        if self.periodic {
            return Err(GeometryError::unsupported(
                GeometryReason::InvalidTopology,
                "periodic",
                operation,
                "Periodic NURBS are not supported by this profile",
            ));
        }
        if !(1..=3).contains(&self.degree) {
            return Err(GeometryError::unsupported(
                GeometryReason::UnsupportedDegree,
                "degree",
                operation,
                "NURBS degree must be between 1 and 3",
            ));
        }
        let count = self.control_points.len();
        let degree = usize::from(self.degree);
        if count > MAX_CONTROL_POINTS {
            return Err(resource("controlPoints", operation));
        }
        if count <= degree {
            return Err(GeometryError::invalid(
                GeometryReason::InsufficientPoints,
                "controlPoints",
                operation,
                "Control point count must exceed the degree",
            ));
        }
        if self.weights.len() != count || self.knots.len() != count + degree + 1 {
            return Err(GeometryError::invalid(
                GeometryReason::InvalidTopology,
                "weightsOrKnots",
                operation,
                "Weight count must equal control point count; knot count must equal control point count plus degree plus one",
            ));
        }
        for point in &self.control_points {
            point
                .validate()
                .map_err(|e| e.at("controlPoints", operation))?;
        }
        let first_control = *at(&self.control_points, 0)?;
        if self
            .control_points
            .iter()
            .all(|point| point.distance(first_control) <= tolerance.linear)
        {
            return Err(GeometryError::invalid(
                GeometryReason::CoincidentControlPoints,
                "controlPoints",
                operation,
                "NURBS control points coincide within the linear tolerance",
            ));
        }
        for weight in &self.weights {
            positive(*weight, "NURBS weight").map_err(|e| e.at("weights", operation))?;
        }
        for knot in &self.knots {
            finite(*knot).map_err(|e| e.at("knots", operation))?;
        }
        let start_knot = *at(&self.knots, degree)?;
        let end_knot = *at(&self.knots, count)?;
        if self
            .knots
            .iter()
            .zip(self.knots.iter().skip(1))
            .any(|(a, b)| a > b)
            || start_knot >= end_knot
        {
            return Err(GeometryError::invalid(
                GeometryReason::InvalidParameterRange,
                "knots",
                operation,
                "Knots must be nondecreasing and the active parameter interval must be positive",
            ));
        }
        if at(&self.knots, ..=degree)?.iter().any(|k| *k != start_knot)
            || at(&self.knots, count..)?.iter().any(|k| *k != end_knot)
        {
            return Err(GeometryError::unsupported(
                GeometryReason::InvalidTopology,
                "knots",
                operation,
                "This profile requires open clamped curves with degree plus one repeated endpoint knots",
            ));
        }
        let mut index = degree + 1;
        while index < count {
            let value = *at(&self.knots, index)?;
            let mut end = index + 1;
            while end < count && *at(&self.knots, end)? == value {
                end += 1;
            }
            if value <= start_knot || value >= end_knot || end - index > degree {
                return Err(GeometryError::unsupported(
                    GeometryReason::InvalidTopology,
                    "knots",
                    operation,
                    "Interior knot multiplicity must not exceed the degree; discontinuous or excess endpoint knots are unsupported",
                ));
            }
            index = end;
        }
        Ok(())
    }

    /// Evaluate a normalized parameter in 0-1. The output's numerical L1 enclosure must fit
    /// tolerance.linear in coordinate units.
    pub fn point_at(
        &self,
        parameter: f64,
        tolerance: &GeometryTolerance,
    ) -> GeometryResult<Point2> {
        self.validate(tolerance)?;
        check_parameter(parameter)?;
        if parameter == 0. {
            return Ok(*at(&self.control_points, 0)?);
        }
        if parameter == 1. {
            return Ok(*last(&self.control_points)?);
        }
        let prepared = self.prepare()?;
        let enclosure = prepared.evaluate(parameter, false)?;
        let point = enclosure.middle()?;
        if enclosure.error_from(point) > tolerance.linear {
            return Err(numeric("parameter", "nurbs.point_at"));
        }
        Ok(point)
    }

    /// Derivative with respect to the normalized parameter, in coordinate units per unit
    /// parameter. Endpoint derivatives are one-sided; interior C0 knots are rejected using the
    /// supplied tolerance.
    pub fn derivative_at(
        &self,
        parameter: f64,
        tolerance: &GeometryTolerance,
    ) -> GeometryResult<Vector2> {
        self.validate(tolerance)?;
        check_parameter(parameter)?;
        let prepared = self.prepare()?;
        let u = prepared.parameter(parameter);
        let degree = usize::from(self.degree);
        for span in prepared.spans.iter().skip(1) {
            if u.lo <= span.start
                && u.hi >= span.start
                && self.knots.iter().filter(|k| **k == span.start).count() == degree
            {
                return Err(GeometryError::unsupported(
                    GeometryReason::InvalidParameterRange,
                    "parameter",
                    "nurbs.derivative_at",
                    "A unique derivative cannot be guaranteed at or numerically indistinguishable from an interior C0 knot",
                ));
            }
        }
        let enclosure = prepared.evaluate(parameter, true)?;
        let derivative = enclosure.middle()?;
        if enclosure.error_from(derivative) > tolerance.linear {
            return Err(numeric("parameter", "nurbs.derivative_at"));
        }
        Ok(Vector2 {
            x: derivative.x,
            y: derivative.y,
        })
    }

    /// Transform source geometry to target coordinates using the supplied tolerance. Reject
    /// singular transforms, unsupported representations or certified numerical loss.
    pub fn transformed(
        &self,
        transform: Transform2,
        tolerance: &GeometryTolerance,
    ) -> GeometryResult<Self> {
        self.validate(tolerance)?;
        transform.inverse(tolerance)?;
        let mut result = self.clone();
        for point in &mut result.control_points {
            let enclosure = BoxPoint {
                x: Interval::exact(transform.m11)
                    .mul(Interval::exact(point.x))
                    .add(Interval::exact(transform.m12).mul(Interval::exact(point.y)))
                    .add(Interval::exact(transform.tx)),
                y: Interval::exact(transform.m21)
                    .mul(Interval::exact(point.x))
                    .add(Interval::exact(transform.m22).mul(Interval::exact(point.y)))
                    .add(Interval::exact(transform.ty)),
            };
            let transformed = transform.apply_point(*point);
            if enclosure.error_from(transformed) > tolerance.linear
                || !transformed.x.is_finite()
                || !transformed.y.is_finite()
            {
                return Err(numeric("controlPoints", "nurbs.transformed"));
            }
            *point = transformed;
        }
        result.validate(tolerance)?;
        Ok(result)
    }

    /// Search with an explicit coordinate-unit distance-error budget and iteration cap. The
    /// supplied tolerance validates geometry; success certifies distance, not point uniqueness.
    pub fn nearest_point_with_options(
        &self,
        query: Point2,
        options: NearestPointOptions,
        tolerance: &GeometryTolerance,
    ) -> GeometryResult<NearestPointResult> {
        self.validate(tolerance)?;
        query
            .validate()
            .map_err(|e| e.at("query", "nurbs.nearest_point"))?;
        options.validate()?;
        let prepared = self.prepare()?;
        let mut best: Option<NurbsCandidate> = None;
        let mut queue = BinaryHeap::new();
        for (index, span) in prepared.spans.iter().enumerate() {
            consider_nurbs(self, &prepared, query, index, [0., 0.5, 1.], &mut best)?;
            queue.push(NurbsSearchNode {
                lower: search_lower(&span.controls, prepared.origin, query)?,
                controls: span.controls.clone(),
                start: 0.,
                end: 1.,
                span: index,
                depth: 0,
            });
        }
        let mut iterations = 0;
        loop {
            let candidate = best
                .as_ref()
                .ok_or_else(|| numeric("candidates", "nurbs.nearest_point"))?;
            let lower = queue.peek().map_or(candidate.lower, |node| node.lower);
            // The true minimum lies between the global lower and candidate upper bounds. The
            // reported f64 distance's largest deviation from this interval also includes
            // coordinate and arithmetic error.
            let error = (candidate.distance - lower)
                .abs()
                .max((candidate.distance - candidate.upper).abs())
                .next_up();
            if error <= options.max_distance_error
                && candidate.point_error <= options.max_distance_error
            {
                return Ok(NearestPointResult {
                    point: candidate.point,
                    parameter: candidate.parameter,
                    distance: candidate.distance,
                    distance_error_bound: error,
                    iterations,
                    converged: true,
                });
            }
            if iterations >= options.max_iterations {
                return Err(GeometryError::unsupported(
                    GeometryReason::ApproximationLimit,
                    "maxIterations",
                    "nurbs.nearest_point",
                    "NURBS global distance bounds did not converge within the iteration budget",
                ));
            }
            let node = queue
                .pop()
                .ok_or_else(|| numeric("candidates", "nurbs.nearest_point"))?;
            if node.lower > candidate.upper {
                continue;
            }
            if node.depth >= 48 {
                return Err(GeometryError::unsupported(
                    GeometryReason::ApproximationLimit,
                    "maxIterations",
                    "nurbs.nearest_point",
                    "NURBS nearest-point search reached the subdivision depth limit",
                ));
            }
            iterations += 1;
            let middle = node.start + (node.end - node.start) * 0.5;
            if middle == node.start || middle == node.end {
                return Err(numeric("parameter", "nurbs.nearest_point"));
            }
            let (left, right) = split(&node.controls)?;
            consider_nurbs(
                self,
                &prepared,
                query,
                node.span,
                [
                    node.start + (middle - node.start) * 0.5,
                    middle,
                    middle + (node.end - middle) * 0.5,
                ],
                &mut best,
            )?;
            for (controls, start, end) in [(left, node.start, middle), (right, middle, node.end)] {
                let lower = search_lower(&controls, prepared.origin, query)?;
                if lower <= best.as_ref().ok_or_else(missing_geometry)?.upper {
                    queue.push(NurbsSearchNode {
                        controls,
                        start,
                        end,
                        lower,
                        span: node.span,
                        depth: node.depth + 1,
                    });
                }
            }
            if queue.len() > 262_144 {
                return Err(resource("candidates", "nurbs.nearest_point"));
            }
        }
    }

    /// Conservative positive-weight control enclosure in caller coordinates, validated with
    /// the supplied tolerance. This is not a tight extremum bound.
    pub fn bounds(&self, tolerance: &GeometryTolerance) -> GeometryResult<Bounds2> {
        self.validate(tolerance)?;
        Bounds2::from_points(&self.control_points)
    }

    /// Return a display polyline with a certified coordinate-unit error bound under the supplied
    /// tolerance; never return partial success on budget exhaustion.
    pub fn tessellate(
        &self,
        max_error: f64,
        tolerance: &GeometryTolerance,
    ) -> GeometryResult<Vec<Point2>> {
        Ok(self
            .tessellate_with_options(NurbsTessellationOptions::with_error(max_error), tolerance)?
            .points)
    }

    /// Produce a certified display polyline using explicit coordinate-unit error and resource
    /// budgets; the supplied tolerance validates the source.
    pub fn tessellate_with_options(
        &self,
        options: NurbsTessellationOptions,
        tolerance: &GeometryTolerance,
    ) -> GeometryResult<NurbsTessellation> {
        self.validate(tolerance)?;
        options.validate()?;
        let prepared = self.prepare()?;
        if prepared.spans.len() > options.max_segments as usize {
            return Err(resource("maxSegments", "nurbs.tessellate"));
        }
        let mut result = NurbsTessellation {
            points: Vec::new(),
            max_error_bound: 0.,
            subdivisions: 0,
            bezier_spans: prepared.spans.len() as u32,
        };
        for span in prepared.spans {
            if result.points.is_empty() {
                result
                    .points
                    .push(at(&span.controls, 0)?.project(prepared.origin)?.middle()?);
            }
            let mut stack = vec![(span.controls, 0u8)];
            while let Some((controls, depth)) = stack.pop() {
                let start = *result.points.last().ok_or_else(missing_geometry)?;
                let end_box = last(controls.as_slice())?.project(prepared.origin)?;
                let end = end_box.middle()?;
                if at(controls.as_slice(), 0)?
                    .project(prepared.origin)?
                    .error_from(start)
                    > options.max_error
                    || end_box.error_from(end) > options.max_error
                {
                    return Err(numeric("maxError", "nurbs.tessellate"));
                }
                let bound = chord_error(&controls, start, end, prepared.origin)?;
                if bound <= options.max_error {
                    if result.points.len() > options.max_segments as usize {
                        return Err(resource("maxSegments", "nurbs.tessellate"));
                    }
                    result.points.push(end);
                    result.max_error_bound = result.max_error_bound.max(bound);
                } else {
                    if depth >= options.max_depth || result.subdivisions >= options.max_subdivisions
                    {
                        return Err(GeometryError::unsupported(
                            GeometryReason::ApproximationLimit,
                            "options",
                            "nurbs.tessellate",
                            "NURBS control enclosure did not converge within the approximation budget; no partial result was returned",
                        ));
                    }
                    result.subdivisions += 1;
                    let (left, right) = split(&controls)?;
                    stack.push((right, depth + 1));
                    stack.push((left, depth + 1));
                }
            }
        }
        Ok(result)
    }

    fn prepare(&self) -> GeometryResult<Prepared> {
        let degree = usize::from(self.degree);
        let origin = *at(&self.control_points, 0)?;
        let max_weight = self.weights.iter().copied().fold(0., f64::max);
        let mut controls = Vec::with_capacity(self.control_points.len());
        for (point, weight) in self.control_points.iter().zip(&self.weights) {
            let w = Interval::exact(*weight).divide(Interval::exact(max_weight))?;
            if w.lo <= 0. {
                return Err(numeric("weights", "nurbs.prepare"));
            }
            controls.push(Homogeneous {
                x: Interval::exact(point.x)
                    .sub(Interval::exact(origin.x))
                    .mul(w),
                y: Interval::exact(point.y)
                    .sub(Interval::exact(origin.y))
                    .mul(w),
                w,
            });
        }
        let mut knots = self.knots.clone();
        let start = *at(&knots, degree)?;
        let end = *at(&knots, controls.len())?;
        let mut inner = at(&self.knots, degree + 1..self.control_points.len())?.to_vec();
        inner.dedup();
        for value in inner {
            while knots.iter().filter(|k| **k == value).count() < degree {
                let k = knots
                    .partition_point(|k| *k <= value)
                    .checked_sub(1)
                    .ok_or_else(missing_geometry)?;
                let multiplicity = knots.iter().filter(|k| **k == value).count();
                let first = k.checked_sub(degree).ok_or_else(missing_geometry)?;
                let end = k.checked_sub(multiplicity).ok_or_else(missing_geometry)?;
                let mut next = Vec::with_capacity(controls.len() + 1);
                next.extend_from_slice(at(&controls, ..=first)?);
                for i in first + 1..=end {
                    let alpha = Interval::exact(value)
                        .sub(Interval::exact(*at(&knots, i)?))
                        .divide(
                            Interval::exact(*at(&knots, i + degree)?)
                                .sub(Interval::exact(*at(&knots, i)?)),
                        )?
                        .unit();
                    next.push(at(&controls, i - 1)?.mix(*at(&controls, i)?, alpha));
                }
                next.extend_from_slice(at(&controls, end..)?);
                controls = next;
                knots.insert(k + 1, value);
            }
        }
        let mut spans = Vec::new();
        for k in degree..controls.len() {
            if *at(&knots, k)? < *at(&knots, k + 1)? {
                spans.push(Span {
                    controls: at(&controls, k - degree..=k)?.to_vec(),
                    start: *at(&knots, k)?,
                    end: *at(&knots, k + 1)?,
                });
            }
        }
        Ok(Prepared {
            spans,
            origin,
            start,
            end,
        })
    }
}

fn check_parameter(parameter: f64) -> GeometryResult<()> {
    if !parameter.is_finite() || !(0. ..=1.).contains(&parameter) {
        return Err(GeometryError::invalid(
            GeometryReason::InvalidParameterRange,
            "parameter",
            "nurbs.evaluate",
            "Normalized parameter must be between 0 and 1",
        ));
    }
    Ok(())
}
fn casteljau(controls: &[Homogeneous], parameter: Interval) -> GeometryResult<Homogeneous> {
    let mut work = controls.to_vec();
    for remaining in (1..work.len()).rev() {
        for i in 0..remaining {
            let value = at(&work, i)?.mix(*at(&work, i + 1)?, parameter);
            *at_mut(&mut work, i)? = value;
        }
    }
    Ok(*at(&work, 0)?)
}
impl Prepared {
    fn parameter(&self, parameter: f64) -> Interval {
        if parameter == 0. {
            return Interval::exact(self.start);
        }
        if parameter == 1. {
            return Interval::exact(self.end);
        }
        Interval::exact(self.start).add(
            Interval::exact(self.end)
                .sub(Interval::exact(self.start))
                .mul(Interval::exact(parameter)),
        )
    }
    fn evaluate(&self, parameter: f64, derivative: bool) -> GeometryResult<BoxPoint> {
        let u = self.parameter(parameter);
        let mut result: Option<BoxPoint> = None;
        for span in &self.spans {
            if u.hi < span.start || u.lo > span.end {
                continue;
            }
            let width = Interval::exact(span.end).sub(Interval::exact(span.start));
            let local = u.sub(Interval::exact(span.start)).divide(width)?.unit();
            let value = casteljau(&span.controls, local)?;
            let enclosure = if derivative {
                let degree = Interval::exact((span.controls.len() - 1) as f64);
                let derivatives: Vec<_> = span
                    .controls
                    .iter()
                    .zip(span.controls.iter().skip(1))
                    .map(|(a, b)| Homogeneous {
                        x: b.x.sub(a.x).mul(degree),
                        y: b.y.sub(a.y).mul(degree),
                        w: b.w.sub(a.w).mul(degree),
                    })
                    .collect();
                let delta = casteljau(&derivatives, local)?;
                let factor = Interval::exact(self.end)
                    .sub(Interval::exact(self.start))
                    .divide(width)?;
                let denominator = value.w.mul(value.w);
                BoxPoint {
                    x: delta
                        .x
                        .mul(value.w)
                        .sub(value.x.mul(delta.w))
                        .divide(denominator)?
                        .mul(factor),
                    y: delta
                        .y
                        .mul(value.w)
                        .sub(value.y.mul(delta.w))
                        .divide(denominator)?
                        .mul(factor),
                }
            } else {
                value.project(self.origin)?
            };
            result = Some(result.map_or(enclosure, |old| old.hull(enclosure)));
        }
        result.ok_or_else(|| numeric("parameter", "nurbs.evaluate"))
    }
}
fn split(controls: &[Homogeneous]) -> GeometryResult<(Vec<Homogeneous>, Vec<Homogeneous>)> {
    let mut work = controls.to_vec();
    let mut left = vec![*at(&work, 0)?];
    let mut right = vec![*last(&work)?];
    for remaining in (1..work.len()).rev() {
        for i in 0..remaining {
            let value = at(&work, i)?.mix(*at(&work, i + 1)?, Interval::exact(0.5));
            *at_mut(&mut work, i)? = value;
        }
        left.push(*at(&work, 0)?);
        right.push(*at(&work, remaining - 1)?);
    }
    right.reverse();
    Ok((left, right))
}
// Map control P_i to chord point L_i=(1-i/p)A+(i/p)B. Positive rational Bernstein coefficients
// combine both sequences with the same convex weights, so ||C(t)-L(t)|| <= max_i ||P_i-L_i||.
// L(t) is continuous from A to B, establishing a two-sided Hausdorff bound. The L1 interval
// enclosure also bounds the Euclidean norm.
fn chord_error(
    controls: &[Homogeneous],
    start: Point2,
    end: Point2,
    origin: Point2,
) -> GeometryResult<f64> {
    let mut bound: f64 = 0.;
    for (index, control) in controls.iter().enumerate() {
        let fraction = Interval::exact(index as f64)
            .divide(Interval::exact((controls.len() - 1) as f64))?
            .unit();
        let target = |a, b| {
            Interval::exact(a)
                .mul(Interval::exact(1.).sub(fraction))
                .add(Interval::exact(b).mul(fraction))
        };
        let point = control.project(origin)?;
        let error = (point.x.sub(target(start.x, end.x)).magnitude()
            + point.y.sub(target(start.y, end.y)).magnitude())
        .next_up();
        if !error.is_finite() {
            return Err(numeric("maxError", "nurbs.tessellate"));
        }
        bound = bound.max(error);
    }
    Ok(bound)
}

struct NurbsCandidate {
    point: Point2,
    parameter: f64,
    distance: f64,
    lower: f64,
    upper: f64,
    point_error: f64,
}
struct NurbsSearchNode {
    controls: Vec<Homogeneous>,
    start: f64,
    end: f64,
    span: usize,
    depth: u8,
    lower: f64,
}
impl PartialEq for NurbsSearchNode {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for NurbsSearchNode {}
impl PartialOrd for NurbsSearchNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for NurbsSearchNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .lower
            .total_cmp(&self.lower)
            .then_with(|| other.span.cmp(&self.span))
            .then_with(|| other.start.total_cmp(&self.start))
    }
}
fn norm_interval(x: Interval, y: Interval) -> GeometryResult<Interval> {
    let scale = x.magnitude().max(y.magnitude());
    if scale == 0. {
        return Ok(Interval::exact(0.));
    }
    if !scale.is_finite() {
        return Err(numeric("query", "nurbs.nearest_point"));
    }
    let square = |v: Interval| {
        let lower = if v.lo <= 0. && v.hi >= 0. {
            0.
        } else {
            v.lo.abs().min(v.hi.abs())
        };
        let upper = v.magnitude();
        Interval::outward(lower * lower, upper * upper)
    };
    let sum =
        square(x.divide(Interval::exact(scale))?).add(square(y.divide(Interval::exact(scale))?));
    let norm = Interval::outward(sum.lo.max(0.).sqrt(), sum.hi.sqrt()).mul(Interval::exact(scale));
    if !norm.hi.is_finite() {
        return Err(numeric("query", "nurbs.nearest_point"));
    }
    Ok(Interval {
        lo: norm.lo.max(0.),
        hi: norm.hi,
    })
}
fn distance_interval(point: BoxPoint, query: Point2) -> GeometryResult<Interval> {
    norm_interval(
        point.x.sub(Interval::exact(query.x)),
        point.y.sub(Interval::exact(query.y)),
    )
}
fn search_lower(controls: &[Homogeneous], origin: Point2, query: Point2) -> GeometryResult<f64> {
    let first = at(controls, 0)?.project(origin)?;
    let last = last(controls)?.project(origin)?;
    let mut bounds = first;
    for control in controls.iter().skip(1) {
        bounds = bounds.hull(control.project(origin)?);
    }
    let box_lower = distance_interval(bounds, query)?.lo;
    let start = first.middle()?;
    let end = last.middle()?;
    let flatness = chord_error(controls, start, end, origin)?;
    let dx = Interval::exact(end.x).sub(Interval::exact(start.x));
    let dy = Interval::exact(end.y).sub(Interval::exact(start.y));
    let length = norm_interval(dx, dy)?;
    if length.hi == 0. {
        return Ok(box_lower);
    }
    let cross = dx
        .mul(Interval::exact(query.y).sub(Interval::exact(start.y)))
        .sub(dy.mul(Interval::exact(query.x).sub(Interval::exact(start.x))));
    let numerator = if cross.lo <= 0. && cross.hi >= 0. {
        0.
    } else {
        cross.lo.abs().min(cross.hi.abs())
    };
    let line_lower = (numerator / length.hi).next_down().max(0.);
    Ok(box_lower.max((line_lower - flatness).next_down().max(0.)))
}
fn consider_nurbs(
    curve: &NurbsCurve,
    prepared: &Prepared,
    query: Point2,
    span_index: usize,
    local_parameters: [f64; 3],
    best: &mut Option<NurbsCandidate>,
) -> GeometryResult<()> {
    let span = at(&prepared.spans, span_index)?;
    for local in local_parameters {
        let offset = Interval::exact(span.start).sub(Interval::exact(prepared.start));
        let width = Interval::exact(span.end).sub(Interval::exact(span.start));
        let parameter = if local == 0. && span.start == prepared.start {
            0.
        } else if local == 1. && span.end == prepared.end {
            1.
        } else {
            offset
                .add(width.mul(Interval::exact(local)))
                .divide(Interval::exact(prepared.end).sub(Interval::exact(prepared.start)))?
                .unit()
                .middle()?
                .clamp(0., 1.)
        };
        let enclosure = if parameter == 0. || parameter == 1. {
            let point = if parameter == 0. {
                *at(&curve.control_points, 0)?
            } else {
                *last(&curve.control_points)?
            };
            BoxPoint {
                x: Interval::exact(point.x),
                y: Interval::exact(point.y),
            }
        } else {
            prepared.evaluate(parameter, false)?
        };
        let range = distance_interval(enclosure, query)?;
        if best
            .as_ref()
            .is_none_or(|candidate| range.hi < candidate.upper)
        {
            let point = enclosure.middle()?;
            let distance = point.distance(query);
            if !distance.is_finite() {
                return Err(numeric("query", "nurbs.nearest_point"));
            }
            *best = Some(NurbsCandidate {
                point,
                parameter,
                distance,
                lower: range.lo,
                upper: range.hi,
                point_error: enclosure.error_from(point),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod checked_span_tests {
    use super::*;

    #[test]
    fn empty_internal_control_sets_return_errors_instead_of_panicking() {
        assert!(casteljau(&[], Interval::exact(0.5)).is_err());
        assert!(split(&[]).is_err());
        assert!(search_lower(&[], Point2::new(0., 0.), Point2::new(1., 1.)).is_err());
    }
}
