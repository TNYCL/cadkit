//! HATCH decoder (§20.4.75), cross-checked with ACadSharp's `readHatch` (MIT).

use cadkit_core::{Error, Result};

use super::{Streams, checked_count, read_handles, spline_degree};
use crate::native::{Hatch, HatchEdgeData, HatchPath, ObjectData, PatternLine};

/// Fewest main-stream bits one boundary path can take: flags, edge or vertex count and
/// boundary-object count as zero `BL`s.
const MIN_PATH_BITS: u64 = 6;
/// Fewest bits of one edge: the type byte plus, for the cheapest (spline) edge, a zero
/// degree and counts and two flag bits.
const MIN_EDGE_BITS: u64 = 16;

/// Counts the paths, edges, vertices and spline points of one hatch against
/// `Limits::max_vertices` ("per entity"), so that a hatch cannot grow past the limit
/// through many small paths.
struct ItemBudget {
    left: u64,
}

impl ItemBudget {
    fn take(&mut self, n: usize) -> Result<()> {
        self.left = self.left.checked_sub(n as u64).ok_or_else(|| {
            Error::LimitExceeded("hatch boundary items above the vertex limit".into())
        })?;
        Ok(())
    }
}

fn edge(s: &mut Streams<'_>, items: &mut ItemBudget) -> Result<HatchEdgeData> {
    let kind = s.main.rc()?;
    Ok(match kind {
        1 => HatchEdgeData::Line {
            start: s.main.rd2()?,
            end: s.main.rd2()?,
        },
        2 => HatchEdgeData::Arc {
            center: s.main.rd2()?,
            radius: s.main.bd()?,
            start: s.main.bd()?,
            end: s.main.bd()?,
            ccw: s.main.b()?,
        },
        3 => HatchEdgeData::Ellipse {
            center: s.main.rd2()?,
            major_axis: s.main.rd2()?,
            ratio: s.main.bd()?,
            start: s.main.bd()?,
            end: s.main.bd()?,
            ccw: s.main.b()?,
        },
        4 => {
            let degree = s.main.bl()?;
            let degree = spline_degree(s, degree);
            let rational = s.main.b()?;
            let periodic = s.main.b()?;
            let knot_count = s.main.bl()?;
            let control_count = s.main.bl()?;
            let n = checked_count(s, knot_count, 2)?;
            items.take(n)?;
            let mut knots = Vec::with_capacity(n);
            for _ in 0..n {
                knots.push(s.main.bd()?);
            }
            let n = checked_count(s, control_count, 128)?;
            items.take(n)?;
            let (mut control_points, mut weights) = (Vec::with_capacity(n), Vec::new());
            for _ in 0..n {
                control_points.push(s.main.rd2()?);
                if rational {
                    weights.push(s.main.bd()?);
                }
            }
            let (mut fit_points, mut tangents) = (Vec::new(), None);
            // R2010+: fit points, followed by the two tangents only when there are fit
            // points (the spec lists the tangents unconditionally).
            if s.version().r2010_plus() {
                let fit_count = s.main.bl()?;
                if fit_count > 0 {
                    let n = checked_count(s, fit_count, 128)?;
                    items.take(n)?;
                    for _ in 0..n {
                        fit_points.push(s.main.rd2()?);
                    }
                    tangents = Some([s.main.rd2()?, s.main.rd2()?]);
                }
            }
            HatchEdgeData::Spline {
                degree,
                rational,
                periodic,
                knots,
                control_points,
                weights,
                fit_points,
                tangents,
            }
        }
        other => return Err(s.main.invalid(format!("hatch edge type {other}"))),
    })
}

/// HATCH.
pub fn hatch(s: &mut Streams<'_>) -> Result<ObjectData> {
    let v = s.version();
    let mut h = Hatch::default();
    if v.r2004_plus() {
        h.gradient = Some(s.main.bl()? != 0);
        let _reserved = s.main.bl()?;
        let _angle = s.main.bd()?;
        let _shift = s.main.bd()?;
        let _single_color = s.main.bl()?;
        let _tint = s.main.bd()?;
        let colors = s.main.bl()?;
        let n = checked_count(s, colors, 8)?;
        for _ in 0..n {
            let _value = s.main.bd()?;
            let _color = s.cmc()?;
        }
        h.gradient_name = s.tv()?;
    }
    h.elevation = s.main.bd()?;
    h.extrusion = s.main.bd3()?;
    h.pattern = s.tv()?;
    h.solid = s.main.b()?;
    h.associative = s.main.b()?;
    let path_count = s.main.bl()?;
    let paths = checked_count(s, path_count, MIN_PATH_BITS)?;
    let mut items = ItemBudget {
        left: u64::from(s.ctx.max_vertices),
    };
    items.take(paths)?;
    let mut derived = false;
    for _ in 0..paths {
        let mut path = HatchPath {
            flags: s.main.bl()?,
            ..HatchPath::default()
        };
        derived |= path.flags & 4 != 0;
        if path.flags & 2 == 0 {
            let edge_count = s.main.bl()?;
            let n = checked_count(s, edge_count, MIN_EDGE_BITS)?;
            items.take(n)?;
            for _ in 0..n {
                path.edges.push(edge(s, &mut items)?);
            }
        } else {
            let has_bulges = s.main.b()?;
            path.closed = s.main.b()?;
            let vertex_count = s.main.bl()?;
            let n = checked_count(s, vertex_count, 128)?;
            items.take(n)?;
            for _ in 0..n {
                path.polyline.push(s.main.rd2()?);
                if has_bulges {
                    path.bulges.push(s.main.bd()?);
                }
            }
        }
        let boundary_count = s.main.bl()?;
        path.boundary_objects = read_handles(s, boundary_count, "hatch boundary")?;
        h.paths.push(path);
    }
    h.style = s.main.bs()?;
    h.pattern_type = s.main.bs()?;
    if !h.solid {
        h.angle = s.main.bd()?;
        h.scale = s.main.bd()?;
        h.double = s.main.b()?;
        let line_count = i32::from(s.main.bs()?);
        let n = checked_count(s, line_count, 8)?;
        for _ in 0..n {
            let mut line = PatternLine {
                angle: s.main.bd()?,
                base: s.main.bd2()?,
                offset: s.main.bd2()?,
                dashes: Vec::new(),
            };
            let dash_count = i32::from(s.main.bs()?);
            let dashes = checked_count(s, dash_count, 2)?;
            for _ in 0..dashes {
                line.dashes.push(s.main.bd()?);
            }
            h.lines.push(line);
        }
    }
    if derived {
        h.pixel_size = Some(s.main.bd()?);
    }
    let seed_count = s.main.bl()?;
    let n = checked_count(s, seed_count, 128)?;
    for _ in 0..n {
        h.seeds.push(s.main.rd2()?);
    }
    Ok(ObjectData::Hatch(h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::testbits::{BitWriter, ctx, streams};
    use crate::version::DwgVersion;

    /// R2000 HATCH prefix: elevation, extrusion, empty pattern name, solid, not
    /// associative, then `paths` boundary paths.
    fn prefix(w: &mut BitWriter, paths: i32) {
        w.bd(0.0)
            .bd(0.0)
            .bd(0.0)
            .bd(1.0)
            .bs(0)
            .bit(true)
            .bit(false)
            .bl(paths);
    }

    /// Style, pattern type and no seed points (solid fill).
    fn suffix(w: &mut BitWriter) {
        w.bs(0).bs(1).bl(0);
    }

    #[test]
    fn spline_edge_degree_is_clamped() {
        let mut w = BitWriter::default();
        prefix(&mut w, 1);
        // Edge path with one spline edge of degree 1000 and no knots/points.
        w.bl(0)
            .bl(1)
            .rc(4)
            .bl(1000)
            .bit(false)
            .bit(false)
            .bl(0)
            .bl(0);
        w.bl(0);
        suffix(&mut w);
        let data = w.bytes();
        let mut s = streams(&data, ctx(DwgVersion::R2000, 100));
        let ObjectData::Hatch(h) = hatch(&mut s).unwrap() else {
            panic!("not a hatch")
        };
        assert!(matches!(
            h.paths[0].edges[0],
            HatchEdgeData::Spline { degree: 25, .. }
        ));
        assert_eq!(s.warnings.len(), 1);
    }

    #[test]
    fn path_count_needs_six_bits_per_path() {
        // 1000 paths followed by 5 bits each: enough for the old 4-bit bound, not for
        // the real minimum of 6.
        let mut w = BitWriter::default();
        prefix(&mut w, 1000);
        for _ in 0..5000 {
            w.bit(false);
        }
        let data = w.bytes();
        let err = hatch(&mut streams(&data, ctx(DwgVersion::R2000, 1_000_000))).unwrap_err();
        assert!(err.to_string().contains("exceeds the object data"), "{err}");
    }

    #[test]
    fn boundary_items_count_against_the_vertex_limit() {
        // Three polyline paths of two vertices: 9 items, limit 5.
        let mut w = BitWriter::default();
        prefix(&mut w, 3);
        for _ in 0..3 {
            w.bl(2).bit(false).bit(true).bl(2);
            for v in [1.5, 2.5, 3.5, 4.5] {
                w.rd(v);
            }
            w.bl(0);
        }
        suffix(&mut w);
        let data = w.bytes();
        let err = hatch(&mut streams(&data, ctx(DwgVersion::R2000, 5))).unwrap_err();
        assert!(matches!(err, Error::LimitExceeded(_)), "{err}");
        // Within the limit the same hatch decodes.
        let ObjectData::Hatch(h) = hatch(&mut streams(&data, ctx(DwgVersion::R2000, 9))).unwrap()
        else {
            panic!("not a hatch")
        };
        assert_eq!(h.paths.len(), 3);
        assert_eq!(h.paths[2].polyline, vec![[1.5, 2.5], [3.5, 4.5]]);
    }
}
