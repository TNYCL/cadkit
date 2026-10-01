//! Curve sampling for versions that lack the native entity (R12 ELLIPSE / SPLINE / HATCH).

use cadkit_core::{Point3, Vec3};

/// Samples an ellipse (or arc) from `start` to `end` (radians, parametric angle).
pub(crate) fn ellipse_points(
    center: Point3,
    major: Vec3,
    ratio: f64,
    start: f64,
    end: f64,
    normal: Vec3,
    segments: usize,
) -> Vec<Point3> {
    let n = normal.normalized().unwrap_or(Vec3::Z);
    // Minor axis direction = N x major, scaled by the ratio.
    let minor = Vec3::new(
        (n.y * major.z - n.z * major.y) * ratio,
        (n.z * major.x - n.x * major.z) * ratio,
        (n.x * major.y - n.y * major.x) * ratio,
    );
    let mut sweep = end - start;
    if sweep <= 0.0 {
        sweep += std::f64::consts::TAU;
    }
    let segments = segments.max(4);
    (0..=segments)
        .map(|i| {
            let t = start + sweep * (i as f64) / (segments as f64);
            let (s, c) = t.sin_cos();
            Point3::new(
                center.x + major.x * c + minor.x * s,
                center.y + major.y * c + minor.y * s,
                center.z + major.z * c + minor.z * s,
            )
        })
        .collect()
}

/// Clamped uniform knot vector for `n` control points of `degree`.
pub(crate) fn clamped_knots(n: usize, degree: usize) -> Vec<f64> {
    if degree > MAX_DEGREE {
        return Vec::new();
    }
    let m = n.saturating_add(degree).saturating_add(1);
    let interior = n.saturating_sub(degree);
    (0..m)
        .map(|i| {
            if i <= degree {
                0.0
            } else if i >= n {
                interior as f64
            } else {
                (i - degree) as f64
            }
        })
        .collect()
}

/// Largest spline degree the writer accepts (foreign formats can store absurd values).
pub(crate) const MAX_DEGREE: usize = 64;

/// Most points sampled from one curve for R12 output.
const MAX_SAMPLES: usize = 4096;

/// Index `s` of the knot span `[knots[s], knots[s+1])` that contains `u`, within
/// `degree..n`; `u` is clamped into the domain.
fn find_span(n: usize, degree: usize, u: f64, knots: &[f64]) -> usize {
    let k = |j: usize| knots.get(j).copied().unwrap_or(0.0);
    let (mut lo, mut hi) = (degree, n);
    // Binary search for the last span start <= u (spans are non-decreasing).
    while lo + 1 < hi {
        let mid = lo + (hi - lo) / 2;
        if k(mid) <= u {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// The `degree + 1` non-zero B-spline basis functions at `u` in span `span`
/// (Cox-de Boor in its triangular, O(degree^2) form).
fn basis_functions(span: usize, u: f64, degree: usize, knots: &[f64]) -> Vec<f64> {
    let k = |j: usize| knots.get(j).copied().unwrap_or(0.0);
    let mut n = vec![0.0; degree + 1];
    let mut left = vec![0.0; degree + 1];
    let mut right = vec![0.0; degree + 1];
    if let Some(first) = n.first_mut() {
        *first = 1.0;
    }
    for j in 1..=degree {
        if let (Some(l), Some(r)) = (left.get_mut(j), right.get_mut(j)) {
            *l = u - k(span + 1 - j);
            *r = k(span + j) - u;
        }
        let mut saved = 0.0;
        for r in 0..j {
            let rv = right.get(r + 1).copied().unwrap_or(0.0);
            let lv = left.get(j - r).copied().unwrap_or(0.0);
            let denom = rv + lv;
            let nr = n.get(r).copied().unwrap_or(0.0);
            let temp = if denom != 0.0 { nr / denom } else { 0.0 };
            if let Some(slot) = n.get_mut(r) {
                *slot = saved + rv * temp;
            }
            saved = lv * temp;
        }
        if let Some(slot) = n.get_mut(j) {
            *slot = saved;
        }
    }
    n
}

/// Samples a (rational) B-spline; returns an empty vector when the data is inconsistent
/// (bad degree, wrong knot count, non-finite or decreasing knots). Cost is bounded by
/// `MAX_SAMPLES * degree^2`.
pub(crate) fn spline_points(
    degree: usize,
    knots: &[f64],
    ctrl: &[Point3],
    weights: &[f64],
    samples: usize,
) -> Vec<Point3> {
    let n = ctrl.len();
    if !(1..=MAX_DEGREE).contains(&degree) || n <= degree || knots.len() != n + degree + 1 {
        return Vec::new();
    }
    if knots.iter().any(|k| !k.is_finite())
        || knots
            .windows(2)
            .any(|w| matches!((w.first(), w.get(1)), (Some(a), Some(b)) if a > b))
    {
        return Vec::new();
    }
    let (Some(&u0), Some(&u1)) = (knots.get(degree), knots.get(n)) else {
        return Vec::new();
    };
    if u1 <= u0 {
        return Vec::new();
    }
    let rational = weights.len() == n;
    let samples = samples.clamp(2, MAX_SAMPLES);
    let mut out = Vec::with_capacity(samples + 1);
    for s in 0..=samples {
        let u = if s == samples {
            u1
        } else {
            u0 + (u1 - u0) * (s as f64) / (samples as f64)
        };
        let span = find_span(n, degree, u, knots);
        let basis = basis_functions(span, u, degree, knots);
        let (mut x, mut y, mut z, mut wsum) = (0.0, 0.0, 0.0, 0.0);
        for (j, b) in basis.iter().enumerate() {
            let Some(i) = (span + j).checked_sub(degree) else {
                continue;
            };
            let Some(p) = ctrl.get(i) else { continue };
            let w = if rational {
                weights.get(i).copied().unwrap_or(1.0)
            } else {
                1.0
            };
            let b = b * w;
            x += b * p.x;
            y += b * p.y;
            z += b * p.z;
            wsum += b;
        }
        if wsum.abs() > 1e-12 {
            out.push(Point3::new(x / wsum, y / wsum, z / wsum));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quadratic_bezier() {
        let ctrl = [
            Point3::xy(0.0, 0.0),
            Point3::xy(1.0, 2.0),
            Point3::xy(2.0, 0.0),
        ];
        let knots = clamped_knots(3, 2);
        let pts = spline_points(2, &knots, &ctrl, &[], 4);
        assert_eq!(pts.len(), 5);
        let mid = pts[2];
        assert!((mid.x - 1.0).abs() < 1e-12 && (mid.y - 1.0).abs() < 1e-12);
        assert!((pts[4].x - 2.0).abs() < 1e-9 && pts[4].y.abs() < 1e-9);
    }

    #[test]
    fn high_degree_is_fast_and_bounded() {
        // Degree 60 with 70 control points used to take O(2^degree) per sample.
        let ctrl: Vec<Point3> = (0..70)
            .map(|i| Point3::xy(f64::from(i), f64::from(i % 3)))
            .collect();
        let knots = clamped_knots(70, 60);
        let t = std::time::Instant::now();
        let pts = spline_points(60, &knots, &ctrl, &[], 8 * 70);
        assert!(pts.len() > 500 && t.elapsed().as_secs() < 5);
        assert!(clamped_knots(10, usize::MAX).is_empty());
        assert!(spline_points(MAX_DEGREE + 1, &knots, &ctrl, &[], 10).is_empty());
        let mut bad = knots.clone();
        bad[5] = f64::NAN;
        assert!(spline_points(60, &bad, &ctrl, &[], 10).is_empty());
    }

    #[test]
    fn ellipse_quarter() {
        let pts = ellipse_points(
            Point3::xy(0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            0.5,
            0.0,
            std::f64::consts::FRAC_PI_2,
            Vec3::Z,
            8,
        );
        let last = pts[pts.len() - 1];
        assert!(last.x.abs() < 1e-12 && (last.y - 1.0).abs() < 1e-12);
    }
}
