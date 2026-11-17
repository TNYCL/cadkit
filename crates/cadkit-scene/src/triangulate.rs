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
}
