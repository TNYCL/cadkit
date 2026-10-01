//! HATCH decoding. Boundary data reuses group codes (10/20, 40, 50, ...) so it is parsed
//! with a cursor instead of lookups.

use cadkit_core::{
    Error, HatchEdge, HatchLoop, Point3, Props, Result, Value as MValue, Vec3, Vertex,
};

use super::common::point_value;
use super::records::Group;
use crate::native::Pair;
use crate::ocs::Ocs;

struct Cursor<'a> {
    pairs: &'a [Pair],
    i: usize,
}

impl<'a> Cursor<'a> {
    fn peek_code(&self) -> Option<i32> {
        self.pairs.get(self.i).map(|p| p.code)
    }

    fn next(&mut self) -> Option<&'a Pair> {
        let p = self.pairs.get(self.i)?;
        self.i += 1;
        Some(p)
    }

    /// Consumes the next pair when it has `code` and returns its number.
    fn take_f64(&mut self, code: i32) -> Option<f64> {
        if self.peek_code() == Some(code) {
            self.next().and_then(|p| p.value.as_f64())
        } else {
            None
        }
    }

    fn take_i64(&mut self, code: i32) -> Option<i64> {
        if self.peek_code() == Some(code) {
            self.next().and_then(|p| p.value.as_i64())
        } else {
            None
        }
    }

    /// Reads an `x` / `y` pair stored as `code` and `code + 10`.
    fn take_xy(&mut self, code: i32) -> Option<(f64, f64)> {
        let x = self.take_f64(code)?;
        let y = self.take_f64(code + 10).unwrap_or(0.0);
        Some((x, y))
    }
}

pub(crate) struct HatchData {
    pub loops: Vec<HatchLoop>,
    pub solid: bool,
    pub pattern: Option<String>,
    pub pattern_scale: f64,
    pub pattern_angle: f64,
}

fn limit(count: i64, max: u32, what: &str) -> Result<usize> {
    let n = usize::try_from(count.max(0)).unwrap_or(usize::MAX);
    if n > max as usize {
        return Err(Error::LimitExceeded(format!(
            "hatch {what} count {n} exceeds {max}"
        )));
    }
    Ok(n)
}

pub(crate) fn parse(
    g: Group,
    ocs: &Ocs,
    max_vertices: u32,
    props: &mut Props,
) -> Result<HatchData> {
    let elevation = g.f64_or(30, 0.0);
    let to_world = |x: f64, y: f64| ocs.to_world(Point3::new(x, y, elevation));
    let dir_world = |x: f64, y: f64| {
        let o = ocs.to_world(Point3::new(x, y, 0.0));
        Vec3::new(o.x, o.y, o.z)
    };
    let solid = g.i64_or(70, 0) == 1;
    let pattern = g.string(2).filter(|s| !s.is_empty());
    let mut loops = Vec::new();

    let start = g.0.iter().position(|p| p.code == 91);
    let mut cur = Cursor {
        pairs: g.0,
        i: start.map_or(g.0.len(), |s| s + 1),
    };
    let npaths = match start
        .and_then(|s| g.0.get(s))
        .and_then(|p| p.value.as_i64())
    {
        Some(n) => limit(n, max_vertices, "path")?,
        None => 0,
    };

    let mut budget = max_vertices as usize;
    for _ in 0..npaths {
        let Some(flags) = cur.take_i64(92) else { break };
        let mut edges = Vec::new();
        if flags & 2 != 0 {
            let has_bulge = cur.take_i64(72).unwrap_or(0) != 0;
            let closed = cur.take_i64(73).unwrap_or(1) != 0;
            let n = limit(cur.take_i64(93).unwrap_or(0), max_vertices, "vertex")?;
            if n > budget {
                return Err(Error::LimitExceeded("hatch vertex budget exceeded".into()));
            }
            budget -= n;
            let mut vertices = Vec::new();
            for _ in 0..n {
                let Some((x, y)) = cur.take_xy(10) else { break };
                let bulge = if has_bulge {
                    cur.take_f64(42).unwrap_or(0.0)
                } else {
                    0.0
                };
                vertices.push(Vertex {
                    position: to_world(x, y),
                    bulge,
                    ..Vertex::default()
                });
            }
            edges.push(HatchEdge::Polyline { vertices, closed });
        } else {
            let n = limit(cur.take_i64(93).unwrap_or(0), max_vertices, "edge")?;
            for _ in 0..n {
                let Some(kind) = cur.take_i64(72) else { break };
                match kind {
                    1 => {
                        let (x1, y1) = cur.take_xy(10).unwrap_or((0.0, 0.0));
                        let (x2, y2) = cur.take_xy(11).unwrap_or((0.0, 0.0));
                        edges.push(HatchEdge::Line {
                            start: to_world(x1, y1),
                            end: to_world(x2, y2),
                        });
                    }
                    2 => {
                        let (cx, cy) = cur.take_xy(10).unwrap_or((0.0, 0.0));
                        let radius = cur.take_f64(40).unwrap_or(0.0);
                        let a0 = cur.take_f64(50).unwrap_or(0.0);
                        let a1 = cur.take_f64(51).unwrap_or(360.0);
                        let ccw = cur.take_i64(73).unwrap_or(1) != 0;
                        edges.push(HatchEdge::Arc {
                            center: to_world(cx, cy),
                            radius,
                            start_angle: a0.to_radians(),
                            end_angle: a1.to_radians(),
                            ccw,
                        });
                    }
                    3 => {
                        let (cx, cy) = cur.take_xy(10).unwrap_or((0.0, 0.0));
                        let (mx, my) = cur.take_xy(11).unwrap_or((1.0, 0.0));
                        let ratio = cur.take_f64(40).unwrap_or(1.0);
                        let a0 = cur.take_f64(50).unwrap_or(0.0);
                        let a1 = cur.take_f64(51).unwrap_or(360.0);
                        let ccw = cur.take_i64(73).unwrap_or(1) != 0;
                        edges.push(HatchEdge::Ellipse {
                            center: to_world(cx, cy),
                            major_axis: dir_world(mx, my),
                            ratio,
                            start_param: a0.to_radians(),
                            end_param: a1.to_radians(),
                            ccw,
                        });
                    }
                    4 => {
                        let degree = cur.take_i64(94).unwrap_or(3).clamp(0, 1000) as u32;
                        let rational = cur.take_i64(73).unwrap_or(0) != 0;
                        let _periodic = cur.take_i64(74);
                        let nknots = limit(cur.take_i64(95).unwrap_or(0), max_vertices, "knot")?;
                        let nctrl =
                            limit(cur.take_i64(96).unwrap_or(0), max_vertices, "control point")?;
                        let mut knots = Vec::new();
                        for _ in 0..nknots {
                            match cur.take_f64(40) {
                                Some(k) => knots.push(k),
                                None => break,
                            }
                        }
                        let mut control_points = Vec::new();
                        let mut weights = Vec::new();
                        for _ in 0..nctrl {
                            let Some((x, y)) = cur.take_xy(10) else { break };
                            control_points.push(to_world(x, y));
                            if let Some(w) = cur.take_f64(42) {
                                weights.push(w);
                            } else if rational {
                                weights.push(1.0);
                            }
                        }
                        if cur.peek_code() == Some(97) {
                            let nfit =
                                limit(cur.take_i64(97).unwrap_or(0), max_vertices, "fit point")?;
                            for _ in 0..nfit {
                                if cur.take_xy(11).is_none() {
                                    break;
                                }
                            }
                        }
                        let _ = cur.take_xy(12);
                        let _ = cur.take_xy(13);
                        edges.push(HatchEdge::Spline {
                            degree,
                            knots,
                            control_points,
                            weights,
                        });
                    }
                    _ => break,
                }
            }
        }
        // Source boundary objects: `97 count` followed by `330 handle` groups.
        while matches!(cur.peek_code(), Some(97 | 330)) {
            cur.next();
        }
        loops.push(HatchLoop {
            edges,
            external: flags & 1 != 0 || flags & 16 != 0,
        });
    }

    // Pattern data follows the paths.
    let tail = Group(cur.pairs.get(cur.i..).unwrap_or(&[]));
    let pattern_angle = tail.f64_or(52, 0.0).to_radians();
    let pattern_scale = tail.f64_or(41, 1.0);
    if let Some(v) = tail.i64(75) {
        props.insert("dxf.hatch.style".into(), MValue::Int(v));
    }
    if let Some(v) = tail.i64(76) {
        props.insert("dxf.hatch.pattern_type".into(), MValue::Int(v));
    }
    if tail.i64(71).is_some_and(|v| v != 0) {
        props.insert("dxf.hatch.associative".into(), MValue::Bool(true));
    }
    if let Some(n) = tail.i64(78) {
        let n = limit(n, max_vertices, "pattern line")?;
        let at = tail
            .0
            .iter()
            .position(|p| p.code == 78)
            .map_or(tail.0.len(), |i| i + 1);
        let mut pc = Cursor {
            pairs: tail.0,
            i: at,
        };
        let mut lines = Vec::new();
        for _ in 0..n {
            let Some(angle) = pc.take_f64(53) else { break };
            let mut row = vec![MValue::Float(angle)];
            for code in [43, 44, 45, 46] {
                row.push(MValue::Float(pc.take_f64(code).unwrap_or(0.0)));
            }
            let nd = limit(pc.take_i64(79).unwrap_or(0), max_vertices, "dash")?;
            for _ in 0..nd {
                match pc.take_f64(49) {
                    Some(d) => row.push(MValue::Float(d)),
                    None => break,
                }
            }
            lines.push(MValue::List(row));
        }
        if !lines.is_empty() {
            props.insert("dxf.hatch.pattern_lines".into(), MValue::List(lines));
        }
    }
    // Seed points (98 count, then 10/20 pairs).
    if let Some(at) = tail.0.iter().position(|p| p.code == 98) {
        let mut sc = Cursor {
            pairs: tail.0,
            i: at + 1,
        };
        let mut seeds = Vec::new();
        while let Some((x, y)) = sc.take_xy(10) {
            if seeds.len() >= max_vertices as usize {
                break;
            }
            seeds.push(point_value(to_world(x, y)));
        }
        if !seeds.is_empty() {
            props.insert("dxf.hatch.seeds".into(), MValue::List(seeds));
        }
    }
    Ok(HatchData {
        loops,
        solid,
        pattern,
        pattern_scale,
        pattern_angle,
    })
}
