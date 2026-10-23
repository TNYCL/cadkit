# Analytic geometry contracts

`cadkit-geom` is independent `f64` geometry. It depends on no other cadkit crate.
The caller defines Cartesian coordinates and units. Lengths use those units,
areas use squared units, and angles are radians, positive counterclockwise from
+x. No application identifiers, architectural semantics, rendering, or implicit
unit conversion belong to this layer.

## Numerical model and supported operations

`GeometryTolerance` budgets length, angle and area independently. Defaults
(`1e-8`, `1e-10`, `1e-12`) are numerical starting values, not physical accuracy
promises. Each must be positive and finite. Primitive arithmetic and unchecked
constructors explicitly documented as such do not apply tolerances.

- Analytic curves: line, circular arc, circle, affine ellipse, degree 1-3 Bezier,
  degree 1-3 positive-weight NURBS, and polyline.
- Length and enclosed area use analytic formulas or budgeted integration,
  independently of display tessellation. NURBS length and area are unsupported.
- Intersections distinguish isolated contacts, positive-length overlap and
  numerical ambiguity. Supported pairs are line/line, line/circle, line/arc,
  circle/circle, circle/arc and arc/arc. Other pairs return typed errors.
- Nearest-point certificates bound **distance error**, not uniqueness or the
  position error of the selected point. Equal minima can have a representative
  result. Exhausted certification budgets never produce partial success.
- Snap thresholds use model units. Screen thresholds, visibility and candidate
  priority are the caller's responsibility.
- Trim, extend and offset support lines and simple polylines. Line trim uses a
  normalized parameter; polyline trim uses path length. Positive offset is left
  of the directed path. Miter overflow is an error, not an implicit bevel.
  General-curve editing, round joins and topology-changing repair are unsupported.
- Affine transformations are not closed over every curve representation.
  Nonuniformly transformed circles become ellipses; arcs require a similarity.

Rings close implicitly; do not repeat the first vertex. Hole area is subtracted
from absolute exterior area, independently of winding. Self-intersections,
touching or intersecting holes, numerical ambiguity and exhausted budgets are
not repaired silently. `validated` and `into_validated` validate once and retain
the same tolerance for cached area and location queries. Candidate box sweeps
and y-band lookup preserve exhaustive scan order and results.

## Calibration

Similarity and affine least-squares fits use all control pairs. Similarity
preserves orientation; affine fitting can express reflection. Source coordinates
are centered before accumulation to reduce cancellation. Conditioning checks
reject coincident or collinear controls. Residuals use target coordinate units.
A fit with large residuals can be returned for diagnosis; acceptance requires
`require_max_residual`. Independent controls can be evaluated with `check_points`.
A fit does not apply itself to any external state.

## NURBS profile

The current profile supports degree 1-3, at most 4096 control points, positive
finite weights, and open clamped nondecreasing knot sequences. Weight count
matches control count; knot count is control count + degree + 1. Endpoint knots
repeat degree + 1 times, interior multiplicity does not exceed degree, and the
active domain is positive. Periodic curves are explicitly unsupported.

Public parameters normalize the active knot domain to 0-1. Parameter 0.5 is not
half the arc length. Derivatives are with respect to that normalized parameter,
one-sided at endpoints, and rejected at or numerically indistinguishable from
an interior C0 knot. Bounds are a conservative positive control enclosure, not
tight extrema.

Tessellation extracts rational Bezier spans and subdivides using outward-rounded
interval arithmetic, including subnormal values. Associate control P_i with
chord point L_i=(1-i/p)A+(i/p)B. Positive rational Bernstein weights combine both
sequences with identical convex weights, so the largest control-to-chord
separation bounds the curve-to-chord separation. The continuous chord mapping
reaches both endpoints, giving a two-sided Hausdorff certificate. The L1 interval
bound also bounds Euclidean distance; dense samples are regression evidence,
not a substitute for certification.

Nearest search bounds the original curve globally. Coordinate enclosure and
arithmetic error are included in the distance certificate. Default tessellation
caps are 262144 segments, 262144 subdivisions and depth 48; supported option
limits are 1000000 segments, 1000000 subdivisions and depth 64. Nearest defaults
to 262144 iterations, with a supported maximum of 1000000; the NURBS search also
caps subdivision depth at 48 and queued candidates at 262144. Budgets are not
relaxed to obtain success. NURBS intersections and editing remain unsupported.

## Linear polygon boolean profile

Profile identifier: `linear_polygon_i64_v1`. Implementation: `src/boolean.rs`.
`polygon_boolean` computes union, intersection or difference. Each operand's
polygon collection means its union: duplicated or overlapping clips do not
subtract twice. Results include `MultiPolygon`, area, diagnostics and boundary
provenance. Empty output is valid zero area. Point or edge contacts alone do not
produce area components (`regularizedAreaOnly: true`); this API is not a contact
catalog.

Inputs pass `Polygon::validate`. Winding is normalized only for the overlay;
source order and edge references remain intact. The input geometry is not
rewritten. Dependencies are pinned:

- `i_overlay =9.0.0`: integer `EdgeOverlay<i64, SourceSet>`, `NonZero` fill,
  collinear input/output points retained, and zero minimum output area filter.
- `robust =1.2.0`: adaptive world-coordinate `orient2d`, preventing local-origin
  cancellation from disguising a small gap as coincidence.

Edge data transports source sets through splits, reversals and merges;
provenance is not reconstructed using a nearest-edge guess. The dependency's
integer range is [-2^62, 2^62-1]; this adapter deliberately restricts coordinates
to [-2^50, 2^50], preserving exact integer-to-f64 conversion and multiplication
headroom. Automatic floating-point scaling is not used.

Vector extraction differs from the dependency's ordinary OGC polygon extraction.
This profile claims no general OGC validity. Every output polygon is validated
again; touching holes, self-intersections or preserved detail below tolerance
produce `BOOLEAN_AMBIGUITY`, not repair.

### Local grid and error budget

The origin is the center of combined input bounds. Grid spacing is a power of
two no larger than either coordinate budget / 16 or area budget / (64 * total
input perimeter). A grid that cannot fit the safe integer range gives
`QUANTIZATION_RANGE_EXCEEDED`; the tolerance is never enlarged to fit it.

`maxCoordinateError` must not exceed `GeometryTolerance.linear`.
`maxAreaError` budgets the total operation independently from
`GeometryTolerance.area`, which is the minimum valid ring/component area.
Defaults are 2.5e-9 coordinate units and 1e-6 squared units. Callers needing
other physical budgets must supply explicit options or `with_error_budget`.

Input encode/decode displacement is measured. Preflight compares robust world
relationships, local f64 relationships and quantized relationships. Shared
endpoints and collinear overlaps reuse existing endpoints rather than divide
by a small angle. General interior intersection construction is separately
budgeted. Near-coincidence, potentially merging nodes, changed contact topology
or excessive displacement is an error. Robust predicates also enforce a safe
exponent profile: nonzero normalized coordinate differences below 1e-100 are
rejected as ambiguous.

The coordinate error bound combines the larger measured input/intersection
shift, 4*sqrt(2)*grid, and world-coordinate roundoff. Output endpoints are
projected back to source edges, checking both normal deviation and overshoot
past source endpoints. This bounds linear boundary deviation; it does not claim
exact intersection construction. `maxIntersectionDisplacement` reports measured
preflight displacement between representations.

For total input perimeter P, coordinate bound delta and input edge count N,
the area boundary-tube allowance is 2*P*delta + pi*N*delta^2. The arithmetic
allowance is 32*f64::EPSILON*span^2*(input edges + output edges). Success requires
the sum to fit `maxAreaError`; the arithmetic part is also reported separately.
Neither coarser grids nor deletion of small components can manufacture success.

### Provenance

Each output polygon/ring/edge references its contributing input operand,
original polygon index, exterior or original hole index, original edge index,
directed start/end edge parameters, and measured maximum deviation. Reversal
can give decreasing parameter intervals. Coincident edges retain every source.
These indices reference the current call only; callers own any external identity
mapping. The API computes a combined subtraction, not invented per-clip shares.

### Resource limits and failures

Default limits are 8192 input edges, 65536 output edges, 1000000 pair checks and
131072 generated edges. The supported maxima are 16384 input edges, 262144
output/generated edges and 10000000 pair checks. Provenance set membership is
bounded by 16 times the generated-edge cap. All limits apply together.

Preflight examines overlapping expanded boxes in exhaustive order. Pair counts
include candidate pairs and interior intersection-node comparisons, not the
square of the input edge count. A conservative generated-edge estimate and
output/provenance limits are checked independently. Candidate filtering is
compared with exhaustive scanning in unit tests.

`RESOURCE_LIMIT`, `BOOLEAN_AMBIGUITY` and `NUMERICAL_ERROR_BUDGET_EXCEEDED` identify
resource, topology and numerical budget failures. An unexpected dependency
unwind becomes `BOOLEAN_KERNEL_FAILURE`. Invalid source-set references recorded
inside the dependency callback also reject the entire result with that code.
Internal checked accesses use existing `INVALID_TOPOLOGY` when an invariant is
missing. No partial result is reported as success. Process aborts and OOM are
outside unwind recovery guarantees.

### Zero-width slicing and contact normalization

`polygon_slice` uses the same integer adapter and `IntSlice::slice_by_paths`.
Separators consume no area; an open end does not close a region. Input/output
edge budgets, topology, total area and source-boundary deviation are checked.
Selecting one resulting component belongs to the caller.

Roundoff can separate identical analytic contacts. `normalize_contacts` only
joins contacts within an explicit caller budget, on mutable computation copies:

- Vertices within budget map to the first encountered vertex. Supply geometry
  to preserve first.
- Endpoints on other edges become nodes; well-conditioned crossings are
  constructed once and shared across participating edges.
- Shallow-angle constructions are certified with robust distances. A candidate
  must lie within half the budget of both supporting lines and inside both
  padded segment intervals. Uncertified intersections remain errors.
- Gaps larger than budget remain open. Default boolean input/generated-node
  limits apply; exceeding them yields `RESOURCE_LIMIT`.

`contact_roundoff_budget` returns four ulps of the largest absolute coordinate,
using a minimum scale of 1. It is a roundoff estimate, not a physical tolerance.
`normalize_contacts` deduplicates nodes within budget. The spacing variant also
rejects insertion too near an existing node or endpoint; using `spacing=linear`
prevents inserted below-tolerance edges. An unresolved contact remains subject
to the boolean kernel's explicit decision. The summary reports budget, moved
vertices, inserted nodes and robustly measured maximum boundary displacement.

## Compatibility and verification boundaries

Public names, serde field names, tags, defaults and strict unknown-field rejection
are retained from the original internal library. JSON Schema generation is an
optional `schemars` feature; serde is unconditional. English diagnostic messages
and schema descriptions intentionally replace the original language. Structural
schemas, reason codes and numerical results are the compatibility contract.

Synthetic tests cover rectangles, holes, nested islands, disconnected results,
rational intersections, winding, translations, duplicate operands, invalid
inputs, budget exhaustion, calibration and NURBS certificates. For example,
subtracting areas 16 and 20 with shared area 6 from area 100 leaves 70.
General polygon repair, curved booleans, mesh booleans, OGC certification and
external application acceptance are outside this evidence. Native GPU, format
reader/writer integration and downstream applications require separate checks.
