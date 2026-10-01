//! Native data of the entities beyond the basic set: curves, polylines and meshes,
//! faces, text, dimensions, leaders, hatches, images, viewports and the types kept as
//! `Unknown` in the model. Coordinates are exactly as stored (OCS where the spec says
//! so, noted per field).

use super::CmColor;

/// ELLIPSE (§20.4.39). Center and major axis are WCS.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Ellipse {
    /// Center (WCS).
    pub center: [f64; 3],
    /// Semi-major axis vector (WCS).
    pub major_axis: [f64; 3],
    /// Extrusion.
    pub extrusion: [f64; 3],
    /// Minor/major ratio.
    pub ratio: f64,
    /// Start parameter (radians).
    pub start: f64,
    /// End parameter (radians).
    pub end: f64,
}

/// SPLINE (§20.4.40). Points are WCS.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Spline {
    /// 1 = control points / knots, 2 = fit points only (recomputed from the R2013+
    /// flags as the files do).
    pub scenario: i32,
    /// R2013+ spline flags 1 (1 fit method, 2 CV frame, 4 closed, 8 use knot parameter).
    pub flags1: Option<i32>,
    /// R2013+ knot parameterization (0 chord, 1 square root, 2 uniform, 15 custom).
    pub knot_parameter: Option<i32>,
    /// Degree.
    pub degree: i32,
    /// Rational (scenario 1).
    pub rational: bool,
    /// Closed (scenario 1, or R2013+ flags1 bit 4).
    pub closed: bool,
    /// Periodic (scenario 1).
    pub periodic: bool,
    /// Knot tolerance (scenario 1).
    pub knot_tolerance: f64,
    /// Control point tolerance (scenario 1).
    pub control_tolerance: f64,
    /// Fit tolerance (scenario 2).
    pub fit_tolerance: f64,
    /// Start tangent (scenario 2).
    pub start_tangent: Option<[f64; 3]>,
    /// End tangent (scenario 2).
    pub end_tangent: Option<[f64; 3]>,
    /// Knots.
    pub knots: Vec<f64>,
    /// Control points.
    pub control_points: Vec<[f64; 3]>,
    /// Weights (rational splines).
    pub weights: Vec<f64>,
    /// Fit points.
    pub fit_points: Vec<[f64; 3]>,
}

/// VERTEX_2D / VERTEX_3D / VERTEX_MESH / VERTEX_PFACE (§20.4.11–14).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct PolylineVertex {
    /// Vertex flags (DXF 70).
    pub flags: u8,
    /// Position: OCS X/Y for 2D vertices (their Z is the polyline elevation), WCS otherwise.
    pub position: [f64; 3],
    /// Start width (2D).
    pub start_width: f64,
    /// End width (2D).
    pub end_width: f64,
    /// Bulge (2D).
    pub bulge: f64,
    /// Vertex id (2D, R2010+).
    pub id: Option<i32>,
    /// Curve-fit tangent direction (2D).
    pub tangent: f64,
}

/// VERTEX_PFACE_FACE (§20.4.15): 1-based vertex indices, negative = invisible edge.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct PfaceFace {
    /// Indices (0 = unused).
    pub indices: [i16; 4],
}

/// Which POLYLINE flavour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PolylineKind {
    /// POLYLINE_2D (OCS, bulges, widths).
    #[default]
    TwoD,
    /// POLYLINE_3D.
    ThreeD,
    /// POLYLINE_PFACE (polyface mesh).
    PolyFace,
    /// POLYLINE_MESH (M×N polygon mesh).
    Mesh,
}

/// POLYLINE_2D/3D/PFACE/MESH (§20.4.16, 17, 33, 34).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Polyline {
    /// Flavour.
    pub kind: PolylineKind,
    /// DXF 70 flags (2D/mesh as stored; 3D synthesized: 1 closed, 4 spline-fit, 8 3D).
    pub flags: i16,
    /// Curve / smooth surface type (DXF 75).
    pub curve_type: i16,
    /// Default start width (2D).
    pub start_width: f64,
    /// Default end width (2D).
    pub end_width: f64,
    /// Thickness (2D).
    pub thickness: f64,
    /// Elevation (2D, OCS Z).
    pub elevation: f64,
    /// Extrusion (2D).
    pub extrusion: [f64; 3],
    /// Mesh M / polyface vertex count.
    pub m: i16,
    /// Mesh N / polyface face count.
    pub n: i16,
    /// Mesh M density.
    pub m_density: i16,
    /// Mesh N density.
    pub n_density: i16,
    /// Vertex handles: owned list (R2004+) or first/last (R13–R2000).
    pub vertices: Vec<u64>,
    /// `vertices` holds first and last of a chain (R13–R2000).
    pub vertices_are_range: bool,
    /// SEQEND handle.
    pub seqend: Option<u64>,
}

/// Which face entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum FaceKind {
    /// SOLID.
    #[default]
    Solid,
    /// TRACE.
    Trace,
    /// 3DFACE.
    Face3d,
}

/// SOLID / TRACE (§20.4.35–36, corners OCS with elevation, stored in "Z" order 1,2,3,4
/// with outline 1,2,4,3) and 3DFACE (§20.4.32, WCS, outline order).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Face {
    /// Entity type.
    pub kind: FaceKind,
    /// Corners as stored.
    pub corners: [[f64; 3]; 4],
    /// Thickness (SOLID/TRACE).
    pub thickness: f64,
    /// Extrusion (SOLID/TRACE).
    pub extrusion: [f64; 3],
    /// Invisible edge flags (3DFACE).
    pub invisible_edges: i16,
}

/// MTEXT (§20.4.46). Insertion point and X direction are WCS.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct MText {
    /// Insertion point (WCS).
    pub insertion: [f64; 3],
    /// Extrusion.
    pub extrusion: [f64; 3],
    /// Text X-axis direction (WCS).
    pub x_axis: [f64; 3],
    /// Reference rectangle width.
    pub rect_width: f64,
    /// Reference rectangle height (R2007+).
    pub rect_height: Option<f64>,
    /// Text height.
    pub height: f64,
    /// Attachment (1..9).
    pub attachment: i16,
    /// Drawing direction.
    pub direction: i16,
    /// Extents height.
    pub extents_height: f64,
    /// Extents width.
    pub extents_width: f64,
    /// Text with format codes.
    pub value: String,
    /// STYLE handle.
    pub style: Option<u64>,
    /// Line spacing style (R2000+).
    pub line_spacing_style: Option<i16>,
    /// Line spacing factor (R2000+).
    pub line_spacing: Option<f64>,
    /// Background flags (R2004+).
    pub background_flags: Option<i32>,
    /// Background scale factor.
    pub background_scale: Option<f64>,
    /// Background color.
    pub background_color: Option<CmColor>,
    /// Background transparency.
    pub background_transparency: Option<i32>,
    /// R2018+: the text is annotative.
    pub annotative: Option<bool>,
    /// R2018+ column type (0 none, 1 static, 2 dynamic).
    pub column_type: Option<i16>,
    /// R2018+ column heights.
    pub column_heights: Vec<f64>,
}

/// DIMENSION_* (§20.4.22–29) and ARC_DIMENSION / LARGE_RADIAL_DIMENSION (classes).
/// Points 10–15 are WCS; 11, 12 and 16 are OCS with `elevation` as Z.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Dimension {
    /// Object type code (0x14–0x1A) or class number.
    pub type_code: u16,
    /// R2010+ version byte.
    pub version: Option<u8>,
    /// Extrusion.
    pub extrusion: [f64; 3],
    /// Text middle point (OCS X/Y).
    pub text_midpoint: [f64; 2],
    /// Elevation of the OCS points.
    pub elevation: f64,
    /// "Flags 1": bit 0 = NOT user-positioned text (DXF 70 bit 128 inverted).
    pub flags1: u8,
    /// User text (DXF 1); empty when the measurement is shown.
    pub user_text: String,
    /// Text rotation.
    pub text_rotation: f64,
    /// Horizontal direction.
    pub horizontal_direction: f64,
    /// Block insertion scale.
    pub insertion_scale: [f64; 3],
    /// Block insertion rotation.
    pub insertion_rotation: f64,
    /// Text attachment (R2000+).
    pub attachment: Option<i16>,
    /// Line spacing style (R2000+).
    pub line_spacing_style: Option<i16>,
    /// Line spacing factor (R2000+).
    pub line_spacing: Option<f64>,
    /// Actual measurement (R2000+).
    pub measurement: Option<f64>,
    /// Flip arrows (R2007+).
    pub flip_arrows: Option<[bool; 2]>,
    /// Point 12 (block insertion point, OCS X/Y).
    pub pt12: [f64; 2],
    /// Point 10 (dimension line definition point).
    pub pt10: [f64; 3],
    /// Point 13.
    pub pt13: Option<[f64; 3]>,
    /// Point 14.
    pub pt14: Option<[f64; 3]>,
    /// Point 15.
    pub pt15: Option<[f64; 3]>,
    /// Point 16 (angular 2-line arc point, OCS X/Y).
    pub pt16: Option<[f64; 2]>,
    /// Extension line rotation (linear, aligned).
    pub ext_line_rotation: Option<f64>,
    /// Dimension rotation (linear).
    pub rotation: Option<f64>,
    /// Leader length (radius, diameter).
    pub leader_length: Option<f64>,
    /// "Flags 2" (ordinate): bit 0 = X-type ordinate (DXF 70 bit 64).
    pub flags2: Option<u8>,
    /// DIMSTYLE handle.
    pub dimstyle: Option<u64>,
    /// Anonymous block handle.
    pub block: Option<u64>,
}

/// LEADER (§20.4.47). Points are WCS.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Leader {
    /// Annotation type (0 MTEXT, 1 TOLERANCE, 2 INSERT, 3 none).
    pub annotation_type: i16,
    /// Path type (0 straight, 1 spline).
    pub path_type: i16,
    /// Vertices.
    pub points: Vec<[f64; 3]>,
    /// Plane origin.
    pub origin: [f64; 3],
    /// Extrusion.
    pub extrusion: [f64; 3],
    /// X direction.
    pub x_direction: [f64; 3],
    /// Offset to block insertion point.
    pub block_offset: [f64; 3],
    /// End point projection (R14+).
    pub annotation_offset: Option<[f64; 3]>,
    /// Text box height (≤ R2007).
    pub box_height: Option<f64>,
    /// Text box width (≤ R2007).
    pub box_width: Option<f64>,
    /// Hook line on the X direction.
    pub hook_line_on_x: bool,
    /// Arrowhead on.
    pub arrowhead: bool,
    /// Associated annotation handle.
    pub annotation: Option<u64>,
    /// DIMSTYLE handle.
    pub dimstyle: Option<u64>,
}

/// One HATCH boundary edge (§20.4.75), OCS X/Y.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum HatchEdgeData {
    /// Line.
    Line {
        /// Start.
        start: [f64; 2],
        /// End.
        end: [f64; 2],
    },
    /// Circular arc; angles in radians.
    Arc {
        /// Center.
        center: [f64; 2],
        /// Radius.
        radius: f64,
        /// Start angle.
        start: f64,
        /// End angle.
        end: f64,
        /// Counter-clockwise.
        ccw: bool,
    },
    /// Elliptic arc; parameters in radians.
    Ellipse {
        /// Center.
        center: [f64; 2],
        /// Major axis end point relative to the center.
        major_axis: [f64; 2],
        /// Minor/major ratio.
        ratio: f64,
        /// Start parameter.
        start: f64,
        /// End parameter.
        end: f64,
        /// Counter-clockwise.
        ccw: bool,
    },
    /// Spline.
    Spline {
        /// Degree.
        degree: i32,
        /// Rational.
        rational: bool,
        /// Periodic.
        periodic: bool,
        /// Knots.
        knots: Vec<f64>,
        /// Control points.
        control_points: Vec<[f64; 2]>,
        /// Weights (rational).
        weights: Vec<f64>,
        /// Fit points (R2010+).
        fit_points: Vec<[f64; 2]>,
        /// Start / end tangents (R2010+ with fit points).
        tangents: Option<[[f64; 2]; 2]>,
    },
}

/// One HATCH boundary path.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct HatchPath {
    /// Path flags (1 external, 2 polyline, 4 derived, 16 outermost).
    pub flags: i32,
    /// Edges (non-polyline paths).
    pub edges: Vec<HatchEdgeData>,
    /// Polyline path: closed.
    pub closed: bool,
    /// Polyline path vertices (OCS X/Y) and bulges (empty when not stored).
    pub polyline: Vec<[f64; 2]>,
    /// Polyline bulges.
    pub bulges: Vec<f64>,
    /// Boundary object handles.
    pub boundary_objects: Vec<u64>,
}

/// One hatch pattern definition line.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct PatternLine {
    /// Angle (radians).
    pub angle: f64,
    /// Base point.
    pub base: [f64; 2],
    /// Offset.
    pub offset: [f64; 2],
    /// Dash lengths.
    pub dashes: Vec<f64>,
}

/// HATCH (§20.4.75).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Hatch {
    /// Gradient fill (R2004+): enabled flag.
    pub gradient: Option<bool>,
    /// Gradient name (R2004+).
    pub gradient_name: String,
    /// Elevation (OCS Z).
    pub elevation: f64,
    /// Extrusion.
    pub extrusion: [f64; 3],
    /// Pattern name.
    pub pattern: String,
    /// Solid fill.
    pub solid: bool,
    /// Associative.
    pub associative: bool,
    /// Boundary paths.
    pub paths: Vec<HatchPath>,
    /// Hatch style (0 odd parity, 1 outermost, 2 whole).
    pub style: i16,
    /// Pattern type (0 user, 1 predefined, 2 custom).
    pub pattern_type: i16,
    /// Pattern angle (radians).
    pub angle: f64,
    /// Pattern scale or spacing.
    pub scale: f64,
    /// Double hatch.
    pub double: bool,
    /// Pattern definition lines.
    pub lines: Vec<PatternLine>,
    /// Pixel size (derived boundaries).
    pub pixel_size: Option<f64>,
    /// Seed points (OCS X/Y).
    pub seeds: Vec<[f64; 2]>,
}

/// IMAGE / WIPEOUT (classes, §20.4.80). Points are WCS.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Image {
    /// Class version.
    pub class_version: i32,
    /// Insertion point (lower-left corner).
    pub insertion: [f64; 3],
    /// U vector (one pixel along the bottom edge).
    pub u: [f64; 3],
    /// V vector (one pixel along the left edge).
    pub v: [f64; 3],
    /// Size in pixels.
    pub size: [f64; 2],
    /// Display flags.
    pub flags: i16,
    /// Clipping on.
    pub clipping: bool,
    /// Brightness.
    pub brightness: u8,
    /// Contrast.
    pub contrast: u8,
    /// Fade.
    pub fade: u8,
    /// R2010+: clip inside (true) / outside.
    pub clip_inside: Option<bool>,
    /// Clip boundary type (1 rectangle, 2 polygon).
    pub clip_type: i16,
    /// Clip boundary vertices (pixel coordinates).
    pub clip_vertices: Vec<[f64; 2]>,
    /// IMAGEDEF handle.
    pub imagedef: Option<u64>,
    /// IMAGEDEF_REACTOR handle.
    pub reactor: Option<u64>,
}

/// IMAGEDEF (class, §20.4.81).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct ImageDef {
    /// Class version.
    pub class_version: i32,
    /// Image size in pixels.
    pub size: [f64; 2],
    /// File name as stored.
    pub file_name: String,
    /// Loaded.
    pub loaded: bool,
    /// Resolution units.
    pub units: u8,
    /// Pixel size in units.
    pub pixel_size: [f64; 2],
}

/// VIEWPORT entity (§20.4.38). Center is WCS (paper space); view center DCS.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Viewport {
    /// Center.
    pub center: [f64; 3],
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
    /// View target (R2000+).
    pub view_target: [f64; 3],
    /// View direction (R2000+).
    pub view_direction: [f64; 3],
    /// Twist angle.
    pub twist: f64,
    /// View height.
    pub view_height: f64,
    /// Lens length.
    pub lens_length: f64,
    /// Front clip plane.
    pub front_clip: f64,
    /// Back clip plane.
    pub back_clip: f64,
    /// Snap angle.
    pub snap_angle: f64,
    /// View center (DCS).
    pub view_center: [f64; 2],
    /// Status flags (R2000+).
    pub status: i32,
    /// Plot style sheet (R2000+).
    pub style_sheet: String,
    /// Frozen layer handles (R2000+).
    pub frozen_layers: Vec<u64>,
}

/// RAY / XLINE (§20.4.42–43). WCS.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct RayLine {
    /// Base point.
    pub point: [f64; 3],
    /// Direction.
    pub direction: [f64; 3],
}

/// SHAPE (§20.4.37). Insertion point OCS.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Shape {
    /// Insertion point.
    pub insertion: [f64; 3],
    /// Size.
    pub size: f64,
    /// Rotation.
    pub rotation: f64,
    /// Width factor.
    pub x_scale: f64,
    /// Oblique angle.
    pub oblique: f64,
    /// Thickness.
    pub thickness: f64,
    /// Shape number.
    pub index: i16,
    /// Extrusion.
    pub extrusion: [f64; 3],
    /// STYLE (shape file) handle.
    pub style: Option<u64>,
}

/// TOLERANCE (§20.4.49). WCS.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Tolerance {
    /// Insertion point.
    pub insertion: [f64; 3],
    /// X direction.
    pub direction: [f64; 3],
    /// Extrusion.
    pub extrusion: [f64; 3],
    /// Text with format codes.
    pub text: String,
    /// DIMSTYLE handle.
    pub dimstyle: Option<u64>,
}

/// MLINE (§20.4.50). WCS.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct MLine {
    /// Scale.
    pub scale: f64,
    /// Justification.
    pub justification: u8,
    /// Base point.
    pub base: [f64; 3],
    /// Extrusion.
    pub extrusion: [f64; 3],
    /// Open/closed flags (3 = closed).
    pub flags: i16,
    /// Number of lines per vertex.
    pub line_count: u8,
    /// Vertex positions.
    pub vertices: Vec<[f64; 3]>,
    /// MLINESTYLE handle.
    pub style: Option<u64>,
}

/// OLE2FRAME (§20.4.88): only sizes are kept.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Ole2Frame {
    /// Version.
    pub version: i16,
    /// Mode (R2000+).
    pub mode: Option<i16>,
    /// Size of the embedded OLE data in bytes.
    pub data_size: i32,
}

/// REGION / 3DSOLID / BODY (§20.4.41): the modeler data header only.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct AcisData {
    /// No modeler data.
    pub empty: bool,
    /// Format version (1 = encoded SAT blocks, 2 = plain SAT/SAB).
    pub version: i16,
    /// Total SAT bytes (version 1).
    pub sat_bytes: Option<u64>,
}
