//! Format-neutral drawing model.
//!
//! Every reader (DWG, DGN, DXF) produces a [`Document`]. The model keeps the
//! common CAD semantics typed and pushes format-specific details into
//! [`Props`] under a format prefix (`dgn.`, `dwg.`, `dxf.`), so nothing a
//! reader understood is thrown away.
//!
//! Conventions:
//! - Coordinates are in drawing units (see [`Units`]); DGN UORs are already
//!   converted to master units.
//! - **Every point and vector is in world coordinates (WCS).** Readers convert
//!   DWG/DXF object coordinates (OCS) to WCS; there is no OCS in this model.
//! - Planar entities carry a `normal` that defines their plane. Angles
//!   (arc start/end, text/insert rotation, hatch pattern angle) are radians,
//!   counter-clockwise about that normal, measured from the plane's X axis as
//!   given by the DXF arbitrary-axis algorithm (`geom_ops::arbitrary_axes`).
//!   For `normal = +Z` this is the ordinary world X axis.
//! - Ordered point lists (polyline vertices, face corners, hatch edges) are in
//!   outline order. Readers fix format quirks such as DXF `SOLID` storing
//!   corners 3 and 4 swapped.
//! - Entities reference layers, blocks, linetypes and text styles by name.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::geom::{Point3, Vec3};

/// Format-specific extra data, keyed `"<format>.<name>"` (e.g. `"dgn.level_id"`).
pub type Props = BTreeMap<String, Value>;

/// A complete drawing.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Document {
    /// Where the data came from.
    pub source: SourceInfo,
    /// Unit of all coordinates in this document.
    pub units: Units,
    /// Layers (DWG/DXF layers, DGN levels).
    pub layers: Vec<Layer>,
    /// Linetype definitions.
    pub linetypes: Vec<Linetype>,
    /// Text style definitions.
    pub text_styles: Vec<TextStyle>,
    /// Block definitions (DWG/DXF blocks, DGN shared cell definitions).
    pub blocks: Vec<Block>,
    /// Model spaces, layouts and sheets that hold the drawn entities.
    pub models: Vec<Model>,
    /// Document-level extras (header variables, file properties).
    pub props: Props,
    /// Recoverable problems met while reading. A non-empty list does not mean failure.
    pub warnings: Vec<Warning>,
}

/// Origin of a [`Document`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SourceInfo {
    /// File format family.
    pub format: Format,
    /// Format version as written in the file, e.g. `"AC1032"`, `"V8"`, `"V7"`.
    pub version: String,
    /// Producing application if recorded, e.g. `"MicroStation v8.11.9.578"`.
    pub application: Option<String>,
    /// Code page used for 8-bit strings, e.g. `"windows-1254"`.
    pub codepage: Option<String>,
}

/// File format family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// Not known (e.g. a document built in memory).
    #[default]
    Unknown,
    /// AutoCAD DWG.
    Dwg,
    /// AutoCAD DXF (ASCII or binary).
    Dxf,
    /// MicroStation DGN V7 (ISFF).
    DgnV7,
    /// MicroStation DGN V8 (OLE compound file).
    DgnV8,
    /// CityGML 2.0; GML 3.1.1 tabanlı XML uygulama şeması.
    CityGml,
}

/// Drawing units.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Units {
    /// Named unit of the coordinates.
    pub unit: LengthUnit,
    /// Size of one drawing unit in meters, when known. Authoritative over `unit`
    /// for non-standard DGN master units.
    pub meters_per_unit: Option<f64>,
}

/// Length unit. Mirrors the DWG `$INSUNITS` set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum LengthUnit {
    #[default]
    Unitless,
    Inch,
    Foot,
    Mile,
    Millimeter,
    Centimeter,
    Meter,
    Kilometer,
    Microinch,
    Mil,
    Yard,
    Angstrom,
    Nanometer,
    Micrometer,
    Decimeter,
    Decameter,
    Hectometer,
    Gigameter,
    AstronomicalUnit,
    LightYear,
    Parsec,
    UsSurveyFoot,
    /// A unit that is not in this list; see [`Units::meters_per_unit`].
    Custom,
}

/// A layer (DGN: level).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Layer {
    /// Unique name.
    pub name: String,
    /// Native identifier (DWG handle, DGN level id).
    pub id: Option<u64>,
    /// Default color of entities on this layer.
    pub color: Color,
    /// Default linetype name.
    pub linetype: Option<String>,
    /// Default lineweight.
    pub lineweight: Lineweight,
    /// Layer is on / displayed.
    pub visible: bool,
    /// Layer is frozen.
    pub frozen: bool,
    /// Layer is locked.
    pub locked: bool,
    /// Layer is plotted.
    pub plottable: bool,
    /// Free-text description.
    pub description: Option<String>,
    /// Format-specific extras.
    pub props: Props,
}

/// A linetype definition.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Linetype {
    /// Unique name.
    pub name: String,
    /// Description text.
    pub description: Option<String>,
    /// Dash pattern in drawing units: positive = dash, negative = gap, zero = dot.
    pub pattern: Vec<f64>,
    /// Format-specific extras.
    pub props: Props,
}

/// A text style definition.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TextStyle {
    /// Unique name.
    pub name: String,
    /// Font file or font name.
    pub font: Option<String>,
    /// Fixed text height, 0 when variable.
    pub height: f64,
    /// Width factor (1.0 = normal).
    pub width_factor: f64,
    /// Oblique angle in radians.
    pub oblique: f64,
    /// Format-specific extras.
    pub props: Props,
}

/// A block definition (DGN: shared cell definition).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Block {
    /// Unique name.
    pub name: String,
    /// Native identifier.
    pub id: Option<u64>,
    /// Insertion base point.
    pub base_point: Point3,
    /// Entities in block coordinates.
    pub entities: Vec<Entity>,
    /// Block is an external reference.
    pub is_xref: bool,
    /// Path of the external reference.
    pub xref_path: Option<String>,
    /// Description text.
    pub description: Option<String>,
    /// Format-specific extras.
    pub props: Props,
}

/// A space that holds drawn entities: model space, a paper-space layout or a DGN model.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Model {
    /// Display name.
    pub name: String,
    /// Native identifier.
    pub id: Option<u64>,
    /// Kind of space.
    pub kind: ModelKind,
    /// Model stores 3D geometry.
    pub is_3d: bool,
    /// Entities in drawing order.
    pub entities: Vec<Entity>,
    /// Format-specific extras.
    pub props: Props,
}

/// Kind of [`Model`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    /// Design / model space.
    #[default]
    Model,
    /// Paper-space layout (DWG/DXF).
    Layout,
    /// Sheet model (DGN).
    Sheet,
}

/// One drawn object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    /// Native identifier (DWG handle, DGN element id).
    pub id: Option<u64>,
    /// Layer name; `None` means the default layer.
    pub layer: Option<String>,
    /// Color.
    pub color: Color,
    /// Linetype name; `None` means by layer.
    pub linetype: Option<String>,
    /// Lineweight.
    pub lineweight: Lineweight,
    /// Entity is displayed.
    pub visible: bool,
    /// Geometry and type-specific data.
    pub kind: EntityKind,
    /// Attached non-graphic data: DWG attributes, DGN tags, item-type properties.
    pub attributes: Vec<Attribute>,
    /// Format-specific extras.
    pub props: Props,
    /// Original record bytes, kept only when [`crate::ReadOptions::keep_raw`] is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Raw>,
}

impl Entity {
    /// An entity with default symbology.
    pub fn new(kind: EntityKind) -> Self {
        Self {
            id: None,
            layer: None,
            color: Color::ByLayer,
            linetype: None,
            lineweight: Lineweight::ByLayer,
            visible: true,
            kind,
            attributes: Vec::new(),
            props: Props::new(),
            raw: None,
        }
    }
}

/// Geometry and type-specific data of an [`Entity`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EntityKind {
    /// A point.
    Point {
        /// Location.
        position: Point3,
    },
    /// A straight segment.
    Line {
        /// Start point.
        start: Point3,
        /// End point.
        end: Point3,
    },
    /// Connected segments, optionally with arc segments (bulge) and widths.
    /// Covers DWG LWPOLYLINE/POLYLINE and DGN line string / shape.
    Polyline {
        /// Vertices in order.
        vertices: Vec<Vertex>,
        /// Last vertex connects back to the first.
        closed: bool,
        /// Plane normal; bulge arcs turn counter-clockwise about it. `+Z` for plain 2D data
        /// and for non-planar 3D polylines (which have no bulges).
        normal: Vec3,
    },
    /// A full circle.
    Circle {
        /// Center.
        center: Point3,
        /// Radius.
        radius: f64,
        /// Plane normal.
        normal: Vec3,
    },
    /// A circular arc from `start_angle` counter-clockwise to `end_angle`.
    Arc {
        /// Center.
        center: Point3,
        /// Radius.
        radius: f64,
        /// Start angle in radians.
        start_angle: f64,
        /// End angle in radians.
        end_angle: f64,
        /// Plane normal.
        normal: Vec3,
    },
    /// An ellipse or elliptical arc.
    Ellipse {
        /// Center.
        center: Point3,
        /// Vector from the center to the end of the major axis (its length is the semi-major axis).
        major_axis: Vec3,
        /// Minor / major axis ratio, in (0, 1].
        ratio: f64,
        /// Start parameter in radians (0 for a full ellipse).
        start_param: f64,
        /// End parameter in radians (2π for a full ellipse).
        end_param: f64,
        /// Plane normal.
        normal: Vec3,
    },
    /// A B-spline / NURBS curve.
    Spline {
        /// Degree.
        degree: u32,
        /// Knot vector (may be empty when only fit points are known).
        knots: Vec<f64>,
        /// Control points.
        control_points: Vec<Point3>,
        /// Weights; empty for non-rational curves.
        weights: Vec<f64>,
        /// Fit points, if stored.
        fit_points: Vec<Point3>,
        /// Curve is closed / periodic.
        closed: bool,
    },
    /// Single-line text.
    Text {
        /// Anchor point implied by `halign`/`valign`: the baseline start for left/baseline
        /// text, otherwise the justification point (DXF/DWG second alignment point, group 11).
        /// For `Aligned`/`Fit` text: the baseline start.
        position: Point3,
        /// Baseline end for `Aligned`/`Fit` text; `None` for every other alignment.
        end_point: Option<Point3>,
        /// Character height.
        height: f64,
        /// Rotation in radians.
        rotation: f64,
        /// Width factor (1.0 = normal).
        width_factor: f64,
        /// Oblique angle in radians.
        oblique: f64,
        /// Decoded text.
        value: String,
        /// Text style name.
        style: Option<String>,
        /// Horizontal alignment.
        halign: HAlign,
        /// Vertical alignment.
        valign: VAlign,
        /// Plane normal.
        normal: Vec3,
    },
    /// Multi-line, formatted text. DGN text nodes map to this when they only hold text.
    MText {
        /// Insertion point.
        position: Point3,
        /// Character height.
        height: f64,
        /// Reference rectangle width, 0 when unconstrained.
        width: f64,
        /// Rotation in radians.
        rotation: f64,
        /// Text including format codes as stored.
        value: String,
        /// Text with format codes removed, lines separated by `\n`.
        plain: String,
        /// Text style name.
        style: Option<String>,
        /// Attachment point.
        attachment: Attachment,
        /// Line spacing factor.
        line_spacing: f64,
        /// Plane normal.
        normal: Vec3,
    },
    /// A placed block (DWG INSERT, DGN shared cell instance).
    /// Attribute values live in [`Entity::attributes`].
    Insert {
        /// Block name.
        block: String,
        /// Insertion point.
        position: Point3,
        /// Scale factors.
        scale: Vec3,
        /// Rotation in radians.
        rotation: f64,
        /// Plane normal.
        normal: Vec3,
        /// Array columns (1 = no array).
        columns: u32,
        /// Array rows (1 = no array).
        rows: u32,
        /// Column spacing.
        column_spacing: f64,
        /// Row spacing.
        row_spacing: f64,
    },
    /// A filled or patterned area.
    Hatch {
        /// Boundary loops; the first is usually the outer one.
        loops: Vec<HatchLoop>,
        /// Solid fill.
        solid: bool,
        /// Pattern name, if patterned.
        pattern: Option<String>,
        /// Pattern scale.
        pattern_scale: f64,
        /// Pattern angle in radians.
        pattern_angle: f64,
        /// Plane normal.
        normal: Vec3,
    },
    /// A dimension annotation.
    Dimension {
        /// Kind of dimension.
        dimension_type: DimensionType,
        /// Definition points in format order (see `props` for their roles).
        points: Vec<Point3>,
        /// Text middle point.
        text_position: Option<Point3>,
        /// Measured value in drawing units (or radians for angular).
        measurement: Option<f64>,
        /// Text override.
        text: Option<String>,
        /// Anonymous block holding the rendered dimension, if any.
        block: Option<String>,
        /// Dimension style name.
        style: Option<String>,
    },
    /// A planar 3- or 4-sided face (DWG SOLID/TRACE/3DFACE).
    Face {
        /// Corner points (3 or 4).
        points: Vec<Point3>,
        /// The face is filled (SOLID/TRACE) rather than a wire face.
        filled: bool,
    },
    /// Çizim birimlerinde, WCS koordinatlı düzlemsel ve delikli yüzey.
    /// Halkalar ilk noktayı son noktada tekrar ederek kapatılır.
    Polygon {
        /// Yüzeyin dış sınırı; en az dört konum içerir.
        exterior: Vec<Point3>,
        /// Dış sınırın içindeki boşlukların kapalı halkaları.
        interiors: Vec<Vec<Point3>>,
    },
    /// A leader line.
    Leader {
        /// Vertices from arrow tip to the end.
        vertices: Vec<Point3>,
        /// Draws an arrowhead at the first vertex.
        arrowhead: bool,
    },
    /// A raster image reference (DWG IMAGE, DGN raster attachment).
    Image {
        /// Image file path as stored.
        path: Option<String>,
        /// Lower-left corner.
        position: Point3,
        /// Vector along the image bottom edge, covering the full width.
        u_vector: Vec3,
        /// Vector along the image left edge, covering the full height.
        v_vector: Vec3,
        /// Pixel size, when known.
        size_px: Option<[u32; 2]>,
    },
    /// A paper-space viewport.
    Viewport {
        /// Center on the sheet.
        center: Point3,
        /// Width on the sheet.
        width: f64,
        /// Height on the sheet.
        height: f64,
        /// Model-space point shown at the center.
        view_center: Point3,
        /// Model-space height shown.
        view_height: f64,
    },
    /// A polygon mesh or polyface.
    Mesh {
        /// Vertex positions.
        vertices: Vec<Point3>,
        /// Faces as vertex indices (0-based).
        faces: Vec<Vec<u32>>,
    },
    /// Entities that belong together: DGN complex chains/shapes, cells, text nodes, groups.
    /// Children are in the same coordinate space as the group.
    Group {
        /// What kind of grouping this is.
        group_kind: GroupKind,
        /// Name (cell name, group name).
        name: Option<String>,
        /// Origin (cell origin, text node origin).
        origin: Option<Point3>,
        /// Member entities.
        children: Vec<Entity>,
    },
    /// A record the reader recognized but does not model; see `props` / `raw`.
    Unknown {
        /// Native type name or code, e.g. `"dgn.type_94"` or `"dwg.ACAD_TABLE"`.
        type_name: String,
    },
}

/// Polyline vertex.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vertex {
    /// Position.
    pub position: Point3,
    /// Bulge of the segment that starts here: tan(included angle / 4), 0 = straight.
    pub bulge: f64,
    /// Start width of the segment that starts here.
    pub start_width: f64,
    /// End width of the segment that starts here.
    pub end_width: f64,
}

impl Vertex {
    /// A straight, zero-width vertex.
    pub fn at(position: Point3) -> Self {
        Self {
            position,
            ..Self::default()
        }
    }
}

/// Horizontal text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum HAlign {
    #[default]
    Left,
    Center,
    Right,
    Aligned,
    Middle,
    Fit,
}

/// Vertical text alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum VAlign {
    #[default]
    Baseline,
    Bottom,
    Middle,
    Top,
}

/// Multi-line text attachment point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum Attachment {
    #[default]
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    MiddleCenter,
    MiddleRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

/// Dimension kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum DimensionType {
    #[default]
    Linear,
    Aligned,
    Angular,
    Angular3Point,
    Diameter,
    Radius,
    Ordinate,
    ArcLength,
    Other,
}

/// What a [`EntityKind::Group`] represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupKind {
    /// DGN complex chain (open).
    ComplexChain,
    /// DGN complex shape (closed).
    ComplexShape,
    /// DGN text node.
    TextNode,
    /// DGN cell (children inline, already placed).
    Cell,
    /// Any other grouping.
    #[default]
    Other,
}

/// One hatch boundary loop.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct HatchLoop {
    /// Edges in order.
    pub edges: Vec<HatchEdge>,
    /// Loop is the outermost boundary.
    pub external: bool,
}

/// A hatch boundary edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum HatchEdge {
    Line {
        start: Point3,
        end: Point3,
    },
    Arc {
        center: Point3,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
        ccw: bool,
    },
    Ellipse {
        center: Point3,
        major_axis: Vec3,
        ratio: f64,
        start_param: f64,
        end_param: f64,
        ccw: bool,
    },
    Spline {
        degree: u32,
        knots: Vec<f64>,
        control_points: Vec<Point3>,
        weights: Vec<f64>,
    },
    Polyline {
        vertices: Vec<Vertex>,
        closed: bool,
    },
}

/// Color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Color {
    /// Use the layer color.
    #[default]
    ByLayer,
    /// Use the color of the inserting block reference.
    ByBlock,
    /// AutoCAD Color Index 1..=255. DGN palette indices are resolved to [`Color::Rgb`]
    /// by the DGN reader; the original index goes to `props["dgn.color_index"]`.
    Aci {
        /// Index.
        index: u8,
    },
    /// True color.
    Rgb {
        /// Red.
        r: u8,
        /// Green.
        g: u8,
        /// Blue.
        b: u8,
    },
}

/// Line weight.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Lineweight {
    /// Use the layer lineweight.
    #[default]
    ByLayer,
    /// Use the block reference lineweight.
    ByBlock,
    /// Application default.
    Default,
    /// Explicit width in millimeters. DGN weight steps are converted; the original goes to props.
    Millimeters(f64),
}

/// Non-graphic data attached to an entity (DWG ATTRIB, DGN tag, item-type property).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Attribute {
    /// Tag / property name.
    pub tag: String,
    /// Value.
    pub value: Value,
    /// Owning tag set / item type / definition name.
    pub set: Option<String>,
    /// Where the attribute text is displayed, if it is displayed.
    pub position: Option<Point3>,
    /// The attribute is not displayed.
    pub invisible: bool,
    /// Layer of the attribute's own record when the format stores one (a DGN tag
    /// element has its own level). `None` uses the owning entity's layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    /// How the value text is drawn at `position`; `None` when unknown or not drawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<AttributeDisplay>,
    /// Format-specific data with no neutral field (e.g. `dgn.color_index`, `dgn.weight`).
    #[serde(default, skip_serializing_if = "Props::is_empty")]
    pub props: Props,
}

/// Text presentation of a displayed attribute value.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AttributeDisplay {
    /// Text height, in drawing units.
    pub height: f64,
    /// Character width, in drawing units (DGN stores it separately from the height).
    pub width: f64,
    /// Text style (DGN font) name; `None` leaves the choice to the writer's default.
    pub style: Option<String>,
    /// Horizontal alignment of the text about `Attribute::position`.
    pub halign: HAlign,
    /// Vertical alignment of the text about `Attribute::position`.
    pub valign: VAlign,
    /// Rotation about the drawing Z axis, in radians.
    pub rotation: f64,
}

/// A loosely typed value for attributes and props.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    /// No value.
    #[default]
    Null,
    /// Boolean.
    Bool(bool),
    /// Integer.
    Int(i64),
    /// Floating point.
    Float(f64),
    /// Text.
    Text(String),
    /// Ordered list.
    List(Vec<Value>),
    /// Binary data.
    Bytes(Vec<u8>),
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}
impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Value::Int(i64::from(v))
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Text(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Text(v.to_owned())
    }
}

/// Original bytes of a record, for lossless inspection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Raw {
    /// Native type code (DGN element type, DWG object type).
    pub type_code: u32,
    /// Record bytes exactly as stored (after decompression).
    pub bytes: Vec<u8>,
}

/// A recoverable problem found while reading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Warning {
    /// Stable machine-readable code, e.g. `"dgn.unknown_element"`.
    pub code: String,
    /// Human-readable explanation.
    pub message: String,
    /// Byte offset in the decoded stream, when meaningful.
    pub offset: Option<u64>,
    /// Native id of the affected object, when known.
    pub object: Option<u64>,
}
