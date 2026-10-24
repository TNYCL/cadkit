//! Neutral 3D triangle meshes in f64 world coordinates.
//!
//! Meshes stay in f64 until upload. [`relative_positions`] subtracts a scene origin in f64
//! and converts to f32 only when the combined error of the subtraction and the conversion
//! stays within a caller budget, so far-from-origin drawings keep their precision or fail
//! explicitly. Measurement data is never recovered from the f32 copy.

use crate::{GroupId, ObjectId, SceneError};

/// One colored triangle mesh of a caller object.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Mesh3d {
    /// Owning object.
    pub id: ObjectId,
    /// Group of the object; see [`GroupId`].
    pub group: GroupId,
    /// Vertex positions in model units.
    pub positions: Vec<[f64; 3]>,
    /// Counter-clockwise triangles as indices into [`Self::positions`].
    pub triangles: Vec<[u32; 3]>,
    /// Unmultiplied sRGBA color.
    pub color: [u8; 4],
}

impl Mesh3d {
    /// A mesh of `positions` and `triangles`.
    pub fn new(
        id: ObjectId,
        group: GroupId,
        positions: Vec<[f64; 3]>,
        triangles: Vec<[u32; 3]>,
        color: [u8; 4],
    ) -> Self {
        Self {
            id,
            group,
            positions,
            triangles,
            color,
        }
    }
}

/// A set of meshes drawn together.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct MeshScene {
    /// Caller revision the meshes were built from; renderers compare it to skip uploads.
    pub revision: u64,
    /// Meshes in drawing order.
    pub meshes: Vec<Mesh3d>,
    /// `false` when the caller stopped early (for example at a [`MeshBudget`]).
    pub complete: bool,
}

impl MeshScene {
    /// A complete scene of `meshes` at `revision`.
    pub fn new(revision: u64, meshes: Vec<Mesh3d>) -> Self {
        Self {
            revision,
            meshes,
            complete: true,
        }
    }

    /// Sets [`Self::complete`].
    #[must_use]
    pub fn with_complete(mut self, complete: bool) -> Self {
        self.complete = complete;
        self
    }

    /// Total vertex and triangle counts.
    pub fn counts(&self) -> (usize, usize) {
        self.meshes.iter().fold((0, 0), |(v, t), mesh| {
            (
                v.saturating_add(mesh.positions.len()),
                t.saturating_add(mesh.triangles.len()),
            )
        })
    }
}

/// Upper bounds on the size of a scene, for example derived from GPU buffer limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct MeshBudget {
    /// Most vertices in one scene.
    pub max_vertices: usize,
    /// Most triangles in one scene.
    pub max_triangles: usize,
}

impl MeshBudget {
    /// A budget of `max_vertices` and `max_triangles`.
    pub const fn new(max_vertices: usize, max_triangles: usize) -> Self {
        Self {
            max_vertices,
            max_triangles,
        }
    }

    /// Whether `scene` fits.
    pub fn fits(&self, scene: &MeshScene) -> bool {
        let (vertices, triangles) = scene.counts();
        vertices <= self.max_vertices && triangles <= self.max_triangles
    }
}

impl Default for MeshBudget {
    fn default() -> Self {
        Self::new(1_000_000, 2_000_000)
    }
}

/// Converts `positions` to f32 offsets from `origin`. Each coordinate's error, from the f64
/// subtraction (recovered exactly with TwoDiff) plus the f32 rounding, must not exceed
/// `max_error` model units.
///
/// # Errors
///
/// [`SceneError::InvalidBudget`] when `max_error` is not positive and finite;
/// [`SceneError::Precision`] for the first vertex over budget or not finite.
pub fn relative_positions(
    positions: &[[f64; 3]],
    origin: [f64; 3],
    max_error: f64,
) -> Result<Vec<[f32; 3]>, SceneError> {
    if !max_error.is_finite() || max_error <= 0. {
        return Err(SceneError::InvalidBudget { name: "max_error" });
    }
    positions
        .iter()
        .enumerate()
        .map(|(vertex, point)| {
            let mut gpu = [0f32; 3];
            for ((slot, coordinate), start) in gpu.iter_mut().zip(point).zip(origin) {
                let coordinate = *coordinate;
                // TwoDiff keeps the part of the subtraction that f64 rounds away.
                let local = coordinate - start;
                let virtual_start = coordinate - local;
                let virtual_coordinate = local + virtual_start;
                let subtraction_error = (coordinate - virtual_coordinate) + (virtual_start - start);
                let encoded = local as f32;
                let total_error =
                    (subtraction_error.abs() + (local - f64::from(encoded)).abs()).next_up();
                if !coordinate.is_finite()
                    || !start.is_finite()
                    || !local.is_finite()
                    || !encoded.is_finite()
                    || !total_error.is_finite()
                    || total_error > max_error
                {
                    return Err(SceneError::Precision { vertex });
                }
                *slot = encoded;
            }
            Ok(gpu)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_coordinates_keep_their_precision_or_fail() {
        let origin = [5.0e6, 4.0e6, 100.];
        let near = [[5.0e6 + 0.125, 4.0e6 - 3.5, 103.25]];
        assert_eq!(
            relative_positions(&near, origin, 1e-6).unwrap(),
            vec![[0.125, -3.5, 3.25]]
        );
        // One millimetre relative to a far origin does not fit a 1 nm budget in f32.
        let far = [[0.001, 0., 0.]];
        assert_eq!(
            relative_positions(&far, origin, 1e-9),
            Err(SceneError::Precision { vertex: 0 })
        );
        for bad in [0., -1., f64::NAN, f64::INFINITY] {
            assert!(matches!(
                relative_positions(&near, origin, bad),
                Err(SceneError::InvalidBudget { .. })
            ));
        }
        let hostile = [[0., 0., 0.], [f64::NAN, 0., 0.]];
        assert_eq!(
            relative_positions(&hostile, [0.; 3], 1.),
            Err(SceneError::Precision { vertex: 1 })
        );
        assert_eq!(
            relative_positions(&[[1e300, 0., 0.]], [-1e300, 0., 0.], 1e300),
            Err(SceneError::Precision { vertex: 0 })
        );
    }

    #[test]
    fn budgets_count_the_whole_scene() {
        let mesh = Mesh3d::new(
            ObjectId(1),
            GroupId(0),
            vec![[0.; 3]; 3],
            vec![[0, 1, 2]],
            [255; 4],
        );
        let scene = MeshScene::new(7, vec![mesh.clone(), mesh]);
        assert_eq!(scene.counts(), (6, 2));
        assert!(MeshBudget::new(6, 2).fits(&scene));
        assert!(!MeshBudget::new(5, 2).fits(&scene));
    }
}
