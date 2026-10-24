use crate::*;
use std::f64::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
/// Unit-independent intersection classification using the operation's linear and angular
/// tolerances.
#[cfg_attr(feature = "schemars", schemars(transform = crate::unit_enum_schema))]
pub enum IntersectionKind {
    /// Transverse crossing of source curves, classified using explicit operation tolerances.
    Crossing,
    /// Tangency classification or tangent snap candidate in caller coordinates.
    Tangent,
    /// Isolated contact such as a shared endpoint, classified using explicit operation tolerances.
    Touch,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Intersection in caller coordinates with dimensionless parameters on both source curves and an
/// explicit contact classification.
pub struct CurveIntersectionPoint {
    /// Result point in the input Cartesian coordinate system.
    pub point: Point2,
    /// Unit-independent classification of this result.
    pub kind: IntersectionKind,
    /// Normalized parameter: 0-1 on lines and arcs; on circles a counterclockwise fraction of
    /// one turn from the positive x axis.
    pub first_parameter: f64,
    /// Normalized parameter on the second curve; circles use a counterclockwise turn fraction from
    /// +x.
    pub second_parameter: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Positive-length overlapping curves and isolated contacts in caller coordinates, classified using
/// the supplied tolerance.
pub struct CurveOverlap {
    /// Overlapping analytic curves in input coordinate units and direction conventions.
    pub curves: Vec<Curve>,
    /// Isolated contacts outside positive-length overlaps, in the input coordinate system.
    pub isolated_points: Vec<CurveIntersectionPoint>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
/// Intersection result in caller coordinates. Points and overlaps are separate; unsupported pairs
/// return an error.
pub enum CurveIntersection {
    /// No intersection within the operation's tolerance; no geometry payload.
    None,
    /// Isolated intersection points in caller coordinates, with normalized source parameters.
    Points(Vec<CurveIntersectionPoint>),
    /// Positive-length overlap in caller coordinates, distinct from isolated contacts.
    Overlap(CurveOverlap),
}

impl Curve {
    /// Intersect supported analytic curve pairs in the same coordinate system using explicit
    /// linear/angular tolerances; do not repair ambiguity.
    pub fn intersections(
        &self,
        other: &Self,
        tolerance: &GeometryTolerance,
    ) -> GeometryResult<CurveIntersection> {
        self.validate(tolerance)?;
        other.validate(tolerance)?;
        match (self, other) {
            (Self::Line(a), Self::Line(b)) => match a.intersection(b, tolerance)? {
                SegmentIntersection::None => Ok(CurveIntersection::None),
                SegmentIntersection::Overlap(line) => {
                    Ok(CurveIntersection::Overlap(CurveOverlap {
                        curves: vec![Self::Line(line)],
                        isolated_points: vec![],
                    }))
                }
                SegmentIntersection::Point(point) => {
                    let parallel = a
                        .start
                        .vector_to(a.end)
                        .scaled(1. / a.length())
                        .cross(b.start.vector_to(b.end).scaled(1. / b.length()))
                        .abs()
                        <= tolerance.angular;
                    collect_points(
                        self,
                        other,
                        vec![(
                            point,
                            if parallel {
                                IntersectionKind::Touch
                            } else {
                                IntersectionKind::Crossing
                            },
                        )],
                        tolerance,
                    )
                }
            },
            (Self::Line(line), Self::Circle(_) | Self::Arc(_)) => collect_points(
                self,
                other,
                line_circle(*line, support_circle(other)?, tolerance)?,
                tolerance,
            ),
            (Self::Circle(_) | Self::Arc(_), Self::Line(line)) => collect_points(
                self,
                other,
                line_circle(*line, support_circle(self)?, tolerance)?,
                tolerance,
            ),
            (Self::Circle(_) | Self::Arc(_), Self::Circle(_) | Self::Arc(_)) => {
                let a = support_circle(self)?;
                let b = support_circle(other)?;
                if a.center == b.center && a.radius == b.radius {
                    return coincident(self, other, a, tolerance);
                }
                if a.center.distance(b.center) <= tolerance.linear
                    && (a.radius - b.radius).abs() <= tolerance.linear
                {
                    return Err(GeometryError::invalid(
                        GeometryReason::AmbiguousIntersection,
                        "curves",
                        "intersections",
                        "Nearby supporting circles cannot be distinguished as identical or distinct within tolerance",
                    ));
                }
                collect_points(self, other, circle_circle(a, b)?, tolerance)
            }
            _ => Err(GeometryError::unsupported(
                GeometryReason::IntersectionNotSupported,
                "curves",
                "intersections",
                "Intersection is not supported for this curve pair",
            )),
        }
    }
}

fn support_circle(curve: &Curve) -> GeometryResult<Circle> {
    Ok(match curve {
        Curve::Circle(circle) => *circle,
        Curve::Arc(arc) => Circle {
            center: arc.center,
            radius: arc.radius,
        },
        _ => return Err(missing_geometry()),
    })
}
fn line_circle(
    line: LineSegment,
    circle: Circle,
    t: &GeometryTolerance,
) -> GeometryResult<Vec<(Point2, IntersectionKind)>> {
    let direction = line.start.vector_to(line.end).scaled(1. / line.length());
    let offset = line.start.vector_to(circle.center);
    let along = offset.dot(direction);
    let perpendicular = offset.cross(direction).abs();
    finite(along)?;
    finite(perpendicular)?;
    if perpendicular > circle.radius {
        return Ok(vec![]);
    }
    let ratio = perpendicular / circle.radius;
    let half_chord = circle.radius * ((1. - ratio) * (1. + ratio)).max(0.).sqrt();
    finite(half_chord)?;
    let kind = if half_chord == 0. {
        IntersectionKind::Tangent
    } else {
        IntersectionKind::Crossing
    };
    let distances = if half_chord == 0. {
        vec![along]
    } else {
        vec![along - half_chord, along + half_chord]
    };
    let mut points = vec![];
    for distance in distances {
        finite(distance)?;
        if distance >= -t.linear && distance <= line.length() + t.linear {
            let point = line
                .start
                .translated(direction.scaled(distance.clamp(0., line.length())));
            point.validate()?;
            if (point.distance(circle.center) - circle.radius).abs() > t.linear {
                return Err(numerical_intersection());
            }
            points.push((point, kind));
        }
    }
    Ok(points)
}
fn circle_circle(a: Circle, b: Circle) -> GeometryResult<Vec<(Point2, IntersectionKind)>> {
    let delta = a.center.vector_to(b.center);
    let distance = delta.length();
    finite(distance)?;
    if distance == 0. {
        return Ok(vec![]);
    }
    let scale = distance.max(a.radius).max(b.radius);
    let d = distance / scale;
    let r = a.radius / scale;
    let s = b.radius / scale;
    if d > r + s || d < (r - s).abs() {
        return Ok(vec![]);
    }
    let along = (d + (r - s) * (r + s) / d) / 2.;
    let product = (r + s + d) * (-r + s + d) * (r - s + d) * (r + s - d);
    if product < -64. * f64::EPSILON {
        return Err(numerical_intersection());
    }
    let height = product.max(0.).sqrt() / (2. * d);
    let u = delta.scaled(1. / distance);
    let base = a.center.translated(u.scaled(along * scale));
    let transverse = Vector2 { x: -u.y, y: u.x }.scaled(height * scale);
    let first = base.translated(transverse);
    first.validate()?;
    if height == 0. {
        Ok(vec![(first, IntersectionKind::Tangent)])
    } else {
        let second = base.translated(transverse.scaled(-1.));
        second.validate()?;
        Ok(vec![
            (first, IntersectionKind::Crossing),
            (second, IntersectionKind::Crossing),
        ])
    }
}
fn collect_points(
    a: &Curve,
    b: &Curve,
    candidates: Vec<(Point2, IntersectionKind)>,
    t: &GeometryTolerance,
) -> GeometryResult<CurveIntersection> {
    let mut points = vec![];
    for (point, kind) in candidates {
        if let (Some(first_parameter), Some(second_parameter)) =
            (parameter(a, point, t)?, parameter(b, point, t)?)
        {
            let duplicate = points
                .iter()
                .any(|entry: &CurveIntersectionPoint| entry.point.distance(point) <= t.linear);
            if duplicate && kind == IntersectionKind::Crossing {
                return Err(GeometryError::invalid(
                    GeometryReason::AmbiguousIntersection,
                    "points",
                    "intersections",
                    "Two intersection points cannot be resolved at the specified tolerance",
                ));
            }
            if !duplicate {
                points.push(CurveIntersectionPoint {
                    point,
                    kind,
                    first_parameter,
                    second_parameter,
                });
            }
        }
    }
    points.sort_by(|a, b| a.first_parameter.total_cmp(&b.first_parameter));
    Ok(if points.is_empty() {
        CurveIntersection::None
    } else {
        CurveIntersection::Points(points)
    })
}
fn parameter(curve: &Curve, point: Point2, t: &GeometryTolerance) -> GeometryResult<Option<f64>> {
    match curve {
        Curve::Line(line) => {
            let u = line.start.vector_to(line.end).scaled(1. / line.length());
            let delta = line.start.vector_to(point);
            let length = delta.dot(u);
            if delta.cross(u).abs() > t.linear {
                return Err(numerical_intersection());
            }
            Ok(
                if length >= -t.linear && length <= line.length() + t.linear {
                    Some((length / line.length()).clamp(0., 1.))
                } else {
                    None
                },
            )
        }
        Curve::Circle(circle) => {
            if (point.distance(circle.center) - circle.radius).abs() > t.linear {
                return Err(numerical_intersection());
            }
            Ok(Some(
                (point.y - circle.center.y)
                    .atan2(point.x - circle.center.x)
                    .rem_euclid(TAU)
                    / TAU,
            ))
        }
        Curve::Arc(arc) => {
            if (point.distance(arc.center) - arc.radius).abs() > t.linear {
                return Err(numerical_intersection());
            }
            if point.distance(arc.point_at(0.)) <= t.linear {
                return Ok(Some(0.));
            }
            if point.distance(arc.point_at(1.)) <= t.linear {
                return Ok(Some(1.));
            }
            let (s, c) = arc.start_angle.sin_cos();
            let start = s.atan2(c);
            let angle = (point.y - arc.center.y).atan2(point.x - arc.center.x);
            let delta = if arc.sweep_angle > 0. {
                (angle - start).rem_euclid(TAU)
            } else {
                (start - angle).rem_euclid(TAU)
            };
            let parameter = delta / arc.sweep_angle.abs();
            Ok(if parameter <= 1. {
                Some(parameter)
            } else {
                None
            })
        }
        _ => Err(missing_geometry()),
    }
}
fn intervals(curve: &Curve) -> GeometryResult<Vec<(f64, f64)>> {
    Ok(match curve {
        Curve::Circle(_) => vec![(0., TAU)],
        Curve::Arc(arc) => {
            if arc.sweep_angle.abs() >= TAU {
                return Ok(vec![(0., TAU)]);
            }
            let point = if arc.sweep_angle > 0. {
                arc.point_at(0.)
            } else {
                arc.point_at(1.)
            };
            let start = (point.y - arc.center.y)
                .atan2(point.x - arc.center.x)
                .rem_euclid(TAU);
            let end = start + arc.sweep_angle.abs();
            if end <= TAU {
                vec![(start, end)]
            } else {
                vec![(start, TAU), (0., end - TAU)]
            }
        }
        _ => return Err(missing_geometry()),
    })
}
fn coincident(
    a: &Curve,
    b: &Curve,
    circle: Circle,
    t: &GeometryTolerance,
) -> GeometryResult<CurveIntersection> {
    let mut ranges: Vec<(f64, f64)> = vec![];
    let mut contacts = vec![];
    for left in intervals(a)? {
        for right in intervals(b)? {
            let lo = left.0.max(right.0);
            let hi = left.1.min(right.1);
            if hi >= lo {
                if (hi - lo) * circle.radius > t.linear {
                    ranges.push((lo, hi));
                } else {
                    if hi > lo {
                        return Err(GeometryError::invalid(
                            GeometryReason::AmbiguousIntersection,
                            "overlap",
                            "intersections",
                            "Positive overlap length cannot be resolved at the specified tolerance",
                        ));
                    }
                    let angle = (lo + hi) / 2.;
                    contacts.push((
                        Point2::new(
                            circle.center.x + circle.radius * angle.cos(),
                            circle.center.y + circle.radius * angle.sin(),
                        ),
                        IntersectionKind::Touch,
                    ));
                }
            }
        }
    }
    // Zero and 2*pi coincide, so check contacts at the ring seam separately.
    let seam = Point2::new(circle.center.x + circle.radius, circle.center.y);
    if parameter(a, seam, t)?.is_some() && parameter(b, seam, t)?.is_some() {
        contacts.push((seam, IntersectionKind::Touch));
    }
    ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = vec![];
    for range in ranges {
        if let Some(last) = merged.last_mut()
            && range.0 <= last.1
        {
            last.1 = last.1.max(range.1);
            continue;
        }
        merged.push(range);
    }
    if merged.len() > 1
        && merged.first().is_some_and(|first| first.0 == 0.)
        && merged.last().is_some_and(|last| last.1 == TAU)
    {
        let first = merged.remove(0);
        let last = merged.last_mut().ok_or_else(missing_geometry)?;
        last.1 = TAU + first.1;
    }
    let curves: Vec<Curve> = merged
        .iter()
        .map(|(start, end)| {
            if end - start >= TAU {
                Curve::Circle(circle)
            } else {
                Curve::Arc(Arc {
                    center: circle.center,
                    radius: circle.radius,
                    start_angle: *start,
                    sweep_angle: end - start,
                })
            }
        })
        .collect();
    // Do not report overlap endpoints again as isolated contacts.
    let mut isolated_contacts = Vec::new();
    for contact in contacts {
        let mut covered = false;
        for curve in &curves {
            if parameter(curve, contact.0, t)?.is_some() {
                covered = true;
                break;
            }
        }
        if !covered {
            isolated_contacts.push(contact);
        }
    }
    let isolated = match collect_points(a, b, isolated_contacts, t)? {
        CurveIntersection::Points(points) => points,
        _ => vec![],
    };
    if curves.is_empty() {
        Ok(if isolated.is_empty() {
            CurveIntersection::None
        } else {
            CurveIntersection::Points(isolated)
        })
    } else {
        Ok(CurveIntersection::Overlap(CurveOverlap {
            curves,
            isolated_points: isolated,
        }))
    }
}
fn numerical_intersection() -> GeometryError {
    GeometryError::invalid(
        GeometryReason::NumericalErrorBudgetExceeded,
        "intersection",
        "intersections",
        "Intersection exceeds the numerical error budget",
    )
}

#[cfg(test)]
mod checked_dispatch_tests {
    use super::*;

    #[test]
    fn unsupported_internal_dispatch_returns_errors_instead_of_panicking() {
        let curve = Curve::Polyline(Polyline {
            points: vec![],
            closed: false,
        });
        assert!(support_circle(&curve).is_err());
        assert!(intervals(&curve).is_err());
        assert!(parameter(&curve, Point2::new(0., 0.), &GeometryTolerance::default()).is_err());
    }
}
