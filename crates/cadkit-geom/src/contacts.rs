//! Numerical contact normalization. Different evaluation orders may separate identical
//! analytic contacts by floating-point roundoff; robust booleans deliberately classify this as
//! ambiguity. Only contacts within the caller's explicit coordinate budget are joined. Larger
//! gaps remain. Callers operate on a derived copy and report the applied budget.
use crate::*;

/// Measured normalization budget and changes, in caller coordinate units unless stated
/// otherwise.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContactNormalization {
    /// Maximum endpoint-to-endpoint or endpoint-to-edge merge distance, in coordinate units.
    pub budget: f64,
    /// Number of vertices moved to an earlier vertex within the budget; dimensionless.
    pub merged_vertices: usize,
    /// Number of contact or intersection nodes inserted on edges, including exact contacts;
    /// dimensionless.
    pub inserted_nodes: usize,
    /// Largest measured distance of moved or inserted vertices from the original boundary, in
    /// coordinate units.
    pub max_displacement: f64,
}

impl ContactNormalization {
    /// Whether boundary geometry moved by a positive distance. Inserting an exact contact
    /// alone does not change geometry; no additional tolerance is applied.
    pub fn changed(&self) -> bool {
        self.max_displacement > 0.
    }
}

/// Four ulps of the largest coordinate magnitude, with scale at least 1. The result uses
/// coordinate units and only measures roundoff; it is not an application tolerance.
pub fn contact_roundoff_budget(points: impl IntoIterator<Item = Point2>) -> f64 {
    let scale = points
        .into_iter()
        .fold(1_f64, |size, p| size.max(p.x.abs()).max(p.y.abs()));
    (scale.next_up() - scale) * 4.
}

fn edges(polygon: &Polygon) -> Vec<LineSegment> {
    std::iter::once(&polygon.exterior)
        .chain(&polygon.holes)
        .flat_map(|ring| crate::polygon::ring_segments(ring))
        .collect()
}

/// Edge box padded by margin plus coordinate-scale roundoff.
fn line_box(line: LineSegment, margin: f64) -> crate::polygon::SegmentBox {
    let scale = [line.start.x, line.start.y, line.end.x, line.end.y]
        .into_iter()
        .fold(1_f64, |scale, value| scale.max(value.abs()));
    let margin = margin + 16. * f64::EPSILON * scale;
    crate::polygon::SegmentBox {
        min_x: line.start.x.min(line.end.x) - margin,
        max_x: line.start.x.max(line.end.x) + margin,
        min_y: line.start.y.min(line.end.y) - margin,
        max_y: line.start.y.max(line.end.y) + margin,
    }
}

fn inside(bounds: crate::polygon::SegmentBox, p: Point2) -> bool {
    crate::polygon::full_scan()
        || p.x >= bounds.min_x && p.x <= bounds.max_x && p.y >= bounds.min_y && p.y <= bounds.max_y
}

/// Canonical vertices. Query returns the first inserted vertex within budget, matching
/// exhaustive order. Cell width 2 * budget keeps candidates in the nine neighboring cells
/// despite division roundoff; unrepresentable cells use the full list.
struct CanonicalPoints {
    budget: f64,
    points: Vec<Point2>,
    cells: std::collections::HashMap<(i64, i64), Vec<usize>>,
    unindexed: Vec<usize>,
}

impl CanonicalPoints {
    fn new(budget: f64) -> Self {
        Self {
            budget,
            points: Vec::new(),
            cells: std::collections::HashMap::new(),
            unindexed: Vec::new(),
        }
    }

    fn key(&self, point: Point2) -> Option<(i64, i64)> {
        const LIMIT: f64 = (1_i64 << 52) as f64;
        let cell = 2. * self.budget;
        let x = (point.x / cell).floor();
        let y = (point.y / cell).floor();
        (cell > 0. && x.abs() < LIMIT && y.abs() < LIMIT).then_some((x as i64, y as i64))
    }

    fn first_within(&self, point: Point2) -> GeometryResult<Option<Point2>> {
        let first_within = |indices: &[usize]| -> GeometryResult<Option<usize>> {
            for &index in indices {
                if at(&self.points, index)?.distance(point) <= self.budget {
                    return Ok(Some(index));
                }
            }
            Ok(None)
        };
        let key = self.key(point).filter(|_| !crate::polygon::full_scan());
        let Some((x, y)) = key else {
            return Ok(self
                .points
                .iter()
                .copied()
                .find(|p| p.distance(point) <= self.budget));
        };
        let mut best = first_within(&self.unindexed)?;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(bucket) = self.cells.get(&(x + dx, y + dy))
                    && let Some(index) = first_within(bucket)?
                {
                    best = Some(best.map_or(index, |b| b.min(index)));
                }
            }
        }
        best.map(|index| at(&self.points, index).copied())
            .transpose()
    }

    fn push(&mut self, point: Point2) {
        let index = self.points.len();
        self.points.push(point);
        match self.key(point) {
            Some(key) => self.cells.entry(key).or_default().push(index),
            None => self.unindexed.push(index),
        }
    }
}

fn resource(field: &str, message: &str) -> GeometryError {
    GeometryError::unsupported(
        GeometryReason::ResourceLimit,
        field,
        "contacts.normalize",
        message,
    )
}

/// Distance from an inserted node to the supporting line; robust orientation preserves exact
/// zero contacts.
fn offset(line: LineSegment, point: Point2) -> f64 {
    let coordinate = |p: Point2| robust::Coord { x: p.x, y: p.y };
    robust::orient2d(
        coordinate(line.start),
        coordinate(line.end),
        coordinate(point),
    )
    .abs()
        / line.length()
}

/// Whether the projection lies within the segment interval, padded by the budget.
fn within(line: LineSegment, point: Point2, budget: f64) -> bool {
    let length = line.length();
    let station = line
        .start
        .vector_to(point)
        .dot(line.start.vector_to(line.end))
        / length;
    station >= -budget && station <= length + budget
}

/// Common-node candidates for two edges. Shallow intersections may produce constructions
/// separated along the edges beyond the budget. Certify a candidate with robust orientation
/// distances: it must lie within half the budget of both edges and inside both segment
/// intervals. Uncertified intersections remain errors.
fn crossing_nodes(
    a: LineSegment,
    b: LineSegment,
    fine: &GeometryTolerance,
    budget: f64,
) -> GeometryResult<Vec<Point2>> {
    match a.intersection(&b, fine) {
        Ok(SegmentIntersection::None) => Ok(vec![]),
        Ok(SegmentIntersection::Point(point)) => Ok(vec![point]),
        Ok(SegmentIntersection::Overlap(line)) => Ok(vec![line.start, line.end]),
        Err(error) if error.diagnostic().reason == GeometryReason::NumericalErrorBudgetExceeded => {
            let r = a.start.vector_to(a.end);
            let s = b.start.vector_to(b.end);
            let q = a.start.vector_to(b.start);
            let denominator = r.cross(s);
            [
                a.start.translated(r.scaled(q.cross(s) / denominator)),
                b.start.translated(s.scaled(q.cross(r) / denominator)),
            ]
            .into_iter()
            .filter(|p| p.x.is_finite() && p.y.is_finite())
            .map(|p| (offset(a, p).max(offset(b, p)), p))
            // Reserve half the budget for roundoff in the later f64 projection check.
            .filter(|(distance, p)| {
                *distance <= budget * 0.5 && within(a, *p, budget) && within(b, *p, budget)
            })
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(_, p)| vec![p])
            .ok_or(error)
        }
        Err(error) => Err(error),
    }
}

/// Join endpoint and edge contacts within an explicit coordinate-unit budget. Earlier vertices
/// win, so place geometry to preserve first. Larger gaps remain untouched. Default boolean
/// limits bound input and generated nodes. Nodes within budget of each other or an endpoint
/// are deduplicated. Tolerances use the same coordinates.
pub fn normalize_contacts(
    polygons: &mut [Polygon],
    separators: &mut Vec<LineSegment>,
    budget: f64,
    tolerance: &GeometryTolerance,
) -> GeometryResult<ContactNormalization> {
    normalize_contacts_with_spacing(polygons, separators, budget, budget, tolerance)
}

/// Normalize contacts with an explicit minimum node spacing in coordinate units. A node closer
/// than spacing to a neighbor or endpoint is not inserted. Using tolerance.linear prevents
/// newly inserted edges below that tolerance; unresolved contacts remain for the boolean
/// kernel to classify.
pub fn normalize_contacts_with_spacing(
    polygons: &mut [Polygon],
    separators: &mut Vec<LineSegment>,
    budget: f64,
    spacing: f64,
    tolerance: &GeometryTolerance,
) -> GeometryResult<ContactNormalization> {
    let mut summary = ContactNormalization {
        budget,
        ..Default::default()
    };
    let count = polygons
        .iter()
        .map(|p| p.exterior.len() + p.holes.iter().map(Vec::len).sum::<usize>())
        .sum::<usize>()
        + separators.len();
    let limits = PolygonBooleanOptions::default();
    if count > limits.max_input_edges {
        return Err(resource(
            "maxInputEdges",
            "Contact noding input budget exceeded",
        ));
    }
    let mut canonical = CanonicalPoints::new(budget);
    for point in polygons
        .iter_mut()
        .flat_map(|p| p.exterior.iter_mut().chain(p.holes.iter_mut().flatten()))
        .chain(
            separators
                .iter_mut()
                .flat_map(|line| [&mut line.start, &mut line.end]),
        )
    {
        if let Some(existing) = canonical.first_within(*point)? {
            if *point != existing {
                summary.merged_vertices += 1;
                summary.max_displacement = summary.max_displacement.max(existing.distance(*point));
            }
            *point = existing;
        } else {
            canonical.push(*point);
        }
    }
    let mut points: Vec<_> = polygons
        .iter()
        .flat_map(|p| p.exterior.iter().chain(p.holes.iter().flatten()).copied())
        .chain(separators.iter().flat_map(|line| [line.start, line.end]))
        .collect();
    // Three or more edges can compute the same intersection in different orders. Construct it
    // once and add the same node to every participating edge.
    let lines: Vec<_> = polygons
        .iter()
        .flat_map(edges)
        .chain(separators.iter().copied())
        .collect();
    let fine = GeometryTolerance {
        linear: budget,
        angular: 64. * f64::EPSILON,
        area: f64::MIN_POSITIVE,
    };
    // Sweep budget-expanded boxes to select candidate pairs, retain the original box predicate
    // below, and visit pairs in exhaustive order.
    let candidates = crate::polygon::overlapping_box_pairs(
        &lines
            .iter()
            .map(|line| line_box(*line, budget))
            .collect::<Vec<_>>(),
        limits.max_pair_checks,
    )
    .ok_or_else(|| resource("maxInputEdges", "Contact noding input budget exceeded"))?;
    let mut crossings = Vec::new();
    for (i, j) in candidates {
        let (a, b) = (at(&lines, i)?, at(&lines, j)?);
        {
            if a.start.x.min(a.end.x) > b.start.x.max(b.end.x) + budget
                || b.start.x.min(b.end.x) > a.start.x.max(a.end.x) + budget
                || a.start.y.min(a.end.y) > b.start.y.max(b.end.y) + budget
                || b.start.y.min(b.end.y) > a.start.y.max(a.end.y) + budget
            {
                continue;
            }
            // An endpoint already within budget of the other edge is already a candidate node.
            // Recomputing a shallow T-junction can introduce a spurious microscopic edge. Do
            // not close larger gaps.
            let endpoint_contact = [(a.start, b), (a.end, b), (b.start, a), (b.end, a)]
                .into_iter()
                .any(|(point, line)| {
                    line.nearest_point(point, &fine)
                        .is_ok_and(|nearest| nearest.distance(point) <= budget)
                });
            if endpoint_contact {
                continue;
            }
            let intersections = crossing_nodes(*a, *b, &fine, budget)?;
            let conditioning = a
                .start
                .vector_to(a.end)
                .cross(b.start.vector_to(b.end))
                .abs()
                / (a.length() * b.length());
            for point in intersections {
                crossings.push((conditioning, point, i, j));
            }
            if crossings.len() + points.len() > limits.max_generated_edges {
                return Err(resource(
                    "maxGeneratedEdges",
                    "Contact noding intersection budget exceeded",
                ));
            }
        }
    }
    // Construct multiway nodes from the best-conditioned, nearly perpendicular pair first.
    // Longitudinal roundoff amplified by a shallow angle must not determine the node.
    crossings.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (_, point, a, b) in crossings {
        let a = *at(&lines, a)?;
        let b = *at(&lines, b)?;
        // A known node lies near either the candidate or both edges within budget. In either
        // case it lies inside both boxes expanded by four budgets; points outside cannot
        // affect the result.
        let (box_a, box_b) = (line_box(a, 4. * budget), line_box(b, 4. * budget));
        let known = points.iter().any(|p| {
            inside(box_a, *p)
                && inside(box_b, *p)
                && (p.distance(point) <= budget
                    || [a, b].iter().all(|line| {
                        line.nearest_point(*p, &fine)
                            .is_ok_and(|nearest| nearest.distance(*p) <= budget)
                    }))
        });
        if !known {
            points.push(point);
        }
    }
    let split = |line: LineSegment,
                 summary: &mut ContactNormalization|
     -> GeometryResult<Vec<Point2>> {
        let direction = line.start.vector_to(line.end);
        let mut nodes = vec![(0., line.start), (1., line.end)];
        // A point outside the expanded box is farther from the edge than the budget. Preserve
        // the first validation error because nearest_point also validates its segment.
        let near = line_box(line, 2. * budget);
        let mut validated = false;
        for point in &points {
            let station = line.start.vector_to(*point).dot(direction) / direction.dot(direction);
            if station > 0. && station < 1. {
                if !inside(near, *point) {
                    if !validated {
                        line.validate(tolerance)?;
                        point.validate()?;
                        validated = true;
                    }
                    continue;
                }
                validated = true;
                if line.nearest_point(*point, tolerance)?.distance(*point) <= budget {
                    nodes.push((station, *point));
                }
            }
        }
        nodes.sort_by(|a, b| a.0.total_cmp(&b.0));
        nodes.dedup_by(|a, b| a.1.distance(b.1) <= spacing);
        for (_, node) in nodes
            .iter()
            .filter(|(_, p)| *p != line.start && *p != line.end)
        {
            summary.inserted_nodes += 1;
            summary.max_displacement = summary.max_displacement.max(offset(line, *node));
        }
        Ok(nodes.into_iter().map(|(_, p)| p).collect())
    };
    for polygon in polygons {
        for ring in std::iter::once(&mut polygon.exterior).chain(&mut polygon.holes) {
            let mut next = Vec::new();
            for segment in crate::polygon::ring_segments(ring) {
                let mut nodes = split(segment, &mut summary)?;
                nodes.pop();
                next.extend(nodes);
            }
            *ring = next;
        }
    }
    let mut next = Vec::new();
    for line in separators.iter() {
        let nodes = split(*line, &mut summary)?;
        next.extend(
            nodes
                .iter()
                .zip(nodes.iter().skip(1))
                .map(|(start, end)| LineSegment {
                    start: *start,
                    end: *end,
                }),
        );
    }
    *separators = next;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Polygon {
        Polygon {
            exterior: vec![
                Point2::new(x0, y0),
                Point2::new(x1, y0),
                Point2::new(x1, y1),
                Point2::new(x0, y1),
            ],
            holes: vec![],
        }
    }

    #[test]
    fn shallow_multiway_contact_does_not_create_a_microscopic_edge() {
        let origin = Point2::new(2.731, 9.314);
        let direction = |angle: f64| Vector2 {
            x: angle.cos(),
            y: angle.sin(),
        };
        let mut polygons: Vec<_> = [0.11, 0.111, 1.1]
            .into_iter()
            .map(|angle| {
                let d = direction(angle);
                let normal = Vector2 { x: -d.y, y: d.x };
                Polygon {
                    exterior: vec![
                        origin.translated(d.scaled(-4.)),
                        origin.translated(d.scaled(4.)),
                        origin.translated(normal.scaled(0.5)),
                    ],
                    holes: vec![],
                }
            })
            .collect();
        let tolerance = GeometryTolerance::default();
        let before: Vec<_> = polygons
            .iter()
            .map(|p| p.area(&tolerance).unwrap())
            .collect();
        let budget = (16_f64.next_up() - 16.) * 4.;
        normalize_contacts(&mut polygons, &mut vec![], budget, &tolerance).unwrap();
        for (polygon, area) in polygons.iter().zip(before) {
            polygon.validate(&tolerance).unwrap();
            assert!((polygon.area(&tolerance).unwrap() - area).abs() < 1e-12);
        }
    }

    /// Synthetic elongated polygon with slope 0.104 and a quadrilateral crossing its side at
    /// about 8e-5 radians, with small lateral offsets.
    fn shallow_crossing() -> (Polygon, Polygon, LineSegment, LineSegment) {
        let norm = (1. + 0.104_f64 * 0.104).sqrt();
        let (c, s) = (1. / norm, 0.104 / norm);
        let at = |station: f64, lateral: f64| {
            Point2::new(
                -1.5 + station * c - lateral * s,
                9.54 + station * s + lateral * c,
            )
        };
        let strip = Polygon {
            exterior: vec![at(0., -0.2), at(18.1, -0.2), at(18.1, 0.), at(0., 0.)],
            holes: vec![],
        };
        // One endpoint lies 1.4e-5 coordinate units inside the side, the other 4.5e-5 outside.
        let (a, b) = (at(5.5, -1.4e-5), at(6.2, 4.5e-5));
        let d = a.vector_to(b).scaled(1. / a.distance(b));
        let n = Vector2 { x: -d.y, y: d.x };
        let quadrilateral = Polygon {
            exterior: vec![
                a,
                b,
                b.translated(n.scaled(0.7)),
                a.translated(n.scaled(0.7)),
            ],
            holes: vec![],
        };
        let face = LineSegment {
            start: at(18.1, 0.),
            end: at(0., 0.),
        };
        (strip, quadrilateral, LineSegment { start: a, end: b }, face)
    }

    #[test]
    fn shallow_crossing_gets_a_certified_node_instead_of_failing_the_whole_set() {
        let tolerance = GeometryTolerance::default();
        let (strip, quadrilateral, edge, face) = shallow_crossing();
        let budget = contact_roundoff_budget(
            strip
                .exterior
                .iter()
                .chain(&quadrilateral.exterior)
                .copied()
                .collect::<Vec<_>>(),
        );
        let fine = GeometryTolerance {
            linear: budget,
            angular: 64. * f64::EPSILON,
            area: f64::MIN_POSITIVE,
        };
        // The two supporting-line constructions differ by more than the budget; the previous
        // noding path rejected the entire set.
        assert_eq!(
            edge.intersection(&face, &fine)
                .unwrap_err()
                .diagnostic()
                .reason,
            GeometryReason::NumericalErrorBudgetExceeded
        );
        let mut polygons = vec![strip, quadrilateral];
        let summary = normalize_contacts(&mut polygons, &mut vec![], budget, &tolerance).unwrap();
        assert!(summary.max_displacement <= budget, "{summary:?}");
        // Two sides of the quadrilateral cross the elongated polygon. Both rings must share
        // both certified nodes; the shallow node must also lie within budget of its source
        // edge.
        let shared: Vec<_> = polygons[0]
            .exterior
            .iter()
            .filter(|p| polygons[1].exterior.contains(p))
            .copied()
            .collect();
        assert_eq!(shared.len(), 2, "{polygons:?}");
        assert!(shared.iter().all(|p| offset(face, *p) <= budget));
        assert!(shared.iter().any(|p| offset(edge, *p) <= budget));
        for polygon in &polygons {
            polygon.validate(&tolerance).unwrap();
        }
        let frame = rect(-10., 0., 30., 20.);
        polygon_boolean(
            &[frame],
            &polygons,
            PolygonBooleanOperation::Difference,
            PolygonBooleanOptions::with_error_budget(tolerance.linear * 0.25, 1e-3),
            &tolerance,
        )
        .unwrap();
    }

    #[test]
    fn roundoff_budget_is_four_ulps_of_the_largest_coordinate() {
        assert_eq!(contact_roundoff_budget([]), 4. * f64::EPSILON);
        let budget = contact_roundoff_budget([Point2::new(-16.6, 3.), Point2::new(2., 8.47)]);
        assert_eq!(budget, (16.6_f64.next_up() - 16.6) * 4.);
        assert!(budget < 1.5e-14);
    }

    #[test]
    fn one_ulp_face_and_cap_contacts_become_shared_nodes_but_real_gaps_stay() {
        // 8.47 - 0.1 and 16.6 + 0.1 differ by one ulp from the directly specified 8.37 and
        // 16.7 coordinates.
        let written = rect(16.5, 8.37, 16.7, 10.86);
        let derived = rect(16.5, 8.47 - 0.1, 16.6 + 0.1, 10.76);
        assert_ne!(derived.exterior[0].y, 8.37);
        assert_ne!(derived.exterior[1].x, 16.7);
        let tolerance = GeometryTolerance::default();
        let error = polygon_boolean(
            std::slice::from_ref(&derived),
            std::slice::from_ref(&written),
            PolygonBooleanOperation::Difference,
            PolygonBooleanOptions::default(),
            &tolerance,
        )
        .unwrap_err();
        assert_eq!(error.diagnostic().reason, GeometryReason::BooleanAmbiguity);
        let mut polygons = vec![written.clone(), derived];
        let budget = contact_roundoff_budget(
            polygons
                .iter()
                .flat_map(|p| p.exterior.iter().copied())
                .collect::<Vec<_>>(),
        );
        let summary = normalize_contacts(&mut polygons, &mut vec![], budget, &tolerance).unwrap();
        assert!(summary.changed());
        assert_eq!(summary.budget, budget);
        assert!(summary.max_displacement > 0. && summary.max_displacement <= budget);
        // The first input retains its specified vertices.
        assert!(
            written
                .exterior
                .iter()
                .all(|p| polygons[0].exterior.contains(p))
        );
        assert!(polygons[1].exterior.contains(&Point2::new(16.7, 8.37)));
        for polygon in &polygons {
            polygon.validate(&tolerance).unwrap();
        }
        let result = polygon_boolean(
            &polygons[1..],
            &polygons[..1],
            PolygonBooleanOperation::Difference,
            PolygonBooleanOptions::default(),
            &tolerance,
        )
        .unwrap();
        assert_eq!(result.area, 0.);

        // Gaps larger than the budget remain open; exact contacts only add nodes and boolean
        // ambiguity remains explicit.
        let gap = rect(16.5, 8.37 + 5e-9, 16.7, 10.76);
        let mut polygons = vec![written.clone(), gap.clone()];
        let summary = normalize_contacts_with_spacing(
            &mut polygons,
            &mut vec![],
            budget,
            tolerance.linear,
            &tolerance,
        )
        .unwrap();
        assert!(!summary.changed(), "{summary:?}");
        assert_eq!(summary.merged_vertices, 0);
        assert_eq!(polygons[1], gap);
        for polygon in &polygons {
            polygon.validate(&tolerance).unwrap();
        }
        let error = polygon_boolean(
            &polygons[1..],
            &polygons[..1],
            PolygonBooleanOperation::Difference,
            PolygonBooleanOptions::default(),
            &tolerance,
        )
        .unwrap_err();
        assert_eq!(error.diagnostic().reason, GeometryReason::BooleanAmbiguity);
        // With budget-sized spacing, an exact contact 5e-9 from an endpoint creates a below-
        // tolerance edge. Use linear-tolerance spacing to avoid that insertion.
        let mut polygons = vec![written, gap];
        normalize_contacts(&mut polygons, &mut vec![], budget, &tolerance).unwrap();
        assert_eq!(
            polygons[0]
                .validate(&tolerance)
                .unwrap_err()
                .diagnostic()
                .reason,
            GeometryReason::BelowTolerance
        );
    }
}
