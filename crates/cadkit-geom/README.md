# cadkit-geom

Independent analytic `f64` geometry with explicit tolerances: curves, polygon
booleans, calibration, trim/offset, and certified NURBS operations. No other
cadkit crate is required. Rust 1.88 or newer; MIT OR Apache-2.0.

Lengths use caller-defined Cartesian units, areas use squared units, and angles
are radians counterclockwise from +x. Rings close implicitly without repeating
the first point. Serde is always available; JSON Schema is optional through the
`schemars` feature.

```rust
use cadkit_geom::{GeometryTolerance, LineSegment, Point2};
let segment = LineSegment {
    start: Point2::new(0.0, 0.0),
    end: Point2::new(4.0, 0.0),
};
let offset = segment.offset(1.0, &GeometryTolerance::default())?;
assert_eq!(offset.start, Point2::new(0.0, 1.0));
# Ok::<(), cadkit_geom::GeometryError>(())
```

See the API documentation for supported profiles and the repository's
[geometry notes](https://github.com/TNYCL/cadkit/blob/main/docs/geom/NOTES.md)
for numerical guarantees and limitations. Display tessellation is not an
analytic measurement source; exhausted budgets return typed errors.
