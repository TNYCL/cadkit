//! Triangulation of one planar polygon with holes by ear clipping.
//!
//! Holes are joined to the outline by bridges between mutually visible vertices, then ears
//! are cut from the single resulting loop. The input is never trusted: repeated points,
//! collinear runs, a repeated closing point, either winding, zero-area rings and
//! self-intersections are accepted. Degenerate parts give fewer triangles (or none), every
//! step is charged to a caller work budget, and the function always terminates.

/// Triangles of the polygon whose outline is `rings[0]` and whose holes are the other rings.
///
/// Points are 2D coordinates in any unit; precision is best when they are small, so callers
/// should subtract a nearby origin first. Each ring may wind either way and may repeat its
/// first point at the end. Returned triangles are indices into the concatenation of `rings`
/// as given and wind counter-clockwise (Y up). A hole that cannot be joined to the outline
/// (it lies outside, or crosses it) is left out.
///
/// `work` is decremented by the work done (about one unit per point visited by a test). When
/// it runs out the triangulation is abandoned and `None` is returned; `Some(vec![])` means the
/// polygon has no area (too few points, collinear, non-finite coordinates) or more than
/// `u32::MAX` points.
pub fn triangulate(rings: &[&[[f64; 2]]], work: &mut u64) -> Option<Vec<[u32; 3]>> {
    let total = rings.iter().fold(0usize, |n, r| n.saturating_add(r.len()));
    if u32::try_from(total).is_err() {
        return Some(Vec::new());
    }
    if !spend(work, total) {
        return None;
    }
    let points: Vec<[f64; 2]> = rings.iter().flat_map(|r| r.iter().copied()).collect();
    let at = |i: u32| points.get(i as usize).copied().unwrap_or([0.; 2]);
    let mut loops: Vec<Vec<u32>> = Vec::new();
    let mut base = 0u32;
    for (k, ring) in rings.iter().enumerate() {
        let start = base;
        // Fits: the total length was checked against u32 above.
        base = base.saturating_add(u32::try_from(ring.len()).unwrap_or(u32::MAX));
        let indices = clean_loop(ring, start);
        let area = indices.as_deref().map_or(0., |l| area2(l, &at));
        match indices {
            Some(mut l) if l.len() >= 3 && area != 0. && area.is_finite() => {
                // Outline counter-clockwise, holes clockwise.
                if (k == 0) == (area < 0.) {
                    l.reverse();
                }
                loops.push(l);
            }
            _ if k == 0 => return Some(Vec::new()),
            _ => {}
        }
    }
    let mut parts = loops.into_iter();
    let Some(mut outline) = parts.next() else {
        return Some(Vec::new());
    };
    let mut holes: Vec<(f64, Vec<u32>)> = parts
        .map(|h| {
            let right = h
                .iter()
                .map(|&i| at(i)[0])
                .fold(f64::NEG_INFINITY, f64::max);
            (right, h)
        })
        .collect();
    // Rightmost holes first, so each bridge only has to avoid holes not yet joined.
    holes.sort_by(|a, b| b.0.total_cmp(&a.0));
    let holes: Vec<Vec<u32>> = holes.into_iter().map(|(_, h)| h).collect();
    for (k, hole) in holes.iter().enumerate() {
        let pending = holes.get(k..).unwrap_or_default();
        outline = bridge(outline, hole, pending, &holes, &at, work)?;
    }
    clip_ears(&outline, &at, work)
}

/// Spends `cost` units of `work`; false when the budget cannot cover it.
fn spend(work: &mut u64, cost: usize) -> bool {
    let cost = u64::try_from(cost).unwrap_or(u64::MAX);
    match work.checked_sub(cost) {
        Some(left) => {
            *work = left;
            true
        }
        None => {
            *work = 0;
            false
        }
    }
}

/// Indices of `ring` (offset by `base`) without consecutive repeats or a closing repeat;
/// `None` when a point is not finite.
fn clean_loop(ring: &[[f64; 2]], base: u32) -> Option<Vec<u32>> {
    let mut out: Vec<u32> = Vec::new();
    let mut last: Option<[f64; 2]> = None;
    for (i, p) in ring.iter().enumerate() {
        if !p[0].is_finite() || !p[1].is_finite() {
            return None;
        }
        if last != Some(*p) {
            out.push(base.saturating_add(u32::try_from(i).ok()?));
            last = Some(*p);
        }
    }
    if out.len() > 1 && ring.first() == last.as_ref() {
        out.pop();
    }
    Some(out)
}

/// Twice the signed area of a loop: positive for counter-clockwise (Y up).
fn area2(lp: &[u32], at: &impl Fn(u32) -> [f64; 2]) -> f64 {
    let Some(&first) = lp.first() else {
        return 0.;
    };
    let o = at(first);
    let mut sum = 0.;
    for pair in lp.windows(2) {
        if let [a, b] = pair {
            let (a, b) = (at(*a), at(*b));
            sum += (a[0] - o[0]) * (b[1] - o[1]) - (b[0] - o[0]) * (a[1] - o[1]);
        }
    }
    sum
}

fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Proper crossing of segments ab and cd; touching end points do not count.
fn segments_cross(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let (d1, d2) = (cross(c, d, a), cross(c, d, b));
    let (d3, d4) = (cross(a, b, c), cross(a, b, d));
    ((d1 > 0. && d2 < 0.) || (d1 < 0. && d2 > 0.)) && ((d3 > 0. && d4 < 0.) || (d3 < 0. && d4 > 0.))
}

/// `q` lies on the open segment ab (collinear, strictly between the end points).
fn on_segment(a: [f64; 2], b: [f64; 2], q: [f64; 2]) -> bool {
    cross(a, b, q) == 0.
        && q != a
        && q != b
        && q[0] >= a[0].min(b[0])
        && q[0] <= a[0].max(b[0])
        && q[1] >= a[1].min(b[1])
        && q[1] <= a[1].max(b[1])
}

/// Even-odd point in loop test; points on the boundary may fall either way.
fn inside(lp: &[u32], q: [f64; 2], at: &impl Fn(u32) -> [f64; 2]) -> bool {
    let mut result = false;
    let mut j = lp.len().wrapping_sub(1);
    for (i, &index) in lp.iter().enumerate() {
        let (a, b) = (at(index), lp.get(j).map_or(at(index), |&k| at(k)));
        if (a[1] > q[1]) != (b[1] > q[1])
            && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            result = !result;
        }
        j = i;
    }
    result
}

/// Edges of a closed loop as point pairs.
fn edges<'a>(
    lp: &'a [u32],
    at: &'a impl Fn(u32) -> [f64; 2],
) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + 'a {
    lp.iter()
        .zip(lp.iter().cycle().skip(1))
        .map(|(&a, &b)| (at(a), at(b)))
}

/// Joins `hole` to `outline` through a bridge from the hole's rightmost vertex to the nearest
/// outline vertex it can see. `pending` are the holes not yet joined (including `hole`),
/// `all` every hole. Returns the outline unchanged when no vertex is visible, `None` when the
/// work budget runs out.
fn bridge(
    outline: Vec<u32>,
    hole: &[u32],
    pending: &[Vec<u32>],
    all: &[Vec<u32>],
    at: &impl Fn(u32) -> [f64; 2],
    work: &mut u64,
) -> Option<Vec<u32>> {
    let mut right = 0usize;
    for (k, &i) in hole.iter().enumerate() {
        if hole.get(right).is_some_and(|&r| at(i)[0] > at(r)[0]) {
            right = k;
        }
    }
    let m = at(*hole.get(right)?);
    let mut candidates: Vec<(f64, usize)> = outline
        .iter()
        .enumerate()
        .map(|(k, &i)| {
            let p = at(i);
            ((p[0] - m[0]).powi(2) + (p[1] - m[1]).powi(2), k)
        })
        .collect();
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    let checks = pending
        .iter()
        .fold(outline.len(), |n, h| n.saturating_add(h.len()))
        .saturating_mul(2)
        .saturating_add(all.iter().fold(0, |n, h| n.saturating_add(h.len())));
    let mut best = None;
    for (_, k) in candidates {
        if !spend(work, checks) {
            return None;
        }
        let Some(&index) = outline.get(k) else {
            continue;
        };
        let p = at(index);
        if !locally_inside(&outline, k, m, at) {
            continue;
        }
        let loops = || std::iter::once(outline.as_slice()).chain(pending.iter().map(Vec::as_slice));
        // The bridge must not cross an edge, run through a corner or cut through a hole.
        if loops().any(|l| edges(l, at).any(|(a, b)| segments_cross(m, p, a, b))) {
            continue;
        }
        if loops().any(|l| l.iter().any(|&c| on_segment(m, p, at(c)))) {
            continue;
        }
        let mid = [(m[0] + p[0]) / 2., (m[1] + p[1]) / 2.];
        if all.iter().any(|h| inside(h, mid, at)) {
            continue;
        }
        best = Some(k);
        break;
    }
    let Some(best) = best else {
        return Some(outline);
    };
    let (head, tail) = outline.split_at(best.saturating_add(1).min(outline.len()));
    let mut joined = Vec::with_capacity(outline.len().saturating_add(hole.len()).saturating_add(2));
    joined.extend_from_slice(head);
    let (before, after) = hole.split_at(right.min(hole.len()));
    joined.extend_from_slice(after);
    joined.extend_from_slice(before);
    joined.extend(hole.get(right).copied());
    joined.extend(head.last().copied());
    joined.extend_from_slice(tail);
    Some(joined)
}

/// The direction from outline vertex `k` towards `m` lies within the polygon's angle at that
/// vertex. A vertex that already carries a bridge occurs once per angle between its bridges;
/// joining a hole to the wrong occurrence makes two bridges cross there, and ear clipping of
/// such a loop cuts triangles outside the polygon.
fn locally_inside(outline: &[u32], k: usize, m: [f64; 2], at: &impl Fn(u32) -> [f64; 2]) -> bool {
    let n = outline.len();
    if n == 0 {
        return false;
    }
    let (Some(&u), Some(&p), Some(&w)) = (
        outline.get((k + n - 1) % n),
        outline.get(k),
        outline.get((k + 1) % n),
    ) else {
        return false;
    };
    let (u, p, w) = (at(u), at(p), at(w));
    if cross(u, p, w) > 0. {
        cross(p, w, m) >= 0. && cross(u, p, m) >= 0.
    } else {
        cross(p, w, m) >= 0. || cross(u, p, m) >= 0.
    }
}

/// Ear clipping of one counter-clockwise loop of point indices (bridges included).
fn clip_ears(lp: &[u32], at: &impl Fn(u32) -> [f64; 2], work: &mut u64) -> Option<Vec<[u32; 3]>> {
    let n = lp.len();
    if n < 3 {
        return Some(Vec::new());
    }
    let mut prev: Vec<usize> = (0..n).map(|i| (i + n - 1) % n).collect();
    let mut next: Vec<usize> = (0..n).map(|i| (i + 1) % n).collect();
    let point = |node: usize| lp.get(node).map_or([0.; 2], |&i| at(i));
    let index = |node: usize| lp.get(node).copied().unwrap_or(0);
    let mut out: Vec<[u32; 3]> = Vec::with_capacity(n - 2);
    let mut remaining = n;
    let mut ear = 0usize;
    let mut stall = 0usize;
    while remaining > 3 {
        if !spend(work, 1) {
            return None;
        }
        let a = prev.get(ear).copied()?;
        let c = next.get(ear).copied()?;
        let (pa, pb, pc) = (point(a), point(ear), point(c));
        let turn = cross(pa, pb, pc);
        let mut cut = false;
        if turn == 0. || !turn.is_finite() {
            // A repeated or collinear point (also the spike of a bridge): drop it without a
            // triangle and look at the previous corner again.
            unlink(&mut prev, &mut next, ear);
            remaining -= 1;
            ear = a;
            stall = 0;
            continue;
        }
        if turn > 0. {
            if !spend(work, remaining) {
                return None;
            }
            cut = empty(&next, c, a, [pa, pb, pc], &point);
        }
        if !cut {
            stall += 1;
            if stall < remaining {
                ear = c;
                continue;
            }
            // No ear in a whole turn: the loop intersects itself. Cut this corner anyway so
            // the loop gets shorter and the clipping ends.
        }
        if turn > 0. {
            out.push([index(a), index(ear), index(c)]);
        }
        unlink(&mut prev, &mut next, ear);
        remaining -= 1;
        ear = c;
        stall = 0;
    }
    let a = prev.get(ear).copied()?;
    let c = next.get(ear).copied()?;
    if cross(point(a), point(ear), point(c)) > 0. {
        out.push([index(a), index(ear), index(c)]);
    }
    Some(out)
}

fn unlink(prev: &mut [usize], next: &mut [usize], node: usize) {
    let (Some(&p), Some(&n)) = (prev.get(node), next.get(node)) else {
        return;
    };
    if let Some(slot) = next.get_mut(p) {
        *slot = n;
    }
    if let Some(slot) = prev.get_mut(n) {
        *slot = p;
    }
}

/// No remaining point other than the corners lies inside or on the triangle; points equal to
/// a corner (bridge copies) are ignored.
fn empty(
    next: &[usize],
    from: usize,
    to: usize,
    [a, b, c]: [[f64; 2]; 3],
    point: &impl Fn(usize) -> [f64; 2],
) -> bool {
    let mut node = next.get(from).copied().unwrap_or(to);
    // Bounded by the list length even if the links were inconsistent.
    for _ in 0..next.len() {
        if node == to {
            break;
        }
        let p = point(node);
        if p != a
            && p != b
            && p != c
            && cross(a, b, p) >= 0.
            && cross(b, c, p) >= 0.
            && cross(c, a, p) >= 0.
        {
            return false;
        }
        node = next.get(node).copied().unwrap_or(to);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: u64 = 10_000_000;

    fn area(rings: &[&[[f64; 2]]], triangles: &[[u32; 3]]) -> f64 {
        let points: Vec<[f64; 2]> = rings.iter().flat_map(|r| r.iter().copied()).collect();
        triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| points[i as usize]);
                let twice = cross(a, b, c);
                assert!(twice > 0., "triangle {t:?} is not counter-clockwise");
                twice / 2.
            })
            .sum()
    }

    fn run(rings: &[&[[f64; 2]]]) -> Vec<[u32; 3]> {
        triangulate(rings, &mut WORK.clone()).unwrap()
    }

    #[test]
    fn convex_concave_either_winding_and_closing_point() {
        let square = [[0., 0.], [2., 0.], [2., 2.], [0., 2.]];
        let mut reversed = square;
        reversed.reverse();
        let closed = [[0., 0.], [2., 0.], [2., 2.], [0., 2.], [0., 0.]];
        for ring in [&square[..], &reversed[..], &closed[..]] {
            let t = run(&[ring]);
            assert_eq!(t.len(), 2);
            assert!((area(&[ring], &t) - 4.).abs() < 1e-12);
            // The closing repeat (index 4) is never referenced.
            assert!(t.iter().flatten().all(|&i| i < 4));
        }
        let ell = [[0., 0.], [3., 0.], [3., 1.], [1., 1.], [1., 3.], [0., 3.]];
        let t = run(&[&ell]);
        assert_eq!(t.len(), 4);
        assert!((area(&[&ell], &t) - 5.).abs() < 1e-12);
        // A gable wall: a pentagon with the ridge on top, as CityGML walls are.
        let gable = [[0., 0.], [8., 0.], [8., 6.], [4., 9.], [0., 6.]];
        let t = run(&[&gable]);
        assert_eq!(t.len(), 3);
        assert!((area(&[&gable], &t) - 60.).abs() < 1e-12);
    }

    #[test]
    fn holes_are_cut_out() {
        let outer = [[0., 0.], [10., 0.], [10., 10.], [0., 10.]];
        let h1 = [[2., 2.], [4., 2.], [4., 4.], [2., 4.]];
        let h2 = [[6., 6.], [8., 6.], [8., 8.], [6., 8.]];
        let mut h1r = h1;
        h1r.reverse();
        let cases: [&[&[[f64; 2]]]; 3] =
            [&[&outer, &h1], &[&outer, &h1, &h2], &[&outer, &h1r, &h2]];
        for rings in cases {
            let t = run(rings);
            let holes = (rings.len() - 1) as f64;
            assert!(
                (area(rings, &t) - (100. - 4. * holes)).abs() < 1e-9,
                "{rings:?}"
            );
            assert_eq!(
                t.len(),
                4 + 4 * (rings.len() - 1) + 2 * (rings.len() - 1) - 2
            );
        }
        // A hole whose nearest outline corner lies behind the hole itself.
        let hole = [[2., 4.], [4., 4.], [4., 2.], [2., 2.]];
        let t = run(&[&outer, &hole]);
        assert_eq!(t.len(), 8);
        assert!((area(&[&outer, &hole], &t) - 96.).abs() < 1e-9);
        // A hole outside the outline cannot be bridged without crossing and is left out.
        let outside = [[20., 20.], [21., 20.], [21., 21.]];
        let t = run(&[&outer, &outside]);
        assert!(area(&[&outer, &outside], &t) > 0.);
    }

    #[test]
    fn holes_bridged_to_the_same_corner() {
        // Both holes see the top right corner first. The one further right is joined first;
        // the other one must be joined to the copy of the corner whose angle faces it, or the
        // two bridges cross and triangles come out overlapping the holes.
        let outer = [[0., 0.], [20., 0.], [20., 20.], [0., 20.]];
        let square = |x: f64, y: f64| {
            [
                [x - 0.5, y - 0.5],
                [x + 0.5, y - 0.5],
                [x + 0.5, y + 0.5],
                [x - 0.5, y + 0.5],
            ]
        };
        let (left, low) = (square(14.1, 18.96), square(18.96, 14.1));
        let rings: [&[[f64; 2]]; 3] = [&outer, &left, &low];
        let t = run(&rings);
        assert!(
            (area(&rings, &t) - 398.).abs() < 1e-9,
            "{}",
            area(&rings, &t)
        );
        assert_eq!(t.len(), 4 + 4 + 4 + 2);
    }

    #[test]
    fn degenerate_input_terminates() {
        assert!(run(&[]).is_empty());
        assert!(run(&[&[[0., 0.], [1., 1.]]]).is_empty());
        assert!(run(&[&[[0., 0.], [1., 1.], [2., 2.]]]).is_empty());
        assert!(run(&[&[[0., 0.], [f64::NAN, 1.], [2., 0.]]]).is_empty());
        // Repeated points and a collinear point.
        let ring = [[0., 0.], [1., 0.], [2., 0.], [2., 2.], [2., 2.], [0., 2.]];
        let t = run(&[&ring]);
        assert!((area(&[&ring], &t) - 4.).abs() < 1e-12);
        // Self-intersecting outlines must not hang: a bow-tie and a five-pointed star drawn
        // in one stroke.
        let bow = [[0., 0.], [2., 2.], [2., 0.], [0., 2.]];
        run(&[&bow]);
        let star: Vec<[f64; 2]> = (0..5)
            .map(|i| {
                let a = f64::from(i) * 4. * std::f64::consts::PI / 5.;
                [a.cos(), a.sin()]
            })
            .collect();
        assert!(run(&[&star]).len() <= 3);
        // A degenerate hole is ignored.
        let outer = [[0., 0.], [10., 0.], [10., 10.], [0., 10.]];
        let flat = [[2., 2.], [3., 3.], [4., 4.]];
        assert_eq!(run(&[&outer, &flat]).len(), 2);
    }

    #[test]
    fn many_points_and_budgets() {
        let circle: Vec<[f64; 2]> = (0..500)
            .map(|i| {
                let a = f64::from(i) / 500. * std::f64::consts::TAU;
                [a.cos(), a.sin()]
            })
            .collect();
        let t = run(&[&circle]);
        assert_eq!(t.len(), 498);
        assert!((area(&[&circle], &t) - std::f64::consts::PI).abs() < 1e-3);
        // A comb: many reflex corners.
        let mut comb = vec![[0., 0.]];
        for i in 0..100 {
            let x = f64::from(i);
            comb.extend([[x + 0.25, 1.], [x + 0.5, 0.1], [x + 0.75, 1.]]);
        }
        comb.extend([[100., 0.], [100., -1.], [0., -1.]]);
        let t = run(&[&comb]);
        assert_eq!(t.len(), comb.len() - 2);
        let mut work = 50;
        assert_eq!(triangulate(&[&circle], &mut work), None);
        assert_eq!(work, 0);
        let mut work = 10_000;
        let square = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
        assert!(triangulate(&[&square], &mut work).is_some());
        assert!(work < 10_000);
    }

    /// Deterministic xorshift64* generator, so failures can be replayed.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
        }

        /// Uniform in [0, 1).
        fn unit(&mut self) -> f64 {
            (self.next() >> 11) as f64 / (1u64 << 53) as f64
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    fn signed_area(ring: &[[f64; 2]]) -> f64 {
        let n = ring.len();
        (0..n)
            .map(|i| {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum::<f64>()
            / 2.
    }

    fn contains(ring: &[[f64; 2]], q: [f64; 2]) -> bool {
        let lp: Vec<u32> = (0..ring.len() as u32).collect();
        inside(&lp, q, &|i| ring[i as usize])
    }

    /// A ring is simple: no two edges cross or overlap, no point repeats.
    fn simple(ring: &[[f64; 2]]) -> bool {
        let n = ring.len();
        for i in 0..n {
            for j in i + 1..n {
                if ring[i] == ring[j] {
                    return false;
                }
            }
        }
        for i in 0..n {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            for j in i + 1..n {
                let (c, d) = (ring[j], ring[(j + 1) % n]);
                if segments_cross(a, b, c, d)
                    || on_segment(a, b, c)
                    || on_segment(a, b, d)
                    || on_segment(c, d, a)
                    || on_segment(c, d, b)
                {
                    return false;
                }
            }
        }
        true
    }

    /// Star-shaped ring around (cx, cy): sorted angles, random radii in [r/4, r].
    fn star(rng: &mut Rng, n: usize, [cx, cy]: [f64; 2], r: f64) -> Vec<[f64; 2]> {
        let mut angles: Vec<f64> = (0..n).map(|_| rng.unit() * std::f64::consts::TAU).collect();
        angles.sort_by(f64::total_cmp);
        angles
            .into_iter()
            .map(|a| {
                let radius = r * (0.25 + 0.75 * rng.unit());
                [cx + radius * a.cos(), cy + radius * a.sin()]
            })
            .collect()
    }

    /// A random simple ring through grid points, untangled by 2-opt moves; None when the
    /// moves do not converge or the result is not simple (collinear overlaps).
    fn untangled(rng: &mut Rng, n: usize) -> Option<Vec<[f64; 2]>> {
        let mut ring: Vec<[f64; 2]> = (0..n)
            .map(|_| [rng.below(24) as f64, rng.below(24) as f64])
            .collect();
        for _ in 0..10_000 {
            let mut changed = false;
            'scan: for i in 0..n {
                for j in i + 2..n {
                    if i == 0 && j == n - 1 {
                        continue;
                    }
                    let (a, b) = (ring[i], ring[i + 1]);
                    let (c, d) = (ring[j], ring[(j + 1) % n]);
                    if segments_cross(a, b, c, d) {
                        ring[i + 1..=j].reverse();
                        changed = true;
                        break 'scan;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        simple(&ring).then_some(ring)
    }

    /// Whether the interiors of two counter-clockwise triangles overlap (separating axes,
    /// with a tolerance so triangles sharing an edge or a corner do not count).
    fn overlap(t: [[f64; 2]; 3], u: [[f64; 2]; 3], eps: f64) -> bool {
        for (tri, other) in [(t, u), (u, t)] {
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                // Every point of 'other' on the outer side of edge ab separates them.
                let len = (b[0] - a[0]).hypot(b[1] - a[1]);
                if other.iter().all(|&p| cross(a, b, p) <= eps * len) {
                    return false;
                }
            }
        }
        true
    }

    /// Triangulates and checks: every triangle counter-clockwise, inside the outline and
    /// outside every hole, no two overlapping, and the area equal to the polygon's.
    fn check(rings: &[&[[f64; 2]]], label: &str) {
        check_result(rings, &run(rings), label);
    }

    fn check_result(rings: &[&[[f64; 2]]], triangles: &[[u32; 3]], label: &str) {
        let points: Vec<[f64; 2]> = rings.iter().flat_map(|r| r.iter().copied()).collect();
        let size = points
            .iter()
            .flat_map(|p| p.iter())
            .fold(0f64, |m, v| m.max(v.abs()))
            .max(1.);
        let eps = 1e-9 * size;
        let expected = signed_area(rings[0]).abs()
            - rings[1..].iter().map(|h| signed_area(h).abs()).sum::<f64>();
        let got = area(rings, triangles);
        assert!(
            (got - expected).abs() <= 1e-9 * size * size,
            "{label}: area {got} instead of {expected}, rings {rings:?}"
        );
        let corners: Vec<[[f64; 2]; 3]> = triangles
            .iter()
            .map(|t| t.map(|i| points[i as usize]))
            .collect();
        for c in &corners {
            // A sliver along a collinear run (its middle point rounded off the line) has its
            // centroid on the boundary, where the inside test may fall either way.
            if cross(c[0], c[1], c[2]) <= eps * size {
                continue;
            }
            let centroid = [
                (c[0][0] + c[1][0] + c[2][0]) / 3.,
                (c[0][1] + c[1][1] + c[2][1]) / 3.,
            ];
            assert!(
                contains(rings[0], centroid) && !rings[1..].iter().any(|h| contains(h, centroid)),
                "{label}: triangle {c:?} lies outside, rings {rings:?}"
            );
        }
        for (i, t) in corners.iter().enumerate() {
            for u in &corners[i + 1..] {
                assert!(
                    !overlap(*t, *u, eps),
                    "{label}: triangles {t:?} and {u:?} overlap, rings {rings:?}"
                );
            }
        }
    }

    /// Distance from q to segment ab.
    fn distance(a: [f64; 2], b: [f64; 2], q: [f64; 2]) -> f64 {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let t = (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / (dx * dx + dy * dy)).clamp(0., 1.);
        (a[0] + t * dx - q[0]).hypot(a[1] + t * dy - q[1])
    }

    /// Adds collinear midpoints and repeated points to a ring, and reverses it at random.
    fn roughen(rng: &mut Rng, ring: &[[f64; 2]]) -> Vec<[f64; 2]> {
        let mut out = Vec::new();
        for (i, &p) in ring.iter().enumerate() {
            out.push(p);
            if rng.below(4) == 0 {
                out.push(p);
            }
            if rng.below(4) == 0 {
                let q = ring[(i + 1) % ring.len()];
                out.push([(p[0] + q[0]) / 2., (p[1] + q[1]) / 2.]);
            }
        }
        if rng.below(2) == 0 {
            out.reverse();
        }
        if rng.below(3) == 0 {
            out.push(out[0]);
        }
        out
    }

    #[test]
    fn random_simple_polygons_are_covered_exactly() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut checked = 0;
        for case in 0..3000 {
            let outline = if case % 2 == 0 {
                let n = 3 + rng.below(40);
                let mut ring = star(&mut rng, n, [0., 0.], 10.);
                if case % 4 == 0 {
                    // On a grid: collinear runs and exact degeneracies.
                    for p in &mut ring {
                        *p = [p[0].round(), p[1].round()];
                    }
                }
                ring
            } else {
                let n = 4 + rng.below(16);
                match untangled(&mut rng, n) {
                    Some(ring) => ring,
                    None => continue,
                }
            };
            if !simple(&outline) || signed_area(&outline) == 0. {
                continue;
            }
            // Up to three holes, each a small star well inside the outline and clear of
            // the others.
            let mut holes: Vec<Vec<[f64; 2]>> = Vec::new();
            let mut placed: Vec<([f64; 2], f64)> = Vec::new();
            let (lo, hi) = outline
                .iter()
                .fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| {
                    (
                        [lo[0].min(p[0]), lo[1].min(p[1])],
                        [hi[0].max(p[0]), hi[1].max(p[1])],
                    )
                });
            for _ in 0..rng.below(4) {
                let c = [
                    lo[0] + rng.unit() * (hi[0] - lo[0]),
                    lo[1] + rng.unit() * (hi[1] - lo[1]),
                ];
                if !contains(&outline, c) {
                    continue;
                }
                let clear = (0..outline.len())
                    .map(|i| distance(outline[i], outline[(i + 1) % outline.len()], c))
                    .fold(f64::MAX, f64::min);
                let r = 0.8
                    * placed.iter().fold(clear, |m, (o, r)| {
                        m.min((o[0] - c[0]).hypot(o[1] - c[1]) - r)
                    });
                if r < 0.05 {
                    continue;
                }
                let n = 3 + rng.below(8);
                // A gap of more than half a turn between two angles can make a star cross
                // itself.
                let hole = star(&mut rng, n, c, r);
                if simple(&hole) {
                    placed.push((c, r));
                    holes.push(hole);
                }
            }
            let roughened: Vec<Vec<[f64; 2]>> = std::iter::once(&outline)
                .chain(&holes)
                .map(|r| {
                    if case % 3 == 0 {
                        roughen(&mut rng, r)
                    } else {
                        r.clone()
                    }
                })
                .collect();
            let rings: Vec<&[[f64; 2]]> = roughened.iter().map(Vec::as_slice).collect();
            check(&rings, &format!("case {case}"));
            checked += 1;
        }
        // About a third of the cases are discarded (untangling that did not give a simple ring).
        assert!(checked > 2000, "{checked}");
    }

    #[test]
    fn self_intersecting_rings_terminate_with_valid_indices() {
        let mut rng = Rng(0x0123_4567_89ab_cdef);
        for _ in 0..2000 {
            let n = 3 + rng.below(30);
            let rings: Vec<Vec<[f64; 2]>> = (0..1 + rng.below(3))
                .map(|_| {
                    (0..n)
                        .map(|_| [rng.below(10) as f64, rng.below(10) as f64])
                        .collect()
                })
                .collect();
            let slices: Vec<&[[f64; 2]]> = rings.iter().map(Vec::as_slice).collect();
            let total: usize = rings.iter().map(Vec::len).sum();
            let mut work = WORK;
            let t = triangulate(&slices, &mut work).unwrap();
            // Each bridge adds two corners to the loop that is clipped.
            assert!(t.len() <= total + 2 * rings.len());
            assert!(t.iter().flatten().all(|&i| (i as usize) < total));
            // Every triangle still winds counter-clockwise.
            area(&slices, &t);
        }
    }

    /// Work used by a 4,000-vertex star with random radii (many reflex corners) and a
    /// 4,000-vertex comb, against the default budget of the surface builder (2e8).
    #[test]
    fn large_polygons_fit_the_default_budget() {
        let mut rng = Rng(42);
        let star = star(&mut rng, 4000, [0., 0.], 100.);
        let mut comb = vec![[0., 0.]];
        for i in 0..1000 {
            let x = f64::from(i);
            comb.extend([[x + 0.25, 1.], [x + 0.5, 0.1], [x + 0.75, 1.]]);
        }
        comb.extend([[1000., 0.], [1000., -1.], [0., -1.]]);
        // A plate with 200 holes in a row: one bridge per hole.
        let plate = [[-1., -2.], [1001., -2.], [1001., 2.], [-1., 2.]];
        let holes: Vec<Vec<[f64; 2]>> = (0..200)
            .map(|i| {
                let x = f64::from(i) * 5.;
                vec![[x, -1.5], [x + 1., -1.5], [x + 1., -1.], [x, -1.]]
            })
            .collect();
        let mut with_holes: Vec<&[[f64; 2]]> = vec![&plate];
        with_holes.extend(holes.iter().map(Vec::as_slice));
        for (name, rings) in [
            ("star", vec![star.as_slice()]),
            ("comb", vec![comb.as_slice()]),
            ("plate with 200 holes", with_holes),
        ] {
            let mut work = 200_000_000;
            let t = triangulate(&rings, &mut work).expect(name);
            let used = 200_000_000 - work;
            println!("{name}: {} triangles, {used} work units", t.len());
            assert!(used < 50_000_000, "{name}: {used}");
            check_result(&rings, &t, name);
        }
    }
}
