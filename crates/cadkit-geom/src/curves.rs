use crate::*;
use std::f64::consts::{PI, TAU};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Directed Cartesian line segment in caller units. Validation requires finite endpoints and length
/// above the supplied linear tolerance.
pub struct LineSegment {
    /// Starting endpoint in caller Cartesian units.
    pub start: Point2,
    /// Ending endpoint in caller Cartesian units.
    pub end: Point2,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Circular arc in caller coordinates. Angles are radians from +x, counterclockwise for positive
/// sweep, with at most one turn.
pub struct Arc {
    /// Center in caller Cartesian units.
    pub center: Point2,
    /// Radius in caller coordinate units; must exceed the supplied linear tolerance.
    pub radius: f64,
    /// Start angle in radians from +x, positive counterclockwise.
    pub start_angle: f64,
    /// Signed sweep in radians; positive is counterclockwise, absolute value at most one turn.
    pub sweep_angle: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Circle in caller coordinates with a positive radius greater than the supplied linear tolerance.
pub struct Circle {
    /// Center in caller Cartesian units.
    pub center: Point2,
    /// Radius in caller coordinate units; must exceed the supplied linear tolerance.
    pub radius: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Affine ellipse center + axis_u*cos(angle) + axis_v*sin(angle). Axes use caller units; angle is
/// in radians. Axes need not be perpendicular.
pub struct Ellipse {
    /// Center in caller Cartesian units.
    pub center: Point2,
    /// Cosine axis vector in caller coordinate units.
    pub axis_u: Vector2,
    /// Sine axis vector in caller coordinate units.
    pub axis_v: Vector2,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Polynomial Bezier curve of degree 1-3 in caller coordinates, using a normalized parameter in 0-1
/// and explicit operation tolerances.
pub struct Bezier {
    /// Ordered control points in caller Cartesian units.
    pub control_points: Vec<Point2>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Ordered Cartesian vertices in caller units. Closed paths close implicitly; validity depends on
/// the operation's supplied tolerance.
pub struct Polyline {
    /// Ordered vertices in caller Cartesian units.
    pub points: Vec<Point2>,
    /// Whether the path closes implicitly from its last vertex to its first; do not repeat the
    /// first vertex.
    pub closed: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
/// Analytic Cartesian curve in caller units. Angles are radians; operations explicitly validate
/// their tolerance and supported profile.
pub enum Curve {
    /// Directed line segment in caller coordinate units.
    Line(LineSegment),
    /// Circular arc in caller units with signed radian sweep.
    Arc(Arc),
    /// Circle in caller units; parameter starts at +x and runs counterclockwise.
    Circle(Circle),
    /// Affine ellipse in caller units, parameterized by an angle in radians.
    Ellipse(Ellipse),
    /// Polynomial Bezier curve in caller units with normalized parameter 0-1.
    Bezier(Bezier),
    /// Positive-weight rational curve in caller units with normalized active knot domain 0-1.
    Nurbs(NurbsCurve),
    /// Ordered line segments in caller units, optionally implicitly closed.
    Polyline(Polyline),
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
/// Segment intersection classified with the supplied linear and angular tolerance, in caller
/// coordinates.
pub enum SegmentIntersection {
    /// No intersection within the operation's tolerance; no geometry payload.
    None,
    /// Isolated contact point in caller coordinate units.
    Point(Point2),
    /// Positive-length overlap in caller coordinates, distinct from isolated contacts.
    Overlap(LineSegment),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
/// Unit-independent classification of an analytic snap candidate; selection and display thresholds
/// belong to the caller.
#[cfg_attr(feature = "schemars", schemars(transform = crate::unit_enum_schema))]
pub enum SnapKind {
    /// Curve endpoint in caller coordinates.
    Endpoint,
    /// Point at normalized parameter 0.5; not necessarily half the arc length.
    Midpoint,
    /// Curve center in caller coordinates.
    Center,
    /// Common area or analytic intersection, according to the enclosing result or operation; input
    /// coordinate units are retained.
    Intersection,
    /// Nearest boundary candidate in caller coordinates.
    Nearest,
    /// Orthogonal projection within a segment, in caller coordinates.
    Perpendicular,
    /// Tangency classification or tangent snap candidate in caller coordinates.
    Tangent,
    /// Point on the caller-specified Cartesian grid.
    Grid,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Snap point and query distance in caller coordinate units; no screen-space convention is imposed.
pub struct SnapCandidate {
    /// Result point in the input Cartesian coordinate system.
    pub point: Point2,
    /// Unit-independent classification of this result.
    pub kind: SnapKind,
    /// Euclidean distance to the query in caller coordinate units.
    pub distance: f64,
}

pub(crate) fn strict_tolerance() -> GeometryTolerance {
    GeometryTolerance {
        linear: f64::MIN_POSITIVE,
        angular: f64::MIN_POSITIVE,
        area: f64::MIN_POSITIVE,
    }
}

impl LineSegment {
    /// Validate finite values and the supported profile. Where supplied, tolerances use caller
    /// units, radians and squared units; no repair or unit conversion is performed.
    pub fn validate(&self, t: &GeometryTolerance) -> GeometryResult<()> {
        t.validate()?;
        self.start.validate()?;
        self.end.validate()?;
        let length = self.start.distance(self.end);
        finite(length)?;
        if length <= t.linear {
            return Err(GeometryError::invalid(
                GeometryReason::BelowTolerance,
                "length",
                "line.validate",
                "Line length is below tolerance",
            ));
        }
        Ok(())
    }
    /// Length in caller coordinate units. Where supplied, the tolerance controls validation and
    /// numerical integration; raw segment/vector length applies no tolerance.
    pub fn length(&self) -> f64 {
        self.start.distance(self.end)
    }
    /// Return a nearest boundary point in caller coordinates using the supplied tolerance;
    /// unsupported or ambiguous cases return a typed error.
    pub fn nearest_point(&self, p: Point2, t: &GeometryTolerance) -> GeometryResult<Point2> {
        self.validate(t)?;
        p.validate()?;
        let direction = self.start.vector_to(self.end).scaled(1. / self.length());
        let distance = self
            .start
            .vector_to(p)
            .dot(direction)
            .clamp(0., self.length());
        let result = self.start.translated(direction.scaled(distance));
        result.validate()?;
        Ok(result)
    }
    /// Intersect directed segments in caller coordinates using the supplied linear and angular
    /// tolerance; distinguish isolated points from positive-length overlaps.
    pub fn intersection(
        &self,
        other: &Self,
        t: &GeometryTolerance,
    ) -> GeometryResult<SegmentIntersection> {
        self.validate(t)?;
        other.validate(t)?;
        let r = self.start.vector_to(self.end);
        let s = other.start.vector_to(other.end);
        let lr = r.length();
        let ls = s.length();
        let u = r.scaled(1. / lr);
        let v = s.scaled(1. / ls);
        let q = self.start.vector_to(other.start);
        let cross = u.cross(v);
        finite(cross)?;
        // Angularly parallel lines require both endpoints to lie on the same supporting line.
        if cross.abs() <= t.angular {
            if q.cross(u).abs() > t.linear
                || self.start.vector_to(other.end).cross(u).abs() > t.linear
            {
                // Near parallelism must not hide a real interior intersection.
                if cross == 0. {
                    return Ok(SegmentIntersection::None);
                }
            } else {
                let a = q.dot(u);
                let b = self.start.vector_to(other.end).dot(u);
                let lo = 0_f64.max(a.min(b));
                let hi = lr.min(a.max(b));
                if hi < lo - t.linear {
                    return Ok(SegmentIntersection::None);
                }
                if hi - lo <= t.linear {
                    return Ok(SegmentIntersection::Point(
                        self.start.translated(u.scaled((lo + hi) / 2.)),
                    ));
                }
                return Ok(SegmentIntersection::Overlap(Self {
                    start: self.start.translated(u.scaled(lo)),
                    end: self.start.translated(u.scaled(hi)),
                }));
            }
        }
        let a = q.cross(v) / cross;
        let b = q.cross(u) / cross;
        finite(a)?;
        finite(b)?;
        if a < -t.linear || a > lr + t.linear || b < -t.linear || b > ls + t.linear {
            return Ok(SegmentIntersection::None);
        }
        let p = self.start.translated(u.scaled(a.clamp(0., lr)));
        let other_p = other.start.translated(v.scaled(b.clamp(0., ls)));
        if p.distance(other_p) > t.linear {
            return Err(GeometryError::invalid(
                GeometryReason::NumericalErrorBudgetExceeded,
                "intersection",
                "line.intersection",
                "Intersection exceeds the numerical error budget",
            ));
        }
        p.validate()?;
        Ok(SegmentIntersection::Point(p))
    }
}

impl Arc {
    /// Evaluate an unchecked normalized arc fraction; 0 is the start and 1 the end. Returns caller
    /// coordinates, with signed radian sweep and no tolerance adjustment.
    pub fn point_at(&self, fraction: f64) -> Point2 {
        // Rotate the direction vector relatively instead of adding a small angle to a large
        // start angle.
        let (start_sin, start_cos) = self.start_angle.sin_cos();
        let (sin, cos) = (self.sweep_angle * fraction).sin_cos();
        Point2::new(
            self.center.x + self.radius * (start_cos * cos - start_sin * sin),
            self.center.y + self.radius * (start_sin * cos + start_cos * sin),
        )
    }
    fn contains_angle(&self, angle: f64) -> bool {
        if self.sweep_angle.abs() >= TAU {
            return true;
        }
        let (sin, cos) = self.start_angle.sin_cos();
        let start = sin.atan2(cos);
        let delta = if self.sweep_angle >= 0. {
            (angle - start).rem_euclid(TAU)
        } else {
            (start - angle).rem_euclid(TAU)
        };
        delta <= self.sweep_angle.abs()
    }
}
impl Ellipse {
    /// Evaluate an unchecked angle in radians using center + axis_u*cos(angle) + axis_v*sin(angle).
    /// Returns caller coordinates with no tolerance adjustment.
    pub fn point_at(&self, angle: f64) -> Point2 {
        let (s, c) = angle.sin_cos();
        self.center.translated(Vector2 {
            x: self.axis_u.x * c + self.axis_v.x * s,
            y: self.axis_u.y * c + self.axis_v.y * s,
        })
    }
}
impl Bezier {
    /// Evaluate a finite normalized parameter in 0-1 using de Casteljau interpolation in caller
    /// units. Validates degree and points; applies no tolerance.
    pub fn point_at(&self, fraction: f64) -> GeometryResult<Point2> {
        finite(fraction).map_err(|error| error.at("fraction", "bezier.point_at"))?;
        if !(0. ..=1.).contains(&fraction) {
            return Err(GeometryError::invalid(
                GeometryReason::InvalidParameterRange,
                "fraction",
                "bezier.point_at",
                "Bezier parameter must be between 0 and 1",
            ));
        }
        if !(2..=4).contains(&self.control_points.len()) {
            return Err(GeometryError::unsupported(
                GeometryReason::UnsupportedDegree,
                "controlPoints",
                "bezier.validate",
                "Bezier degree must be between 1 and 3",
            ));
        }
        let mut points = self.control_points.clone();
        for p in &points {
            p.validate()?;
        }
        for count in (1..points.len()).rev() {
            for index in 0..count {
                let value = at(&points, index)?.lerp(*at(&points, index + 1)?, fraction);
                *at_mut(&mut points, index)? = value;
            }
        }
        let point = *at(&points, 0)?;
        point.validate()?;
        Ok(point)
    }
    fn derivative(&self, fraction: f64) -> GeometryResult<Vector2> {
        let degree = self
            .control_points
            .len()
            .checked_sub(1)
            .ok_or_else(missing_geometry)?;
        let mut points: Vec<Vector2> = self
            .control_points
            .iter()
            .zip(self.control_points.iter().skip(1))
            .map(|(a, b)| a.vector_to(*b).scaled(degree as f64))
            .collect();
        for count in (1..points.len()).rev() {
            for i in 0..count {
                let a = *at(&points, i)?;
                let b = *at(&points, i + 1)?;
                *at_mut(&mut points, i)? = Vector2 {
                    x: a.x * (1. - fraction) + b.x * fraction,
                    y: a.y * (1. - fraction) + b.y * fraction,
                };
            }
        }
        Ok(*at(&points, 0)?)
    }
}
impl Polyline {
    /// Return consecutive directed segments in caller coordinates, adding last-to-first for closed
    /// paths with at least two vertices. Does not validate geometry or apply tolerances.
    pub fn segments(&self) -> Vec<LineSegment> {
        let mut result: Vec<_> = self
            .points
            .iter()
            .zip(self.points.iter().skip(1))
            .map(|(a, b)| LineSegment { start: *a, end: *b })
            .collect();
        if self.closed
            && self.points.len() > 1
            && let (Some(start), Some(end)) = (self.points.last(), self.points.first())
        {
            result.push(LineSegment {
                start: *start,
                end: *end,
            });
        }
        result
    }
}

impl Curve {
    /// Validate finite values and the supported profile. Where supplied, tolerances use caller
    /// units, radians and squared units; no repair or unit conversion is performed.
    pub fn validate(&self, t: &GeometryTolerance) -> GeometryResult<()> {
        t.validate()?;
        match self {
            Self::Line(v) => v.validate(t),
            Self::Nurbs(v) => v.validate(t),
            Self::Circle(v) => validate_radius(v.center, v.radius, t),
            Self::Arc(v) => {
                validate_radius(v.center, v.radius, t)?;
                finite(v.start_angle).map_err(|error| error.at("startAngle", "arc.validate"))?;
                finite(v.sweep_angle).map_err(|error| error.at("sweepAngle", "arc.validate"))?;
                if v.sweep_angle.abs() <= t.angular || v.sweep_angle.abs() > TAU {
                    return Err(GeometryError::invalid(
                        GeometryReason::InvalidParameterRange,
                        "sweepAngle",
                        "arc.validate",
                        "Arc sweep must exceed the tolerance and be at most one full turn",
                    ));
                }
                if v.radius * v.sweep_angle.abs() <= t.linear {
                    return Err(GeometryError::invalid(
                        GeometryReason::BelowTolerance,
                        "length",
                        "arc.validate",
                        "Arc length is below tolerance",
                    ));
                }
                Ok(())
            }
            Self::Ellipse(v) => {
                v.center.validate()?;
                for n in [v.axis_u.x, v.axis_u.y, v.axis_v.x, v.axis_v.y] {
                    finite(n)?;
                }
                let a = v.axis_u.length();
                let b = v.axis_v.length();
                finite(a)?;
                finite(b)?;
                if a <= t.linear
                    || b <= t.linear
                    || v.axis_u.scaled(1. / a).cross(v.axis_v.scaled(1. / b)).abs() <= t.angular
                {
                    return Err(GeometryError::invalid(
                        GeometryReason::DegenerateAxes,
                        "axes",
                        "ellipse.validate",
                        "Ellipse axes must be independent and longer than the tolerance",
                    ));
                }
                Ok(())
            }
            Self::Bezier(v) => {
                if !(2..=4).contains(&v.control_points.len()) {
                    return Err(GeometryError::unsupported(
                        GeometryReason::UnsupportedDegree,
                        "controlPoints",
                        "bezier.validate",
                        "Bezier degree must be between 1 and 3",
                    ));
                }
                for p in &v.control_points {
                    p.validate()?;
                }
                for (a, b) in v.control_points.iter().zip(v.control_points.iter().skip(1)) {
                    finite(a.distance(*b))
                        .map_err(|error| error.at("controlPoints", "bezier.validate"))?;
                }
                let first = *at(&v.control_points, 0)?;
                if v.control_points
                    .iter()
                    .all(|p| p.distance(first) <= t.linear)
                {
                    return Err(GeometryError::invalid(
                        GeometryReason::CoincidentControlPoints,
                        "controlPoints",
                        "bezier.validate",
                        "Bezier control points coincide",
                    ));
                }
                Ok(())
            }
            Self::Polyline(v) => {
                if v.points.len() < if v.closed { 3 } else { 2 } {
                    return Err(GeometryError::invalid(
                        GeometryReason::InsufficientPoints,
                        "points",
                        "polyline.validate",
                        "Insufficient polyline points",
                    ));
                }
                for s in v.segments() {
                    s.validate(t)?;
                }
                Ok(())
            }
        }
    }
    /// Compute axis-aligned bounds in caller coordinates; curve validity is checked with the strict
    /// numerical profile, while polygon bounds only check exterior points. No padding is added.
    pub fn bounds(&self) -> GeometryResult<Bounds2> {
        self.validate(&strict_tolerance())?;
        let mut points = Vec::new();
        match self {
            Self::Line(v) => points.extend([v.start, v.end]),
            Self::Nurbs(v) => return v.bounds(&strict_tolerance()),
            Self::Circle(v) => points.extend([
                Point2::new(v.center.x - v.radius, v.center.y - v.radius),
                Point2::new(v.center.x + v.radius, v.center.y + v.radius),
            ]),
            Self::Arc(v) => {
                points.extend([v.point_at(0.), v.point_at(1.)]);
                for angle in [0., PI / 2., PI, 3. * PI / 2.] {
                    if v.contains_angle(angle) {
                        points.push(polar(v.center, v.radius, angle));
                    }
                }
            }
            Self::Ellipse(v) => {
                let dx = v.axis_u.x.hypot(v.axis_v.x);
                let dy = v.axis_u.y.hypot(v.axis_v.y);
                points.extend([
                    Point2::new(v.center.x - dx, v.center.y - dy),
                    Point2::new(v.center.x + dx, v.center.y + dy),
                ]);
            }
            Self::Bezier(v) => {
                points.extend([*at(&v.control_points, 0)?, *last(&v.control_points)?]);
                for axis in 0..2 {
                    let values: Vec<f64> = v
                        .control_points
                        .iter()
                        .map(|p| if axis == 0 { p.x } else { p.y })
                        .collect();
                    for root in bezier_extrema(&values) {
                        if root > 0. && root < 1. {
                            points.push(v.point_at(root)?);
                        }
                    }
                }
            }
            Self::Polyline(v) => points.extend(&v.points),
        }
        Bounds2::from_points(&points)
    }
    /// Length in caller coordinate units. Where supplied, the tolerance controls validation and
    /// numerical integration; raw segment/vector length applies no tolerance.
    pub fn length(&self, t: &GeometryTolerance) -> GeometryResult<f64> {
        self.validate(t)?;
        let result = match self {
            Self::Line(v) => v.length(),
            Self::Nurbs(_) => {
                return Err(GeometryError::unsupported(
                    GeometryReason::MeasurementNotSupported,
                    "curve",
                    "nurbs.length",
                    "A NURBS length algorithm with an error budget is not yet implemented",
                ));
            }
            Self::Circle(v) => TAU * v.radius,
            Self::Arc(v) => v.radius * v.sweep_angle.abs(),
            Self::Polyline(v) => v.segments().iter().map(LineSegment::length).sum(),
            Self::Ellipse(v) => integrate(
                |a| {
                    let (s, c) = a.sin_cos();
                    Ok((-v.axis_u.x * s + v.axis_v.x * c).hypot(-v.axis_u.y * s + v.axis_v.y * c))
                },
                0.,
                TAU,
                t.linear,
            )?,
            Self::Bezier(v) => integrate(|a| Ok(v.derivative(a)?.length()), 0., 1., t.linear)?,
        };
        finite(result)?;
        Ok(result)
    }
    /// Compute enclosed area in squared coordinate units independently of display
    /// tessellation. The supplied tolerance controls validity and numerical integration where
    /// applicable.
    pub fn enclosed_area(&self, t: &GeometryTolerance) -> GeometryResult<f64> {
        self.validate(t)?;
        let result = match self {
            Self::Circle(v) => PI * v.radius * v.radius,
            Self::Nurbs(_) => {
                return Err(GeometryError::unsupported(
                    GeometryReason::MeasurementNotSupported,
                    "curve",
                    "nurbs.enclosed_area",
                    "A certified NURBS area algorithm is not yet implemented",
                ));
            }
            Self::Ellipse(v) => PI * v.axis_u.cross(v.axis_v).abs(),
            Self::Polyline(v) if v.closed => {
                return Polygon {
                    exterior: v.points.clone(),
                    holes: vec![],
                }
                .area(t);
            }
            _ => {
                return Err(GeometryError::unsupported(
                    GeometryReason::NotClosed,
                    "curve",
                    "enclosed_area",
                    "This curve does not define an enclosed area on its own",
                ));
            }
        };
        finite(result)?;
        Ok(result)
    }
    /// Transform source geometry to target coordinates using the supplied tolerance. Reject
    /// singular transforms, unsupported representations or certified numerical loss.
    pub fn transformed(
        &self,
        transform: Transform2,
        t: &GeometryTolerance,
    ) -> GeometryResult<Self> {
        self.validate(t)?;
        transform.inverse(t)?;
        let result = match self {
            Self::Line(v) => Self::Line(LineSegment {
                start: transform.apply_point(v.start),
                end: transform.apply_point(v.end),
            }),
            Self::Circle(v) => match transform.similarity_scale(t) {
                Ok(scale) => Self::Circle(Circle {
                    center: transform.apply_point(v.center),
                    radius: v.radius * scale,
                }),
                Err(GeometryError::Unsupported(_)) => Self::Ellipse(Ellipse {
                    center: transform.apply_point(v.center),
                    axis_u: transform.apply_vector(Vector2 { x: v.radius, y: 0. }),
                    axis_v: transform.apply_vector(Vector2 { x: 0., y: v.radius }),
                }),
                Err(e) => return Err(e),
            },
            Self::Arc(v) => {
                let scale = transform.similarity_scale(t)?;
                let center = transform.apply_point(v.center);
                let direction = transform.apply_vector(Vector2 {
                    x: v.start_angle.cos(),
                    y: v.start_angle.sin(),
                });
                Self::Arc(Arc {
                    center,
                    radius: v.radius * scale,
                    start_angle: direction.y.atan2(direction.x),
                    sweep_angle: if transform.determinant() < 0. {
                        -v.sweep_angle
                    } else {
                        v.sweep_angle
                    },
                })
            }
            Self::Ellipse(v) => Self::Ellipse(Ellipse {
                center: transform.apply_point(v.center),
                axis_u: transform.apply_vector(v.axis_u),
                axis_v: transform.apply_vector(v.axis_v),
            }),
            Self::Bezier(v) => Self::Bezier(Bezier {
                control_points: v
                    .control_points
                    .iter()
                    .map(|p| transform.apply_point(*p))
                    .collect(),
            }),
            Self::Nurbs(v) => Self::Nurbs(v.transformed(transform, t)?),
            Self::Polyline(v) => Self::Polyline(Polyline {
                points: v.points.iter().map(|p| transform.apply_point(*p)).collect(),
                closed: v.closed,
            }),
        };
        result.validate(t)?;
        Ok(result)
    }
    /// Return a nearest boundary point in caller coordinates using the supplied tolerance;
    /// unsupported or ambiguous cases return a typed error.
    pub fn nearest_point(&self, p: Point2, t: &GeometryTolerance) -> GeometryResult<Point2> {
        self.validate(t)?;
        p.validate()?;
        let result = match self {
            Self::Line(v) => v.nearest_point(p, t)?,
            Self::Circle(v) => {
                let direction = v.center.vector_to(p);
                if direction.length() <= t.linear {
                    return Err(GeometryError::invalid(
                        GeometryReason::AmbiguousNearestPoint,
                        "query",
                        "circle.nearest_point",
                        "The nearest circle point is not unique at the center",
                    ));
                }
                v.center
                    .translated(direction.scaled(v.radius / direction.length()))
            }
            Self::Arc(v) => {
                let direction = v.center.vector_to(p);
                if direction.length() <= t.linear {
                    return Err(GeometryError::invalid(
                        GeometryReason::AmbiguousNearestPoint,
                        "query",
                        "arc.nearest_point",
                        "The nearest arc point is not unique at the center",
                    ));
                }
                let angle = direction.y.atan2(direction.x);
                if v.contains_angle(angle) {
                    polar(v.center, v.radius, angle)
                } else {
                    let a = v.point_at(0.);
                    let b = v.point_at(1.);
                    if a.distance(p) <= b.distance(p) { a } else { b }
                }
            }
            Self::Polyline(v) => {
                let candidates: GeometryResult<Vec<_>> =
                    v.segments().iter().map(|s| s.nearest_point(p, t)).collect();
                nearest(candidates?, p)?
            }
            Self::Ellipse(_) | Self::Bezier(_) | Self::Nurbs(_) => {
                self.nearest_point_with_options(p, NearestPointOptions::for_tolerance(t), t)?
                    .point
            }
        };
        result.validate()?;
        Ok(result)
    }
    /// Tessellate for display only, with max_error in coordinate units. Length and area
    /// measurements do not use this approximation; supplied tolerances still validate the
    /// source.
    pub fn tessellate(&self, max_error: f64, t: &GeometryTolerance) -> GeometryResult<Vec<Point2>> {
        self.validate(t)?;
        positive(max_error, "Approximation error")
            .map_err(|error| error.at("maxError", "tessellate"))?;
        if !matches!(self, Self::Line(_) | Self::Polyline(_)) {
            let bounds = self.bounds()?;
            let magnitude = [bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y]
                .into_iter()
                .map(f64::abs)
                .fold(0., f64::max);
            if max_error < 4. * f64::EPSILON * magnitude {
                return Err(GeometryError::invalid(
                    GeometryReason::NumericalErrorBudgetExceeded,
                    "maxError",
                    "tessellate",
                    "Approximation error is below world coordinate resolution",
                ));
            }
        }
        let result = match self {
            Self::Line(v) => vec![v.start, v.end],
            Self::Nurbs(v) => v.tessellate(max_error, t)?,
            Self::Polyline(v) => {
                let mut points = v.points.clone();
                if v.closed {
                    points.push(*at(&v.points, 0)?);
                }
                points
            }
            Self::Circle(v) => sample_arc(v.radius, TAU, max_error, |f| {
                polar(v.center, v.radius, TAU * f)
            })?,
            Self::Arc(v) => sample_arc(v.radius, v.sweep_angle, max_error, |f| v.point_at(f))?,
            Self::Ellipse(v) => {
                let bound = v.axis_u.length() + v.axis_v.length();
                sample_arc(bound, TAU, max_error, |f| v.point_at(TAU * f))?
            }
            Self::Bezier(v) => {
                let mut points = vec![*at(&v.control_points, 0)?];
                flatten_bezier(&v.control_points, max_error, 0, &mut points)?;
                points
            }
        };
        for p in &result {
            p.validate()?;
        }
        Ok(result)
    }
    /// Return analytic candidates within max_distance in caller units, ordered by distance. The
    /// supplied tolerance validates geometry and deduplicates matching kinds.
    pub fn snap_candidates(
        &self,
        query: Point2,
        max_distance: f64,
        t: &GeometryTolerance,
    ) -> GeometryResult<Vec<SnapCandidate>> {
        self.validate(t)?;
        query.validate()?;
        positive(max_distance, "Snap distance")
            .map_err(|error| error.at("maxDistance", "snap_candidates"))?;
        let mut raw = Vec::new();
        match self {
            Self::Line(v) => {
                raw.extend([
                    (v.start, SnapKind::Endpoint),
                    (v.end, SnapKind::Endpoint),
                    (v.start.lerp(v.end, 0.5), SnapKind::Midpoint),
                ]);
                let d = v.start.vector_to(v.end).scaled(1. / v.length());
                let along = v.start.vector_to(query).dot(d);
                if (0. ..=v.length()).contains(&along) {
                    raw.push((v.start.translated(d.scaled(along)), SnapKind::Perpendicular));
                }
            }
            Self::Arc(v) => raw.extend([
                (v.point_at(0.), SnapKind::Endpoint),
                (v.point_at(1.), SnapKind::Endpoint),
                (v.point_at(0.5), SnapKind::Midpoint),
                (v.center, SnapKind::Center),
            ]),
            Self::Circle(v) => {
                raw.push((v.center, SnapKind::Center));
                let d = v.center.distance(query);
                if d > v.radius + t.linear {
                    let base = (query.y - v.center.y).atan2(query.x - v.center.x);
                    let offset = (v.radius / d).acos();
                    raw.extend([
                        (polar(v.center, v.radius, base + offset), SnapKind::Tangent),
                        (polar(v.center, v.radius, base - offset), SnapKind::Tangent),
                    ]);
                }
            }
            Self::Ellipse(v) => raw.push((v.center, SnapKind::Center)),
            Self::Bezier(v) => raw.extend([
                (*at(&v.control_points, 0)?, SnapKind::Endpoint),
                (*last(&v.control_points)?, SnapKind::Endpoint),
                (v.point_at(0.5)?, SnapKind::Midpoint),
            ]),
            Self::Nurbs(v) => raw.extend([
                (v.point_at(0., t)?, SnapKind::Endpoint),
                (v.point_at(1., t)?, SnapKind::Endpoint),
                (v.point_at(0.5, t)?, SnapKind::Midpoint),
            ]),
            Self::Polyline(v) => {
                for s in v.segments() {
                    raw.extend([
                        (s.start, SnapKind::Endpoint),
                        (s.end, SnapKind::Endpoint),
                        (s.start.lerp(s.end, 0.5), SnapKind::Midpoint),
                    ]);
                }
            }
        }
        {
            let ambiguous = match self {
                Self::Circle(v) => v.center.distance(query) <= t.linear,
                Self::Arc(v) => v.center.distance(query) <= t.linear,
                Self::Ellipse(v) => {
                    let a = v.axis_u.length();
                    let b = v.axis_v.length();
                    v.center.distance(query) <= t.linear
                        && (a - b).abs() <= t.linear
                        && v.axis_u.scaled(1. / a).dot(v.axis_v.scaled(1. / b)).abs() <= t.angular
                }
                _ => false,
            };
            if !ambiguous {
                raw.push((self.nearest_point(query, t)?, SnapKind::Nearest));
            }
        }
        let mut result: Vec<SnapCandidate> = raw
            .into_iter()
            .filter_map(|(point, kind)| {
                let distance = point.distance(query);
                if distance <= max_distance {
                    Some(SnapCandidate {
                        point,
                        kind,
                        distance,
                    })
                } else {
                    None
                }
            })
            .collect();
        result.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        result.dedup_by(|a, b| a.kind == b.kind && a.point.distance(b.point) <= t.linear);
        Ok(result)
    }
}

/// Round to the nearest grid coordinate relative to origin, using positive spacing in caller units.
/// Halfway cases round away from zero relative to the grid; no tolerance is applied.
pub fn grid_snap(point: Point2, origin: Point2, spacing: f64) -> GeometryResult<SnapCandidate> {
    point.validate()?;
    origin.validate()?;
    positive(spacing, "Grid spacing").map_err(|error| error.at("spacing", "grid_snap"))?;
    let snapped = Point2::new(
        origin.x + ((point.x - origin.x) / spacing).round() * spacing,
        origin.y + ((point.y - origin.y) / spacing).round() * spacing,
    );
    snapped.validate()?;
    Ok(SnapCandidate {
        point: snapped,
        kind: SnapKind::Grid,
        distance: point.distance(snapped),
    })
}
fn validate_radius(center: Point2, radius: f64, t: &GeometryTolerance) -> GeometryResult<()> {
    center.validate()?;
    positive(radius, "Radius").map_err(|error| error.at("radius", "curve.validate"))?;
    if radius <= t.linear {
        return Err(GeometryError::invalid(
            GeometryReason::BelowTolerance,
            "radius",
            "curve.validate",
            "Radius is below tolerance",
        ));
    }
    Ok(())
}
fn polar(center: Point2, radius: f64, angle: f64) -> Point2 {
    let (s, c) = angle.sin_cos();
    Point2::new(center.x + radius * c, center.y + radius * s)
}
fn nearest(points: Vec<Point2>, query: Point2) -> GeometryResult<Point2> {
    points
        .into_iter()
        .min_by(|a, b| a.distance(query).total_cmp(&b.distance(query)))
        .ok_or_else(|| {
            GeometryError::invalid(
                GeometryReason::InsufficientPoints,
                "candidates",
                "nearest_point",
                "No nearest-point candidate",
            )
        })
}
fn bezier_extrema(p: &[f64]) -> Vec<f64> {
    // Derivative coefficients use differences to reduce cancellation from large common
    // coordinates.
    let mut d: Vec<f64> = p.iter().zip(p.iter().skip(1)).map(|(a, b)| b - a).collect();
    let scale = d.iter().copied().map(f64::abs).fold(0., f64::max);
    if scale == 0. {
        return vec![];
    }
    for value in &mut d {
        *value /= scale;
    }
    match d.as_slice() {
        [_] => vec![],
        [a, b] => {
            let denominator = a - b;
            if denominator == 0. {
                vec![]
            } else {
                vec![a / denominator]
            }
        }
        [a, b, c] => quadratic_roots(a - 2. * b + c, 2. * (b - a), *a),
        _ => vec![],
    }
}
fn quadratic_roots(a: f64, b: f64, c: f64) -> Vec<f64> {
    if a == 0. {
        return if b == 0. { vec![] } else { vec![-c / b] };
    }
    let scale = a.abs().max(b.abs()).max(c.abs());
    let (a, b, c) = (a / scale, b / scale, c / scale);
    let disc = b * b - 4. * a * c;
    if disc < 0. {
        return vec![];
    }
    let q = -0.5 * (b + disc.sqrt().copysign(b));
    if q == 0. {
        vec![-b / (2. * a)]
    } else {
        vec![q / a, c / q]
    }
}
fn sample_arc(
    radius_bound: f64,
    sweep: f64,
    error: f64,
    point: impl Fn(f64) -> Point2,
) -> GeometryResult<Vec<Point2>> {
    // The asin/sqrt form avoids cancellation in acos(1-e/r) for small error budgets.
    let step = 4. * (error / (2. * radius_bound)).min(0.5).sqrt().asin();
    let count = (sweep.abs() / step).ceil().max(2.);
    if !count.is_finite() || count > 1_000_000. {
        return Err(GeometryError::unsupported(
            GeometryReason::ApproximationLimit,
            "maxError",
            "tessellate",
            "Curve tessellation exceeds the point limit",
        ));
    }
    let count = count as usize;
    Ok((0..=count)
        .map(|i| point(i as f64 / count as f64))
        .collect())
}
fn flatten_bezier(
    points: &[Point2],
    error: f64,
    depth: u32,
    out: &mut Vec<Point2>,
) -> GeometryResult<()> {
    let first = *at(points, 0)?;
    let end = *last(points)?;
    let chord = LineSegment { start: first, end };
    let flat = if chord.length() == 0. {
        points.iter().all(|p| p.distance(first) <= error)
    } else {
        let t = strict_tolerance();
        points.iter().all(|p| {
            chord
                .nearest_point(*p, &t)
                .is_ok_and(|q| q.distance(*p) <= error)
        })
    };
    if flat {
        out.push(end);
        return Ok(());
    }
    if depth >= 32 || out.len() >= 1_000_000 {
        return Err(GeometryError::unsupported(
            GeometryReason::ApproximationLimit,
            "maxError",
            "bezier.tessellate",
            "Bezier tessellation did not converge within the error budget",
        ));
    }
    let mut work = points.to_vec();
    let mut left = vec![first];
    let mut right = vec![end];
    for n in (1..points.len()).rev() {
        for i in 0..n {
            let value = at(&work, i)?.lerp(*at(&work, i + 1)?, 0.5);
            *at_mut(&mut work, i)? = value;
        }
        left.push(*at(&work, 0)?);
        right.push(*at(&work, n - 1)?);
    }
    right.reverse();
    flatten_bezier(&left, error, depth + 1, out)?;
    flatten_bezier(&right, error, depth + 1, out)
}
fn integrate(
    f: impl Fn(f64) -> GeometryResult<f64>,
    a: f64,
    b: f64,
    error: f64,
) -> GeometryResult<f64> {
    fn step(
        f: &impl Fn(f64) -> GeometryResult<f64>,
        a: f64,
        b: f64,
        values: [f64; 3],
        whole: f64,
        error: f64,
        depth: u32,
    ) -> GeometryResult<f64> {
        let m = (a + b) / 2.;
        let l = f((a + m) / 2.)?;
        let r = f((m + b) / 2.)?;
        finite(l)?;
        finite(r)?;
        let left = (m - a) * (values[0] + 4. * l + values[1]) / 6.;
        let right = (b - m) * (values[1] + 4. * r + values[2]) / 6.;
        let difference = left + right - whole;
        if difference.abs() <= 15. * error {
            return Ok(left + right + difference / 15.);
        }
        if depth == 0 {
            return Err(GeometryError::unsupported(
                GeometryReason::IntegrationNotConverged,
                "tolerance.linear",
                "length",
                "Length integration did not converge within the error budget",
            ));
        }
        Ok(step(
            f,
            a,
            m,
            [values[0], l, values[1]],
            left,
            error / 2.,
            depth - 1,
        )? + step(
            f,
            m,
            b,
            [values[1], r, values[2]],
            right,
            error / 2.,
            depth - 1,
        )?)
    }
    // Initial subdivisions prevent periodic integrands from producing a false zero error
    // estimate.
    let mut sum = 0.;
    for i in 0..16 {
        let lo = a + (b - a) * i as f64 / 16.;
        let hi = a + (b - a) * (i + 1) as f64 / 16.;
        let values = [f(lo)?, f((lo + hi) / 2.)?, f(hi)?];
        for v in values {
            finite(v)?;
        }
        let whole = (hi - lo) * (values[0] + 4. * values[1] + values[2]) / 6.;
        sum += step(&f, lo, hi, values, whole, error / 16., 24)?;
    }
    finite(sum)?;
    Ok(sum)
}
