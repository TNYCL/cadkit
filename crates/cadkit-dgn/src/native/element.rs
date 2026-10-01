//! Decoded element data shared by V7 and V8.
//!
//! Coordinates stay in the file's native storage units (UOR, "units of resolution") as
//! `f64`; the mapping layer converts them to master units. Angles are radians.

use crate::native::linkage::Linkage;
use crate::text::StringEncoding;

/// A point in storage units (UOR). 2D elements have `z = 0`.
pub type UorPoint = [f64; 3];

/// Common header fields of an element.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ElementHeader {
    /// Element type number (3 = line, 17 = text, ...).
    pub type_code: u16,
    /// Level: V8 level id, V7 level number 1..=63 (0 for non-graphic records).
    pub level: u32,
    /// V8 element id (unique per file). `None` for V7.
    pub id: Option<u64>,
    /// The element starts a complex group (V8 role bit `0x2000_0000`).
    pub complex_header: bool,
    /// The element belongs to a complex group (V8 `0x4000_0000`, V7 complex bit).
    pub complex_component: bool,
    /// V7 deleted bit.
    pub deleted: bool,
    /// Element stores 3D geometry (V8 flag `0x0800`, V7 file dimension).
    pub is_3d: bool,
    /// Element carries the standard display header (symbology, range).
    pub has_display_header: bool,
    /// Graphic group number.
    pub graphic_group: u32,
    /// Property flags: V8 u32 at `0x28`, V7 properties word. `0x8000` = hole (shapes).
    pub properties: u32,
    /// Line style number (0..=7 standard, larger values are custom styles).
    pub style: u32,
    /// Line weight step.
    pub weight: u32,
    /// Color index (0..=255 palette, larger values are V8 extended colors).
    pub color: u32,
    /// Range low / high corner in UOR, when stored.
    pub range: Option<[UorPoint; 2]>,
    /// V8: high 16 bits of the type word (role and other flags); V7: 0.
    pub type_flags: u16,
    /// V8: f64 at `0x18`, a last-modified time in milliseconds since 1970 (observed).
    pub modified_ms: Option<f64>,
}

/// Orientation of a planar element.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rotation {
    /// No orientation stored (identity).
    None,
    /// Rotation about +Z, radians, counter-clockwise.
    Angle(f64),
    /// Quaternion `(w, x, y, z)` as stored (normalized when converted).
    Quaternion([f64; 4]),
    /// Row-major 3x3 matrix mapping local to world (`world = M * local`); may include scale.
    Matrix([f64; 9]),
}

impl Rotation {
    /// Row-major 3x3 matrix, `world = M * local`.
    ///
    /// Quaternions follow the convention of GDAL dgnlib (`DGNQuaternionToMatrix` with the
    /// writer `DGNRotationToQuaternion`): a stored `(w, 0, 0, z)` with `z = sin(-θ/2)`
    /// yields a counter-clockwise rotation by `θ`.
    pub fn matrix(&self) -> [f64; 9] {
        match *self {
            Rotation::None => [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            Rotation::Angle(a) => {
                let (s, c) = a.sin_cos();
                [c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0]
            }
            Rotation::Quaternion([w, x, y, z]) => {
                let n = (w * w + x * x + y * y + z * z).sqrt();
                if !n.is_finite() || n < 1e-12 {
                    return Rotation::None.matrix();
                }
                let (w, x, y, z) = (w / n, x / n, y / n, z / n);
                [
                    x * x - y * y - z * z + w * w,
                    2.0 * (z * w + x * y),
                    2.0 * (x * z - y * w),
                    2.0 * (x * y - z * w),
                    -x * x + y * y - z * z + w * w,
                    2.0 * (x * w + y * z),
                    2.0 * (x * z + y * w),
                    2.0 * (y * z - x * w),
                    -x * x - y * y + z * z + w * w,
                ]
            }
            Rotation::Matrix(m) => m,
        }
    }
}

/// Text element data (type 17).
#[derive(Debug, Clone, PartialEq)]
pub struct TextData {
    /// Font number (resolved through the font table).
    pub font: u32,
    /// Justification code (0 = left-top ... 14 = right-bottom).
    pub justification: u16,
    /// Character width in UOR.
    pub width: f64,
    /// Character height in UOR.
    pub height: f64,
    /// Orientation.
    pub rotation: Rotation,
    /// Insertion point in UOR.
    pub origin: UorPoint,
    /// Decoded text.
    pub text: String,
    /// How the text was stored.
    pub encoding: StringEncoding,
    /// Text bytes as stored.
    pub raw_text: Vec<u8>,
    /// Length of the text along its baseline in UOR, when the file stores it (V8 `0x80`)
    /// or it can be derived from the range (unrotated V7 text). `None`: unknown.
    pub measured_length: Option<f64>,
}

/// Text node header data (type 7).
#[derive(Debug, Clone, PartialEq)]
pub struct TextNodeData {
    /// Number of component elements.
    pub children: u32,
    /// Node number.
    pub node_number: u32,
    /// Font number.
    pub font: u32,
    /// Justification code.
    pub justification: u16,
    /// Line spacing in UOR.
    pub line_spacing: f64,
    /// Character width in UOR.
    pub width: f64,
    /// Character height in UOR.
    pub height: f64,
    /// Orientation.
    pub rotation: Rotation,
    /// Origin in UOR.
    pub origin: UorPoint,
}

/// Cell header, shared cell definition or shared cell instance data.
#[derive(Debug, Clone, PartialEq)]
pub struct CellData {
    /// Cell name, when stored.
    pub name: Option<String>,
    /// Description, when stored.
    pub description: Option<String>,
    /// Declared number of components (V8 headers).
    pub children: u32,
    /// Range low / high corner in UOR, when stored.
    pub range: Option<[UorPoint; 2]>,
    /// Local-to-world matrix (row-major, includes scale).
    pub matrix: [f64; 9],
    /// Origin in UOR.
    pub origin: UorPoint,
}

/// A tag value.
#[derive(Debug, Clone, PartialEq)]
pub enum TagValue {
    /// Text value.
    Text(String),
    /// 32-bit integer.
    Int(i64),
    /// Double.
    Float(f64),
    /// Binary or unknown-typed value.
    Binary(Vec<u8>),
}

/// Tag element data (type 37).
#[derive(Debug, Clone, PartialEq)]
pub struct TagData {
    /// V8: element id of the tag set definition (type 39).
    pub set_id: Option<u64>,
    /// V7: tag set number.
    pub set_number: Option<u32>,
    /// Tag number within its set.
    pub tag_index: u16,
    /// Value type code (1 text, 3 integer, 4 double, 5 binary).
    pub value_type: u16,
    /// Value.
    pub value: TagValue,
    /// V8: element id of the tagged element; V7: association id of the tagged element.
    pub target: Option<u64>,
    /// Display origin in UOR.
    pub origin: UorPoint,
    /// Display offset from the origin in UOR.
    pub offset: UorPoint,
    /// Text character width / height multipliers in UOR.
    pub size: [f64; 2],
}

/// One tag definition in a tag set.
#[derive(Debug, Clone, PartialEq)]
pub struct TagDef {
    /// Tag number referenced by tag elements.
    pub id: u16,
    /// Tag name.
    pub name: String,
    /// Prompt text.
    pub prompt: String,
    /// Value type code.
    pub value_type: u16,
    /// Default value.
    pub default: TagValue,
    /// The 5 bytes between the value type and the default, as stored (observed: `u16`
    /// default length or 0, `u16` value type or 0, flag byte; not interpreted).
    pub flags: [u8; 5],
}

/// Tag set definition data (V8 type 39, V7 type 66 on level 24).
#[derive(Debug, Clone, PartialEq)]
pub struct TagSetData {
    /// Set name.
    pub name: Option<String>,
    /// V7 set number.
    pub number: Option<u32>,
    /// Tag definitions.
    pub tags: Vec<TagDef>,
}

/// V8 level table entry (type 95 in table 1).
#[derive(Debug, Clone, PartialEq)]
pub struct LevelData {
    /// Level id referenced by elements.
    pub id: u32,
    /// Level name.
    pub name: Option<String>,
    /// Description.
    pub description: Option<String>,
    /// Parent level id (`0xffff_ffff` = none).
    pub parent: u32,
    /// Raw flags word at `0x28` (display / freeze / lock bits not yet identified).
    pub flags: u32,
}

/// V7 design file header (type 9, "TCB").
#[derive(Debug, Clone, PartialEq)]
pub struct TcbData {
    /// The design is 3D.
    pub is_3d: bool,
    /// Sub-units per master unit.
    pub subunits_per_master: i32,
    /// UOR per sub-unit.
    pub uor_per_subunit: i32,
    /// Master unit label.
    pub master_label: String,
    /// Sub-unit label.
    pub sub_label: String,
    /// Global origin in UOR.
    pub origin: UorPoint,
}

/// Type-specific decoded data.
#[derive(Debug, Clone, PartialEq)]
pub enum ElementData {
    /// Type 3.
    Line {
        /// Start point.
        start: UorPoint,
        /// End point.
        end: UorPoint,
    },
    /// Type 4.
    LineString {
        /// Vertices.
        points: Vec<UorPoint>,
    },
    /// Type 6 (closed).
    Shape {
        /// Vertices (first == last for a closed outline).
        points: Vec<UorPoint>,
    },
    /// Type 11: interpolated curve; the first two and last two points only set end tangents.
    Curve {
        /// Stored points.
        points: Vec<UorPoint>,
    },
    /// Type 22: disconnected points with per-point orientation quaternions.
    PointString {
        /// Points.
        points: Vec<UorPoint>,
        /// Orientation per point (V8: 4 doubles each).
        orientations: Vec<[f64; 4]>,
    },
    /// Type 21: B-spline poles.
    BSplinePoles {
        /// Poles.
        points: Vec<UorPoint>,
    },
    /// Type 26: B-spline knots (interior knots).
    BSplineKnots {
        /// Knot values.
        values: Vec<f64>,
    },
    /// Type 28: B-spline weights.
    BSplineWeights {
        /// Weights.
        values: Vec<f64>,
    },
    /// Type 27: B-spline curve header.
    BSplineCurve {
        /// Declared components (V8).
        children: u32,
        /// Order (degree + 1).
        order: u8,
        /// Property bits: `0x10` curve display, `0x20` polygon display, `0x40` rational,
        /// `0x80` closed.
        flags: u8,
        /// Curve type code.
        curve_type: u8,
        /// The curve is closed (periodic).
        closed: bool,
        /// Declared pole count.
        num_poles: u32,
        /// Declared knot count (0 = uniform knots, not stored).
        num_knots: u32,
    },
    /// Type 15.
    Ellipse {
        /// Primary semi-axis in UOR (along the local X axis).
        primary: f64,
        /// Secondary semi-axis in UOR (along the local Y axis).
        secondary: f64,
        /// Orientation.
        rotation: Rotation,
        /// Center in UOR.
        center: UorPoint,
    },
    /// Type 16.
    Arc {
        /// Start angle, radians from the local X axis.
        start: f64,
        /// Signed sweep, radians (negative = clockwise).
        sweep: f64,
        /// Primary semi-axis in UOR.
        primary: f64,
        /// Secondary semi-axis in UOR.
        secondary: f64,
        /// Orientation.
        rotation: Rotation,
        /// Center in UOR.
        center: UorPoint,
    },
    /// Type 17.
    Text(TextData),
    /// Type 7.
    TextNode(TextNodeData),
    /// Types 12, 14, 18, 19 and other complex headers with a component count.
    Complex {
        /// Declared component count.
        children: u32,
    },
    /// Type 2.
    Cell(CellData),
    /// Type 34.
    SharedCellDefinition(CellData),
    /// Type 35.
    SharedCellInstance(CellData),
    /// Type 37.
    Tag(TagData),
    /// Tag set definition.
    TagSet(TagSetData),
    /// V8 type 94 raster frame.
    RasterFrame {
        /// Row-major 4x4 pixel-to-UOR matrix as stored (translation in the last column).
        matrix: [f64; 16],
        /// Opposite (upper-right) corner of the frame extent in UOR (`0x108`, `0x110`).
        corner: Option<[f64; 2]>,
    },
    /// V8 type 90 raster reference (control element).
    RasterReference {
        /// Stored file name.
        file_name: Option<String>,
        /// Stored full path / URL.
        full_path: Option<String>,
    },
    /// Color table (V7 type 5 on level 1, V8 type 5).
    ColorTable {
        /// 256 RGB entries.
        colors: Vec<[u8; 3]>,
    },
    /// V7 design file header.
    Tcb(TcbData),
    /// V8 table header (type 96).
    TableHeader {
        /// Table number (1 = levels, 2 = fonts, ...), from the level slot at `0x0c`.
        table: u32,
        /// Declared entry count.
        children: u32,
    },
    /// V8 level table entry.
    Level(LevelData),
    /// V8 font table entry.
    Font {
        /// Font number referenced by text.
        number: u32,
        /// Font name.
        name: String,
    },
    /// Not decoded; see the raw bytes.
    Unknown,
}

impl ElementData {
    /// Declared number of components for complex headers.
    pub fn child_count(&self) -> Option<u32> {
        match self {
            ElementData::BSplineCurve { children, .. }
            | ElementData::Complex { children }
            | ElementData::TableHeader { children, .. } => Some(*children),
            ElementData::TextNode(t) => Some(t.children),
            ElementData::Cell(c) | ElementData::SharedCellDefinition(c) => Some(c.children),
            _ => None,
        }
    }
}

/// A decoded element.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    /// Common header.
    pub header: ElementHeader,
    /// Type-specific data.
    pub data: ElementData,
    /// Attribute linkages.
    pub linkages: Vec<Linkage>,
    /// Why the body could not be (fully) decoded, if so.
    pub problem: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f64; 9], b: [f64; 9]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-12)
    }

    #[test]
    fn quaternion_convention_matches_planar_rotation() {
        let theta: f64 = 0.7;
        // GDAL dgnlib writes rotation θ as (cos(-θ/2), 0, 0, sin(-θ/2)).
        let q = Rotation::Quaternion([(-theta / 2.0).cos(), 0.0, 0.0, (-theta / 2.0).sin()]);
        assert!(close(q.matrix(), Rotation::Angle(theta).matrix()));
        assert!(close(
            Rotation::Quaternion([1.0, 0.0, 0.0, 0.0]).matrix(),
            Rotation::None.matrix()
        ));
        assert!(close(
            Rotation::Quaternion([0.0; 4]).matrix(),
            Rotation::None.matrix()
        ));
    }
}
