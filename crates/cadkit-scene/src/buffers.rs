//! Flat, renderer-ready buffers of a drawing model: triangles and line segments in f32
//! relative to an f64 origin, grouped by layer, with a table that leads every triangle and
//! segment back to its entity.
//!
//! The buffers are plain `Vec`s of scalars so they can be handed to WebGL / wgpu (or across a
//! WASM boundary) without conversion:
//!
//! | buffer | element | per |
//! |---|---|---|
//! | `positions` | 3 x f32: x, y, z relative to `origin` | vertex |
//! | `normals` | 3 x f32: unit face normal | vertex |
//! | `colors` | 3 x u8: sRGB | vertex |
//! | `indices` | 3 x u32: vertex indices, counter-clockwise about the normal | triangle |
//! | `triangle_items` | u32: index into `items` | triangle |
//! | `segments` | 2 x (2 or 3) f32: both end points relative to `origin` | segment |
//! | `segment_colors` | 2 x 3 x u8: sRGB of both end points | segment |
//! | `segment_items` | u32: index into `items` | segment |
//!
//! Triangles and segments of one layer are contiguous ([`LayerRange`]), so a renderer can
//! hide a layer by skipping its range without rebuilding anything. The builders live in
//! [`crate::document`] (feature `document`); this module only holds the data and the ray pick.

use std::collections::BTreeMap;

/// Where one layer's triangles and segments sit in the buffers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct LayerRange {
    /// Layer name.
    pub name: String,
    /// First triangle (index into `triangle_items`; its vertex indices start at
    /// `indices[3 * first_triangle]`).
    pub first_triangle: u32,
    /// Number of triangles.
    pub triangle_count: u32,
    /// First segment (index into `segment_items`).
    pub first_segment: u32,
    /// Number of segments.
    pub segment_count: u32,
}

/// One drawn piece of an entity: a face (one polygon, one mesh face, one hatch region) or the
/// wire geometry of one entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SceneItem {
    /// Start of the entity's walk path in [`SceneItems::paths`].
    pub path_start: u32,
    /// Length of the walk path.
    pub path_len: u32,
    /// Length of the leading part of the path that addresses an entity of the model itself
    /// (for geometry drawn from a block: the path of the outermost block reference).
    pub model_path_len: u32,
    /// Index into the layer table (`layers` of the buffer).
    pub layer: u32,
    /// The entity kind's stable name (`EntityKind::type_name` of cadkit-core: `"polygon"`,
    /// `"face"`, `"mesh"`, `"hatch"`, `"polyline"`, `"line"`, ...).
    pub kind: &'static str,
    /// The item is a face (it owns triangles) rather than wire geometry.
    pub surface: bool,
}

/// The item table of a buffer.
///
/// A walk path is a list of indices from the model's entity list downwards: a step into a
/// group goes to its children, a step into a block reference goes to the block's entities
/// (`WalkContext::path` and `Document::entity_at` of cadkit-core).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct SceneItems {
    /// One entry per item.
    pub items: Vec<SceneItem>,
    /// The concatenated walk paths of all items.
    pub paths: Vec<u32>,
}

impl SceneItems {
    /// Number of items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are no items.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Walk path of item `item`, or `None` for an index out of range.
    pub fn path(&self, item: usize) -> Option<&[u32]> {
        let entry = self.items.get(item)?;
        let start = entry.path_start as usize;
        self.paths
            .get(start..start.checked_add(entry.path_len as usize)?)
    }

    /// [`Self::path`] as `usize` indices, as `Document::entity_at` takes them.
    pub fn path_indices(&self, item: usize) -> Option<Vec<usize>> {
        Some(self.path(item)?.iter().map(|&i| i as usize).collect())
    }
}

/// Triangles and outline / wire segments of one model; see the [module documentation](self)
/// for the memory layout.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct SurfaceMesh {
    /// Drawing coordinates of the local origin (f64); every position is relative to it.
    pub origin: [f64; 3],
    /// Lowest corner of the box around all positions, relative to `origin`, drawing units.
    pub min: [f64; 3],
    /// Highest corner of that box, relative to `origin`, drawing units.
    pub max: [f64; 3],
    /// x, y, z per vertex, relative to `origin`, drawing units.
    pub positions: Vec<f32>,
    /// Unit face normal per vertex (x, y, z); vertices are not shared between faces.
    pub normals: Vec<f32>,
    /// sRGB per vertex (r, g, b).
    pub colors: Vec<u8>,
    /// Three vertex indices per triangle, counter-clockwise about the face normal.
    pub indices: Vec<u32>,
    /// Item (index into [`Self::items`]) of each triangle.
    pub triangle_items: Vec<u32>,
    /// Two end points per segment, x, y, z each, relative to `origin`, drawing units.
    pub segments: Vec<f32>,
    /// sRGB of both end points of each segment (6 bytes per segment).
    pub segment_colors: Vec<u8>,
    /// Item (index into [`Self::items`]) of each segment.
    pub segment_items: Vec<u32>,
    /// Layer table: `items[i].layer` indexes it; triangles and segments are grouped in its
    /// order.
    pub layers: Vec<LayerRange>,
    /// What every triangle and segment was drawn from.
    pub items: SceneItems,
    /// Largest absolute error of one stored coordinate, in drawing units: `origin` plus the
    /// f32 value differs from the f64 drawing coordinate by at most this much.
    pub max_error: f64,
    /// What was drawn and what was not.
    pub report: SurfaceReport,
}

/// What a [`SurfaceMesh`] holds and what was left out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct SurfaceReport {
    /// Faces drawn.
    pub faces: usize,
    /// Triangles drawn.
    pub triangles: usize,
    /// Segments drawn (face outlines and wires).
    pub segments: usize,
    /// Entities drawn as wire geometry.
    pub wires: usize,
    /// Faces left out because another face has the same set of corners.
    pub duplicates: usize,
    /// Faces and wires without area or length, or with non-finite coordinates.
    pub degenerate: usize,
    /// Curved entities flattened into segments.
    pub flattened: usize,
    /// Entities drawn neither as faces nor as wires, by `EntityKind::type_name`.
    pub skipped: BTreeMap<&'static str, usize>,
    /// A budget (walk work, triangulation work, triangles, segments) ran out: the buffers
    /// hold a prefix of the model in walk order.
    pub truncated: bool,
}

/// Line segments of one model for a canvas or line renderer; see the
/// [module documentation](self) for the memory layout.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct LineBuffer {
    /// Drawing coordinates of the local origin (f64); z is 0 for a 2D buffer.
    pub origin: [f64; 3],
    /// Coordinates per end point: 2 (x, y) or 3 (x, y, z).
    pub components: u8,
    /// Lowest corner of the box around all segments, relative to `origin`, drawing units.
    pub min: [f64; 3],
    /// Highest corner of that box, relative to `origin`, drawing units.
    pub max: [f64; 3],
    /// Two end points per segment, `components` f32 each, relative to `origin`.
    pub segments: Vec<f32>,
    /// sRGB of both end points of each segment (6 bytes per segment).
    pub segment_colors: Vec<u8>,
    /// Item (index into [`Self::items`]) of each segment.
    pub segment_items: Vec<u32>,
    /// Layer table; segments are grouped in its order (triangle ranges are empty).
    pub layers: Vec<LayerRange>,
    /// What every segment was drawn from.
    pub items: SceneItems,
    /// Largest absolute error of one stored coordinate, in drawing units (see
    /// [`SurfaceMesh::max_error`]).
    pub max_error: f64,
    /// What was drawn and what was not.
    pub report: LineReport,
}

/// What a [`LineBuffer`] holds and what was left out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct LineReport {
    /// Entities drawn.
    pub drawn: usize,
    /// Segments drawn.
    pub segments: usize,
    /// Curved entities flattened into segments.
    pub flattened: usize,
    /// Entities without length, or with non-finite coordinates.
    pub degenerate: usize,
    /// Entities not drawn (text, hatches, points, images, ...), by `EntityKind::type_name`.
    pub skipped: BTreeMap<&'static str, usize>,
    /// A budget (walk work, segments) ran out: the buffer holds a prefix of the model in walk
    /// order.
    pub truncated: bool,
}

/// The face a ray hits first.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct RayHit {
    /// Triangle hit.
    pub triangle: u32,
    /// Item of that triangle (index into `items`).
    pub item: u32,
    /// Ray parameter of the hit: the point is `origin + distance * direction`, so it is a
    /// length in drawing units when `direction` has unit length.
    pub distance: f64,
}

impl SurfaceMesh {
    /// The nearest triangle hit by the ray `origin + t * direction` (`t > 0`), both faces of a
    /// triangle counting; triangles on the layers listed in `hidden_layers` (indices into
    /// [`Self::layers`]) are ignored. `origin` is relative to [`Self::origin`], like the
    /// positions. The test runs in f64 on the stored f32 positions.
    pub fn pick(
        &self,
        origin: [f64; 3],
        direction: [f64; 3],
        hidden_layers: &[u32],
    ) -> Option<RayHit> {
        if !origin.iter().chain(&direction).all(|v| v.is_finite()) {
            return None;
        }
        let vertex = |i: u32| -> Option<[f64; 3]> {
            let start = (i as usize).checked_mul(3)?;
            match self.positions.get(start..start.checked_add(3)?)? {
                [x, y, z] => Some([f64::from(*x), f64::from(*y), f64::from(*z)]),
                _ => None,
            }
        };
        let mut best: Option<RayHit> = None;
        for (layer, range) in self.layers.iter().enumerate() {
            if hidden_layers.iter().any(|&h| h as usize == layer) {
                continue;
            }
            let end = range.first_triangle.saturating_add(range.triangle_count);
            for triangle in range.first_triangle..end {
                let Some(corners) = (triangle as usize)
                    .checked_mul(3)
                    .and_then(|s| self.indices.get(s..s.checked_add(3)?))
                else {
                    break;
                };
                let (Some(a), Some(b), Some(c)) = (
                    corners.first().and_then(|&i| vertex(i)),
                    corners.get(1).and_then(|&i| vertex(i)),
                    corners.get(2).and_then(|&i| vertex(i)),
                ) else {
                    continue;
                };
                let Some(distance) = intersect(origin, direction, a, b, c) else {
                    continue;
                };
                if best.is_none_or(|hit| distance < hit.distance) {
                    let item = self
                        .triangle_items
                        .get(triangle as usize)
                        .copied()
                        .unwrap_or(u32::MAX);
                    best = Some(RayHit {
                        triangle,
                        item,
                        distance,
                    });
                }
            }
        }
        best
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Ray parameter of the hit with triangle abc (Moller-Trumbore, both faces), if any.
fn intersect(
    origin: [f64; 3],
    direction: [f64; 3],
    a: [f64; 3],
    b: [f64; 3],
    c: [f64; 3],
) -> Option<f64> {
    let (e1, e2) = (sub(b, a), sub(c, a));
    let p = cross(direction, e2);
    let det = dot(e1, p);
    // Parallel to the plane, relative to the sizes involved so tiny triangles still count.
    let scale = dot(e1, e1).sqrt() * dot(e2, e2).sqrt() * dot(direction, direction).sqrt();
    if !det.is_finite() || det.abs() <= 1e-12 * scale {
        return None;
    }
    let inv = 1. / det;
    let t = sub(origin, a);
    let u = dot(t, p) * inv;
    if !(0. ..=1.).contains(&u) {
        return None;
    }
    let q = cross(t, e1);
    let v = dot(direction, q) * inv;
    if v < 0. || u + v > 1. {
        return None;
    }
    let distance = dot(e2, q) * inv;
    (distance > 0. && distance.is_finite()).then_some(distance)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two unit squares on separate layers: one at z = 0, one at z = 1 above it.
    fn two_floors() -> SurfaceMesh {
        let mut mesh = SurfaceMesh::default();
        for z in [0f32, 1.] {
            mesh.positions
                .extend([0., 0., z, 1., 0., z, 1., 1., z, 0., 1., z]);
        }
        mesh.indices = vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
        mesh.triangle_items = vec![0, 0, 1, 1];
        for (layer, name) in ["ground", "upper"].into_iter().enumerate() {
            mesh.layers.push(LayerRange {
                name: name.into(),
                first_triangle: 2 * layer as u32,
                triangle_count: 2,
                ..LayerRange::default()
            });
        }
        mesh
    }

    #[test]
    fn picks_the_nearest_visible_face() {
        let mesh = two_floors();
        let down = [0., 0., -1.];
        let hit = mesh.pick([0.25, 0.5, 5.], down, &[]).unwrap();
        assert_eq!(hit.item, 1);
        assert!((hit.distance - 4.).abs() < 1e-12);
        let below = mesh.pick([0.25, 0.5, 5.], down, &[1]).unwrap();
        assert_eq!(below.item, 0);
        assert!((below.distance - 5.).abs() < 1e-12);
        // From below the ground: the back face counts too.
        assert_eq!(
            mesh.pick([0.5, 0.5, -1.], [0., 0., 1.], &[]).unwrap().item,
            0
        );
        assert_eq!(mesh.pick([0.25, 0.5, 5.], down, &[0, 1]), None);
        assert_eq!(mesh.pick([5., 5., 5.], down, &[]), None);
        assert_eq!(mesh.pick([0.25, 0.5, 5.], [0., 0., 1.], &[]), None);
        assert_eq!(mesh.pick([0.25, 0.5, 5.], [1., 0., 0.], &[]), None);
        assert_eq!(mesh.pick([f64::NAN, 0., 0.], down, &[]), None);
    }

    #[test]
    fn inconsistent_buffers_do_not_panic() {
        let mut mesh = two_floors();
        mesh.indices.truncate(4);
        mesh.positions.truncate(5);
        mesh.layers[1].triangle_count = u32::MAX;
        assert_eq!(mesh.pick([0.25, 0.5, 5.], [0., 0., -1.], &[]), None);
        let items = SceneItems {
            items: vec![SceneItem {
                path_start: 1,
                path_len: 5,
                model_path_len: 1,
                layer: 0,
                kind: "line",
                surface: false,
            }],
            paths: vec![0, 1],
        };
        assert_eq!(items.path(0), None);
        assert_eq!(items.path(1), None);
    }
}
