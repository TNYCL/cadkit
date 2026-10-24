use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
/// Join policy for signed offsets in caller coordinate units. Positive offsets lie left of the
/// directed path; limits are explicit.
pub enum OffsetJoin {
    /// Extend offset supports to their intersection, subject to a dimensionless miter limit.
    Miter {
        /// Dimensionless miter-length to absolute-offset ratio; must be finite and at least 1.
        limit: f64,
    },
    /// Connect outer offset corners with a straight segment in caller coordinates.
    Bevel,
}

impl LineSegment {
    /// Retain the ordered subsegment at normalized parameters 0-1, preserving direction. The
    /// supplied tolerance uses the segment's coordinate units.
    pub fn trim(
        &self,
        start_parameter: f64,
        end_parameter: f64,
        t: &GeometryTolerance,
    ) -> GeometryResult<Self> {
        self.validate(t)?;
        range(start_parameter, end_parameter, 1., "line.trim")?;
        let result = Self {
            start: self.start.lerp(self.end, start_parameter),
            end: self.start.lerp(self.end, end_parameter),
        };
        result.validate(t)?;
        check_position(
            self,
            result.start,
            start_parameter * self.length(),
            0.,
            t,
            "line.trim",
        )?;
        check_position(
            self,
            result.end,
            end_parameter * self.length(),
            0.,
            t,
            "line.trim",
        )?;
        Ok(result)
    }
    /// Positive distances extend the start backward and the end forward in coordinate units.
    /// Use trim to shorten; the supplied tolerance validates the result.
    pub fn extend(
        &self,
        start_distance: f64,
        end_distance: f64,
        t: &GeometryTolerance,
    ) -> GeometryResult<Self> {
        self.validate(t)?;
        nonnegative(start_distance, "startDistance", "line.extend")?;
        nonnegative(end_distance, "endDistance", "line.extend")?;
        let direction = self.start.vector_to(self.end).scaled(1. / self.length());
        let result = Self {
            start: self.start.translated(direction.scaled(-start_distance)),
            end: self.end.translated(direction.scaled(end_distance)),
        };
        result.validate(t)?;
        check_position(self, result.start, -start_distance, 0., t, "line.extend")?;
        check_position(
            self,
            result.end,
            self.length() + end_distance,
            0.,
            t,
            "line.extend",
        )?;
        Ok(result)
    }
    /// Offset in coordinate units; positive distance is left of the directed segment. The
    /// supplied tolerance validates the resulting distance.
    pub fn offset(&self, distance: f64, t: &GeometryTolerance) -> GeometryResult<Self> {
        self.validate(t)?;
        finite(distance).map_err(|e| e.at("distance", "line.offset"))?;
        let direction = self.start.vector_to(self.end).scaled(1. / self.length());
        let shift = Vector2 {
            x: -direction.y,
            y: direction.x,
        }
        .scaled(distance);
        let result = Self {
            start: self.start.translated(shift),
            end: self.end.translated(shift),
        };
        result.validate(t)?;
        check_position(self, result.start, 0., distance, t, "line.offset")?;
        check_position(self, result.end, self.length(), distance, t, "line.offset")?;
        Ok(result)
    }
}
impl Polyline {
    /// Retain an interval of physical arc length measured from the start of an open simple
    /// path, in coordinate units. The supplied tolerance validates the result.
    pub fn trim(
        &self,
        start_distance: f64,
        end_distance: f64,
        t: &GeometryTolerance,
    ) -> GeometryResult<Self> {
        require_open(self, "polyline.trim")?;
        validate_simple(self, t)?;
        let segments = self.segments();
        let total: f64 = segments.iter().map(LineSegment::length).sum();
        finite(total)?;
        range(start_distance, end_distance, total, "polyline.trim")?;
        let mut points = vec![point_at_distance(&segments, start_distance, t)?];
        let mut cumulative = 0.;
        for segment in &segments {
            cumulative += segment.length();
            if cumulative > start_distance && cumulative < end_distance {
                points.push(segment.end);
            }
        }
        points.push(point_at_distance(&segments, end_distance, t)?);
        let result = Self {
            points,
            closed: false,
        };
        validate_simple(&result, t)?;
        Ok(result)
    }
    /// Extend an open simple path at both ends by nonnegative distances in coordinate units,
    /// retaining direction and validating with the supplied tolerance.
    pub fn extend(
        &self,
        start_distance: f64,
        end_distance: f64,
        t: &GeometryTolerance,
    ) -> GeometryResult<Self> {
        require_open(self, "polyline.extend")?;
        validate_simple(self, t)?;
        nonnegative(start_distance, "startDistance", "polyline.extend")?;
        nonnegative(end_distance, "endDistance", "polyline.extend")?;
        let segments = self.segments();
        let first = at(&segments, 0)?.extend(start_distance, 0., t)?;
        let last = last(&segments)?.extend(0., end_distance, t)?;
        let mut result = self.clone();
        *at_mut(&mut result.points, 0)? = first.start;
        let last_index = result.points.len() - 1;
        *at_mut(&mut result.points, last_index)? = last.end;
        validate_simple(&result, t)?;
        Ok(result)
    }
    /// Offset a simple linear path in coordinate units; positive distance is left of its
    /// direction. Open endpoints retain butt ends. Joins and supplied tolerances are explicit.
    pub fn offset(
        &self,
        distance: f64,
        join: OffsetJoin,
        t: &GeometryTolerance,
    ) -> GeometryResult<Self> {
        validate_simple(self, t)?;
        finite(distance).map_err(|e| e.at("distance", "polyline.offset"))?;
        if let OffsetJoin::Miter { limit } = join {
            finite(limit).map_err(|e| e.at("join.limit", "polyline.offset"))?;
            if limit < 1. {
                return Err(GeometryError::invalid(
                    GeometryReason::InvalidParameterRange,
                    "join.limit",
                    "polyline.offset",
                    "Miter limit must be at least 1",
                ));
            }
        }
        if distance == 0. {
            return Ok(self.clone());
        }
        let segments = self.segments();
        let directions: Vec<_> = segments
            .iter()
            .map(|line| line.start.vector_to(line.end).scaled(1. / line.length()))
            .collect();
        let mut vertices: Vec<Vec<Point2>> = Vec::with_capacity(self.points.len());
        for (index, point) in self.points.iter().copied().enumerate() {
            if !self.closed && index == 0 {
                vertices.push(vec![
                    point.translated(normal(*at(&directions, 0)?).scaled(distance)),
                ]);
                continue;
            }
            if !self.closed && index == self.points.len() - 1 {
                vertices.push(vec![
                    point.translated(normal(*last(&directions)?).scaled(distance)),
                ]);
                continue;
            }
            let previous = *at(
                &directions,
                (index + directions.len() - 1) % directions.len(),
            )?;
            let next = *at(&directions, index % directions.len())?;
            let a = normal(previous);
            let b = normal(next);
            let dot = previous.dot(next);
            let turn = previous.cross(next);
            let denominator = 1. + dot;
            if denominator <= 64. * f64::EPSILON {
                return Err(GeometryError::invalid(
                    GeometryReason::OffsetCollapsed,
                    "corner",
                    "polyline.offset",
                    "A reversing corner has no finite offset join",
                ));
            }
            let miter = Vector2 {
                x: a.x + b.x,
                y: a.y + b.y,
            }
            .scaled(distance / denominator);
            let ratio = miter.length() / distance.abs();
            finite(ratio)?;
            let outside = turn * distance < 0.;
            match join {
                OffsetJoin::Miter { limit } => {
                    if ratio > limit {
                        return Err(GeometryError::invalid(
                            GeometryReason::MiterLimitExceeded,
                            "join.limit",
                            "polyline.offset",
                            "Corner exceeds the miter limit; join type cannot change implicitly",
                        ));
                    }
                    vertices.push(vec![point.translated(miter)]);
                }
                OffsetJoin::Bevel if outside && turn.abs() > t.angular => vertices.push(vec![
                    point.translated(a.scaled(distance)),
                    point.translated(b.scaled(distance)),
                ]),
                OffsetJoin::Bevel => vertices.push(vec![point.translated(miter)]),
            }
        }
        // Every source edge must retain its direction; a collapsed contour that grows again is
        // not valid.
        for (index, direction) in directions.iter().enumerate() {
            let start = *at(&vertices, index)?.last().ok_or_else(offset_collapsed)?;
            let end = *at(at(&vertices, (index + 1) % vertices.len())?, 0)?;
            let span = start.vector_to(end);
            for point in [start, end] {
                let actual = at(&segments, index)?
                    .start
                    .vector_to(point)
                    .dot(normal(*direction));
                finite(actual)?;
                if (actual - distance).abs() > t.linear {
                    return Err(GeometryError::invalid(
                        GeometryReason::NumericalErrorBudgetExceeded,
                        "distance",
                        "polyline.offset",
                        "Coordinate resolution cannot preserve the offset distance",
                    ));
                }
            }
            if span.dot(*direction) <= t.linear {
                return Err(offset_collapsed());
            }
            if span.cross(*direction).abs() > t.linear {
                return Err(GeometryError::invalid(
                    GeometryReason::NumericalErrorBudgetExceeded,
                    "distance",
                    "polyline.offset",
                    "Offset edge exceeds the parallelism error budget",
                ));
            }
        }
        let result = Self {
            points: vertices.into_iter().flatten().collect(),
            closed: self.closed,
        };
        validate_simple(&result, t)?;
        Ok(result)
    }
}
fn range(start: f64, end: f64, maximum: f64, operation: &str) -> GeometryResult<()> {
    finite(start).map_err(|e| e.at("start", operation))?;
    finite(end).map_err(|e| e.at("end", operation))?;
    if start < 0. || end > maximum || start >= end {
        return Err(GeometryError::invalid(
            GeometryReason::InvalidParameterRange,
            "range",
            operation,
            "The retained range must be ordered and within source bounds",
        ));
    }
    Ok(())
}
fn nonnegative(value: f64, field: &str, operation: &str) -> GeometryResult<()> {
    finite(value).map_err(|e| e.at(field, operation))?;
    if value < 0. {
        return Err(GeometryError::invalid(
            GeometryReason::InvalidParameterRange,
            field,
            operation,
            "Uzatma mesafesi negatif olamaz",
        ));
    }
    Ok(())
}
fn require_open(polyline: &Polyline, operation: &str) -> GeometryResult<()> {
    if polyline.closed {
        return Err(GeometryError::unsupported(
            GeometryReason::EditingNotSupported,
            "closed",
            operation,
            "This trim/extend profile only supports open polylines",
        ));
    }
    Ok(())
}
fn normal(direction: Vector2) -> Vector2 {
    Vector2 {
        x: -direction.y,
        y: direction.x,
    }
}
fn offset_collapsed() -> GeometryError {
    GeometryError::invalid(
        GeometryReason::OffsetCollapsed,
        "distance",
        "polyline.offset",
        "Offset collapses or reverses an edge",
    )
}
fn point_at_distance(
    segments: &[LineSegment],
    distance: f64,
    t: &GeometryTolerance,
) -> GeometryResult<Point2> {
    let mut remaining = distance;
    for (index, segment) in segments.iter().enumerate() {
        if remaining <= segment.length() || index == segments.len() - 1 {
            let result = segment
                .start
                .lerp(segment.end, (remaining / segment.length()).clamp(0., 1.));
            result.validate()?;
            check_position(segment, result, remaining, 0., t, "polyline.trim")?;
            return Ok(result);
        }
        remaining -= segment.length();
    }
    Err(GeometryError::invalid(
        GeometryReason::InsufficientPoints,
        "segments",
        "polyline.trim",
        "No polyline segment",
    ))
}
fn validate_simple(polyline: &Polyline, t: &GeometryTolerance) -> GeometryResult<()> {
    Curve::Polyline(polyline.clone()).validate(t)?;
    let segments = polyline.segments();
    for i in 0..segments.len() {
        for j in i + 1..segments.len() {
            let adjacent = j == i + 1 || (polyline.closed && i == 0 && j == segments.len() - 1);
            match at(&segments, i)?.intersection(at(&segments, j)?, t)? {
                SegmentIntersection::None => {}
                SegmentIntersection::Point(point) if adjacent => {
                    let expected = if j == i + 1 {
                        at(&segments, i)?.end
                    } else {
                        at(&segments, i)?.start
                    };
                    if point.distance(expected) > t.linear {
                        return Err(GeometryError::invalid(
                            GeometryReason::InvalidTopology,
                            "points",
                            "polyline.edit",
                            "Adjacent edges intersect away from their shared vertex",
                        ));
                    }
                }
                _ => {
                    return Err(GeometryError::invalid(
                        GeometryReason::InvalidTopology,
                        "points",
                        "polyline.edit",
                        "This editing profile requires a path without self-intersections or overlaps",
                    ));
                }
            }
        }
    }
    if polyline.closed && signed_ring_area(&polyline.points)?.abs() <= t.area {
        return Err(GeometryError::invalid(
            GeometryReason::BelowTolerance,
            "area",
            "polyline.edit",
            "Closed path area is below tolerance",
        ));
    }
    Ok(())
}

fn check_position(
    line: &LineSegment,
    point: Point2,
    along: f64,
    across: f64,
    t: &GeometryTolerance,
    operation: &str,
) -> GeometryResult<()> {
    finite(along)?;
    finite(across)?;
    let direction = line.start.vector_to(line.end).scaled(1. / line.length());
    let delta = line.start.vector_to(point);
    let actual_along = delta.dot(direction);
    let actual_across = delta.dot(normal(direction));
    finite(actual_along)?;
    finite(actual_across)?;
    if (actual_along - along).abs() > t.linear || (actual_across - across).abs() > t.linear {
        return Err(GeometryError::invalid(
            GeometryReason::NumericalErrorBudgetExceeded,
            "point",
            operation,
            "Coordinate resolution cannot preserve the edit distance",
        ));
    }
    Ok(())
}
