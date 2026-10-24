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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Document {
    /// Version of this JSON layout; see [`SchemaVersion`].
    #[serde(default)]
    pub schema_version: SchemaVersion,
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

impl Document {
    /// Creates an empty drawing document.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the primary design model (`ModelKind::Model`), falling back to the first model.
    pub fn model_space(&self) -> Option<&Model> {
        self.models
            .iter()
            .find(|m| m.kind == ModelKind::Model)
            .or_else(|| self.models.first())
    }

    /// Returns a mutable reference to the primary design model, creating `"Model"` if `models` is empty.
    pub fn model_space_mut(&mut self) -> &mut Model {
        if self.models.is_empty() {
            self.models.push(Model {
                name: "Model".to_owned(),
                kind: ModelKind::Model,
                ..Model::default()
            });
        }
        let idx = self
            .models
            .iter()
            .position(|m| m.kind == ModelKind::Model)
            .unwrap_or(0);
        #[allow(clippy::expect_used)]
        self.models
            .get_mut(idx)
            .expect("idx is in bounds of non-empty models")
    }

    /// Iterates over paper-space layout and sheet models (`kind != ModelKind::Model`).
    pub fn layouts(&self) -> impl Iterator<Item = &Model> {
        self.models.iter().filter(|m| m.kind != ModelKind::Model)
    }

    /// Iterates over top-level entities across all [`Document::models`].
    pub fn all_entities(&self) -> impl Iterator<Item = &Entity> {
        self.models.iter().flat_map(|m| m.entities.iter())
    }

    /// Iterates over top-level entities across all models whose effective layer matches `layer`
    /// (treating `None` on an entity as layer `"0"`).
    pub fn entities_on_layer<'a>(
        &'a self,
        layer: &'a str,
    ) -> impl Iterator<Item = &'a Entity> + 'a {
        self.all_entities()
            .filter(move |e| e.layer.as_deref().unwrap_or("0") == layer)
    }

    /// Looks up a layer by exact name first, then ASCII case-insensitively.
    pub fn layer(&self, name: &str) -> Option<&Layer> {
        self.layers.iter().find(|l| l.name == name).or_else(|| {
            self.layers
                .iter()
                .find(|l| l.name.eq_ignore_ascii_case(name))
        })
    }

    /// Looks up a block definition by exact name first, then ASCII case-insensitively.
    pub fn block(&self, name: &str) -> Option<&Block> {
        self.blocks.iter().find(|b| b.name == name).or_else(|| {
            self.blocks
                .iter()
                .find(|b| b.name.eq_ignore_ascii_case(name))
        })
    }

    /// Total number of entities in [`Document::models`], counting nested [`EntityKind::Group`] children.
    pub fn entity_count(&self) -> usize {
        fn count_slice(slice: &[Entity]) -> usize {
            let mut total = 0usize;
            let mut stack: Vec<&[Entity]> = vec![slice];
            while let Some(items) = stack.pop() {
                total = total.saturating_add(items.len());
                for e in items {
                    if let EntityKind::Group { children, .. } = &e.kind
                        && !children.is_empty()
                    {
                        stack.push(children);
                    }
                }
            }
            total
        }
        self.models.iter().fold(0usize, |acc, m| {
            acc.saturating_add(count_slice(&m.entities))
        })
    }
}

/// Version of the [`Document`] JSON layout, serialized as the integer `schema_version`.
///
/// Every document this crate writes carries [`SchemaVersion::CURRENT`]. JSON without the
/// field predates versioning and reads as version 1. [`crate::export::document_from_json`]
/// rejects other versions with [`crate::Error::SchemaVersion`]. The version increases only
/// when an existing field changes meaning or shape; new optional fields do not change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct SchemaVersion(u32);

impl SchemaVersion {
    /// The version this crate reads and writes.
    pub const CURRENT: Self = Self(1);

    /// The version number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl Default for SchemaVersion {
    fn default() -> Self {
        Self::CURRENT
    }
}

/// Origin of a [`Document`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
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

/// Lightweight header summary returned by header-only `inspect` routines without decoding entity streams.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Summary {
    /// File format family.
    pub format: Format,
    /// Format version as written in the file, e.g. `"AC1032"`, `"V8"`, `"V7"`, `"2.0"`.
    pub version: String,
    /// Whether the primary model is 3D, when recorded in the header.
    pub is_3d: Option<bool>,
    /// Drawing coordinate units, when recorded in the header.
    pub units: Option<Units>,
    /// Code page or encoding name recorded or implied by the header.
    pub codepage: Option<String>,
    /// Producing application if recorded in the header.
    pub application: Option<String>,
}

impl Summary {
    /// Creates a new header summary for `format` and `version`.
    #[must_use]
    pub fn new(format: Format, version: impl Into<String>) -> Self {
        Self {
            format,
            version: version.into(),
            is_3d: None,
            units: None,
            codepage: None,
            application: None,
        }
    }

    /// Sets [`Self::is_3d`].
    #[must_use]
    pub fn with_is_3d(mut self, is_3d: Option<bool>) -> Self {
        self.is_3d = is_3d;
        self
    }

    /// Sets [`Self::units`].
    #[must_use]
    pub fn with_units(mut self, units: Option<Units>) -> Self {
        self.units = units;
        self
    }

    /// Sets [`Self::codepage`].
    #[must_use]
    pub fn with_codepage(mut self, codepage: Option<String>) -> Self {
        self.codepage = codepage;
        self
    }

    /// Sets [`Self::application`].
    #[must_use]
    pub fn with_application(mut self, application: Option<String>) -> Self {
        self.application = application;
        self
    }
}

/// File format family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
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
    /// CityGML 2.0 (XML application schema based on GML 3.1.1).
    CityGml,
}

/// Drawing units.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Units {
    /// Named unit of the coordinates.
    pub unit: LengthUnit,
    /// Size of one drawing unit in meters, when known. Authoritative over `unit`
    /// for non-standard DGN master units.
    pub meters_per_unit: Option<f64>,
}

/// Length unit. Mirrors the DWG `$INSUNITS` set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
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

    /// Constructs a [`EntityKind::Point`] entity.
    pub fn point(position: Point3) -> Self {
        Self::new(EntityKind::Point { position })
    }

    /// Constructs a [`EntityKind::Line`] entity.
    pub fn line(start: Point3, end: Point3) -> Self {
        Self::new(EntityKind::Line { start, end })
    }

    /// Constructs a straight-segment [`EntityKind::Polyline`] in the world XY plane (`normal = +Z`).
    pub fn polyline(points: impl IntoIterator<Item = Point3>, closed: bool) -> Self {
        Self::new(EntityKind::Polyline {
            vertices: points.into_iter().map(Vertex::at).collect(),
            closed,
            normal: Vec3::Z,
        })
    }

    /// Constructs a [`EntityKind::Circle`] in the world XY plane (`normal = +Z`).
    pub fn circle(center: Point3, radius: f64) -> Self {
        Self::new(EntityKind::Circle {
            center,
            radius,
            normal: Vec3::Z,
        })
    }

    /// Constructs a counter-clockwise [`EntityKind::Arc`] in the world XY plane (`normal = +Z`).
    pub fn arc(center: Point3, radius: f64, start_angle: f64, end_angle: f64) -> Self {
        Self::new(EntityKind::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            normal: Vec3::Z,
        })
    }

    /// Constructs a left-baseline [`EntityKind::Text`] entity in the world XY plane (`normal = +Z`).
    pub fn text(position: Point3, height: f64, value: impl Into<String>) -> Self {
        Self::new(EntityKind::Text {
            position,
            end_point: None,
            height,
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            value: value.into(),
            style: None,
            halign: HAlign::Left,
            valign: VAlign::Baseline,
            normal: Vec3::Z,
        })
    }

    /// Constructs a top-left [`EntityKind::MText`] entity from plain UTF-8 text.
    pub fn mtext(position: Point3, height: f64, plain: impl Into<String>) -> Self {
        let plain = plain.into();
        let value = plain.replace('\n', "\\P");
        Self::new(EntityKind::MText {
            position,
            height,
            width: 0.0,
            rotation: 0.0,
            value,
            plain,
            style: None,
            attachment: Attachment::TopLeft,
            line_spacing: 1.0,
            normal: Vec3::Z,
        })
    }

    /// Constructs a unit-scale [`EntityKind::Insert`] block reference at `position`.
    pub fn insert(block: impl Into<String>, position: Point3) -> Self {
        Self::new(EntityKind::Insert {
            block: block.into(),
            position,
            scale: Vec3::ONE,
            rotation: 0.0,
            normal: Vec3::Z,
            columns: 1,
            rows: 1,
            column_spacing: 0.0,
            row_spacing: 0.0,
        })
    }

    /// Assigns the entity's layer name.
    pub fn on_layer(mut self, layer: impl Into<String>) -> Self {
        self.layer = Some(layer.into());
        self
    }

    /// Assigns the entity's [`Color`].
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Assigns the entity's linetype name.
    pub fn with_linetype(mut self, linetype: impl Into<String>) -> Self {
        self.linetype = Some(linetype.into());
        self
    }

    /// Assigns the entity's [`Lineweight`].
    pub fn with_lineweight(mut self, lineweight: Lineweight) -> Self {
        self.lineweight = lineweight;
        self
    }

    /// Sets [`Entity::visible`].
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Inserts a key-value entry into [`Entity::props`].
    pub fn with_prop(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.props.insert(key.into(), value.into());
        self
    }

    /// Appends a non-graphic [`Attribute`] tag/value pair to [`Entity::attributes`].
    pub fn with_attribute(mut self, tag: impl Into<String>, value: impl Into<Value>) -> Self {
        self.attributes.push(Attribute {
            tag: tag.into(),
            value: value.into(),
            invisible: true,
            ..Attribute::default()
        });
        self
    }
}

/// Geometry and type-specific data of an [`Entity`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
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
    /// A planar surface with optional holes, in world coordinates (WCS) and drawing units.
    /// Each ring is explicitly closed by repeating its first point as its last point.
    Polygon {
        /// Closed outer boundary ring containing at least four positions.
        exterior: Vec<Point3>,
        /// Closed hole rings inside the exterior boundary.
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct HatchLoop {
    /// Edges in order.
    pub edges: Vec<HatchEdge>,
    /// Loop is the outermost boundary.
    pub external: bool,
}

/// A hatch boundary edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

impl Color {
    /// Shorthand for `Color::Aci { index }`.
    pub const fn aci(index: u8) -> Self {
        Self::Aci { index }
    }

    /// Shorthand for `Color::Rgb { r, g, b }`.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::Rgb { r, g, b }
    }
}

/// Line weight.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
#[non_exhaustive]
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

impl Value {
    /// True when `self` is [`Value::Null`].
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Returns the boolean when `self` is [`Value::Bool`].
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the integer when `self` is [`Value::Int`].
    pub const fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns a floating-point number for [`Value::Float`] or lossless/cast [`Value::Int`].
    pub const fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(v) => Some(*v),
            Self::Int(v) => Some(*v as f64),
            _ => None,
        }
    }

    /// Returns the string slice when `self` is [`Value::Text`].
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Returns the slice when `self` is [`Value::List`].
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Self::List(items) => Some(items.as_slice()),
            _ => None,
        }
    }

    /// Returns the byte slice when `self` is [`Value::Bytes`].
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(bytes) => Some(bytes.as_slice()),
            _ => None,
        }
    }
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
impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Value::Int(i64::from(v))
    }
}
impl From<i16> for Value {
    fn from(v: i16) -> Self {
        Value::Int(i64::from(v))
    }
}
impl From<u32> for Value {
    fn from(v: u32) -> Self {
        Value::Int(i64::from(v))
    }
}
impl From<u16> for Value {
    fn from(v: u16) -> Self {
        Value::Int(i64::from(v))
    }
}
impl From<u8> for Value {
    fn from(v: u8) -> Self {
        Value::Int(i64::from(v))
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}
impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(f64::from(v))
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
impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Value::Bytes(v)
    }
}
impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Self {
        Value::List(v)
    }
}

/// Original bytes of a record, for lossless inspection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Raw {
    /// Native type code (DGN element type, DWG object type).
    pub type_code: u32,
    /// Record bytes exactly as stored (after decompression).
    pub bytes: Vec<u8>,
}

/// A recoverable problem found while reading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
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
