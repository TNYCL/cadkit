use crate::*;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::f64::consts::{PI, TAU};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Distance certification budget in caller units and an explicit dimensionless iteration limit.
pub struct NearestPointOptions {
    /// Maximum error of the returned distance relative to the global minimum, in coordinate
    /// units; not a point-position bound.
    pub max_distance_error: f64,
    /// Dimensionless iteration cap, from 1 through 1000000.
    pub max_iterations: u32,
}
impl NearestPointOptions {
    /// Use tolerance.linear as the coordinate-unit distance budget and the default iteration cap;
    /// validation occurs when an operation runs.
    pub fn for_tolerance(t: &GeometryTolerance) -> Self {
        Self {
            max_distance_error: t.linear,
            max_iterations: 262_144,
        }
    }
    /// Validate finite values and the supported profile. Where supplied, tolerances use caller
    /// units, radians and squared units; no repair or unit conversion is performed.
    pub fn validate(self) -> GeometryResult<()> {
        positive(self.max_distance_error, "Nearest-distance error budget")
            .map_err(|e| e.at("maxDistanceError", "nearest_point"))?;
        if self.max_iterations == 0 || self.max_iterations > 1_000_000 {
            return Err(GeometryError::invalid(
                GeometryReason::InvalidParameterRange,
                "maxIterations",
                "nearest_point",
                "Iteration limit must be between 1 and 1000000",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Nearest-point result in caller coordinates. The error bound certifies distance, not uniqueness
/// or position accuracy.
pub struct NearestPointResult {
    /// Result point in the input Cartesian coordinate system.
    pub point: Point2,
    /// Dimensionless source parameter: normalized 0-1; circles use counterclockwise turns from +x,
    /// polylines normalize segment index plus local fraction.
    pub parameter: f64,
    /// Euclidean distance to the query in caller coordinate units.
    pub distance: f64,
    /// Certified distance error in coordinate units; does not bound the returned point's position.
    pub distance_error_bound: f64,
    /// Number of search subdivisions performed; dimensionless.
    pub iterations: u32,
    /// True only when global distance bounds converge within the requested coordinate-unit
    /// budget.
    pub converged: bool,
}

impl Curve {
    /// Search with an explicit coordinate-unit distance-error budget and iteration cap. The
    /// supplied tolerance validates geometry; success certifies distance, not point uniqueness.
    pub fn nearest_point_with_options(
        &self,
        query: Point2,
        options: NearestPointOptions,
        t: &GeometryTolerance,
    ) -> GeometryResult<NearestPointResult> {
        if let Curve::Nurbs(curve) = self {
            return curve.nearest_point_with_options(query, options, t);
        }
        self.validate(t)?;
        query.validate()?;
        options.validate()?;
        let bounds = self.bounds()?;
        let scale = [
            bounds.min.x,
            bounds.min.y,
            bounds.max.x,
            bounds.max.y,
            query.x,
            query.y,
        ]
        .into_iter()
        .map(f64::abs)
        .fold(0., f64::max);
        let roundoff = 64. * f64::EPSILON * scale;
        if options.max_distance_error <= 2. * roundoff {
            return Err(GeometryError::invalid(
                GeometryReason::NumericalErrorBudgetExceeded,
                "maxDistanceError",
                "nearest_point",
                "Nearest-distance budget is below coordinate resolution",
            ));
        }
        if !matches!(self, Curve::Ellipse(_) | Curve::Bezier(_)) {
            let point = self.nearest_point(query, t)?;
            let distance = point.distance(query);
            finite(distance)?;
            return Ok(NearestPointResult {
                point,
                parameter: analytic_parameter(self, point, t)?,
                distance,
                distance_error_bound: 2. * roundoff,
                iterations: 0,
                converged: true,
            });
        }
        if let Curve::Ellipse(ellipse) = self {
            let a = ellipse.axis_u.length();
            let b = ellipse.axis_v.length();
            if (a - b).abs() <= t.linear
                && ellipse
                    .axis_u
                    .scaled(1. / a)
                    .dot(ellipse.axis_v.scaled(1. / b))
                    .abs()
                    <= t.angular
                && query.distance(ellipse.center) <= t.linear
            {
                return Err(GeometryError::invalid(
                    GeometryReason::AmbiguousNearestPoint,
                    "query",
                    "ellipse.nearest_point",
                    "The nearest point is not unique at the center of a circular ellipse",
                ));
            }
        }
        let root = match self {
            Curve::Bezier(curve) => NodeShape::Bezier(curve.control_points.clone()),
            Curve::Ellipse(ellipse) => NodeShape::Ellipse(*ellipse),
            _ => return Err(missing_geometry()),
        };
        let mut best = Candidate {
            point: point_at(self, 0.)?,
            parameter: 0.,
            distance: f64::INFINITY,
            start: 0.,
            end: 1.,
        };
        let node = build_node(self, root, 0., 1., query, &mut best)?;
        let mut queue = BinaryHeap::from([node]);
        let mut iterations = 0;
        loop {
            let lower = queue.peek().map_or(best.distance, |node| node.lower);
            let error = (best.distance - lower + 2. * roundoff).max(2. * roundoff);
            if error <= options.max_distance_error {
                refine(self, query, &mut best)?;
                return Ok(NearestPointResult {
                    point: best.point,
                    parameter: best.parameter,
                    distance: best.distance,
                    distance_error_bound: (best.distance - lower + 2. * roundoff)
                        .max(2. * roundoff),
                    iterations,
                    converged: true,
                });
            }
            if iterations >= options.max_iterations {
                return Err(GeometryError::unsupported(
                    GeometryReason::ApproximationLimit,
                    "maxIterations",
                    "nearest_point",
                    "Nearest-distance search did not converge within the iteration budget",
                ));
            }
            let node = queue.pop().ok_or_else(|| {
                GeometryError::invalid(
                    GeometryReason::InsufficientPoints,
                    "candidates",
                    "nearest_point",
                    "No nearest-point search interval remains",
                )
            })?;
            iterations += 1;
            if node.lower > best.distance {
                continue;
            }
            let middle = (node.start + node.end) / 2.;
            if middle == node.start || middle == node.end {
                return Err(GeometryError::invalid(
                    GeometryReason::NumericalErrorBudgetExceeded,
                    "parameter",
                    "nearest_point",
                    "The parameter interval cannot be subdivided further",
                ));
            }
            let (left, right) = node.shape.split()?;
            for (shape, start, end) in [(left, node.start, middle), (right, middle, node.end)] {
                let next = build_node(self, shape, start, end, query, &mut best)?;
                if next.lower <= best.distance {
                    queue.push(next);
                }
            }
        }
    }
}
struct Candidate {
    point: Point2,
    parameter: f64,
    distance: f64,
    start: f64,
    end: f64,
}
#[derive(Clone)]
enum NodeShape {
    Bezier(Vec<Point2>),
    Ellipse(Ellipse),
}
impl NodeShape {
    fn split(self) -> GeometryResult<(Self, Self)> {
        Ok(match self {
            Self::Ellipse(value) => (Self::Ellipse(value), Self::Ellipse(value)),
            Self::Bezier(points) => {
                let mut work = points.clone();
                let mut left = vec![*at(&points, 0)?];
                let mut right = vec![*last(&points)?];
                for n in (1..points.len()).rev() {
                    for i in 0..n {
                        let value = at(&work, i)?.lerp(*at(&work, i + 1)?, 0.5);
                        *at_mut(&mut work, i)? = value;
                    }
                    left.push(*at(&work, 0)?);
                    right.push(*at(&work, n - 1)?);
                }
                right.reverse();
                (Self::Bezier(left), Self::Bezier(right))
            }
        })
    }
}
struct SearchNode {
    lower: f64,
    start: f64,
    end: f64,
    shape: NodeShape,
}
impl PartialEq for SearchNode {
    fn eq(&self, other: &Self) -> bool {
        self.lower == other.lower && self.start == other.start && self.end == other.end
    }
}
impl Eq for SearchNode {}
impl PartialOrd for SearchNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for SearchNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .lower
            .total_cmp(&self.lower)
            .then_with(|| other.start.total_cmp(&self.start))
            .then_with(|| other.end.total_cmp(&self.end))
    }
}
fn build_node(
    curve: &Curve,
    shape: NodeShape,
    start: f64,
    end: f64,
    query: Point2,
    best: &mut Candidate,
) -> GeometryResult<SearchNode> {
    let first = point_at(curve, start)?;
    let last = point_at(curve, end)?;
    let (_, fraction) = segment_distance(first, last, query);
    for parameter in [
        start,
        (start + end) / 2.,
        end,
        start + (end - start) * fraction,
    ] {
        consider(curve, query, parameter, start, end, best)?;
    }
    let (bounds, flatness) = match &shape {
        NodeShape::Bezier(points) => {
            let bounds = Bounds2::from_points(points)?;
            let flat = points
                .iter()
                .map(|p| segment_distance(first, last, *p).0)
                .fold(0., f64::max);
            (bounds, flat)
        }
        NodeShape::Ellipse(ellipse) => {
            let mut points = vec![first, last];
            let lo = start * TAU;
            let hi = end * TAU;
            for angle in [
                ellipse.axis_v.x.atan2(ellipse.axis_u.x),
                ellipse.axis_v.y.atan2(ellipse.axis_u.y),
            ] {
                for angle in [angle.rem_euclid(TAU), (angle + PI).rem_euclid(TAU)] {
                    if angle >= lo && angle <= hi {
                        points.push(ellipse.point_at(angle));
                    }
                }
            }
            let flat = (ellipse.axis_u.length() + ellipse.axis_v.length()) * (hi - lo).powi(2) / 8.;
            (Bounds2::from_points(&points)?, flat)
        }
    };
    let box_distance = (query.x - bounds.max.x)
        .max(bounds.min.x - query.x)
        .max(0.)
        .hypot((query.y - bounds.max.y).max(bounds.min.y - query.y).max(0.));
    let lower = box_distance.max((segment_distance(first, last, query).0 - flatness).max(0.));
    finite(lower)?;
    Ok(SearchNode {
        lower,
        start,
        end,
        shape,
    })
}
fn point_at(curve: &Curve, parameter: f64) -> GeometryResult<Point2> {
    let point = match curve {
        Curve::Bezier(curve) => curve.point_at(parameter)?,
        Curve::Ellipse(ellipse) => ellipse.point_at(parameter * TAU),
        _ => return Err(missing_geometry()),
    };
    point.validate()?;
    Ok(point)
}
fn segment_distance(start: Point2, end: Point2, query: Point2) -> (f64, f64) {
    let length = start.distance(end);
    if length == 0. {
        return (start.distance(query), 0.);
    }
    let direction = start.vector_to(end).scaled(1. / length);
    let along = start.vector_to(query).dot(direction).clamp(0., length);
    let point = start.translated(direction.scaled(along));
    (query.distance(point), along / length)
}
fn consider(
    curve: &Curve,
    query: Point2,
    parameter: f64,
    start: f64,
    end: f64,
    best: &mut Candidate,
) -> GeometryResult<()> {
    let point = point_at(curve, parameter)?;
    let distance = point.distance(query);
    finite(distance)?;
    if distance < best.distance {
        *best = Candidate {
            point,
            parameter,
            distance,
            start,
            end,
        };
    }
    Ok(())
}
fn refine(curve: &Curve, query: Point2, best: &mut Candidate) -> GeometryResult<()> {
    // Local refinement only lowers the upper bound; global acceptance uses the lower bound.
    let (mut left, mut right) = (best.start, best.end);
    let ratio = (5_f64.sqrt() - 1.) / 2.;
    let mut x = right - ratio * (right - left);
    let mut y = left + ratio * (right - left);
    let mut fx = point_at(curve, x)?.distance(query);
    let mut fy = point_at(curve, y)?.distance(query);
    for _ in 0..64 {
        if fx < fy {
            right = y;
            y = x;
            fy = fx;
            x = right - ratio * (right - left);
            fx = point_at(curve, x)?.distance(query);
        } else {
            left = x;
            x = y;
            fx = fy;
            y = left + ratio * (right - left);
            fy = point_at(curve, y)?.distance(query);
        }
    }
    consider(curve, query, x, left, right, best)?;
    consider(curve, query, y, left, right, best)
}
fn analytic_parameter(curve: &Curve, point: Point2, t: &GeometryTolerance) -> GeometryResult<f64> {
    Ok(match curve {
        Curve::Line(line) => line.start.distance(point) / line.length(),
        Curve::Circle(circle) => {
            (point.y - circle.center.y)
                .atan2(point.x - circle.center.x)
                .rem_euclid(TAU)
                / TAU
        }
        Curve::Arc(arc) => {
            let (s, c) = arc.start_angle.sin_cos();
            let start = s.atan2(c);
            let angle = (point.y - arc.center.y).atan2(point.x - arc.center.x);
            if point.distance(arc.point_at(0.)) <= t.linear {
                0.
            } else if point.distance(arc.point_at(1.)) <= t.linear {
                1.
            } else {
                let delta = if arc.sweep_angle > 0. {
                    (angle - start).rem_euclid(TAU)
                } else {
                    (start - angle).rem_euclid(TAU)
                };
                delta / arc.sweep_angle.abs()
            }
        }
        Curve::Polyline(polyline) => {
            let segments = polyline.segments();
            let mut result = None;
            for (index, segment) in segments.iter().enumerate() {
                if segment.nearest_point(point, t)?.distance(point) <= t.linear {
                    result = Some(
                        (index as f64 + segment.start.distance(point) / segment.length())
                            / segments.len() as f64,
                    );
                    break;
                }
            }
            result.ok_or_else(|| {
                GeometryError::invalid(
                    GeometryReason::NumericalErrorBudgetExceeded,
                    "parameter",
                    "nearest_point",
                    "Polyline parameter exceeds the error budget",
                )
            })?
        }
        _ => return Err(missing_geometry()),
    })
}

#[cfg(test)]
mod checked_search_tests {
    use super::*;

    #[test]
    fn malformed_search_state_returns_errors_instead_of_panicking() {
        assert!(NodeShape::Bezier(vec![]).split().is_err());
        let curve = Curve::Line(LineSegment {
            start: Point2::new(0., 0.),
            end: Point2::new(1., 0.),
        });
        assert!(point_at(&curve, 0.5).is_err());
        let ellipse = Curve::Ellipse(Ellipse {
            center: Point2::new(0., 0.),
            axis_u: Vector2 { x: 2., y: 0. },
            axis_v: Vector2 { x: 0., y: 1. },
        });
        assert!(
            analytic_parameter(&ellipse, Point2::new(2., 0.), &GeometryTolerance::default())
                .is_err()
        );
    }
}
