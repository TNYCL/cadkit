//! Geometry helpers: vector math, affine [`Transform`], circular/elliptic arcs, polyline
//! bulges and NURBS evaluation.
//!
//! All angles are radians. Nothing here panics; degenerate input yields `None` or an
//! identity-like result.

use std::f64::consts::{PI, TAU};

use crate::geom::{BBox, Point3, Vec3};

// ---------------------------------------------------------------------------------------------
// Vector helpers
// ---------------------------------------------------------------------------------------------

impl Vec3 {
    /// Dot product.
    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    /// Cross product `self x o`.
    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    /// Euclidean length.
    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    /// Unit vector, or `None` when the length is zero or not finite.
    pub fn normalized(self) -> Option<Vec3> {
        let l = self.length();
        if l.is_finite() && l > 1e-300 {
            Some(self.scaled(1.0 / l))
        } else {
            None
        }
    }

    /// Multiplies every component by `k`.
    pub fn scaled(self, k: f64) -> Vec3 {
        Vec3::new(self.x * k, self.y * k, self.z * k)
    }

    /// Component-wise sum.
    pub fn plus(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }

    /// Component-wise difference.
    pub fn minus(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Point3 {
    /// Vector from `self` to `other`.
    pub fn vector_to(self, other: Point3) -> Vec3 {
        Vec3::new(other.x - self.x, other.y - self.y, other.z - self.z)
    }

    /// The point moved by `v`.
    pub fn translated(self, v: Vec3) -> Point3 {
        Point3::new(self.x + v.x, self.y + v.y, self.z + v.z)
    }

    /// True when all coordinates are finite.
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

impl BBox {
    /// A degenerate box containing exactly `p`.
    pub fn from_point(p: Point3) -> Self {
        Self { min: p, max: p }
    }

    /// Grows the box to contain `p`. Non-finite points are ignored.
    pub fn extend(&mut self, p: Point3) {
        if !p.is_finite() {
            return;
        }
        self.min = Point3::new(
            self.min.x.min(p.x),
            self.min.y.min(p.y),
            self.min.z.min(p.z),
        );
        self.max = Point3::new(
            self.max.x.max(p.x),
            self.max.y.max(p.y),
            self.max.z.max(p.z),
        );
    }

    /// Grows the box to contain `other`.
    pub fn union(&mut self, other: &BBox) {
        self.extend(other.min);
        self.extend(other.max);
    }

    /// Extent along X.
    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }

    /// Extent along Y.
    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }
}

/// Adds `p` to an optional box, creating it on first use. Non-finite points are ignored.
pub fn extend_bbox(bb: &mut Option<BBox>, p: Point3) {
    if !p.is_finite() {
        return;
    }
    match bb {
        Some(b) => b.extend(p),
        None => *bb = Some(BBox::from_point(p)),
    }
}

// ---------------------------------------------------------------------------------------------
// Transform
// ---------------------------------------------------------------------------------------------

/// DXF arbitrary-axis algorithm: the plane axes `(Ax, Ay, Az)` for an entity `normal`
/// (Ax is the zero-angle direction of the plane, Az the normal).
///
/// If both |Nx| and |Ny| are below 1/64 the world Y axis is crossed with N, otherwise the
/// world Z axis; `Ay = N x Ax`. A zero or non-finite normal yields the world axes.
pub fn arbitrary_axes(normal: Vec3) -> (Vec3, Vec3, Vec3) {
    let Some(n) = normal.normalized() else {
        return (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::Z);
    };
    let wz = Vec3::Z;
    let wy = Vec3::new(0.0, 1.0, 0.0);
    let ax = if n.x.abs() < 1.0 / 64.0 && n.y.abs() < 1.0 / 64.0 {
        wy.cross(n)
    } else {
        wz.cross(n)
    };
    let ax = ax.normalized().unwrap_or(Vec3::new(1.0, 0.0, 0.0));
    let ay = n.cross(ax).normalized().unwrap_or(Vec3::new(0.0, 1.0, 0.0));
    (ax, ay, n)
}

/// A 3x4 affine transform: `p' = x*p.x + y*p.y + z*p.z + t`.
///
/// `x`, `y`, `z` are the images of the unit axes (the matrix columns) and `t` is the
/// translation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Image of the X axis.
    pub x: Vec3,
    /// Image of the Y axis.
    pub y: Vec3,
    /// Image of the Z axis.
    pub z: Vec3,
    /// Translation.
    pub t: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self::identity()
    }
}

impl Transform {
    /// The identity transform.
    pub fn identity() -> Self {
        Self {
            x: Vec3::new(1.0, 0.0, 0.0),
            y: Vec3::new(0.0, 1.0, 0.0),
            z: Vec3::new(0.0, 0.0, 1.0),
            t: Vec3::new(0.0, 0.0, 0.0),
        }
    }

    /// Pure translation.
    pub fn translation(v: Vec3) -> Self {
        Self {
            t: v,
            ..Self::identity()
        }
    }

    /// Axis-aligned scaling about the origin.
    pub fn scaling(sx: f64, sy: f64, sz: f64) -> Self {
        Self {
            x: Vec3::new(sx, 0.0, 0.0),
            y: Vec3::new(0.0, sy, 0.0),
            z: Vec3::new(0.0, 0.0, sz),
            t: Vec3::new(0.0, 0.0, 0.0),
        }
    }

    /// Rotation about the Z axis, counter-clockwise by `angle` radians.
    pub fn rotation_z(angle: f64) -> Self {
        let (s, c) = angle.sin_cos();
        Self {
            x: Vec3::new(c, s, 0.0),
            y: Vec3::new(-s, c, 0.0),
            z: Vec3::new(0.0, 0.0, 1.0),
            t: Vec3::new(0.0, 0.0, 0.0),
        }
    }

    /// Transform whose columns are the given axes, with `origin` as translation.
    pub fn from_axes(x: Vec3, y: Vec3, z: Vec3, origin: Vec3) -> Self {
        Self { x, y, z, t: origin }
    }

    /// Maps plane-local coordinates to world coordinates: local `(x, y)` lie in the plane
    /// through `origin` spanned by [`arbitrary_axes`]`(normal)`, local `z` runs along the normal.
    pub fn plane(origin: Point3, normal: Vec3) -> Self {
        let (ax, ay, az) = arbitrary_axes(normal);
        Self::from_axes(ax, ay, az, Vec3::new(origin.x, origin.y, origin.z))
    }

    /// Applies the linear part only (directions, extents).
    pub fn apply_vector(&self, v: Vec3) -> Vec3 {
        self.x
            .scaled(v.x)
            .plus(self.y.scaled(v.y))
            .plus(self.z.scaled(v.z))
    }

    /// Applies the full transform to a point.
    pub fn apply_point(&self, p: Point3) -> Point3 {
        let v = self.apply_vector(Vec3::new(p.x, p.y, p.z));
        Point3::new(v.x + self.t.x, v.y + self.t.y, v.z + self.t.z)
    }

    /// Composition: the result applies `inner` first, then `self`
    /// (`self.compose(inner).apply_point(p) == self.apply_point(inner.apply_point(p))`).
    pub fn compose(&self, inner: &Transform) -> Transform {
        let t = self.apply_point(Point3::new(inner.t.x, inner.t.y, inner.t.z));
        Transform {
            x: self.apply_vector(inner.x),
            y: self.apply_vector(inner.y),
            z: self.apply_vector(inner.z),
            t: Vec3::new(t.x, t.y, t.z),
        }
    }

    /// Determinant of the linear part; negative when the transform mirrors.
    pub fn determinant(&self) -> f64 {
        self.x.dot(self.y.cross(self.z))
    }

    /// Transform of a block reference: block coordinates to the coordinates of the space
    /// holding the insert (WCS).
    ///
    /// `p_world = position + Plane(normal) * Rz(rotation) * (offset + scale * (p - base_point))`,
    /// where `Plane(normal)` is the linear map of [`Transform::plane`]. `position` is a world
    /// point; `offset` is the array offset of one cell in the rotated, unscaled insert frame.
    pub fn from_insert_offset(
        position: Point3,
        scale: Vec3,
        rotation: f64,
        normal: Vec3,
        base_point: Point3,
        offset: Vec3,
    ) -> Transform {
        let minus_base =
            Transform::translation(Vec3::new(-base_point.x, -base_point.y, -base_point.z));
        let s = Transform::scaling(scale.x, scale.y, scale.z);
        let off = Transform::translation(offset);
        let r = Transform::rotation_z(rotation);
        Transform::plane(position, normal)
            .compose(&r)
            .compose(&off)
            .compose(&s)
            .compose(&minus_base)
    }

    /// [`Transform::from_insert_offset`] without an array offset.
    pub fn from_insert(
        position: Point3,
        scale: Vec3,
        rotation: f64,
        normal: Vec3,
        base_point: Point3,
    ) -> Transform {
        Self::from_insert_offset(
            position,
            scale,
            rotation,
            normal,
            base_point,
            Vec3::new(0.0, 0.0, 0.0),
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Arcs and ellipses
// ---------------------------------------------------------------------------------------------

/// A (possibly partial) ellipse in parametric form
/// `P(t) = center + u*cos(t) + v*sin(t)`, travelled from `t0` to `t1`.
///
/// `u` and `v` are conjugate semi-diameters (perpendicular for a true ellipse, but any pair
/// is valid, which keeps the form closed under affine transforms). `t1 < t0` means the
/// curve is travelled clockwise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EllipseArc {
    /// Center.
    pub center: Point3,
    /// First semi-diameter (point at `t = 0` is `center + u`).
    pub u: Vec3,
    /// Second semi-diameter (point at `t = pi/2` is `center + v`).
    pub v: Vec3,
    /// Start parameter in radians.
    pub t0: f64,
    /// End parameter in radians.
    pub t1: f64,
}

/// Counter-clockwise sweep from `start` to `end`, in (0, 2*pi]. Equal angles mean a full turn.
pub fn ccw_sweep(start: f64, end: f64) -> f64 {
    let d = (end - start).rem_euclid(TAU);
    if d < 1e-12 { TAU } else { d }
}

impl EllipseArc {
    /// Circular arc in the XY plane of the local frame: center `c`, radius `r`, CCW from
    /// `start` to `end` (equal angles give a full circle).
    pub fn circle(c: Point3, r: f64, start: f64, end: f64) -> Self {
        Self {
            center: c,
            u: Vec3::new(r, 0.0, 0.0),
            v: Vec3::new(0.0, r, 0.0),
            t0: start,
            t1: start + ccw_sweep(start, end),
        }
    }

    /// Point at parameter `t`.
    pub fn point(&self, t: f64) -> Point3 {
        let (s, c) = t.sin_cos();
        Point3::new(
            self.center.x + self.u.x * c + self.v.x * s,
            self.center.y + self.u.y * c + self.v.y * s,
            self.center.z + self.u.z * c + self.v.z * s,
        )
    }

    /// Signed parameter sweep `t1 - t0`.
    pub fn sweep(&self) -> f64 {
        self.t1 - self.t0
    }

    /// The arc after applying an affine transform (parameters are preserved).
    pub fn transformed(&self, tf: &Transform) -> Self {
        Self {
            center: tf.apply_point(self.center),
            u: tf.apply_vector(self.u),
            v: tf.apply_vector(self.v),
            t0: self.t0,
            t1: self.t1,
        }
    }

    /// Exact bounding box of the traveled curve, merged into `bb`.
    pub fn extend_bbox(&self, bb: &mut Option<BBox>) {
        extend_bbox(bb, self.point(self.t0));
        extend_bbox(bb, self.point(self.t1));
        let (lo, hi) = if self.t0 <= self.t1 {
            (self.t0, self.t1)
        } else {
            (self.t1, self.t0)
        };
        let hi = hi.min(lo + TAU);
        let axes = [
            (self.u.x, self.v.x),
            (self.u.y, self.v.y),
            (self.u.z, self.v.z),
        ];
        for (a, b) in axes {
            if !(a.is_finite() && b.is_finite()) {
                continue;
            }
            // d/dt (a cos t + b sin t) = 0 at t = atan2(b, a) + k*pi.
            let te = b.atan2(a);
            let mut k = ((lo - te) / PI).ceil();
            let mut guard = 0;
            while guard < 4 {
                let t = te + k * PI;
                if t > hi {
                    break;
                }
                extend_bbox(bb, self.point(t));
                k += 1.0;
                guard += 1;
            }
        }
    }
}

/// Principal axes of the ellipse spanned by conjugate semi-diameters `u`, `v` (2D).
/// Returns `(semi_major, semi_minor, rotation_of_major_axis)`.
pub fn conjugate_to_axes(u: (f64, f64), v: (f64, f64)) -> (f64, f64, f64) {
    let uu = u.0 * u.0 + u.1 * u.1;
    let vv = v.0 * v.0 + v.1 * v.1;
    let uv = u.0 * v.0 + u.1 * v.1;
    // |P(t)|^2 is extremal where tan(2t) = 2 u.v / (u.u - v.v).
    let t0 = 0.5 * (2.0 * uv).atan2(uu - vv);
    let (s, c) = t0.sin_cos();
    let a = (u.0 * c + v.0 * s, u.1 * c + v.1 * s);
    let b = (-u.0 * s + v.0 * c, -u.1 * s + v.1 * c);
    let ra = a.0.hypot(a.1);
    let rb = b.0.hypot(b.1);
    if ra >= rb {
        (ra, rb, a.1.atan2(a.0))
    } else {
        (rb, ra, b.1.atan2(b.0))
    }
}

/// Ellipse (or elliptical arc) from DXF-style data: `major_axis` is the vector from the
/// center to the end of the major axis, `ratio` = minor / major, parameters in radians.
/// The minor axis direction is `normal x major_axis`.
pub fn ellipse_arc(
    center: Point3,
    major_axis: Vec3,
    ratio: f64,
    normal: Vec3,
    start: f64,
    end: f64,
) -> EllipseArc {
    let n = normal.normalized().unwrap_or(Vec3::Z);
    let minor_dir = n
        .cross(major_axis)
        .scaled(if ratio.is_finite() { ratio } else { 1.0 });
    EllipseArc {
        center,
        u: major_axis,
        v: minor_dir,
        t0: start,
        t1: start + ccw_sweep(start, end),
    }
}

/// Point of an ellipse at parameter `t` (see [`ellipse_arc`] for the conventions).
pub fn ellipse_point(center: Point3, major_axis: Vec3, ratio: f64, normal: Vec3, t: f64) -> Point3 {
    ellipse_arc(center, major_axis, ratio, normal, 0.0, TAU).point(t)
}

/// A circular arc recovered from a polyline segment with a bulge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BulgeArc {
    /// Center in the plane of the segment.
    pub center: (f64, f64),
    /// Radius.
    pub radius: f64,
    /// Angle of the segment start as seen from the center.
    pub start_angle: f64,
    /// Signed sweep: positive counter-clockwise, `4 * atan(bulge)`.
    pub sweep: f64,
}

impl BulgeArc {
    /// Angle of the segment end (`start_angle + sweep`).
    pub fn end_angle(&self) -> f64 {
        self.start_angle + self.sweep
    }

    /// True when the arc runs counter-clockwise.
    pub fn is_ccw(&self) -> bool {
        self.sweep > 0.0
    }
}

/// Converts the segment `p0 -> p1` with the given `bulge` (tan of a quarter of the included
/// angle, positive = counter-clockwise) into a circular arc. `None` for a straight segment
/// (|bulge| ~ 0), coincident end points or non-finite input.
pub fn bulge_to_arc(p0: (f64, f64), p1: (f64, f64), bulge: f64) -> Option<BulgeArc> {
    if !bulge.is_finite() || bulge.abs() < 1e-12 {
        return None;
    }
    let (dx, dy) = (p1.0 - p0.0, p1.1 - p0.1);
    let d = dx.hypot(dy);
    if !d.is_finite() || d < 1e-300 {
        return None;
    }
    let radius = d * (1.0 + bulge * bulge) / (4.0 * bulge.abs());
    // Distance from the chord midpoint to the center, signed to the left of p0 -> p1:
    // (d/2) / tan(theta/2) with tan(2*atan(b)) = 2b / (1 - b^2).
    let off = (d / 2.0) * (1.0 - bulge * bulge) / (2.0 * bulge);
    let mid = ((p0.0 + p1.0) / 2.0, (p0.1 + p1.1) / 2.0);
    let center = (mid.0 - dy / d * off, mid.1 + dx / d * off);
    let start_angle = (p0.1 - center.1).atan2(p0.0 - center.0);
    Some(BulgeArc {
        center,
        radius,
        start_angle,
        sweep: 4.0 * bulge.atan(),
    })
}

// ---------------------------------------------------------------------------------------------
// NURBS
// ---------------------------------------------------------------------------------------------

/// Clamped uniform knot vector for `n` control points of the given degree
/// (`n + degree + 1` knots, domain `[0, n - degree]`).
pub fn clamped_uniform_knots(n: usize, degree: usize) -> Vec<f64> {
    let spans = n.saturating_sub(degree);
    let mut k = Vec::with_capacity(n + degree + 1);
    for i in 0..(n + degree + 1) {
        let v = if i <= degree {
            0.0
        } else if i >= n {
            spans as f64
        } else {
            (i - degree) as f64
        };
        k.push(v);
    }
    k
}

/// Highest B-spline degree that is evaluated; anything above is drawn as its control polygon.
pub const MAX_NURBS_DEGREE: usize = 25;

/// Upper bound on de Boor work (samples x degree^2) per flattened spline.
const MAX_NURBS_FLOP: usize = 5_000_000;

#[derive(Clone, Copy)]
struct H {
    x: f64,
    y: f64,
    z: f64,
    w: f64,
}

impl H {
    const ZERO: H = H {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 0.0,
    };

    fn lerp(a: H, b: H, alpha: f64) -> H {
        let m = 1.0 - alpha;
        H {
            x: a.x * m + b.x * alpha,
            y: a.y * m + b.y * alpha,
            z: a.z * m + b.z * alpha,
            w: a.w * m + b.w * alpha,
        }
    }
}

/// Index of the knot span containing `t`: the largest `k` in `[degree, n - 1]` with
/// `knots[k] <= t`.
fn find_span(knots: &[f64], n: usize, degree: usize, t: f64) -> Option<usize> {
    let s = knots.get(degree..n)?;
    let count = s.partition_point(|&x| x <= t);
    Some(degree + count.saturating_sub(1))
}

fn nurbs_eval_h(degree: usize, knots: &[f64], pts: &[H], t: f64) -> Option<Point3> {
    let n = pts.len();
    if degree == 0 || degree > MAX_NURBS_DEGREE || degree >= n || knots.len() != n + degree + 1 {
        return None;
    }
    let k = find_span(knots, n, degree, t)?;
    // Fixed scratch space: evaluation allocates nothing.
    let mut d = [H::ZERO; MAX_NURBS_DEGREE + 1];
    for (slot, p) in d.iter_mut().zip(pts.get(k.checked_sub(degree)?..=k)?) {
        *slot = *p;
    }
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = j + k - degree;
            let lo = *knots.get(i)?;
            let hi = *knots.get(i + degree + 1 - r)?;
            let den = hi - lo;
            let alpha = if den.abs() < 1e-300 {
                0.0
            } else {
                (t - lo) / den
            };
            let a = *d.get(j - 1)?;
            let b = *d.get(j)?;
            *d.get_mut(j)? = H::lerp(a, b, alpha);
        }
    }
    let r = *d.get(degree)?;
    if r.w.abs() < 1e-300 {
        return None;
    }
    Some(Point3::new(r.x / r.w, r.y / r.w, r.z / r.w))
}

fn homogeneous(control: &[Point3], weights: &[f64]) -> Vec<H> {
    let rational =
        weights.len() == control.len() && weights.iter().all(|w| w.is_finite() && *w > 0.0);
    control
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let w = if rational {
                weights.get(i).copied().unwrap_or(1.0)
            } else {
                1.0
            };
            H {
                x: p.x * w,
                y: p.y * w,
                z: p.z * w,
                w,
            }
        })
        .collect()
}

/// Evaluates a (rational) B-spline at parameter `t` with de Boor's algorithm.
///
/// `weights` may be empty (non-rational). `t` is clamped to the curve domain
/// `[knots[degree], knots[n]]`. Returns `None` when the arguments are inconsistent
/// (`knots.len() != control.len() + degree + 1`, degree 0, too few points).
pub fn nurbs_eval(
    degree: usize,
    knots: &[f64],
    control: &[Point3],
    weights: &[f64],
    t: f64,
) -> Option<Point3> {
    let n = control.len();
    let lo = *knots.get(degree)?;
    let hi = *knots.get(n)?;
    let t = if hi >= lo { t.clamp(lo, hi) } else { t };
    nurbs_eval_h(degree, knots, &homogeneous(control, weights), t)
}

/// Samples a NURBS curve into a polyline.
///
/// Robust against messy input: a missing or inconsistent knot vector is replaced by a
/// clamped uniform one, invalid weights are ignored, and a degree that is too high for the
/// number of control points is lowered. With fewer than 2 control points the control points
/// are returned unchanged, and when the domain is empty the control polygon is returned.
/// `samples_per_span` points are generated per non-empty knot span (total capped at 100 000).
pub fn nurbs_flatten(
    degree: u32,
    knots: &[f64],
    control: &[Point3],
    weights: &[f64],
    samples_per_span: usize,
) -> Vec<Point3> {
    let n = control.len();
    if n < 2 {
        return control.to_vec();
    }
    let p = (degree as usize).clamp(1, n - 1);
    if p > MAX_NURBS_DEGREE {
        return control.to_vec();
    }
    let knots_ok = knots.len() == n + p + 1
        && knots.iter().all(|k| k.is_finite())
        && knots.windows(2).all(|w| w.first() <= w.get(1));
    let knots: Vec<f64> = if knots_ok {
        knots.to_vec()
    } else {
        clamped_uniform_knots(n, p)
    };
    let (Some(&lo), Some(&hi)) = (knots.get(p), knots.get(n)) else {
        return control.to_vec();
    };
    if hi <= lo {
        return control.to_vec();
    }
    let pts = homogeneous(control, weights);
    let spans: Vec<(f64, f64)> = knots
        .get(p..=n)
        .map(|s| {
            s.windows(2)
                .filter_map(|w| Some((*w.first()?, *w.get(1)?)))
                .filter(|(a, b)| b > a)
                .collect()
        })
        .unwrap_or_default();
    let max_samples = (MAX_NURBS_FLOP / (p * p)).clamp(2, 100_000);
    let per = samples_per_span.clamp(1, (max_samples / spans.len().max(1)).max(1));
    let mut out = Vec::with_capacity(spans.len() * per + 1);
    for (a, b) in &spans {
        for i in 0..per {
            let t = a + (b - a) * (i as f64) / (per as f64);
            if let Some(pt) = nurbs_eval_h(p, &knots, &pts, t) {
                out.push(pt);
            }
        }
    }
    if let Some(pt) = nurbs_eval_h(p, &knots, &pts, hi) {
        out.push(pt);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }
    fn pclose(a: Point3, b: Point3) -> bool {
        close(a.x, b.x) && close(a.y, b.y) && close(a.z, b.z)
    }

    #[test]
    fn vec_ops() {
        let a = Vec3::new(1.0, 0.0, 0.0);
        let b = Vec3::new(0.0, 1.0, 0.0);
        assert_eq!(a.cross(b), Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(a.dot(b), 0.0);
        assert!(Vec3::new(0.0, 0.0, 0.0).normalized().is_none());
        assert!(close(Vec3::new(3.0, 4.0, 0.0).length(), 5.0));
    }

    #[test]
    fn bbox_helpers() {
        let mut bb = None;
        extend_bbox(&mut bb, Point3::new(f64::NAN, 0.0, 0.0));
        assert!(bb.is_none());
        extend_bbox(&mut bb, Point3::xy(1.0, 2.0));
        extend_bbox(&mut bb, Point3::xy(-1.0, 5.0));
        let b = bb.unwrap();
        assert!(close(b.width(), 2.0) && close(b.height(), 3.0));
    }

    #[test]
    fn arbitrary_axis_default_normal() {
        let (ax, ay, az) = arbitrary_axes(Vec3::Z);
        assert_eq!(ax, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(ay, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(az, Vec3::Z);
    }

    #[test]
    fn arbitrary_axis_flipped_normal() {
        // N = -Z: Ax = Wy x N = (-1, 0, 0); Ay = N x Ax = (0, 1, 0).
        let (ax, ay, az) = arbitrary_axes(Vec3::new(0.0, 0.0, -1.0));
        assert_eq!(ax, Vec3::new(-1.0, 0.0, 0.0));
        assert_eq!(ay, Vec3::new(0.0, 1.0, 0.0));
        assert_eq!(az, Vec3::new(0.0, 0.0, -1.0));
    }

    #[test]
    fn arbitrary_axis_oblique_normal_is_orthonormal() {
        let n = Vec3::new(1.0, 2.0, 3.0).normalized().unwrap();
        let (ax, ay, az) = arbitrary_axes(n);
        assert!(close(ax.length(), 1.0) && close(ay.length(), 1.0));
        assert!(close(ax.dot(ay), 0.0) && close(ax.dot(az), 0.0) && close(ay.dot(az), 0.0));
        // Ax = Wz x N is horizontal.
        assert!(close(ax.z, 0.0));
        // Right-handed.
        assert!(close(ax.cross(ay).dot(az), 1.0));
    }

    #[test]
    fn arbitrary_axis_near_z_uses_wy() {
        let n = Vec3::new(0.001, 0.001, 1.0).normalized().unwrap();
        let (ax, _, _) = arbitrary_axes(n);
        let expect = Vec3::new(0.0, 1.0, 0.0).cross(n).normalized().unwrap();
        assert!(close(ax.x, expect.x) && close(ax.y, expect.y) && close(ax.z, expect.z));
    }

    #[test]
    fn arbitrary_axis_zero_normal_is_identity() {
        let (ax, ay, az) = arbitrary_axes(Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(
            (ax, ay, az),
            (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), Vec3::Z)
        );
    }

    #[test]
    fn transform_compose_order() {
        let t = Transform::translation(Vec3::new(10.0, 0.0, 0.0));
        let r = Transform::rotation_z(PI / 2.0);
        // rotate first, then translate
        let c = t.compose(&r);
        let p = c.apply_point(Point3::new(1.0, 0.0, 0.0));
        assert!(pclose(p, Point3::new(10.0, 1.0, 0.0)));
        // translate first, then rotate
        let c = r.compose(&t);
        let p = c.apply_point(Point3::new(1.0, 0.0, 0.0));
        assert!(pclose(p, Point3::new(0.0, 11.0, 0.0)));
    }

    #[test]
    fn transform_vector_ignores_translation() {
        let t = Transform::translation(Vec3::new(5.0, 5.0, 5.0));
        assert_eq!(
            t.apply_vector(Vec3::new(1.0, 2.0, 3.0)),
            Vec3::new(1.0, 2.0, 3.0)
        );
        assert_eq!(Transform::identity().determinant(), 1.0);
        assert!(Transform::scaling(-1.0, 1.0, 1.0).determinant() < 0.0);
    }

    #[test]
    fn insert_transform() {
        // base (1,1), scale 2, rotate 90, position (10,20).
        let tf = Transform::from_insert(
            Point3::new(10.0, 20.0, 0.0),
            Vec3::new(2.0, 2.0, 2.0),
            PI / 2.0,
            Vec3::Z,
            Point3::new(1.0, 1.0, 0.0),
        );
        // block point (2,1): minus base -> (1,0); scale -> (2,0); rotate -> (0,2); + pos.
        assert!(pclose(
            tf.apply_point(Point3::new(2.0, 1.0, 0.0)),
            Point3::new(10.0, 22.0, 0.0)
        ));
    }

    #[test]
    fn insert_transform_with_offset_and_flipped_normal() {
        let tf = Transform::from_insert_offset(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 1.0),
            0.0,
            Vec3::new(0.0, 0.0, -1.0),
            Point3::default(),
            Vec3::new(5.0, 0.0, 0.0),
        );
        // The plane x axis is -X in the world; the position is a plain world point.
        assert!(pclose(
            tf.apply_point(Point3::new(1.0, 2.0, 0.0)),
            Point3::new(-6.0, 2.0, 0.0)
        ));
    }

    #[test]
    fn bulge_semicircle() {
        let a = bulge_to_arc((0.0, 0.0), (2.0, 0.0), 1.0).unwrap();
        assert!(close(a.radius, 1.0));
        assert!(close(a.center.0, 1.0) && close(a.center.1, 0.0));
        assert!(close(a.sweep, PI));
        assert!(a.is_ccw());
        assert!(close(a.start_angle.abs(), PI));
        // CCW from (0,0) to (2,0) passes below the chord: point at start + sweep/2 is (1,-1).
        let mid = a.start_angle + a.sweep / 2.0;
        assert!(close(a.center.0 + a.radius * mid.cos(), 1.0));
        assert!(close(a.center.1 + a.radius * mid.sin(), -1.0));
    }

    #[test]
    fn bulge_negative_and_quarter() {
        let a = bulge_to_arc((0.0, 0.0), (2.0, 0.0), -1.0).unwrap();
        assert!(!a.is_ccw());
        assert!(close(a.sweep, -PI));
        // Quarter circle: bulge = tan(pi/8), chord from (1,0) to (0,1) about the origin, CCW.
        let b = (PI / 8.0).tan();
        let a = bulge_to_arc((1.0, 0.0), (0.0, 1.0), b).unwrap();
        assert!(close(a.radius, 1.0));
        assert!(close(a.center.0, 0.0) && close(a.center.1, 0.0));
        assert!(close(a.start_angle, 0.0));
        assert!(close(a.end_angle(), PI / 2.0));
    }

    #[test]
    fn bulge_large_arc_center_other_side() {
        // bulge 2 > 1: more than a semicircle; the center lies on the bulge side of the chord.
        let a = bulge_to_arc((0.0, 0.0), (2.0, 0.0), 2.0).unwrap();
        assert!(a.sweep > PI);
        assert!(a.center.1 < 0.0);
        let end = a.end_angle();
        assert!(close(a.center.0 + a.radius * end.cos(), 2.0));
        assert!(close(a.center.1 + a.radius * end.sin(), 0.0));
    }

    #[test]
    fn bulge_degenerate() {
        assert!(bulge_to_arc((0.0, 0.0), (1.0, 0.0), 0.0).is_none());
        assert!(bulge_to_arc((1.0, 1.0), (1.0, 1.0), 1.0).is_none());
        assert!(bulge_to_arc((0.0, 0.0), (1.0, 0.0), f64::NAN).is_none());
    }

    #[test]
    fn ellipse_points() {
        let c = Point3::new(1.0, 2.0, 0.0);
        let major = Vec3::new(4.0, 0.0, 0.0);
        let p0 = ellipse_point(c, major, 0.5, Vec3::Z, 0.0);
        assert!(pclose(p0, Point3::new(5.0, 2.0, 0.0)));
        let p1 = ellipse_point(c, major, 0.5, Vec3::Z, PI / 2.0);
        assert!(pclose(p1, Point3::new(1.0, 4.0, 0.0)));
        // Rotated major axis.
        let p = ellipse_point(
            Point3::default(),
            Vec3::new(0.0, 2.0, 0.0),
            0.5,
            Vec3::Z,
            PI / 2.0,
        );
        assert!(pclose(p, Point3::new(-1.0, 0.0, 0.0)));
    }

    #[test]
    fn conjugate_axes_recovers_ellipse() {
        let (a, b, rot) = conjugate_to_axes((4.0, 0.0), (0.0, 2.0));
        assert!(close(a, 4.0) && close(b, 2.0) && close(rot, 0.0));
        // Unit circle sheared by x' = x + y: semi-axes are phi and 1/phi.
        let (a, b, _) = conjugate_to_axes((1.0, 0.0), (1.0, 1.0));
        let phi = (1.0 + 5f64.sqrt()) / 2.0;
        assert!(close(a, phi) && close(b, 1.0 / phi));
    }

    #[test]
    fn ccw_sweep_wraps() {
        assert!(close(ccw_sweep(0.0, PI / 2.0), PI / 2.0));
        assert!(close(ccw_sweep(3.0 * PI / 2.0, PI / 2.0), PI));
        assert!(close(ccw_sweep(1.0, 1.0), TAU));
    }

    #[test]
    fn arc_bbox_includes_extrema() {
        let arc = EllipseArc::circle(Point3::default(), 1.0, -0.1, 0.1);
        let mut bb = None;
        arc.extend_bbox(&mut bb);
        let bb = bb.unwrap();
        assert!(close(bb.max.x, 1.0));
        assert!(bb.min.x > 0.99);
        let full = EllipseArc::circle(Point3::new(1.0, 1.0, 0.0), 2.0, 0.0, 0.0);
        let mut bb = None;
        full.extend_bbox(&mut bb);
        let bb = bb.unwrap();
        assert!(
            close(bb.min.x, -1.0)
                && close(bb.max.x, 3.0)
                && close(bb.min.y, -1.0)
                && close(bb.max.y, 3.0)
        );
    }

    #[test]
    fn nurbs_bezier() {
        let c = [
            Point3::xy(0.0, 0.0),
            Point3::xy(1.0, 2.0),
            Point3::xy(2.0, 0.0),
        ];
        let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let p = nurbs_eval(2, &knots, &c, &[], 0.5).unwrap();
        // Quadratic Bezier at 0.5: 0.25 P0 + 0.5 P1 + 0.25 P2.
        assert!(pclose(p, Point3::xy(1.0, 1.0)));
        assert!(pclose(nurbs_eval(2, &knots, &c, &[], 0.0).unwrap(), c[0]));
        assert!(pclose(nurbs_eval(2, &knots, &c, &[], 1.0).unwrap(), c[2]));
        // Out-of-domain parameter is clamped.
        assert!(pclose(nurbs_eval(2, &knots, &c, &[], 5.0).unwrap(), c[2]));
    }

    #[test]
    fn nurbs_rational_quarter_circle() {
        let w = 2f64.sqrt() / 2.0;
        let c = [
            Point3::xy(1.0, 0.0),
            Point3::xy(1.0, 1.0),
            Point3::xy(0.0, 1.0),
        ];
        let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        for i in 0..=10 {
            let t = f64::from(i) / 10.0;
            let p = nurbs_eval(2, &knots, &c, &[1.0, w, 1.0], t).unwrap();
            assert!(close(p.x.hypot(p.y), 1.0), "t={t} r={}", p.x.hypot(p.y));
        }
        // Midpoint is at 45 degrees.
        let p = nurbs_eval(2, &knots, &c, &[1.0, w, 1.0], 0.5).unwrap();
        assert!(close(p.x, w) && close(p.y, w));
    }

    #[test]
    fn nurbs_cubic_linear_precision() {
        // Control points at the Greville abscissae reproduce x(t) = t exactly.
        let knots = clamped_uniform_knots(5, 3);
        assert_eq!(knots, vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0, 2.0]);
        let g = [0.0, 1.0 / 3.0, 1.0, 5.0 / 3.0, 2.0];
        let c: Vec<Point3> = g.iter().map(|&x| Point3::xy(x, x * x)).collect();
        for t in [0.0, 0.25, 0.5, 1.0, 1.7, 2.0] {
            let p = nurbs_eval(3, &knots, &c, &[], t).unwrap();
            assert!(close(p.x, t), "t={t} x={}", p.x);
        }
    }

    #[test]
    fn nurbs_degree_and_work_are_capped() {
        let c: Vec<Point3> = (0..200).map(|i| Point3::xy(f64::from(i), 0.0)).collect();
        // Degree above the cap: control polygon.
        assert_eq!(nurbs_flatten(60, &[], &c, &[], 16).len(), 200);
        assert!(nurbs_eval(60, &clamped_uniform_knots(200, 60), &c, &[], 1.0).is_none());
        // Degree at the cap: bounded number of samples.
        let pts = nurbs_flatten(25, &[], &c, &[], 100_000);
        assert!(pts.len() <= 5_000_000 / (25 * 25) + 1, "{}", pts.len());
        assert!(pts.len() > 10);
    }

    #[test]
    fn nurbs_rejects_inconsistent_input() {
        let c = [Point3::xy(0.0, 0.0), Point3::xy(1.0, 1.0)];
        assert!(nurbs_eval(2, &[0.0, 1.0], &c, &[], 0.5).is_none());
        assert!(nurbs_eval(0, &[0.0, 1.0, 2.0], &c, &[], 0.5).is_none());
    }

    #[test]
    fn flatten_endpoints_and_fallbacks() {
        let c = [
            Point3::xy(0.0, 0.0),
            Point3::xy(1.0, 2.0),
            Point3::xy(2.0, 0.0),
        ];
        let pts = nurbs_flatten(2, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &c, &[], 8);
        assert_eq!(pts.len(), 9);
        assert!(pclose(*pts.first().unwrap(), c[0]));
        assert!(pclose(*pts.last().unwrap(), c[2]));
        // Missing knots and bogus degree still work.
        let pts = nurbs_flatten(9, &[], &c, &[], 4);
        assert!(pts.len() >= 3);
        assert!(pclose(*pts.last().unwrap(), c[2]));
        // Single point.
        assert_eq!(nurbs_flatten(3, &[], &c[..1], &[], 4).len(), 1);
        // Bad weights are ignored.
        let pts = nurbs_flatten(2, &[], &c, &[1.0, -1.0, 1.0], 4);
        assert!(pts.iter().all(|p| p.is_finite()));
    }
}
