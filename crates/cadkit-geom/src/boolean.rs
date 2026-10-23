use crate::polygon::{SegmentBox, full_scan, overlapping_box_pairs};
use crate::*;
use i_overlay::core::edge_data::{EdgeDataMerge, OverlayEdgeData};
use i_overlay::core::edge_overlay::{EdgeOverlay, InputEdge};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay::{IntOverlayOptions, ShapeType};
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::i_float::int::point::IntPoint;
use i_overlay::string::slice::IntSlice;
use std::f64::consts::{PI, SQRT_2};

const INTEGER_LIMIT: i64 = 1_i64 << 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
/// Regularized area operation using caller-coordinate and squared-area error budgets; each operand
/// collection represents a union.
#[cfg_attr(feature = "schemars", schemars(transform = crate::unit_enum_schema))]
pub enum PolygonBooleanOperation {
    /// Union of both operand collections; area semantics use the explicit squared-unit budget.
    Union,
    /// Common area or analytic intersection, according to the enclosing result or operation; input
    /// coordinate units are retained.
    Intersection,
    /// Area in the subject collection outside the clip collection; uses the explicit squared-unit
    /// budget.
    Difference,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
/// Unit-independent identity of an input operand collection in a boolean call.
#[cfg_attr(feature = "schemars", schemars(transform = crate::unit_enum_schema))]
pub enum BooleanOperand {
    /// Subject operand collection, independent of units.
    Subject,
    /// Clip operand collection, independent of units.
    Clip,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
/// Unit-independent ring reference into one polygon; hole indices preserve input order.
pub enum PolygonRing {
    /// Exterior ring reference, independent of units.
    Exterior,
    /// Hole ring reference, independent of units.
    Hole {
        /// Zero-based hole index within the polygon; dimensionless.
        index: usize,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Collection of polygons in the same caller coordinate system; empty collections represent zero
/// area. Operations use explicit tolerances.
pub struct MultiPolygon {
    /// Polygon components in the same caller coordinate system.
    pub polygons: Vec<Polygon>,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Boolean error budgets in caller units and squared units, with dimensionless resource limits.
/// Coordinate error must not exceed the supplied linear tolerance.
pub struct PolygonBooleanOptions {
    /// Positive coordinate-unit deviation budget, no larger than the supplied linear tolerance.
    pub max_coordinate_error: f64,
    /// Positive squared-unit budget for the total operation's area error, separate from minimum
    /// valid area.
    pub max_area_error: f64,
    /// Dimensionless input edge cap, 3-16384; default 8192.
    pub max_input_edges: usize,
    /// Dimensionless output edge cap, 3-262144; default 65536.
    pub max_output_edges: usize,
    /// Dimensionless candidate-pair check cap, 1-10000000; default 1000000.
    pub max_pair_checks: usize,
    /// Dimensionless generated-edge cap, 3-262144; default 131072.
    pub max_generated_edges: usize,
}
impl Default for PolygonBooleanOptions {
    fn default() -> Self {
        Self {
            max_coordinate_error: 2.5e-9,
            max_area_error: 1e-6,
            max_input_edges: 8192,
            max_output_edges: 65536,
            max_pair_checks: 1_000_000,
            max_generated_edges: 131072,
        }
    }
}
impl PolygonBooleanOptions {
    /// Construct boolean options with coordinate and squared-area budgets, retaining default
    /// resource caps. Validation occurs when used.
    pub fn with_error_budget(max_coordinate_error: f64, max_area_error: f64) -> Self {
        Self {
            max_coordinate_error,
            max_area_error,
            ..Self::default()
        }
    }
    fn validate(self, t: &GeometryTolerance) -> GeometryResult<()> {
        t.validate()?;
        positive(self.max_coordinate_error, "Boolean coordinate error budget")
            .map_err(|e| e.at("maxCoordinateError", "polygon.boolean"))?;
        positive(self.max_area_error, "Boolean area error budget")
            .map_err(|e| e.at("maxAreaError", "polygon.boolean"))?;
        if self.max_coordinate_error > t.linear {
            return Err(invalid(
                GeometryReason::InvalidParameterRange,
                "maxCoordinateError",
                "Boolean coordinate budget must not exceed the linear tolerance",
            ));
        }
        if self.max_input_edges < 3
            || self.max_input_edges > 16384
            || self.max_output_edges < 3
            || self.max_output_edges > 262144
            || self.max_pair_checks == 0
            || self.max_pair_checks > 10_000_000
            || self.max_generated_edges < 3
            || self.max_generated_edges > 262144
        {
            return Err(invalid(
                GeometryReason::InvalidParameterRange,
                "limits",
                "Boolean resource limits are outside the supported range",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Reference into the original boolean call's operand, polygon, ring and edge order; indices are
/// dimensionless and zero-based.
pub struct BooleanSourceEdge {
    /// Input operand collection; independent of coordinates or units.
    pub operand: BooleanOperand,
    /// Zero-based polygon index in the referenced collection; dimensionless.
    pub polygon_index: usize,
    /// Ring reference retaining its polygon's original order; dimensionless.
    pub ring: PolygonRing,
    /// Zero-based edge index within the referenced ring; dimensionless.
    pub edge_index: usize,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Directed interval on a source edge. Parameters are dimensionless in 0-1, and deviation uses
/// caller coordinate units.
pub struct BooleanSourceSpan {
    /// Original edge reference in input order; indices are dimensionless.
    pub source: BooleanSourceEdge,
    /// Directed start fraction along the original edge, in 0-1; dimensionless.
    pub start_parameter: f64,
    /// Directed end fraction along the original edge, in 0-1; may be less than start_parameter when
    /// reversed.
    pub end_parameter: f64,
    /// Largest measured deviation from the source edge in coordinate units.
    pub max_deviation: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Output edge index and all contributing source spans; indices retain ring order and deviations
/// use caller units.
pub struct BooleanEdgeProvenance {
    /// Zero-based edge index within the referenced ring; dimensionless.
    pub edge_index: usize,
    /// All contributing source spans, including coincident edges; each deviation uses caller units.
    pub sources: Vec<BooleanSourceSpan>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Output polygon/ring reference and edge provenance in the boolean result's coordinate system.
pub struct BooleanBoundaryProvenance {
    /// Zero-based polygon index in the referenced collection; dimensionless.
    pub polygon_index: usize,
    /// Ring reference retaining its polygon's original order; dimensionless.
    pub ring: PolygonRing,
    /// Edge provenance in output ring order; indices are dimensionless.
    pub edges: Vec<BooleanEdgeProvenance>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Measured displacements and conservative error bounds in caller units, areas in squared units,
/// plus dimensionless resource counts.
pub struct BooleanDiagnostics {
    /// Stable identifier of the numerical boolean profile; independent of units.
    pub profile: String,
    /// Boolean dependency name and version; independent of units.
    pub kernel: String,
    /// World-coordinate origin used by the local integer adapter, in caller units.
    pub local_origin: Point2,
    /// Quantization grid spacing in caller coordinate units.
    pub grid_size: f64,
    /// Maximum absolute integer grid coordinate; dimensionless, currently 2^50.
    pub integer_coordinate_limit: i64,
    /// Largest measured input encode/decode displacement in coordinate units.
    pub max_input_displacement: f64,
    /// Largest measured preflight intersection displacement between representations, in coordinate
    /// units.
    pub max_intersection_displacement: f64,
    /// Largest measured output-to-source edge deviation in coordinate units.
    pub max_output_boundary_deviation: f64,
    /// Conservative linear boundary displacement bound in caller coordinate units.
    pub coordinate_error_bound: f64,
    /// Total conservative area error bound in squared caller units.
    pub area_error_bound: f64,
    /// Arithmetic contribution to the total area error bound, in squared caller units.
    pub arithmetic_area_error_bound: f64,
    /// Number of source edges, dimensionless.
    pub input_edge_count: usize,
    /// Number of output edges, dimensionless.
    pub output_edge_count: usize,
    /// Number of candidate-pair and interior-node checks, dimensionless.
    pub pair_checks: usize,
    /// Only area geometry is returned; point and edge contacts are not separate area
    /// components.
    pub regularized_area_only: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Regularized area geometry, diagnostics and source provenance in caller coordinates; area is in
/// squared units.
pub struct PolygonBooleanResult {
    /// Area operation performed; coordinate and area budgets are recorded in diagnostics.
    pub operation: PolygonBooleanOperation,
    /// Polygon components in input coordinate units; empty output is valid zero area.
    pub geometry: MultiPolygon,
    /// Area in squared caller coordinate units.
    pub area: f64,
    /// Measured and bounded errors in the input coordinate and squared-area units.
    pub diagnostics: BooleanDiagnostics,
    /// Output boundary provenance referencing original input edge order.
    pub boundaries: Vec<BooleanBoundaryProvenance>,
}

/// Split closed regions with zero-width separators without consuming area. Open ends do not
/// close a region; output pieces are checked against area and coordinate budgets in the
/// caller's units.
pub fn polygon_slice(
    polygons: &[Polygon],
    separators: &[LineSegment],
    options: PolygonBooleanOptions,
    tolerance: &GeometryTolerance,
) -> GeometryResult<MultiPolygon> {
    options.validate(tolerance)?;
    if separators.is_empty() {
        for polygon in polygons {
            polygon.validate(tolerance)?;
        }
        return Ok(MultiPolygon {
            polygons: polygons.to_vec(),
        });
    }
    let mut points = Vec::new();
    let mut lines = Vec::new();
    let mut input_area = 0.;
    for polygon in polygons {
        input_area += polygon.area(tolerance)?;
        for ring in std::iter::once(&polygon.exterior).chain(&polygon.holes) {
            points.extend(ring.iter().copied());
            lines.extend(crate::polygon::ring_segments(ring));
        }
    }
    for separator in separators {
        separator.validate(tolerance)?;
        points.extend([separator.start, separator.end]);
        lines.push(*separator);
    }
    if lines.len() > options.max_input_edges {
        return Err(resource("maxInputEdges"));
    }
    let perimeter: f64 = lines.iter().map(LineSegment::length).sum();
    let adapter = Adapter::new(&points, perimeter, options)?;
    let sources = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            Ok(SourceEdge {
                reference: BooleanSourceEdge {
                    operand: BooleanOperand::Subject,
                    polygon_index: 0,
                    ring: PolygonRing::Exterior,
                    edge_index: index,
                },
                world_line: *line,
                line: LineSegment {
                    start: adapter.local(line.start),
                    end: adapter.local(line.end),
                },
                quantized: LineSegment {
                    start: adapter.decode_local(adapter.encode(line.start)?),
                    end: adapter.decode_local(adapter.encode(line.end)?),
                },
            })
        })
        .collect::<GeometryResult<Vec<_>>>()?;
    let (intersection_error, _) = preflight(&sources, &adapter, options, tolerance)?;
    let error_bound = intersection_error + adapter.grid * 4. * SQRT_2 + adapter.roundoff;
    if error_bound > options.max_coordinate_error {
        return Err(ambiguity(
            "maxCoordinateError",
            "Slicing coordinate error budget exceeded",
        ));
    }
    let mut shapes = Vec::new();
    for polygon in polygons {
        let mut shape = Vec::new();
        for (index, ring) in std::iter::once(&polygon.exterior)
            .chain(&polygon.holes)
            .enumerate()
        {
            let mut encoded = ring
                .iter()
                .map(|p| adapter.encode(*p))
                .collect::<GeometryResult<Vec<_>>>()?;
            if (signed_ring_area(ring)? > 0.) != (index == 0) {
                encoded.reverse();
            }
            shape.push(encoded);
        }
        shapes.push(shape);
    }
    let paths = separators
        .iter()
        .map(|line| Ok(vec![adapter.encode(line.start)?, adapter.encode(line.end)?]))
        .collect::<GeometryResult<Vec<_>>>()?;
    let sliced = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        shapes.slice_by_paths(&paths, FillRule::NonZero)
    }))
    .map_err(|_| {
        invalid(
            GeometryReason::BooleanKernelFailure,
            "kernel",
            "Polygon slicing kernel failed to produce a result",
        )
    })?;
    if sliced.iter().flatten().map(Vec::len).sum::<usize>() > options.max_output_edges {
        return Err(resource("maxOutputEdges"));
    }
    // Only edges whose budget-expanded boxes contain the output point can meet the distance
    // budget. Validate all lines at the first point to preserve the exhaustive scan's first
    // error.
    let scale = points
        .iter()
        .fold(1_f64, |scale, p| scale.max(p.x.abs()).max(p.y.abs()));
    let margin = options.max_coordinate_error + 16. * f64::EPSILON * scale;
    let boxes: Vec<SegmentBox> = lines
        .iter()
        .map(|line| SegmentBox {
            min_x: line.start.x.min(line.end.x) - margin,
            max_x: line.start.x.max(line.end.x) + margin,
            min_y: line.start.y.min(line.end.y) - margin,
            max_y: line.start.y.max(line.end.y) + margin,
        })
        .collect();
    let mut by_min_x: Vec<_> = boxes.iter().enumerate().collect();
    by_min_x.sort_by(|a, b| a.1.min_x.total_cmp(&b.1.min_x));
    let mut lines_validated = false;
    let mut result = Vec::new();
    let mut output_area = 0.;
    for shape in sliced {
        let mut rings = shape
            .iter()
            .map(|ring| {
                ring.iter()
                    .map(|p| adapter.decode(*p))
                    .collect::<GeometryResult<Vec<_>>>()
            })
            .collect::<GeometryResult<Vec<_>>>()?;
        if rings.is_empty() {
            return Err(ambiguity(
                "output",
                "Slicing kernel returned an empty shell",
            ));
        }
        let polygon = Polygon {
            exterior: rings.remove(0),
            holes: rings,
        };
        output_area += polygon.area(tolerance)?;
        for ring in std::iter::once(&polygon.exterior).chain(&polygon.holes) {
            for segment in crate::polygon::ring_segments(ring) {
                for point in [segment.start, segment.start.lerp(segment.end, 0.5)] {
                    if !lines_validated {
                        for line in &lines {
                            line.validate(tolerance)?;
                            point.validate()?;
                        }
                        lines_validated = true;
                    }
                    let end = if full_scan() {
                        by_min_x.len()
                    } else {
                        by_min_x.partition_point(|(_, bounds)| bounds.min_x <= point.x)
                    };
                    let mut distance = f64::INFINITY;
                    for &(index, bounds) in by_min_x.iter().take(end) {
                        if full_scan()
                            || point.x <= bounds.max_x
                                && point.y >= bounds.min_y
                                && point.y <= bounds.max_y
                        {
                            let nearest = at(&lines, index)?.nearest_point(point, tolerance)?;
                            distance = distance.min(nearest.distance(point));
                        }
                    }
                    if distance > options.max_coordinate_error {
                        return Err(ambiguity(
                            "boundary",
                            "Sliced boundary deviates from source edges",
                        ));
                    }
                }
            }
        }
        result.push(polygon);
    }
    if (input_area - output_area).abs() > options.max_area_error {
        return Err(ambiguity("area", "A zero-width separator changed the area"));
    }
    Ok(MultiPolygon { polygons: result })
}

impl Polygon {
    /// Regularized union in input coordinate units, preserving all source edge provenance under
    /// explicit coordinate, area and validation budgets.
    pub fn union(
        &self,
        other: &Self,
        options: PolygonBooleanOptions,
        t: &GeometryTolerance,
    ) -> GeometryResult<PolygonBooleanResult> {
        polygon_boolean(
            std::slice::from_ref(self),
            std::slice::from_ref(other),
            PolygonBooleanOperation::Union,
            options,
            t,
        )
    }
    /// Regularized common area in input coordinate units with explicit coordinate, squared-area and
    /// validation budgets.
    pub fn intersection(
        &self,
        other: &Self,
        options: PolygonBooleanOptions,
        t: &GeometryTolerance,
    ) -> GeometryResult<PolygonBooleanResult> {
        polygon_boolean(
            std::slice::from_ref(self),
            std::slice::from_ref(other),
            PolygonBooleanOperation::Intersection,
            options,
            t,
        )
    }
    /// Subtract the second polygon from the first in input coordinate units under explicit
    /// coordinate, area and validation budgets.
    pub fn difference(
        &self,
        other: &Self,
        options: PolygonBooleanOptions,
        t: &GeometryTolerance,
    ) -> GeometryResult<PolygonBooleanResult> {
        polygon_boolean(
            std::slice::from_ref(self),
            std::slice::from_ref(other),
            PolygonBooleanOperation::Difference,
            options,
            t,
        )
    }
}

/// Each operand collection represents a union; overlapping clips are subtracted only once.
/// Coordinates and linear budgets use caller units; area budgets use squared units.
pub fn polygon_boolean(
    subjects: &[Polygon],
    clips: &[Polygon],
    operation: PolygonBooleanOperation,
    options: PolygonBooleanOptions,
    t: &GeometryTolerance,
) -> GeometryResult<PolygonBooleanResult> {
    options.validate(t)?;
    let count = subjects
        .iter()
        .chain(clips)
        .try_fold(0_usize, |count, polygon| {
            polygon
                .holes
                .iter()
                .chain(std::iter::once(&polygon.exterior))
                .try_fold(count, |sum, ring| {
                    sum.checked_add(ring.len())
                        .ok_or_else(|| resource("maxInputEdges"))
                })
        })?;
    if count > options.max_input_edges {
        return Err(resource("maxInputEdges"));
    }
    for polygon in subjects.iter().chain(clips) {
        polygon.validate(t)?;
    }
    let input_points: Vec<_> = subjects
        .iter()
        .chain(clips)
        .flat_map(|p| std::iter::once(&p.exterior).chain(&p.holes))
        .flatten()
        .copied()
        .collect();
    let perimeter: f64 = subjects
        .iter()
        .chain(clips)
        .map(|p| p.perimeter(t))
        .collect::<GeometryResult<Vec<_>>>()?
        .into_iter()
        .sum();
    finite(perimeter)?;
    let adapter = Adapter::new(&input_points, perimeter, options)?;
    let mut sources = Vec::with_capacity(count);
    let mut overlay: EdgeOverlay<i64, SourceSet> = EdgeOverlay::new(count);
    overlay.options = IntOverlayOptions::keep_all_points();
    overlay.data_store_mut().max_members = options.max_generated_edges.saturating_mul(16);
    let mut max_input_displacement = 0_f64;
    for (operand, polygons) in [
        (BooleanOperand::Subject, subjects),
        (BooleanOperand::Clip, clips),
    ] {
        for (polygon_index, polygon) in polygons.iter().enumerate() {
            for (ring_index, ring) in std::iter::once(&polygon.exterior)
                .chain(&polygon.holes)
                .enumerate()
            {
                let ring_ref = if ring_index == 0 {
                    PolygonRing::Exterior
                } else {
                    PolygonRing::Hole {
                        index: ring_index - 1,
                    }
                };
                let local: Vec<_> = ring.iter().map(|p| adapter.local(*p)).collect();
                let integers = ring
                    .iter()
                    .map(|p| adapter.encode(*p))
                    .collect::<GeometryResult<Vec<_>>>()?;
                let decoded = integers
                    .iter()
                    .map(|p| adapter.decode(*p))
                    .collect::<GeometryResult<Vec<_>>>()?;
                for (a, b) in ring.iter().zip(&decoded) {
                    max_input_displacement = max_input_displacement.max(a.distance(*b));
                }
                let forward = (signed_ring_area(&local)? > 0.) == (ring_index == 0);
                for index in 0..ring.len() {
                    let next = (index + 1) % ring.len();
                    if *at(&integers, index)? == *at(&integers, next)? {
                        return Err(ambiguity("edges", "Quantization collapses a source edge"));
                    }
                    let source_index = sources.len();
                    sources.push(SourceEdge {
                        reference: BooleanSourceEdge {
                            operand,
                            polygon_index,
                            ring: ring_ref,
                            edge_index: index,
                        },
                        world_line: LineSegment {
                            start: *at(ring, index)?,
                            end: *at(ring, next)?,
                        },
                        line: LineSegment {
                            start: *at(&local, index)?,
                            end: *at(&local, next)?,
                        },
                        quantized: LineSegment {
                            start: adapter.decode_local(*at(&integers, index)?),
                            end: adapter.decode_local(*at(&integers, next)?),
                        },
                    });
                    overlay.data_store_mut().sets.push(vec![source_index]);
                    overlay.data_store_mut().members += 1;
                    let (a, b) = if forward {
                        (*at(&integers, index)?, *at(&integers, next)?)
                    } else {
                        (*at(&integers, next)?, *at(&integers, index)?)
                    };
                    overlay.add_edge(
                        InputEdge {
                            a,
                            b,
                            data: SourceSet(source_index),
                        },
                        if operand == BooleanOperand::Subject {
                            ShapeType::Subject
                        } else {
                            ShapeType::Clip
                        },
                    );
                }
            }
        }
    }
    if max_input_displacement > options.max_coordinate_error {
        return Err(ambiguity(
            "maxCoordinateError",
            "Input quantization displacement exceeds the budget",
        ));
    }
    let (max_intersection_displacement, pair_checks) = preflight(&sources, &adapter, options, t)?;
    let coordinate_error_bound = max_input_displacement.max(max_intersection_displacement)
        + adapter.grid * 4. * SQRT_2
        + adapter.roundoff;
    if coordinate_error_bound > options.max_coordinate_error {
        return Err(invalid(
            GeometryReason::NumericalErrorBudgetExceeded,
            "maxCoordinateError",
            "Boolean coordinate error bound exceeds the budget",
        ));
    }
    let area_error_bound = 2. * perimeter * coordinate_error_bound
        + PI * coordinate_error_bound.powi(2) * count as f64;
    if !area_error_bound.is_finite() || area_error_bound > options.max_area_error {
        return Err(invalid(
            GeometryReason::NumericalErrorBudgetExceeded,
            "maxAreaError",
            "Boolean area error bound exceeds the budget",
        ));
    }
    let rule = match operation {
        PolygonBooleanOperation::Union => OverlayRule::Union,
        PolygonBooleanOperation::Intersection => OverlayRule::Intersect,
        PolygonBooleanOperation::Difference => OverlayRule::Difference,
    };
    let shapes = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        overlay.build_vector_shapes(rule, FillRule::NonZero)
    }))
    .map_err(|_| {
        invalid(
            GeometryReason::BooleanKernelFailure,
            "kernel",
            "Boolean kernel failed to produce a result",
        )
    })?;
    if overlay.data_store().invalid_reference {
        return Err(invalid(
            GeometryReason::BooleanKernelFailure,
            "provenance",
            "Boolean kernel referenced an invalid source set",
        ));
    }
    if overlay.data_store().exceeded {
        return Err(resource("provenance"));
    }
    let output_edge_count: usize = shapes.iter().flatten().map(Vec::len).sum();
    if output_edge_count > options.max_output_edges {
        return Err(resource("maxOutputEdges"));
    }
    let mut polygons = vec![];
    let mut boundaries = vec![];
    let mut area = 0.;
    let mut max_output_boundary_deviation = 0_f64;
    for (polygon_index, shape) in shapes.iter().enumerate() {
        let mut rings = vec![];
        for (ring_index, path) in shape.iter().enumerate() {
            let ring = if ring_index == 0 {
                PolygonRing::Exterior
            } else {
                PolygonRing::Hole {
                    index: ring_index - 1,
                }
            };
            let mut points = vec![];
            let mut provenance = vec![];
            for (edge_index, edge) in path.iter().enumerate() {
                let start = adapter.decode(edge.a)?;
                let end = adapter.decode(edge.b)?;
                points.push(start);
                let source_ids = overlay.data_store().sets.get(edge.data.0).ok_or_else(|| {
                    invalid(
                        GeometryReason::BooleanKernelFailure,
                        "provenance",
                        "Boolean source edge record is missing",
                    )
                })?;
                let mut spans = vec![];
                for id in source_ids {
                    let source = sources.get(*id).ok_or_else(|| {
                        invalid(
                            GeometryReason::BooleanKernelFailure,
                            "provenance",
                            "Invalid boolean source index",
                        )
                    })?;
                    let (a, da) = source_parameter(
                        source,
                        adapter.local(start),
                        options.max_coordinate_error,
                    )?;
                    let (b, db) =
                        source_parameter(source, adapter.local(end), options.max_coordinate_error)?;
                    let max_deviation = da.max(db);
                    max_output_boundary_deviation =
                        max_output_boundary_deviation.max(max_deviation);
                    spans.push(BooleanSourceSpan {
                        source: source.reference.clone(),
                        start_parameter: a,
                        end_parameter: b,
                        max_deviation,
                    });
                }
                if spans.is_empty() {
                    return Err(invalid(
                        GeometryReason::BooleanKernelFailure,
                        "provenance",
                        "Output edge has no source reference",
                    ));
                }
                provenance.push(BooleanEdgeProvenance {
                    edge_index,
                    sources: spans,
                });
            }
            rings.push(points);
            boundaries.push(BooleanBoundaryProvenance {
                polygon_index,
                ring,
                edges: provenance,
            });
        }
        if rings.is_empty() {
            return Err(invalid(
                GeometryReason::BooleanKernelFailure,
                "output",
                "Boolean kernel returned an empty shell",
            ));
        }
        let polygon = Polygon {
            exterior: rings.remove(0),
            holes: rings,
        };
        polygon.validate(t).map_err(|_| {
            ambiguity(
                "output",
                "Boolean output cannot form valid rings or preserves detail below the tolerance",
            )
        })?;
        area += polygon.area(t)?;
        polygons.push(polygon);
    }
    finite(area)?;
    let coordinate_error_bound = coordinate_error_bound.max(max_output_boundary_deviation);
    let arithmetic_area_error_bound =
        32. * f64::EPSILON * adapter.span * adapter.span * (count + output_edge_count) as f64;
    let area_error_bound = 2. * perimeter * coordinate_error_bound
        + PI * coordinate_error_bound.powi(2) * count as f64
        + arithmetic_area_error_bound;
    if !area_error_bound.is_finite() || area_error_bound > options.max_area_error {
        return Err(invalid(
            GeometryReason::NumericalErrorBudgetExceeded,
            "maxAreaError",
            "Output area exceeds the geometric and arithmetic error budget",
        ));
    }
    Ok(PolygonBooleanResult {
        operation,
        geometry: MultiPolygon { polygons },
        area,
        boundaries,
        diagnostics: BooleanDiagnostics {
            profile: "linear_polygon_i64_v1".into(),
            kernel: "i_overlay@9.0.0".into(),
            local_origin: adapter.origin,
            grid_size: adapter.grid,
            integer_coordinate_limit: INTEGER_LIMIT,
            max_input_displacement,
            max_intersection_displacement,
            max_output_boundary_deviation,
            coordinate_error_bound,
            area_error_bound,
            arithmetic_area_error_bound,
            input_edge_count: count,
            output_edge_count,
            pair_checks,
            regularized_area_only: true,
        },
    })
}

struct Adapter {
    origin: Point2,
    grid: f64,
    roundoff: f64,
    span: f64,
}
impl Adapter {
    fn new(
        points: &[Point2],
        perimeter: f64,
        options: PolygonBooleanOptions,
    ) -> GeometryResult<Self> {
        let bounds = if points.is_empty() {
            Bounds2 {
                min: Point2::new(0., 0.),
                max: Point2::new(0., 0.),
            }
        } else {
            Bounds2::from_points(points)?
        };
        let dx = bounds.max.x - bounds.min.x;
        let dy = bounds.max.y - bounds.min.y;
        finite(dx)?;
        finite(dy)?;
        let origin = Point2::new(bounds.min.x + dx / 2., bounds.min.y + dy / 2.);
        origin.validate()?;
        let magnitude = [bounds.min.x, bounds.max.x, bounds.min.y, bounds.max.y]
            .into_iter()
            .map(f64::abs)
            .fold(0., f64::max);
        let roundoff = (magnitude.next_up() - magnitude) * SQRT_2;
        if !roundoff.is_finite() || roundoff >= options.max_coordinate_error / 4. {
            return Err(invalid(
                GeometryReason::NumericalErrorBudgetExceeded,
                "maxCoordinateError",
                "World coordinate resolution exceeds the boolean error budget",
            ));
        }
        let target = (options.max_coordinate_error / 16.)
            .min(options.max_area_error / (64. * perimeter.max(1.)));
        let grid = 2_f64.powf(target.log2().floor());
        positive(grid, "Boolean grid")?;
        let span = dx.max(dy);
        if !((span / 2.) / grid).is_finite() || span / 2. / grid > INTEGER_LIMIT as f64 {
            return Err(invalid(
                GeometryReason::QuantizationRangeExceeded,
                "gridSize",
                "Requested error budget exceeds the safe integer coordinate range",
            ));
        }
        Ok(Self {
            origin,
            grid,
            roundoff,
            span,
        })
    }
    fn local(&self, p: Point2) -> Point2 {
        Point2::new(p.x - self.origin.x, p.y - self.origin.y)
    }
    fn encode(&self, p: Point2) -> GeometryResult<IntPoint<i64>> {
        let local = self.local(p);
        let x = (local.x / self.grid).round();
        let y = (local.y / self.grid).round();
        if !x.is_finite()
            || !y.is_finite()
            || x.abs() > INTEGER_LIMIT as f64
            || y.abs() > INTEGER_LIMIT as f64
        {
            return Err(invalid(
                GeometryReason::QuantizationRangeExceeded,
                "coordinates",
                "Integer conversion exceeds the safe range",
            ));
        }
        Ok(IntPoint::new(x as i64, y as i64))
    }
    fn decode_local(&self, p: IntPoint<i64>) -> Point2 {
        Point2::new(p.x as f64 * self.grid, p.y as f64 * self.grid)
    }
    fn decode(&self, p: IntPoint<i64>) -> GeometryResult<Point2> {
        let local = self.decode_local(p);
        let result = Point2::new(self.origin.x + local.x, self.origin.y + local.y);
        result.validate()?;
        Ok(result)
    }
}
struct SourceEdge {
    reference: BooleanSourceEdge,
    line: LineSegment,
    world_line: LineSegment,
    quantized: LineSegment,
}
#[derive(Clone, Copy, PartialEq)]
struct SourceSet(usize);
#[derive(Default)]
struct SourceStore {
    sets: Vec<Vec<usize>>,
    members: usize,
    max_members: usize,
    exceeded: bool,
    invalid_reference: bool,
}
impl OverlayEdgeData for SourceSet {
    type Store = SourceStore;
    fn merge(
        context: EdgeDataMerge<i_overlay::segm::boolean::ShapeCountBoolean, Self>,
        store: &mut SourceStore,
    ) -> Self {
        if context.lhs_data == context.rhs_data {
            return context.lhs_data;
        }
        let (Some(left), Some(right)) = (
            store.sets.get(context.lhs_data.0),
            store.sets.get(context.rhs_data.0),
        ) else {
            // The dependency trait cannot return Result. Record the failure and
            // reject the entire operation immediately after kernel extraction.
            store.invalid_reference = true;
            return context.lhs_data;
        };
        let mut merged = left.clone();
        merged.extend_from_slice(right);
        merged.sort_unstable();
        merged.dedup();
        if store.members.saturating_add(merged.len()) > store.max_members {
            store.exceeded = true;
            return context.lhs_data;
        }
        store.members += merged.len();
        let index = store.sets.len();
        store.sets.push(merged);
        Self(index)
    }
}
fn preflight(
    sources: &[SourceEdge],
    adapter: &Adapter,
    options: PolygonBooleanOptions,
    t: &GeometryTolerance,
) -> GeometryResult<(f64, usize)> {
    let numeric = 64. * f64::EPSILON * adapter.span.max(1.);
    if numeric > options.max_coordinate_error / 4. {
        return Err(invalid(
            GeometryReason::NumericalErrorBudgetExceeded,
            "maxCoordinateError",
            "Local segment comparison exceeds the error budget",
        ));
    }
    let fine = GeometryTolerance {
        linear: numeric.max(f64::MIN_POSITIVE),
        angular: 64. * f64::EPSILON,
        area: f64::MIN_POSITIVE,
    };
    let mut max_shift = 0_f64;
    let mut checks = 0;
    let mut generated = sources.len();
    let mut interior_intersections = NearPoints::new(t.linear);
    // Disjoint expanded boxes cannot produce intersections or contacts within the linear
    // tolerance. Visit remaining pairs in exhaustive order to preserve both the first error
    // and the result.
    let pairs = overlapping_box_pairs(
        &preflight_boxes(sources, options, t, numeric),
        options.max_pair_checks,
    )
    .ok_or_else(|| resource("maxPairChecks"))?;
    for (i, j) in pairs {
        let (a, b) = (at(sources, i)?, at(sources, j)?);
        {
            checks += 1;
            if checks > options.max_pair_checks {
                return Err(resource("maxPairChecks"));
            }
            let exact_relation = relation(a.world_line, b.world_line)?;
            let original = intersection_with_relation(a.line, b.line, exact_relation, &fine)
                .map_err(|_| ambiguity("input", "Source edge relation is numerically unstable"))?;
            let quantized = intersection_with_relation(
                a.quantized,
                b.quantized,
                relation(a.quantized, b.quantized)?,
                &fine,
            )
            .map_err(|_| ambiguity("gridSize", "Edge relation is unstable after quantization"))?;
            match original {
                SegmentIntersection::None if exact_relation != 0 => {
                    return Err(ambiguity(
                        "input",
                        "Numerical segment relation disagrees with the robust predicate",
                    ));
                }
                SegmentIntersection::Point(_) if exact_relation != 1 => {
                    return Err(ambiguity(
                        "input",
                        "Near segment contact cannot be resolved as an exact intersection",
                    ));
                }
                SegmentIntersection::Overlap(_) if exact_relation != 2 => {
                    return Err(ambiguity(
                        "input",
                        "Nearby supporting lines cannot be resolved as an exact overlap",
                    ));
                }
                _ => {}
            }
            for (point, line) in [
                (a.line.start, b.line),
                (a.line.end, b.line),
                (b.line.start, a.line),
                (b.line.end, a.line),
            ] {
                let on_support = orientation(line.start, line.end, point)? == 0.;
                let distance = if point == line.start || point == line.end {
                    0.
                } else {
                    line.nearest_point(point, &fine)?.distance(point)
                };
                if !on_support && distance > 0. && distance <= t.linear {
                    return Err(ambiguity(
                        "input",
                        "Distinct boundaries lie within the linear tolerance; implicit merging is not permitted",
                    ));
                }
            }
            match (original, quantized) {
                (SegmentIntersection::None, SegmentIntersection::None) => {}
                (
                    SegmentIntersection::Point(point),
                    SegmentIntersection::Point(quantized_point),
                ) => {
                    if [a.line.start, a.line.end, b.line.start, b.line.end]
                        .iter()
                        .all(|end| end.distance(point) > numeric)
                    {
                        let (near, compared) = interior_intersections.insert(point);
                        checks += compared;
                        if checks > options.max_pair_checks {
                            return Err(resource("maxPairChecks"));
                        }
                        if near {
                            return Err(ambiguity(
                                "intersections",
                                "Distinct intersection nodes may merge within the linear tolerance",
                            ));
                        }
                    }
                    max_shift = max_shift.max(point.distance(quantized_point));
                    generated = generated.saturating_add(4);
                }
                (SegmentIntersection::Overlap(a), SegmentIntersection::Overlap(b)) => {
                    let shift = (a.start.distance(b.start).max(a.end.distance(b.end)))
                        .min(a.start.distance(b.end).max(a.end.distance(b.start)));
                    max_shift = max_shift.max(shift);
                    generated = generated.saturating_add(4);
                }
                _ => {
                    return Err(ambiguity(
                        "gridSize",
                        "Quantization changes edge contact or intersection topology",
                    ));
                }
            }
            if max_shift > options.max_coordinate_error / 2. {
                return Err(ambiguity(
                    "gridSize",
                    "Quantization moves an intersection outside the error budget",
                ));
            }
            if generated > options.max_generated_edges {
                return Err(resource("maxGeneratedEdges"));
            }
        }
    }
    Ok((max_shift, checks))
}

/// Preflight box enclosing local and quantized endpoints. Padding covers the linear proximity
/// threshold, quantization displacement, local comparison roundoff and world-to-local
/// subtraction roundoff.
fn preflight_boxes(
    sources: &[SourceEdge],
    options: PolygonBooleanOptions,
    t: &GeometryTolerance,
    numeric: f64,
) -> Vec<SegmentBox> {
    let scale = sources
        .iter()
        .flat_map(|s| [s.world_line.start, s.world_line.end])
        .fold(1_f64, |scale, p| scale.max(p.x.abs()).max(p.y.abs()));
    let margin =
        t.linear + options.max_coordinate_error + 4. * numeric + 16. * f64::EPSILON * scale;
    sources
        .iter()
        .map(|s| {
            let points = [s.line.start, s.line.end, s.quantized.start, s.quantized.end];
            let (min_x, max_x, min_y, max_y) = points.iter().fold(
                (
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                ),
                |(a, b, c, d), p| (a.min(p.x), b.max(p.x), c.min(p.y), d.max(p.y)),
            );
            SegmentBox {
                min_x: min_x - margin,
                max_x: max_x + margin,
                min_y: min_y - margin,
                max_y: max_y + margin,
            }
        })
        .collect()
}

/// Grid of interior intersection nodes. Cells have width 2 * cell, so all nodes within cell
/// distance occur in the nine neighboring cells despite division roundoff. Unrepresentable
/// cell coordinates use the full list.
struct NearPoints {
    cell: f64,
    cells: std::collections::HashMap<(i64, i64), Vec<Point2>>,
    unindexed: Vec<Point2>,
    all: Vec<Point2>,
}

impl NearPoints {
    fn new(cell: f64) -> Self {
        Self {
            cell,
            cells: std::collections::HashMap::new(),
            unindexed: Vec::new(),
            all: Vec::new(),
        }
    }

    fn key(&self, point: Point2) -> Option<(i64, i64)> {
        const LIMIT: f64 = (1_i64 << 52) as f64;
        if full_scan() {
            return None;
        }
        let x = (point.x / (2. * self.cell)).floor();
        let y = (point.y / (2. * self.cell)).floor();
        (x.abs() < LIMIT && y.abs() < LIMIT).then_some((x as i64, y as i64))
    }

    /// Insert a point and return (distinct node within tolerance, comparison count). Exact
    /// duplicates are not inserted again.
    fn insert(&mut self, point: Point2) -> (bool, usize) {
        let near = |previous: &Point2| {
            let distance = previous.distance(point);
            distance > 0. && distance <= self.cell
        };
        let Some((x, y)) = self.key(point) else {
            let found = self.all.iter().any(near);
            if !found && !self.all.contains(&point) {
                self.all.push(point);
                self.unindexed.push(point);
            }
            return (found, self.all.len());
        };
        let mut compared = self.unindexed.len();
        let mut found = self.unindexed.iter().any(near);
        let mut duplicate = self.unindexed.contains(&point);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(bucket) = self.cells.get(&(x + dx, y + dy)) {
                    compared += bucket.len();
                    found |= bucket.iter().any(near);
                    duplicate |= bucket.contains(&point);
                }
            }
        }
        if !found && !duplicate {
            self.all.push(point);
            self.cells.entry((x, y)).or_default().push(point);
        }
        (found, compared)
    }
}
/// Robust predicates classify contacts. Shared endpoints and collinear overlaps reuse existing
/// endpoints instead of dividing by a small angle and adding construction error.
fn intersection_with_relation(
    a: LineSegment,
    b: LineSegment,
    kind: u8,
    tolerance: &GeometryTolerance,
) -> GeometryResult<SegmentIntersection> {
    match kind {
        0 => Ok(SegmentIntersection::None),
        1 => {
            if let Some(point) = [a.start, a.end]
                .into_iter()
                .find(|p| *p == b.start || *p == b.end)
            {
                Ok(SegmentIntersection::Point(point))
            } else {
                a.intersection(&b, tolerance)
            }
        }
        2 => {
            let use_x = (a.end.x - a.start.x).abs() >= (a.end.y - a.start.y).abs();
            let mut points = [a.start, a.end, b.start, b.end];
            points.sort_by(|a, b| {
                if use_x {
                    a.x.total_cmp(&b.x)
                } else {
                    a.y.total_cmp(&b.y)
                }
            });
            Ok(SegmentIntersection::Overlap(LineSegment {
                start: points[1],
                end: points[2],
            }))
        }
        _ => Err(ambiguity("input", "Unknown edge relation")),
    }
}

fn source_parameter(source: &SourceEdge, point: Point2, budget: f64) -> GeometryResult<(f64, f64)> {
    let direction = source
        .line
        .start
        .vector_to(source.line.end)
        .scaled(1. / source.line.length());
    let offset = source.line.start.vector_to(point);
    let along = offset.dot(direction);
    let perpendicular = offset.cross(direction).abs();
    let outside = (-along).max(along - source.line.length()).max(0.);
    let deviation = perpendicular.hypot(outside);
    finite(along)?;
    finite(deviation)?;
    if deviation > budget || along < -budget || along > source.line.length() + budget {
        return Err(ambiguity(
            "provenance",
            "Output edge lies outside the source edge error budget",
        ));
    }
    Ok(((along / source.line.length()).clamp(0., 1.), deviation))
}
fn invalid(reason: GeometryReason, field: &str, message: &str) -> GeometryError {
    GeometryError::invalid(reason, field, "polygon.boolean", message)
}
fn ambiguity(field: &str, message: &str) -> GeometryError {
    invalid(GeometryReason::BooleanAmbiguity, field, message)
}
fn resource(field: &str) -> GeometryError {
    GeometryError::unsupported(
        GeometryReason::ResourceLimit,
        field,
        "polygon.boolean",
        "Boolean resource limit exceeded",
    )
}

fn orientation(a: Point2, b: Point2, c: Point2) -> GeometryResult<f64> {
    let scale = [a.x, a.y, b.x, b.y, c.x, c.y]
        .into_iter()
        .map(f64::abs)
        .fold(0., f64::max);
    let divisor = if scale == 0. {
        1.
    } else {
        2_f64.powf(scale.log2().floor())
    };
    let points = [a, b, c].map(|p| robust::Coord {
        x: p.x / divisor,
        y: p.y / divisor,
    });
    for (i, a) in points.iter().enumerate() {
        for b in points.iter().skip(i + 1) {
            for delta in [a.x - b.x, a.y - b.y] {
                if delta != 0. && delta.abs() < 1e-100 {
                    return Err(ambiguity(
                        "input",
                        "Predicate exponent range is outside the supported numerical profile",
                    ));
                }
            }
        }
    }
    let value = robust::orient2d(points[0], points[1], points[2]);
    finite(value)?;
    Ok(value)
}
fn relation(a: LineSegment, b: LineSegment) -> GeometryResult<u8> {
    let ab = [
        orientation(a.start, a.end, b.start)?,
        orientation(a.start, a.end, b.end)?,
    ];
    let ba = [
        orientation(b.start, b.end, a.start)?,
        orientation(b.start, b.end, a.end)?,
    ];
    if ab == [0., 0.] && ba == [0., 0.] {
        let use_x = (a.end.x - a.start.x).abs() >= (a.end.y - a.start.y).abs();
        let coordinates = [a.start, a.end, b.start, b.end].map(|p| if use_x { p.x } else { p.y });
        let lo = coordinates[0]
            .min(coordinates[1])
            .max(coordinates[2].min(coordinates[3]));
        let hi = coordinates[0]
            .max(coordinates[1])
            .min(coordinates[2].max(coordinates[3]));
        return Ok(if lo < hi {
            2
        } else if lo == hi {
            1
        } else {
            0
        });
    }
    let straddles = |v: [f64; 2]| {
        v[0] == 0. || v[1] == 0. || v[0].is_sign_positive() != v[1].is_sign_positive()
    };
    Ok(if straddles(ab) && straddles(ba) { 1 } else { 0 })
}
