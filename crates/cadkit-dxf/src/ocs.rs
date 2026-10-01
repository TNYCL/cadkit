//! Object coordinate systems (OCS) and the arbitrary-axis algorithm.
//!
//! DXF stores most planar entities in an OCS defined only by an extrusion direction
//! (group codes 210/220/230). The X and Y axes of that system are derived with the
//! *arbitrary axis algorithm* of the public DXF reference:
//!
//! * if `|Nx| < 1/64` and `|Ny| < 1/64` then `Ax = Y x N`, otherwise `Ax = Z x N`;
//! * `Ay = N x Ax`; all three axes are normalised.
//!
//! The axes come from `cadkit_core::geom_ops::arbitrary_axes` so every crate agrees.

use cadkit_core::{Point3, Vec3};

/// An object coordinate system built from an extrusion direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ocs {
    /// Unit X axis of the OCS in world coordinates.
    pub ax: Vec3,
    /// Unit Y axis of the OCS in world coordinates.
    pub ay: Vec3,
    /// Unit extrusion direction (OCS Z axis) in world coordinates.
    pub az: Vec3,
}

/// Normalises `n`; a zero or non-finite vector becomes +Z.
pub fn normalize_normal(n: Vec3) -> Vec3 {
    n.normalized().unwrap_or(Vec3::Z)
}

impl Ocs {
    /// Builds the OCS for extrusion direction `normal` (need not be normalised).
    pub fn new(normal: Vec3) -> Self {
        let (ax, ay, az) = cadkit_core::geom_ops::arbitrary_axes(normal);
        Self { ax, ay, az }
    }

    /// True when the OCS is the world system (normal exactly +Z).
    pub fn is_world(&self) -> bool {
        self.az == Vec3::Z
            && self.ax == Vec3::new(1.0, 0.0, 0.0)
            && self.ay == Vec3::new(0.0, 1.0, 0.0)
    }

    /// OCS point to world point.
    pub fn to_world(self, p: Point3) -> Point3 {
        if self.is_world() {
            return p;
        }
        Point3::new(
            self.ax.x * p.x + self.ay.x * p.y + self.az.x * p.z,
            self.ax.y * p.x + self.ay.y * p.y + self.az.y * p.z,
            self.ax.z * p.x + self.ay.z * p.y + self.az.z * p.z,
        )
    }

    /// World point to OCS point.
    pub fn to_local(self, p: Point3) -> Point3 {
        if self.is_world() {
            return p;
        }
        let dot = |a: Vec3| a.x * p.x + a.y * p.y + a.z * p.z;
        Point3::new(dot(self.ax), dot(self.ay), dot(self.az))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_is_identity() {
        let o = Ocs::new(Vec3::Z);
        assert!(o.is_world());
        let p = Point3::new(1.0, 2.0, 3.0);
        assert_eq!(o.to_world(p), p);
    }

    #[test]
    fn flipped_normal() {
        // N = -Z: Ax = Y x N = (-1, 0, 0), Ay = N x Ax = (0, 1, 0).
        let o = Ocs::new(Vec3::new(0.0, 0.0, -1.0));
        let w = o.to_world(Point3::new(1.0, 2.0, 0.0));
        assert!((w.x + 1.0).abs() < 1e-12 && (w.y - 2.0).abs() < 1e-12 && w.z.abs() < 1e-12);
    }

    #[test]
    fn round_trip_arbitrary_normal() {
        let o = Ocs::new(Vec3::new(1.0, 2.0, 3.0));
        let p = Point3::new(4.0, -5.0, 6.0);
        let q = o.to_local(o.to_world(p));
        assert!(
            (p.x - q.x).abs() < 1e-12 && (p.y - q.y).abs() < 1e-12 && (p.z - q.z).abs() < 1e-12
        );
        assert!(o.ax.x * o.ay.x + o.ax.y * o.ay.y + o.ax.z * o.ay.z < 1e-12);
    }
}
