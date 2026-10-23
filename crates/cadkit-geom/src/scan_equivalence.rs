//! Compare candidate-filtered scans against exhaustive scans on deterministic random inputs.
//! Only the pairChecks diagnostic differs because of filtering.
use crate::polygon::FULL_SCAN;
use crate::*;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1_u64 << 53) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() * n as f64) as usize % n
    }
}

fn tolerance() -> GeometryTolerance {
    GeometryTolerance {
        linear: 1e-4,
        angular: 1e-9,
        area: 1e-8,
    }
}

fn both<T>(run: impl Fn() -> T) -> (T, T) {
    FULL_SCAN.with(|f| f.set(false));
    let filtered = run();
    FULL_SCAN.with(|f| f.set(true));
    let full = run();
    FULL_SCAN.with(|f| f.set(false));
    (filtered, full)
}

/// Grid-aligned boxes, partly touching or overlapping, with some displacements near the
/// tolerance threshold.
fn boxes(rng: &mut Lcg, count: usize, t: &GeometryTolerance) -> Vec<Polygon> {
    (0..count)
        .map(|_| {
            let x = (rng.below(40) as f64) * 0.25;
            let y = (rng.below(40) as f64) * 0.25;
            let w = 0.25 + (rng.below(8) as f64) * 0.25;
            let h = 0.25 + (rng.below(8) as f64) * 0.25;
            let nudge = match rng.below(6) {
                0 => t.linear * 0.5,
                1 => t.linear * 3.,
                2 => f64::EPSILON * 64.,
                _ => 0.,
            };
            let angle = if rng.below(4) == 0 {
                (rng.next() - 0.5) * 0.2
            } else {
                0.
            };
            let (s, c) = angle.sin_cos();
            let corners = [(0., 0.), (w, 0.), (w, h), (0., h)];
            Polygon {
                exterior: corners
                    .iter()
                    .map(|(dx, dy)| Point2::new(x + nudge + dx * c - dy * s, y + dx * s + dy * c))
                    .collect(),
                holes: vec![],
            }
        })
        .collect()
}

fn strip_pair_checks(
    result: GeometryResult<PolygonBooleanResult>,
) -> GeometryResult<PolygonBooleanResult> {
    result.map(|mut r| {
        r.diagnostics.pair_checks = 0;
        r
    })
}

#[test]
fn boolean_preflight_matches_full_scan() {
    let t = tolerance();
    let mut rng = Lcg(7);
    let mut outcomes = [0_usize; 2];
    for round in 0..300 {
        let polygons = {
            let count = 2 + rng.below(40);
            boxes(&mut rng, count, &t)
        };
        let options = PolygonBooleanOptions::with_error_budget(t.linear * 0.25, 1e-3);
        let world = Polygon {
            exterior: vec![
                Point2::new(-1., -1.),
                Point2::new(13., -1.),
                Point2::new(13., 13.),
                Point2::new(-1., 13.),
            ],
            holes: vec![],
        };
        let operation = [
            PolygonBooleanOperation::Union,
            PolygonBooleanOperation::Difference,
            PolygonBooleanOperation::Intersection,
        ][round % 3];
        let (subjects, clips) = if operation == PolygonBooleanOperation::Union {
            (polygons.clone(), vec![])
        } else {
            (vec![world], polygons.clone())
        };
        let (filtered, full) =
            both(|| strip_pair_checks(polygon_boolean(&subjects, &clips, operation, options, &t)));
        assert_eq!(
            format!("{filtered:?}"),
            format!("{full:?}"),
            "round {round}"
        );
        outcomes[usize::from(full.is_ok())] += 1;
    }
    // Exercise both successful and ambiguous/error paths.
    assert!(outcomes[0] > 0 && outcomes[1] > 0, "{outcomes:?}");
}

#[test]
fn polygon_slice_matches_full_scan() {
    let t = tolerance();
    let mut rng = Lcg(11);
    for round in 0..200 {
        let polygons: Vec<_> = boxes(&mut rng, 1, &t).into_iter().take(1).collect();
        let bounds = polygons[0].bounds().unwrap();
        let separators: Vec<_> = (0..1 + rng.below(6))
            .map(|_| {
                let fx = rng.next();
                let fy = rng.next();
                if rng.below(2) == 0 {
                    let x = bounds.min.x + (bounds.max.x - bounds.min.x) * fx;
                    LineSegment {
                        start: Point2::new(x, bounds.min.y - 0.1),
                        end: Point2::new(x, bounds.max.y + 0.1),
                    }
                } else {
                    let y = bounds.min.y + (bounds.max.y - bounds.min.y) * fy;
                    LineSegment {
                        start: Point2::new(bounds.min.x - 0.1, y),
                        end: Point2::new(bounds.max.x + 0.1, y),
                    }
                }
            })
            .collect();
        let options = PolygonBooleanOptions::with_error_budget(t.linear * 0.25, 1e-3);
        let (filtered, full) = both(|| polygon_slice(&polygons, &separators, options, &t));
        assert_eq!(
            format!("{filtered:?}"),
            format!("{full:?}"),
            "round {round}"
        );
    }
}

#[test]
fn contact_normalization_matches_full_scan() {
    let t = tolerance();
    let mut rng = Lcg(23);
    let mut changed = 0;
    for round in 0..300 {
        // Touching boxes with vertices displaced by a few ulps or near the budget boundary.
        let mut polygons = {
            let count = 2 + rng.below(40);
            boxes(&mut rng, count, &t)
        };
        let budget_scale =
            contact_roundoff_budget(polygons.iter().flat_map(|p| p.exterior.iter().copied()));
        for polygon in &mut polygons {
            for point in &mut polygon.exterior {
                let shift = match rng.below(5) {
                    0 => budget_scale * 0.5,
                    1 => budget_scale * 0.999,
                    2 => budget_scale * 1.001,
                    _ => 0.,
                };
                point.x += shift;
                point.y -= shift * (rng.below(2) as f64);
            }
        }
        let mut separators: Vec<_> = (0..rng.below(4))
            .map(|_| {
                let y = (rng.below(40) as f64) * 0.25 + budget_scale * (rng.below(3) as f64);
                LineSegment {
                    start: Point2::new(-0.5, y),
                    end: Point2::new(11., y),
                }
            })
            .collect();
        let budget = if round % 2 == 0 {
            budget_scale
        } else {
            t.linear * 0.1
        };
        let (filtered, full) = both(|| {
            let mut p = polygons.clone();
            let mut s = separators.clone();
            let summary = normalize_contacts(&mut p, &mut s, budget, &t);
            (summary, p, s)
        });
        assert_eq!(
            format!("{filtered:?}"),
            format!("{full:?}"),
            "round {round}"
        );
        if full.0.as_ref().is_ok_and(ContactNormalization::changed) {
            changed += 1;
        }
        separators.clear();
    }
    assert!(
        changed > 0,
        "Normalization did not change geometry in any case"
    );
}
