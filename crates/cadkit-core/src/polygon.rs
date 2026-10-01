//! Çizim birimlerinde, kapalı ve düzlemsel halkaların sınırlı doğrulaması.

use crate::{Error, Point3, Result, Vec3};

/// İlk noktaya göre hesaplanan birim yüzey normali; dejenere halkada `None`.
pub fn normal(ring: &[Point3]) -> Option<Vec3> {
    let o = ring.first()?;
    let mut n = Vec3::default();
    for edge in ring.windows(2) {
        let a = edge.first()?;
        let b = edge.get(1)?;
        let u = Vec3::new(a.x - o.x, a.y - o.y, a.z - o.z);
        let v = Vec3::new(b.x - o.x, b.y - o.y, b.z - o.z);
        n.x += u.y * v.z - u.z * v.y;
        n.y += u.z * v.x - u.x * v.z;
        n.z += u.x * v.y - u.y * v.x;
    }
    let length = n.x.hypot(n.y).hypot(n.z);
    (length.is_finite() && length > 0.0)
        .then(|| Vec3::new(n.x / length, n.y / length, n.z / length))
}

/// Checks ring closure, coplanarity, intersections and hole containment.
/// Both tolerances use drawing units: `planarity_tolerance` bounds distance from
/// the exterior plane; `tolerance` controls boundary intersection checks.
/// `work` is the shared budget for edge comparisons.
pub fn validate(
    exterior: &[Point3],
    interiors: &[Vec<Point3>],
    planarity_tolerance: f64,
    tolerance: f64,
    work: &mut u64,
) -> Result<()> {
    let invalid = |s: &str| Error::invalid(0, s);
    if !tolerance.is_finite() || tolerance <= 0.0 {
        return Err(invalid("polygon tolerance must be finite and positive"));
    }
    if !planarity_tolerance.is_finite() || planarity_tolerance <= 0.0 {
        return Err(invalid(
            "polygon planarity tolerance must be finite and positive",
        ));
    }
    let n = normal(exterior).ok_or_else(|| invalid("polygon exterior is degenerate"))?;
    let o = exterior
        .first()
        .ok_or_else(|| invalid("polygon exterior is empty"))?;
    let axis = if n.x.abs() >= n.y.abs().max(n.z.abs()) {
        0
    } else if n.y.abs() >= n.z.abs() {
        1
    } else {
        2
    };
    let project = |p: &Point3| match axis {
        0 => (p.y - o.y, p.z - o.z),
        1 => (p.x - o.x, p.z - o.z),
        _ => (p.x - o.x, p.y - o.y),
    };
    let rings: Vec<&[Point3]> = std::iter::once(exterior)
        .chain(interiors.iter().map(Vec::as_slice))
        .collect();
    for ring in &rings {
        if ring.len() < 4 || ring.first() != ring.last() {
            return Err(invalid(
                "polygon ring must be closed and contain at least four positions",
            ));
        }
        for p in *ring {
            if !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite() {
                return Err(invalid("polygon contains non-finite coordinates"));
            }
            if ((p.x - o.x) * n.x + (p.y - o.y) * n.y + (p.z - o.z) * n.z).abs()
                > planarity_tolerance
            {
                return Err(invalid("polygon rings are not coplanar"));
            }
        }
        if normal(ring).is_none() {
            return Err(invalid("polygon ring is degenerate"));
        }
        for (i, a) in ring.windows(2).enumerate() {
            if a.first() == a.get(1) {
                return Err(invalid("polygon has a zero-length edge"));
            }
            for (j, b) in ring.windows(2).enumerate().skip(i + 1) {
                if j == i + 1 || (i == 0 && j + 2 == ring.len()) {
                    continue;
                }
                charge(work)?;
                if let (Some(a0), Some(a1), Some(b0), Some(b1)) =
                    (a.first(), a.get(1), b.first(), b.get(1))
                {
                    if intersects(
                        project(a0),
                        project(a1),
                        project(b0),
                        project(b1),
                        tolerance,
                    ) {
                        return Err(invalid("polygon ring intersects itself"));
                    }
                }
            }
        }
    }
    for (i, a) in rings.iter().enumerate() {
        for b in rings.iter().skip(i + 1) {
            for ae in a.windows(2) {
                for be in b.windows(2) {
                    charge(work)?;
                    if let (Some(a0), Some(a1), Some(b0), Some(b1)) =
                        (ae.first(), ae.get(1), be.first(), be.get(1))
                    {
                        if intersects(
                            project(a0),
                            project(a1),
                            project(b0),
                            project(b1),
                            tolerance,
                        ) {
                            return Err(invalid("polygon boundaries intersect"));
                        }
                    }
                }
            }
        }
    }
    for (i, hole) in interiors.iter().enumerate() {
        let p = hole.first().ok_or_else(|| invalid("empty interior ring"))?;
        if !inside(project(p), exterior, &project, work)? {
            return Err(invalid("polygon hole is outside exterior"));
        }
        for other in interiors.iter().skip(i + 1) {
            if let Some(q) = other.first() {
                if inside(project(p), other, &project, work)?
                    || inside(project(q), hole, &project, work)?
                {
                    return Err(invalid("polygon holes overlap"));
                }
            }
        }
    }
    Ok(())
}

fn charge(work: &mut u64) -> Result<()> {
    *work = work
        .checked_sub(1)
        .ok_or_else(|| Error::LimitExceeded("polygon intersection budget".into()))?;
    Ok(())
}

fn inside(
    p: (f64, f64),
    ring: &[Point3],
    project: &impl Fn(&Point3) -> (f64, f64),
    work: &mut u64,
) -> Result<bool> {
    let mut result = false;
    for edge in ring.windows(2) {
        charge(work)?;
        if let (Some(a), Some(b)) = (edge.first(), edge.get(1)) {
            let (a, b) = (project(a), project(b));
            if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
                result = !result;
            }
        }
    }
    Ok(result)
}

fn intersects(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64), tolerance: f64) -> bool {
    let side = |a: (f64, f64), b: (f64, f64), p: (f64, f64)| {
        (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
    };
    let near = |a: (f64, f64), b: (f64, f64), p: (f64, f64)| {
        side(a, b, p).abs() <= tolerance * (b.0 - a.0).hypot(b.1 - a.1)
            && p.0 >= a.0.min(b.0) - tolerance
            && p.0 <= a.0.max(b.0) + tolerance
            && p.1 >= a.1.min(b.1) - tolerance
            && p.1 <= a.1.max(b.1) + tolerance
    };
    let (ac, ad, ca, cb) = (side(a, b, c), side(a, b, d), side(c, d, a), side(c, d, b));
    (((ac > 0.0 && ad < 0.0) || (ac < 0.0 && ad > 0.0))
        && ((ca > 0.0 && cb < 0.0) || (ca < 0.0 && cb > 0.0)))
        || near(a, b, c)
        || near(a, b, d)
        || near(c, d, a)
        || near(c, d, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn square(a: f64, b: f64) -> Vec<Point3> {
        vec![
            Point3::xy(a, a),
            Point3::xy(b, a),
            Point3::xy(b, b),
            Point3::xy(a, b),
            Point3::xy(a, a),
        ]
    }
    #[test]
    fn holes_and_budgets() {
        let ok = |e: &[Point3], h: &[Vec<Point3>], work: u64| {
            validate(e, h, 1e-8, 1e-8, &mut { work }).is_ok()
        };
        assert!(ok(&square(0.0, 10.0), &[square(2.0, 3.0)], 1000));
        assert!(!ok(&square(0.0, 10.0), &[square(9.0, 11.0)], 1000));
        assert!(!ok(&square(0.0, 10.0), &[], 0));
    }

    #[test]
    fn planarity_tolerance_is_separate_from_intersection_tolerance() {
        // A hole 0.05 mm off the exterior plane, as 0.1 mm coordinate rounding produces.
        let holes = vec![
            square(2.0, 3.0)
                .into_iter()
                .map(|p| Point3::new(p.x, p.y, 0.000_05))
                .collect::<Vec<_>>(),
        ];
        let exterior = square(0.0, 10.0);
        let check = |planarity: f64| validate(&exterior, &holes, planarity, 1e-6, &mut 1000);
        assert!(check(1e-6).is_err());
        assert!(check(0.01).is_ok());
        assert!(check(0.0).is_err());
        assert!(check(f64::NAN).is_err());
        // A larger planarity tolerance does not turn nearby edges into intersections.
        let near = square(9.999, 9.9995);
        assert!(validate(&exterior, &[near], 0.01, 1e-6, &mut 1000).is_ok());
    }
}
