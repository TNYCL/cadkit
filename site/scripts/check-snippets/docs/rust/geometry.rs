use cadkit_geom::{GeometryTolerance, LineSegment, Point2, Polygon, PolygonBooleanOptions};

fn main() -> Result<(), cadkit_geom::GeometryError> {
    let t = GeometryTolerance::default(); // explicit linear, angular and area tolerances

    // Offset a wall axis by half the wall thickness; a positive distance offsets to the left.
    let axis = LineSegment { start: Point2::new(0.0, 0.0), end: Point2::new(4.0, 0.0) };
    let face = axis.offset(0.1, &t)?;
    println!("offset: {:?} -> {:?}", face.start, face.end);

    // Rings close implicitly: do not repeat the first point.
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
        vec![Point2::new(x0, y0), Point2::new(x1, y0), Point2::new(x1, y1), Point2::new(x0, y1)]
    };
    let room = Polygon { exterior: rect(0.0, 0.0, 5.0, 4.0), holes: vec![rect(2.0, 1.5, 2.4, 1.9)] };
    println!("room area: {:.2}", room.area(&t)?); // 20 - 0.16 square units
    println!("{:?}", room.locate(Point2::new(1.0, 1.0), &t)?);

    // Regularized union with explicit coordinate and area error budgets.
    let annex = Polygon { exterior: rect(4.0, 1.0, 8.0, 3.0), holes: vec![] };
    let union = room.union(&annex, PolygonBooleanOptions::default(), &t)?;
    println!("union: {} polygon(s), area {:.2}", union.geometry.polygons.len(), union.area);
    Ok(())
}
