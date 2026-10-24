use crate::*;

/// Polygon rings close implicitly from last to first; do not repeat the first point. Winding
/// is preserved. Area is absolute exterior area minus hole areas, in squared caller units.
/// Operations take an explicit tolerance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Polygon {
    /// Exterior ring in caller coordinates, implicitly closed without a repeated first vertex;
    /// either winding is accepted.
    pub exterior: Vec<Point2>,
    /// Interior rings in caller coordinates, strictly inside the exterior and mutually disjoint
    /// under the supplied tolerance.
    pub holes: Vec<Vec<Point2>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
/// Unit-independent relation to a polygon; boundary classification uses the operation's linear
/// tolerance.
#[cfg_attr(feature = "schemars", schemars(transform = crate::unit_enum_schema))]
pub enum PointLocation {
    /// Outside the polygon including hole interiors; boundary tolerance is applied separately.
    Outside,
    /// Within the linear tolerance of an exterior or hole edge.
    Boundary,
    /// Strictly inside the exterior and outside all holes under the supplied tolerance.
    Inside,
}

impl Polygon {
    /// Validate finite values and the supported profile. Where supplied, tolerances use caller
    /// units, radians and squared units; no repair or unit conversion is performed.
    pub fn validate(&self, t: &GeometryTolerance) -> GeometryResult<()> {
        t.validate()?;
        validate_ring(&self.exterior, t)?;
        for (index, hole) in self.holes.iter().enumerate() {
            validate_ring(hole, t)?;
            if rings_intersect(&self.exterior, hole, t)?
                || ring_location(&self.exterior, *at(hole, 0)?, t)? != PointLocation::Inside
            {
                return Err(GeometryError::invalid(
                    GeometryReason::HoleOutsideBoundary,
                    "holes",
                    "polygon.validate",
                    "A hole must be strictly inside the exterior boundary without touching it",
                ));
            }
            for previous in self.holes.iter().take(index) {
                if rings_intersect(previous, hole, t)?
                    || ring_location(previous, *at(hole, 0)?, t)? != PointLocation::Outside
                    || ring_location(hole, *at(previous, 0)?, t)? != PointLocation::Outside
                {
                    return Err(GeometryError::invalid(
                        GeometryReason::IntersectingHoles,
                        "holes",
                        "polygon.validate",
                        "Holes must not intersect, touch or contain one another",
                    ));
                }
            }
        }
        let area = self.raw_area()?;
        if area <= t.area {
            return Err(GeometryError::invalid(
                GeometryReason::BelowTolerance,
                "area",
                "polygon.validate",
                "Polygon area is below tolerance",
            ));
        }
        Ok(())
    }
    /// Return exterior absolute area minus holes in squared caller units, validated with the
    /// supplied tolerance.
    pub fn area(&self, t: &GeometryTolerance) -> GeometryResult<f64> {
        self.validate(t)?;
        self.raw_area()
    }
    fn raw_area(&self) -> GeometryResult<f64> {
        let mut result = signed_ring_area(&self.exterior)?.abs();
        for hole in &self.holes {
            result -= signed_ring_area(hole)?.abs();
        }
        finite(result)?;
        Ok(result)
    }
    /// Sum exterior and hole edge lengths in caller units after validation with the supplied
    /// tolerance.
    pub fn perimeter(&self, t: &GeometryTolerance) -> GeometryResult<f64> {
        self.validate(t)?;
        let value = std::iter::once(&self.exterior)
            .chain(&self.holes)
            .flat_map(|r| ring_segments(r))
            .map(|s| s.length())
            .sum();
        finite(value)?;
        Ok(value)
    }
    /// Compute axis-aligned bounds in caller coordinates; curve validity is checked with the strict
    /// numerical profile, while polygon bounds only check exterior points. No padding is added.
    pub fn bounds(&self) -> GeometryResult<Bounds2> {
        Bounds2::from_points(&self.exterior)
    }
    /// Classify a point in the polygon's coordinate system. Boundary distance uses the supplied or
    /// retained linear tolerance.
    pub fn locate(&self, point: Point2, t: &GeometryTolerance) -> GeometryResult<PointLocation> {
        self.validate(t)?;
        point.validate()?;
        combine_locations(
            ring_location(&self.exterior, point, t)?,
            self.holes.iter().map(|hole| ring_location(hole, point, t)),
        )
    }
    /// Validate once with the supplied coordinate-unit tolerance and reuse the returned query
    /// view without revalidation.
    pub fn validated(&self, t: &GeometryTolerance) -> GeometryResult<ValidatedPolygon<'_>> {
        ValidatedPolygon::new(std::borrow::Cow::Borrowed(self), t)
    }
    /// Like validated, but owns the polygon for caching. Retains the supplied tolerance and
    /// coordinate system.
    pub fn into_validated(
        self,
        t: &GeometryTolerance,
    ) -> GeometryResult<ValidatedPolygon<'static>> {
        ValidatedPolygon::new(std::borrow::Cow::Owned(self), t)
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
        let result = Self {
            exterior: self
                .exterior
                .iter()
                .map(|p| transform.apply_point(*p))
                .collect(),
            holes: self
                .holes
                .iter()
                .map(|h| h.iter().map(|p| transform.apply_point(*p)).collect())
                .collect(),
        };
        result.validate(t)?;
        Ok(result)
    }
    /// Return a nearest boundary point in caller coordinates using the supplied tolerance;
    /// unsupported or ambiguous cases return a typed error.
    pub fn nearest_point(&self, point: Point2, t: &GeometryTolerance) -> GeometryResult<Point2> {
        self.validate(t)?;
        point.validate()?;
        let mut best = None;
        for segment in std::iter::once(&self.exterior)
            .chain(&self.holes)
            .flat_map(|r| ring_segments(r))
        {
            let candidate = segment.nearest_point(point, t)?;
            if best.is_none_or(|p: Point2| candidate.distance(point) < p.distance(point)) {
                best = Some(candidate);
            }
        }
        best.ok_or_else(|| {
            GeometryError::invalid(
                GeometryReason::InsufficientPoints,
                "exterior",
                "polygon.nearest_point",
                "Polygon boundary is empty",
            )
        })
    }
}

/// Compute signed shoelace area in squared caller units; counterclockwise is positive. Requires at
/// least three finite vertices, closes implicitly and does not validate topology or apply
/// tolerance.
pub fn signed_ring_area(points: &[Point2]) -> GeometryResult<f64> {
    if points.len() < 3 {
        return Err(GeometryError::invalid(
            GeometryReason::InsufficientPoints,
            "ring",
            "polygon.validate",
            "A ring requires at least three points",
        ));
    }
    let origin = *at(points, 0)?;
    origin.validate()?;
    let mut sum = 0.;
    let mut correction = 0.;
    // A local origin and compensated summation reduce cancellation at large world coordinates.
    for (a, b) in points.iter().skip(1).zip(points.iter().skip(2)) {
        a.validate()?;
        b.validate()?;
        let value = origin.vector_to(*a).cross(origin.vector_to(*b));
        finite(value)?;
        let adjusted = value - correction;
        let next = sum + adjusted;
        correction = (next - sum) - adjusted;
        sum = next;
    }
    finite(sum)?;
    Ok(sum / 2.)
}
fn ring_segment(points: &[Point2], index: usize) -> GeometryResult<LineSegment> {
    let start = *at(points, index)?;
    let next = index.checked_add(1).ok_or_else(missing_geometry)? % points.len();
    Ok(LineSegment {
        start,
        end: *at(points, next)?,
    })
}
pub(crate) fn ring_segments(points: &[Point2]) -> impl Iterator<Item = LineSegment> + '_ {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(start, end)| LineSegment {
            start: *start,
            end: *end,
        })
}

/// Tolerance-expanded segment box; disjoint boxes cannot produce segment intersections.
#[derive(Clone, Copy)]
pub(crate) struct SegmentBox {
    pub(crate) min_x: f64,
    pub(crate) max_x: f64,
    pub(crate) min_y: f64,
    pub(crate) max_y: f64,
}

impl SegmentBox {
    fn overlaps_y(self, other: Self) -> bool {
        self.min_y <= other.max_y && other.min_y <= self.max_y
    }
}

/// Intersection tests also detect contacts and overlaps within tolerance. Box padding includes
/// that distance and coordinate-scale roundoff.
fn box_margin(rings: &[&[Point2]], t: &GeometryTolerance) -> f64 {
    let scale = rings
        .iter()
        .flat_map(|ring| ring.iter())
        .fold(0_f64, |scale, p| scale.max(p.x.abs()).max(p.y.abs()));
    2. * t.linear + 16. * f64::EPSILON * scale
}

fn segment_boxes(points: &[Point2], margin: f64) -> Vec<SegmentBox> {
    ring_segments(points)
        .map(|segment| SegmentBox {
            min_x: segment.start.x.min(segment.end.x) - margin,
            max_x: segment.start.x.max(segment.end.x) + margin,
            min_y: segment.start.y.min(segment.end.y) - margin,
            max_y: segment.start.y.max(segment.end.y) + margin,
        })
        .collect()
}

fn order_by_min_x(boxes: &[SegmentBox]) -> Vec<(usize, &SegmentBox)> {
    let mut order: Vec<_> = boxes.iter().enumerate().collect();
    order.sort_by(|a, b| a.1.min_x.total_cmp(&b.1.min_x));
    order
}

/// Pairs (i, j), i < j, with overlapping boxes, in exhaustive lexicographic order.
fn ring_candidate_pairs(boxes: &[SegmentBox]) -> GeometryResult<Vec<(usize, usize)>> {
    overlapping_box_pairs(boxes, usize::MAX).ok_or_else(|| {
        GeometryError::unsupported(
            GeometryReason::ResourceLimit,
            "pairs",
            "polygon.validate",
            "Candidate pair limit exceeded",
        )
    })
}

#[cfg(test)]
thread_local! {
    /// Test-only switch between candidate filtering and exhaustive scans for equivalence
    /// checks.
    pub(crate) static FULL_SCAN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether tests request exhaustive scanning; always false in production.
#[inline]
pub(crate) fn full_scan() -> bool {
    #[cfg(test)]
    {
        FULL_SCAN.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        false
    }
}

/// Overlapping box pairs in exhaustive lexicographic order. Return None when the candidate
/// count exceeds limit, enforcing the budget before growing the candidate list.
pub(crate) fn overlapping_box_pairs(
    boxes: &[SegmentBox],
    limit: usize,
) -> Option<Vec<(usize, usize)>> {
    if full_scan() {
        let count = boxes.len();
        if count.saturating_mul(count.saturating_sub(1)) / 2 > limit {
            return None;
        }
        return Some(
            (0..count)
                .flat_map(|i| (i + 1..count).map(move |j| (i, j)))
                .collect(),
        );
    }
    let order = order_by_min_x(boxes);
    let mut pairs = Vec::new();
    for (position, &(left, left_box)) in order.iter().enumerate() {
        for &(right, right_box) in order.iter().skip(position + 1) {
            if right_box.min_x > left_box.max_x {
                break;
            }
            if left_box.overlaps_y(*right_box) {
                if pairs.len() == limit {
                    return None;
                }
                pairs.push((left.min(right), left.max(right)));
            }
        }
    }
    pairs.sort_unstable();
    Some(pairs)
}

/// Cross-ring box pairs (a, b), in exhaustive lexicographic order.
fn cross_candidate_pairs(a: &[SegmentBox], b: &[SegmentBox]) -> Vec<(usize, usize)> {
    let b_order = order_by_min_x(b);
    let mut pairs = Vec::new();
    for (left, left_box) in a.iter().enumerate() {
        // Boxes are sorted by min_x; those beyond the right endpoint cannot intersect.
        let end = b_order.partition_point(|(_, bounds)| bounds.min_x <= left_box.max_x);
        for &(right, bounds) in b_order.iter().take(end) {
            if bounds.max_x >= left_box.min_x && left_box.overlaps_y(*bounds) {
                pairs.push((left, right));
            }
        }
    }
    pairs.sort_unstable();
    pairs
}

fn validate_ring(points: &[Point2], t: &GeometryTolerance) -> GeometryResult<()> {
    if points.len() < 3 {
        return Err(GeometryError::invalid(
            GeometryReason::InsufficientPoints,
            "ring",
            "polygon.validate",
            "A ring requires at least three points",
        ));
    }
    for s in ring_segments(points) {
        s.validate(t)?;
    }
    let count = points.len();
    let boxes = segment_boxes(points, box_margin(&[points], t));
    // Disjoint padded boxes lie beyond tolerance. Visit remaining pairs in exhaustive order.
    for (i, j) in ring_candidate_pairs(&boxes)? {
        let adjacent = j == i + 1 || (i == 0 && j == count - 1);
        let first = ring_segment(points, i)?;
        match first.intersection(&ring_segment(points, j)?, t)? {
            SegmentIntersection::None => {}
            SegmentIntersection::Point(p) if adjacent => {
                let shared = if j == i + 1 { first.end } else { first.start };
                if p.distance(shared) > t.linear {
                    return Err(GeometryError::invalid(
                        GeometryReason::InvalidTopology,
                        "ring",
                        "polygon.validate",
                        "Adjacent edges intersect away from their shared vertex",
                    ));
                }
            }
            _ => {
                return Err(GeometryError::invalid(
                    GeometryReason::InvalidTopology,
                    "ring",
                    "polygon.validate",
                    "The ring intersects or overlaps itself",
                ));
            }
        }
    }
    if signed_ring_area(points)?.abs() <= t.area {
        return Err(GeometryError::invalid(
            GeometryReason::BelowTolerance,
            "ring.area",
            "polygon.validate",
            "Ring area is below tolerance",
        ));
    }
    Ok(())
}
fn rings_intersect(a: &[Point2], b: &[Point2], t: &GeometryTolerance) -> GeometryResult<bool> {
    let margin = box_margin(&[a, b], t);
    for (left, right) in cross_candidate_pairs(&segment_boxes(a, margin), &segment_boxes(b, margin))
    {
        if ring_segment(a, left)?.intersection(&ring_segment(b, right)?, t)?
            != SegmentIntersection::None
        {
            return Ok(true);
        }
    }
    Ok(false)
}
fn ring_location(
    points: &[Point2],
    point: Point2,
    t: &GeometryTolerance,
) -> GeometryResult<PointLocation> {
    ring_location_among(points, 0..points.len(), point, t)
}

/// Visit only the supplied segments in order. Skipped segments must be outside boundary
/// distance and unable to cross the query ray.
fn ring_location_among(
    points: &[Point2],
    segments: impl Iterator<Item = usize>,
    point: Point2,
    t: &GeometryTolerance,
) -> GeometryResult<PointLocation> {
    let mut inside = false;
    for index in segments {
        let segment = ring_segment(points, index)?;
        if segment.nearest_point(point, t)?.distance(point) <= t.linear {
            return Ok(PointLocation::Boundary);
        }
        let a = segment.start;
        let b = segment.end;
        if (a.y > point.y) != (b.y > point.y) {
            let x = (b.x - a.x) * ((point.y - a.y) / (b.y - a.y)) + (a.x - point.x);
            finite(x)?;
            if x > 0. {
                inside = !inside;
            }
        }
    }
    Ok(if inside {
        PointLocation::Inside
    } else {
        PointLocation::Outside
    })
}

fn combine_locations(
    exterior: PointLocation,
    holes: impl Iterator<Item = GeometryResult<PointLocation>>,
) -> GeometryResult<PointLocation> {
    if exterior != PointLocation::Inside {
        return Ok(exterior);
    }
    for hole in holes {
        match hole? {
            PointLocation::Boundary => return Ok(PointLocation::Boundary),
            PointLocation::Inside => return Ok(PointLocation::Outside),
            PointLocation::Outside => {}
        }
    }
    Ok(PointLocation::Inside)
}

/// Validated polygon query view retaining its coordinate system and tolerance. Location
/// matches Polygon::locate; y bands select only segments near the boundary tolerance or
/// horizontal ray.
#[derive(Debug, Clone)]
pub struct ValidatedPolygon<'a> {
    polygon: std::borrow::Cow<'a, Polygon>,
    tolerance: GeometryTolerance,
    area: f64,
    rings: Vec<RingIndex>,
}

impl<'a> ValidatedPolygon<'a> {
    fn new(polygon: std::borrow::Cow<'a, Polygon>, t: &GeometryTolerance) -> GeometryResult<Self> {
        polygon.validate(t)?;
        let area = polygon.raw_area()?;
        let rings = std::iter::once(&polygon.exterior)
            .chain(&polygon.holes)
            .map(|ring| RingIndex::new(ring, t))
            .collect::<GeometryResult<Vec<_>>>()?;
        Ok(Self {
            polygon,
            tolerance: *t,
            area,
            rings,
        })
    }
    /// Borrow the original polygon in its original coordinate units and ring order.
    pub fn polygon(&self) -> &Polygon {
        &self.polygon
    }
    /// Borrow the validated query view's retained length, angular and area tolerances.
    pub fn tolerance(&self) -> &GeometryTolerance {
        &self.tolerance
    }
    /// Absolute area minus holes in squared coordinate units, computed once during validation
    /// with the retained tolerance.
    pub fn area(&self) -> f64 {
        self.area
    }
    /// Classify a point in the polygon's coordinate system. Boundary distance uses the supplied or
    /// retained linear tolerance.
    pub fn locate(&self, point: Point2) -> GeometryResult<PointLocation> {
        point.validate()?;
        let rings = std::iter::once(&self.polygon.exterior).chain(&self.polygon.holes);
        let mut locations = rings
            .zip(&self.rings)
            .map(|(ring, index)| index.locate(ring, point, &self.tolerance));
        let exterior = locations.next().ok_or_else(missing_geometry)??;
        combine_locations(exterior, locations)
    }
}

/// Index segment identifiers by tolerance-expanded y bands.
#[derive(Debug, Clone)]
struct RingIndex {
    min_y: f64,
    max_y: f64,
    scale: f64,
    bands: Vec<Vec<u32>>,
}

impl RingIndex {
    fn new(points: &[Point2], t: &GeometryTolerance) -> GeometryResult<Self> {
        let min_y = points.iter().map(|p| p.y).fold(f64::INFINITY, f64::min) - t.linear;
        let max_y = points.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max) + t.linear;
        let count = (points.len() as f64).sqrt().ceil().clamp(1., 4096.) as usize;
        let height = max_y - min_y;
        let scale = if height > 0. && height.is_finite() {
            count as f64 / height
        } else {
            0.
        };
        let mut index = Self {
            min_y,
            max_y,
            scale,
            bands: vec![Vec::new(); count],
        };
        for segment in 0..points.len() {
            let s = ring_segment(points, segment)?;
            let first = index.band(s.start.y.min(s.end.y) - t.linear);
            let last = index.band(s.start.y.max(s.end.y) + t.linear);
            for band in at_mut(&mut index.bands, first..=last)? {
                band.push(segment as u32);
            }
        }
        Ok(index)
    }
    fn band(&self, y: f64) -> usize {
        // Floating-point subtraction and multiplication are monotonic, so endpoint bands
        // enclose the query band.
        (((y - self.min_y) * self.scale).floor().max(0.) as usize).min(self.bands.len() - 1)
    }
    fn locate(
        &self,
        points: &[Point2],
        point: Point2,
        t: &GeometryTolerance,
    ) -> GeometryResult<PointLocation> {
        if point.y < self.min_y || point.y > self.max_y {
            // No segment is within tolerance or crosses the horizontal ray.
            return Ok(PointLocation::Outside);
        }
        let band = at(&self.bands, self.band(point.y))?;
        ring_location_among(points, band.iter().map(|index| *index as usize), point, t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exhaustive O(n^2) scan used as the reference for accelerated validation.
    fn exhaustive_validate_ring(points: &[Point2], t: &GeometryTolerance) -> GeometryResult<()> {
        let segments: Vec<_> = ring_segments(points).collect();
        for s in &segments {
            s.validate(t)?;
        }
        for i in 0..segments.len() {
            for j in i + 1..segments.len() {
                let adjacent = j == i + 1 || (i == 0 && j == segments.len() - 1);
                match segments[i].intersection(&segments[j], t)? {
                    SegmentIntersection::None => {}
                    SegmentIntersection::Point(p) if adjacent => {
                        let shared = if j == i + 1 {
                            segments[i].end
                        } else {
                            segments[i].start
                        };
                        if p.distance(shared) > t.linear {
                            return Err(GeometryError::invalid(
                                GeometryReason::InvalidTopology,
                                "ring",
                                "polygon.validate",
                                "Adjacent edges intersect away from their shared vertex",
                            ));
                        }
                    }
                    _ => {
                        return Err(GeometryError::invalid(
                            GeometryReason::InvalidTopology,
                            "ring",
                            "polygon.validate",
                            "The ring intersects or overlaps itself",
                        ));
                    }
                }
            }
        }
        if signed_ring_area(points)?.abs() <= t.area {
            return Err(GeometryError::invalid(
                GeometryReason::BelowTolerance,
                "ring.area",
                "polygon.validate",
                "Ring area is below tolerance",
            ));
        }
        Ok(())
    }

    fn exhaustive_rings_intersect(
        a: &[Point2],
        b: &[Point2],
        t: &GeometryTolerance,
    ) -> GeometryResult<bool> {
        for left in ring_segments(a) {
            for right in ring_segments(b) {
                if left.intersection(&right, t)? != SegmentIntersection::None {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Deterministic linear congruential generator; identical input on every run.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 11) as f64 / (1_u64 << 53) as f64
        }
    }

    fn star(
        random: &mut Lcg,
        count: usize,
        center: Point2,
        radius: f64,
        jitter: f64,
    ) -> Vec<Point2> {
        (0..count)
            .map(|index| {
                let angle = index as f64 / count as f64 * std::f64::consts::TAU;
                let r = radius * (1. - jitter * random.next());
                Point2::new(center.x + r * angle.cos(), center.y + r * angle.sin())
            })
            .collect()
    }

    #[test]
    fn pruned_ring_validation_matches_exhaustive_scan() {
        let t = GeometryTolerance::default();
        let mut random = Lcg(7);
        let mut invalid = 0;
        for case in 0..400 {
            let count = 3 + case % 60;
            let mut ring = star(&mut random, count, Point2::new(1e5, -2e5), 10., 0.6);
            // Swap vertices for self-intersections and repeat vertices for zero-length edges
            // in selected cases.
            match case % 5 {
                1 if count > 4 => ring.swap(1, count / 2),
                2 if count > 3 => ring[2] = ring[1],
                3 => {
                    let p = ring[0];
                    ring.push(Point2::new(p.x + 1e-9, p.y));
                }
                _ => {}
            }
            let expected = exhaustive_validate_ring(&ring, &t);
            let actual = validate_ring(&ring, &t);
            invalid += usize::from(expected.is_err());
            assert_eq!(actual, expected, "case {case}");
        }
        assert!(
            invalid > 50,
            "Insufficient invalid-case coverage: {invalid}"
        );
    }

    #[test]
    fn pruned_ring_intersection_and_indexed_locate_match_exhaustive_results() {
        let t = GeometryTolerance::default();
        let mut random = Lcg(11);
        for case in 0..200 {
            let outer = star(&mut random, 8 + case % 90, Point2::new(3., 4.), 10., 0.3);
            let offset = random.next() * 16. - 8.;
            let radius = 1. + random.next() * 3.;
            let hole = star(
                &mut random,
                3 + case % 13,
                Point2::new(3. + offset, 4.),
                radius,
                0.2,
            );
            assert_eq!(
                rings_intersect(&outer, &hole, &t),
                exhaustive_rings_intersect(&outer, &hole, &t),
                "case {case}"
            );
            let polygon = Polygon {
                exterior: outer.clone(),
                holes: Vec::new(),
            };
            let Ok(validated) = polygon.validated(&t) else {
                continue;
            };
            for probe in 0..40 {
                let point = if probe % 4 == 0 {
                    // Boundary queries also exercise the tolerance branch.
                    let segment = ring_segment(&outer, probe % outer.len()).unwrap();
                    segment.start.lerp(segment.end, random.next())
                } else {
                    Point2::new(random.next() * 24. - 9., random.next() * 24. - 8.)
                };
                assert_eq!(
                    validated.locate(point),
                    polygon.locate(point, &t),
                    "case {case}, point {point:?}"
                );
            }
        }
    }
}
