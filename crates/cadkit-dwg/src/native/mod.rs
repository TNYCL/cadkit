//! Native, lossless view of a DWG file: version, header variables, classes, the
//! object map and every decoded object with its common data, type-specific data and
//! raw bytes.
//!
//! Coordinates here are exactly as stored: entity points of planar entities
//! (CIRCLE, ARC, TEXT, LWPOLYLINE, …) are in the entity's object coordinate system
//! (OCS) defined by its extrusion vector; LINE and POINT are in WCS. The mapping to
//! [`cadkit_core::Document`] converts everything to WCS.

use std::collections::{BTreeMap, HashMap};

use cadkit_core::{ReadOptions, Result, Value, Warning};

mod entities;

pub use crate::bits::{BitReader, HandleRef};
pub use crate::version::DwgVersion;
pub use entities::*;

/// Reads a file into the native model (all objects, raw bytes kept).
pub fn read_native(bytes: &[u8], options: &ReadOptions) -> Result<DwgFile> {
    crate::reader::read_file(bytes, options, true)
}

/// A decoded DWG file.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DwgFile {
    /// Format version.
    pub version: DwgVersion,
    /// Maintenance release byte of the file header (offset 0x0B).
    pub maintenance_version: u8,
    /// `$DWGCODEPAGE` index from the file header (e.g. 30 = `ANSI_1252`).
    pub codepage: u16,
    /// WHATWG name of the encoding used for 8-bit strings (pre-R2007 files).
    pub encoding: &'static str,
    /// Header variables.
    pub header: HeaderVars,
    /// `MEASUREMENT` from the template section (0 = English, 1 = Metric), when present.
    pub measurement: Option<u16>,
    /// Custom classes; class number `n` describes object type `n`.
    pub classes: Vec<DwgClass>,
    /// Class number → position in `classes`, for [`DwgFile::class`].
    pub(crate) class_index: HashMap<u16, usize>,
    /// The classes section CRC and end sentinel matched, when they could be checked.
    pub classes_crc_ok: Option<bool>,
    /// Object map: handle → byte offset of the object in the object stream
    /// (absolute file offset for R13–R15, offset in AcDb:AcDbObjects for R2004+).
    pub handle_map: BTreeMap<u64, u64>,
    /// Decoded objects by handle.
    pub objects: BTreeMap<u64, DwgObject>,
    /// Names of the sections present (R2004+).
    pub section_names: Vec<String>,
    /// Recoverable problems.
    pub warnings: Vec<Warning>,
}

impl DwgFile {
    /// The class that describes object type `type_code` (≥ 500), if any. Uses the
    /// index built when the file was read (the first class with a number wins); a
    /// scan is only needed when `classes` was edited since.
    pub fn class(&self, type_code: u16) -> Option<&DwgClass> {
        match self.class_index.get(&type_code) {
            Some(&i) => self
                .classes
                .get(i)
                .filter(|c| c.number == type_code)
                .or_else(|| self.classes.iter().find(|c| c.number == type_code)),
            None if self.class_index.len() != self.classes.len() => {
                self.classes.iter().find(|c| c.number == type_code)
            }
            None => None,
        }
    }
}

/// Classes by number, so that type lookups do not scan the class list per object.
#[derive(Debug, Clone)]
pub(crate) struct ClassTable<'a> {
    classes: &'a [DwgClass],
    index: HashMap<u16, usize>,
}

impl<'a> ClassTable<'a> {
    /// Indexes `classes`; the first class with a given number wins.
    pub(crate) fn new(classes: &'a [DwgClass]) -> Self {
        let mut index = HashMap::with_capacity(classes.len());
        for (i, c) in classes.iter().enumerate() {
            index.entry(c.number).or_insert(i);
        }
        Self { classes, index }
    }

    /// The class for type code `number`.
    pub(crate) fn get(&self, number: u16) -> Option<&'a DwgClass> {
        self.index.get(&number).and_then(|&i| self.classes.get(i))
    }

    /// The number → position index, kept by [`DwgFile`].
    pub(crate) fn into_index(self) -> HashMap<u16, usize> {
        self.index
    }
}

/// Header variables (spec chapter 9), in file order.
///
/// Every decoded variable is kept under its spec name (e.g. `"INSUNITS"`,
/// `"EXTMIN"`); points are lists of floats, colors are integers (raw index) or
/// lists, handles are integers. Unknown fields are not stored.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct HeaderVars {
    /// Variables in file order.
    pub vars: Vec<(String, Value)>,
    /// Handle-valued variables (control objects, dictionaries, current settings).
    pub handles: BTreeMap<String, u64>,
    /// All variables up to the end of the section were decoded.
    pub complete: bool,
    /// The section CRC matched, when it could be checked.
    pub crc_ok: Option<bool>,
}

impl HeaderVars {
    /// Looks up a variable by spec name.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.vars.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    /// Looks up a handle-valued variable (0 handles are reported as `None`).
    pub fn handle(&self, name: &str) -> Option<u64> {
        self.handles.get(name).copied().filter(|&h| h != 0)
    }

    /// An integer variable.
    pub fn int(&self, name: &str) -> Option<i64> {
        match self.get(name)? {
            Value::Int(v) => Some(*v),
            Value::Bool(b) => Some(i64::from(*b)),
            _ => None,
        }
    }

    /// A 3D point variable.
    pub fn point3(&self, name: &str) -> Option<[f64; 3]> {
        match self.get(name)? {
            Value::List(items) => match items.as_slice() {
                [Value::Float(x), Value::Float(y), Value::Float(z)] => Some([*x, *y, *z]),
                _ => None,
            },
            _ => None,
        }
    }
}

/// One custom class (spec chapter 10).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct DwgClass {
    /// Class number = object type code (≥ 500).
    pub number: u16,
    /// Proxy flags (R14: version).
    pub proxy_flags: u16,
    /// Application name.
    pub app_name: String,
    /// C++ class name.
    pub cpp_class_name: String,
    /// DXF record name, e.g. `"LWPOLYLINE"`.
    pub dxf_name: String,
    /// Was a zombie.
    pub was_zombie: bool,
    /// 0x1F2 for entity classes, 0x1F3 for object classes.
    pub item_class_id: u16,
    /// Number of instances (R2004+).
    pub instance_count: Option<i32>,
    /// DWG version of the class (R2004+).
    pub dwg_version: Option<i32>,
    /// Maintenance version of the class (R2004+).
    pub maintenance_version: Option<i32>,
}

impl DwgClass {
    /// Objects of this class are entities.
    pub fn is_entity(&self) -> bool {
        self.item_class_id == 0x1F2
    }
}

/// One decoded object.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DwgObject {
    /// Object handle.
    pub handle: u64,
    /// Object type code (fixed codes < 500, class numbers ≥ 500).
    pub type_code: u16,
    /// Type name: the spec name for fixed codes (`"LINE"`, `"BLOCK_HEADER"`), the class
    /// DXF name otherwise, or `"TYPE_<code>"` when unknown.
    pub type_name: String,
    /// The object is an entity (has common entity data).
    pub is_entity: bool,
    /// Offset of the object in the object stream (see [`DwgFile::handle_map`]).
    pub offset: u64,
    /// Object size in bytes (the `MS` value), excluding size prefix and CRC.
    pub size: u64,
    /// The object CRC matched, when it could be checked.
    pub crc_ok: Option<bool>,
    /// Common object / entity data.
    pub common: CommonData,
    /// Type-specific data.
    pub data: ObjectData,
    /// The `size` object bytes (starting at the type field), when kept.
    pub raw: Vec<u8>,
    /// Bit position in `raw` where the handle stream starts.
    pub handle_stream_bit: u64,
    /// Bit range in `raw` of the R2007+ string stream, when present.
    pub string_stream: Option<(u64, u64)>,
    /// Why type-specific decoding failed, if it did.
    pub error: Option<String>,
    /// Recoverable problems the type decoder found (e.g. a clamped value); the reader
    /// also reports them in [`DwgFile::warnings`].
    pub warnings: Vec<Warning>,
    /// Main-stream bits a successful type decoder left unread. From R2000 the data
    /// size is exact, so a complete decoder leaves 0 (1 in R2007+ objects without a
    /// string stream: the "string stream present" flag). A tool for decoder authors.
    pub data_bits_left: Option<u64>,
    /// Handle-stream bits left after a successful decode (a complete decoder leaves
    /// only the padding to the byte boundary, i.e. fewer than 8).
    pub handle_bits_left: Option<u64>,
}

/// Common data of every object (spec §20.1, §20.4.1).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct CommonData {
    /// Extended entity data blocks.
    pub eed: Vec<EedBlock>,
    /// Owner handle (absent for entities with entity mode 1/2 — owned by paper/model space).
    pub owner: Option<u64>,
    /// Persistent reactor handles.
    pub reactors: Vec<u64>,
    /// Extension dictionary handle.
    pub xdictionary: Option<u64>,
    /// R2013+: object has data in the data store section.
    pub has_ds_binary: bool,
    /// Entity part, for entities.
    pub entity: Option<EntityCommon>,
}

/// Common entity data (spec §20.4.1, §20.4.2).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct EntityCommon {
    /// Size in bytes of the proxy graphics, when present (the bytes are skipped).
    pub graphics_size: Option<u64>,
    /// Entity mode: 0 = owner handle present, 1 = paper space, 2 = model space.
    pub entity_mode: u8,
    /// Layer handle.
    pub layer: Option<u64>,
    /// Linetype flags: 0 BYLAYER, 1 BYBLOCK, 2 CONTINUOUS, 3 handle in `linetype`.
    pub linetype_flags: u8,
    /// Linetype handle when flags = 3.
    pub linetype: Option<u64>,
    /// Plot style flags (R2000+), as linetype flags.
    pub plotstyle_flags: u8,
    /// Plot style handle when flags = 3.
    pub plotstyle: Option<u64>,
    /// Material flags (R2007+), as linetype flags.
    pub material_flags: u8,
    /// Material handle when flags = 3.
    pub material: Option<u64>,
    /// Shadow flags (R2007+).
    pub shadow_flags: u8,
    /// R2010+ full / face / edge visual style handles.
    pub visual_styles: [Option<u64>; 3],
    /// Entity color.
    pub color: CmColor,
    /// Color book (DBCOLOR) handle (R2004+).
    pub color_book: Option<u64>,
    /// Linetype scale.
    pub linetype_scale: f64,
    /// Entity is invisible.
    pub invisible: bool,
    /// Raw lineweight byte (R2000+); see [`lineweight_mm`].
    pub lineweight: Option<u8>,
    /// R13–R2000: previous/next links are implicit (handle ∓ 1).
    pub no_links: bool,
    /// R13–R2000: previous entity handle when stored.
    pub prev: Option<u64>,
    /// R13–R2000: next entity handle when stored.
    pub next: Option<u64>,
}

/// A color as stored (CMC / ENC, spec §2.11).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct CmColor {
    /// Color index (R13–R2000 and ENC without flags): 0 = BYBLOCK, 256 = BYLAYER,
    /// negative on layers that are off. For R2004+ ENC this is the BS with flags removed.
    pub index: i16,
    /// Raw 32-bit color value (R2004+): high byte is the method (0xC0 BYLAYER,
    /// 0xC1 BYBLOCK, 0xC2 RGB, 0xC3 ACI, 0xC8 none), low 24 bits RGB or index.
    pub value: Option<u32>,
    /// Color name (CMC flag 1).
    pub name: Option<String>,
    /// Color book name (CMC flag 2).
    pub book: Option<String>,
    /// Raw transparency value (ENC flag 0x2000): high byte type (0 BYLAYER,
    /// 1 BYBLOCK, 3 value in low byte).
    pub transparency: Option<u32>,
    /// ENC flag 0x4000: a DBCOLOR handle follows in the handle stream.
    pub has_book_reference: bool,
}

/// Extended entity data of one application (spec chapter 28).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct EedBlock {
    /// Handle of the APPID.
    pub app: u64,
    /// Raw data bytes.
    pub data: Vec<u8>,
    /// Decoded items (empty when decoding failed; `data` is always kept).
    pub items: Vec<EedItem>,
}

/// One EED item.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EedItem {
    /// 1000: string.
    String(String),
    /// 1002: `{` (false) or `}` (true).
    Control(bool),
    /// 1003: layer handle.
    Layer(u64),
    /// 1004: binary chunk.
    Binary(Vec<u8>),
    /// 1005: entity handle.
    Handle(u64),
    /// 1010–1013: point; first field is the group code.
    Point(u16, [f64; 3]),
    /// 1040–1042: real; first field is the group code.
    Real(u16, f64),
    /// 1070: 16-bit integer.
    Short(i16),
    /// 1071: 32-bit integer.
    Long(i32),
}

/// Type-specific data. One variant per decoded object type; types without a
/// decoder are [`ObjectData::Unparsed`].
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub enum ObjectData {
    /// No type-specific decoder exists (or decoding failed); see [`DwgObject::error`].
    #[default]
    Unparsed,
    /// LINE.
    Line(Line),
    /// POINT.
    Point(PointEntity),
    /// CIRCLE.
    Circle(Circle),
    /// ARC.
    Arc(Arc),
    /// LWPOLYLINE.
    LwPolyline(LwPolyline),
    /// TEXT.
    Text(Text),
    /// ATTRIB.
    Attrib(Attrib),
    /// ATTDEF.
    AttDef(Attrib),
    /// INSERT and MINSERT.
    Insert(Insert),
    /// BLOCK (start of a block definition).
    Block {
        /// Block name.
        name: String,
    },
    /// ENDBLK.
    EndBlock,
    /// SEQEND.
    Seqend,
    /// A table control object.
    Control(Control),
    /// BLOCK_HEADER (block table record).
    BlockHeader(BlockHeader),
    /// LAYER.
    Layer(LayerRecord),
    /// LTYPE.
    Linetype(LinetypeRecord),
    /// STYLE.
    TextStyle(TextStyleRecord),
    /// APPID and other table records decoded only up to their name.
    TableRecord(TableEntry),
    /// DICTIONARY and DICTIONARYWDFLT.
    Dictionary(Dictionary),
    /// LAYOUT.
    Layout(Layout),
    /// DBCOLOR: a color-book color referenced by entities (ENC flag 0x4000).
    DbColor(CmColor),
    /// ELLIPSE.
    Ellipse(Ellipse),
    /// SPLINE.
    Spline(Spline),
    /// VERTEX_2D, VERTEX_3D, VERTEX_MESH, VERTEX_PFACE.
    Vertex(PolylineVertex),
    /// VERTEX_PFACE_FACE.
    PfaceFace(PfaceFace),
    /// POLYLINE_2D, POLYLINE_3D, POLYLINE_PFACE, POLYLINE_MESH.
    Polyline(Polyline),
    /// SOLID, TRACE, 3DFACE.
    Face(Face),
    /// MTEXT.
    MText(MText),
    /// DIMENSION_* and the dimension classes.
    Dimension(Dimension),
    /// LEADER.
    Leader(Leader),
    /// HATCH.
    Hatch(Hatch),
    /// IMAGE and WIPEOUT.
    Image(Image),
    /// IMAGEDEF.
    ImageDef(ImageDef),
    /// VIEWPORT.
    Viewport(Viewport),
    /// RAY and XLINE.
    RayLine(RayLine),
    /// SHAPE.
    Shape(Shape),
    /// TOLERANCE.
    Tolerance(Tolerance),
    /// MLINE.
    MLine(MLine),
    /// OLE2FRAME.
    Ole2Frame(Ole2Frame),
    /// REGION, 3DSOLID, BODY.
    Acis(AcisData),
}

/// LINE (§20.4.21), WCS.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Line {
    /// Start point.
    pub start: [f64; 3],
    /// End point.
    pub end: [f64; 3],
    /// Thickness.
    pub thickness: f64,
    /// Extrusion.
    pub extrusion: [f64; 3],
}

/// POINT (§20.4.31), WCS.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct PointEntity {
    /// Location.
    pub position: [f64; 3],
    /// Thickness.
    pub thickness: f64,
    /// Extrusion.
    pub extrusion: [f64; 3],
    /// X-axis angle (radians).
    pub x_axis_angle: f64,
}

/// CIRCLE (§20.4.20), center in OCS.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Circle {
    /// Center (OCS).
    pub center: [f64; 3],
    /// Radius.
    pub radius: f64,
    /// Thickness.
    pub thickness: f64,
    /// Extrusion (OCS normal).
    pub extrusion: [f64; 3],
}

/// ARC (§20.4.18), center in OCS, angles in radians.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Arc {
    /// Center (OCS).
    pub center: [f64; 3],
    /// Radius.
    pub radius: f64,
    /// Thickness.
    pub thickness: f64,
    /// Extrusion (OCS normal).
    pub extrusion: [f64; 3],
    /// Start angle.
    pub start_angle: f64,
    /// End angle.
    pub end_angle: f64,
}

/// LWPOLYLINE (§20.4.85), vertices in OCS.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct LwPolyline {
    /// Raw flag word: 0x200 closed, 0x100 plinegen, 0x4 const width, 0x8 elevation,
    /// 0x2 thickness, 0x1 extrusion, 0x10 bulges, 0x20 widths, 0x400 vertex ids.
    pub flags: u16,
    /// Constant width.
    pub const_width: f64,
    /// Elevation (OCS Z).
    pub elevation: f64,
    /// Thickness.
    pub thickness: f64,
    /// Extrusion.
    pub extrusion: [f64; 3],
    /// Vertices (OCS X/Y).
    pub points: Vec<[f64; 2]>,
    /// Bulges (empty or one per vertex).
    pub bulges: Vec<f64>,
    /// Vertex ids (R2010+).
    pub vertex_ids: Vec<i32>,
    /// Start/end widths (empty or one pair per vertex).
    pub widths: Vec<[f64; 2]>,
}

impl LwPolyline {
    /// The polyline is closed.
    pub fn closed(&self) -> bool {
        self.flags & 0x200 != 0
    }
}

/// TEXT (§20.4.3); points in OCS.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Text {
    /// Elevation (OCS Z of both points).
    pub elevation: f64,
    /// Insertion point (OCS X/Y).
    pub insertion: [f64; 2],
    /// Alignment point (OCS X/Y), when stored.
    pub alignment: Option<[f64; 2]>,
    /// Extrusion.
    pub extrusion: [f64; 3],
    /// Thickness.
    pub thickness: f64,
    /// Oblique angle (radians).
    pub oblique: f64,
    /// Rotation (radians).
    pub rotation: f64,
    /// Height.
    pub height: f64,
    /// Width factor.
    pub width_factor: f64,
    /// Text value.
    pub value: String,
    /// Generation flags (2 = backward, 4 = upside down).
    pub generation: i16,
    /// Horizontal alignment (DXF 72).
    pub halign: i16,
    /// Vertical alignment (DXF 73).
    pub valign: i16,
    /// Text style handle.
    pub style: Option<u64>,
}

/// ATTRIB / ATTDEF (§20.4.4, §20.4.5): TEXT data plus tag and flags.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Attrib {
    /// The text part.
    pub text: Text,
    /// R2010+ version byte.
    pub version: Option<u8>,
    /// R2018+ attribute type (1 single line, 2 multi-line ATTRIB, 4 multi-line ATTDEF).
    pub attribute_type: Option<u8>,
    /// Tag.
    pub tag: String,
    /// Field length (unused).
    pub field_length: i16,
    /// Flags: 1 invisible, 2 constant, 4 verify, 8 preset.
    pub flags: u8,
    /// R2007+: lock position.
    pub lock_position: bool,
    /// ATTDEF prompt.
    pub prompt: Option<String>,
    /// R2018+ multi-line attribute: the embedded MTEXT (its common entity part is
    /// not kept).
    pub mtext: Option<MText>,
}

/// INSERT / MINSERT (§20.4.9, §20.4.10); insertion point in OCS.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Insert {
    /// Insertion point (OCS).
    pub position: [f64; 3],
    /// Scale factors.
    pub scale: [f64; 3],
    /// Rotation (radians).
    pub rotation: f64,
    /// Extrusion.
    pub extrusion: [f64; 3],
    /// ATTRIBs follow.
    pub has_attribs: bool,
    /// MINSERT columns / rows / spacing; `None` for INSERT.
    pub array: Option<InsertArray>,
    /// BLOCK_HEADER handle.
    pub block_header: Option<u64>,
    /// ATTRIB handles: owned list (R2004+) or first/last (R13–R2000).
    pub attribs: Vec<u64>,
    /// R13–R2000: attributes given as a first..last chain.
    pub attribs_are_range: bool,
    /// SEQEND handle.
    pub seqend: Option<u64>,
}

/// MINSERT array parameters.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct InsertArray {
    /// Columns.
    pub columns: i16,
    /// Rows.
    pub rows: i16,
    /// Column spacing.
    pub column_spacing: f64,
    /// Row spacing.
    pub row_spacing: f64,
}

/// A table control object (§20.4.51 and following).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Control {
    /// Entry handles in table order.
    pub entries: Vec<u64>,
    /// Extra hard-owned entries: *MODEL_SPACE/*PAPER_SPACE for BLOCK_CONTROL,
    /// BYLAYER/BYBLOCK for LTYPE_CONTROL.
    pub extra: Vec<u64>,
}

/// Fields shared by table records.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct TableEntry {
    /// Entry name.
    pub name: String,
    /// The 64-flag (referenced), pre-R2007.
    pub flag64: bool,
    /// xrefindex+1 as stored.
    pub xref_index_plus1: i16,
    /// Dependent on an xref.
    pub xref_dependent: bool,
    /// Externally referenced block handle.
    pub xref_block: Option<u64>,
}

/// BLOCK_HEADER (§20.4.52).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct BlockHeader {
    /// Name and xref fields. Anonymous blocks may store only `*U`/`*D`; the BLOCK
    /// entity carries the full name.
    pub entry: TableEntry,
    /// Anonymous block.
    pub anonymous: bool,
    /// Contains attribute definitions.
    pub has_attdefs: bool,
    /// Is an xref.
    pub is_xref: bool,
    /// Is an overlaid xref.
    pub is_overlaid: bool,
    /// Xref is unloaded (R2000+).
    pub unloaded: bool,
    /// Base point.
    pub base_point: [f64; 3],
    /// Xref path.
    pub xref_path: String,
    /// Description (R2000+).
    pub description: String,
    /// Preview image bytes (R2000+).
    pub preview: Vec<u8>,
    /// Insert units (R2007+).
    pub insert_units: Option<i16>,
    /// Explodable (R2007+).
    pub explodable: Option<bool>,
    /// Block scaling (R2007+).
    pub scaling: Option<u8>,
    /// BLOCK entity handle.
    pub block_entity: Option<u64>,
    /// R13–R2000: first and last entity of the definition.
    pub first_entity: Option<u64>,
    /// R13–R2000: last entity.
    pub last_entity: Option<u64>,
    /// R2004+: owned entities in drawing order.
    pub entities: Vec<u64>,
    /// ENDBLK entity handle.
    pub end_block: Option<u64>,
    /// INSERT handles referencing this block (R2000+).
    pub inserts: Vec<u64>,
    /// LAYOUT handle (R2000+).
    pub layout: Option<u64>,
}

/// LAYER (§20.4.54).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct LayerRecord {
    /// Name and xref fields.
    pub entry: TableEntry,
    /// Frozen.
    pub frozen: bool,
    /// On.
    pub on: bool,
    /// Frozen in new viewports.
    pub frozen_new_viewports: bool,
    /// Locked.
    pub locked: bool,
    /// Plotted (R2000+; always true before).
    pub plot: bool,
    /// Raw lineweight index (R2000+).
    pub lineweight: Option<u8>,
    /// Color.
    pub color: CmColor,
    /// Plot style handle (R2000+).
    pub plotstyle: Option<u64>,
    /// Material handle (R2007+).
    pub material: Option<u64>,
    /// Linetype handle.
    pub linetype: Option<u64>,
}

/// One LTYPE dash (§20.4.58).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Dash {
    /// Length: positive dash, negative gap, zero dot.
    pub length: f64,
    /// Complex shape code / text index.
    pub shape_code: i16,
    /// X offset.
    pub x_offset: f64,
    /// Y offset.
    pub y_offset: f64,
    /// Scale.
    pub scale: f64,
    /// Rotation (radians).
    pub rotation: f64,
    /// Shape flags (1 absolute rotation, 2 text, 4 shape).
    pub shape_flags: i16,
    /// STYLE handle of the shape file.
    pub style: Option<u64>,
}

/// LTYPE (§20.4.58).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct LinetypeRecord {
    /// Name and xref fields.
    pub entry: TableEntry,
    /// Description.
    pub description: String,
    /// Total pattern length.
    pub pattern_length: f64,
    /// Alignment (always `'A'`).
    pub alignment: u8,
    /// Dashes.
    pub dashes: Vec<Dash>,
    /// Text area bytes (256 for pre-R2007, 512 for R2007+ text linetypes).
    pub text_area: Vec<u8>,
}

/// STYLE (§20.4.56).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct TextStyleRecord {
    /// Name and xref fields.
    pub entry: TableEntry,
    /// Entry is a shape file.
    pub is_shape: bool,
    /// Vertical text.
    pub vertical: bool,
    /// Fixed height.
    pub fixed_height: f64,
    /// Width factor.
    pub width_factor: f64,
    /// Oblique angle (radians).
    pub oblique: f64,
    /// Generation flags.
    pub generation: u8,
    /// Last height used.
    pub last_height: f64,
    /// Font file name.
    pub font: String,
    /// Big font file name.
    pub big_font: String,
}

/// DICTIONARY (§20.4.44).
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Dictionary {
    /// Cloning flag (R2000+).
    pub cloning: Option<i16>,
    /// Hard owner flag (R2000+).
    pub hard_owner: Option<u8>,
    /// Entries (name, handle) in stored order.
    pub entries: Vec<(String, u64)>,
    /// DICTIONARYWDFLT default entry.
    pub default_entry: Option<u64>,
}

/// LAYOUT (§20.4.84), the parts needed to place paper-space blocks.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct Layout {
    /// Page setup name.
    pub page_setup: String,
    /// Layout name (`"Model"` for model space).
    pub name: String,
    /// Tab order.
    pub tab_order: i32,
    /// Layout flags.
    pub flags: i16,
    /// Paper width in millimeters.
    pub paper_width: f64,
    /// Paper height in millimeters.
    pub paper_height: f64,
    /// Limits min.
    pub limits_min: [f64; 2],
    /// Limits max.
    pub limits_max: [f64; 2],
    /// Extents min.
    pub extents_min: [f64; 3],
    /// Extents max.
    pub extents_max: [f64; 3],
    /// Associated BLOCK_HEADER (paper space block).
    pub block_header: Option<u64>,
    /// Last active viewport.
    pub active_viewport: Option<u64>,
    /// Viewports (R2004+).
    pub viewports: Vec<u64>,
}

/// Lineweight in millimeters for a raw lineweight byte: `None` for BYLAYER (29),
/// BYBLOCK (30), DEFAULT (31) and unknown values.
pub fn lineweight_mm(raw: u8) -> Option<f64> {
    const HUNDREDTHS: [u16; 24] = [
        0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158,
        200, 211,
    ];
    HUNDREDTHS
        .get(usize::from(raw))
        .map(|&v| f64::from(v) / 100.0)
}
