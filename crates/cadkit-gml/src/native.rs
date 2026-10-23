//! Lossless CityGML 2.0 model preserving qualified XML names, namespaces, and object relationships.

use cadkit_core::{Point3, Value};
use serde::{Deserialize, Serialize};

/// CityGML 2.0 core namespace URI.
pub const CORE: &str = "http://www.opengis.net/citygml/2.0";
/// GML 3.1.1 namespace URI.
pub const GML: &str = "http://www.opengis.net/gml";
/// CityGML 2.0 building module namespace URI.
pub const BUILDING: &str = "http://www.opengis.net/citygml/building/2.0";
/// CityGML 2.0 generics module namespace URI.
pub const GENERICS: &str = "http://www.opengis.net/citygml/generics/2.0";
/// CityGML 2.0 city object group module namespace URI.
pub const GROUPS: &str = "http://www.opengis.net/citygml/cityobjectgroup/2.0";
/// W3C XLink namespace URI.
pub const XLINK: &str = "http://www.w3.org/1999/xlink";

/// An XML name storing both its qualified form and resolved namespace URI so prefix choices do not alter semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Name {
    /// Qualified XML name (including prefix, if any).
    pub qualified: String,
    /// Resolved namespace URI; an empty string indicates no namespace.
    pub namespace: String,
}

impl Name {
    /// Creates an XML name from a namespace URI and a qualified name.
    pub fn new(namespace: &str, qualified: &str) -> Self {
        Self {
            qualified: qualified.into(),
            namespace: namespace.into(),
        }
    }
    /// Prefix-independent local name.
    pub fn local(&self) -> &str {
        self.qualified.rsplit(':').next().unwrap_or(&self.qualified)
    }
    /// Returns whether this name matches the given namespace URI and local name.
    pub fn is(&self, namespace: &str, local: &str) -> bool {
        self.namespace == namespace && self.local() == local
    }
}

/// An XML attribute whose value is stored after entity unescaping and normalization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XmlAttribute {
    /// Expanded name of the attribute.
    pub name: Name,
    /// Decoded Unicode string value.
    pub value: String,
}

/// An XML child node preserving child order and mixed text content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Node {
    /// Nested XML element.
    Element(Element),
    /// Character data (including whitespace).
    Text(String),
    /// XML comment.
    Comment(String),
}

/// A namespace-resolved XML element; unknown application schema extensions are preserved verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    /// Element name.
    pub name: Name,
    /// Attributes in source order.
    pub attributes: Vec<XmlAttribute>,
    /// Child nodes in source order.
    pub children: Vec<Node>,
    /// Byte offset in the source XML stream, or zero for programmatically constructed elements.
    #[serde(default)]
    pub offset: u64,
}

impl Element {
    /// Creates an empty element; any prefix used in `name` must be declared on the document root.
    pub fn new(namespace: &str, name: &str) -> Self {
        Self {
            name: Name::new(namespace, name),
            attributes: vec![],
            children: vec![],
            offset: 0,
        }
    }
    /// Inserts an attribute or updates an existing attribute with the same expanded name.
    pub fn set_attribute(&mut self, namespace: &str, name: &str, value: impl Into<String>) {
        let key = Name::new(namespace, name);
        if let Some(a) = self
            .attributes
            .iter_mut()
            .find(|a| a.name.is(namespace, key.local()))
        {
            a.value = value.into();
        } else {
            self.attributes.push(XmlAttribute {
                name: key,
                value: value.into(),
            });
        }
    }
    /// Looks up an attribute value by namespace URI and local name.
    pub fn attribute(&self, namespace: &str, local: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| a.name.is(namespace, local))
            .map(|a| a.value.as_str())
    }
    /// Iterates over direct child elements.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| {
            if let Node::Element(e) = n {
                Some(e)
            } else {
                None
            }
        })
    }
    /// Concatenates direct child text nodes.
    pub fn text(&self) -> String {
        self.children
            .iter()
            .filter_map(|n| {
                if let Node::Text(t) = n {
                    Some(t.as_str())
                } else {
                    None
                }
            })
            .collect()
    }
    /// Appends a child element.
    pub fn push(&mut self, child: Element) {
        self.children.push(Node::Element(child));
    }
    /// Creates an element containing a single text node.
    pub fn with_text(namespace: &str, name: &str, text: impl Into<String>) -> Self {
        let mut e = Self::new(namespace, name);
        e.children.push(Node::Text(text.into()));
        e
    }
}

/// Typed CityGML 2.0 city object and boundary surface kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[allow(missing_docs)]
pub enum ObjectKind {
    Building,
    BuildingPart,
    Room,
    Door,
    Window,
    BuildingInstallation,
    WallSurface,
    InteriorWallSurface,
    RoofSurface,
    GroundSurface,
    FloorSurface,
    CeilingSurface,
    ClosureSurface,
    OuterFloorSurface,
    OuterCeilingSurface,
    GenericCityObject,
    CityObjectGroup,
}

impl ObjectKind {
    /// Standard namespace URI and qualified XML element name.
    pub fn xml_name(self) -> (&'static str, &'static str) {
        match self {
            Self::GenericCityObject => (GENERICS, "gen:GenericCityObject"),
            Self::CityObjectGroup => (GROUPS, "grp:CityObjectGroup"),
            Self::Building => (BUILDING, "bldg:Building"),
            Self::BuildingPart => (BUILDING, "bldg:BuildingPart"),
            Self::Room => (BUILDING, "bldg:Room"),
            Self::Door => (BUILDING, "bldg:Door"),
            Self::Window => (BUILDING, "bldg:Window"),
            Self::BuildingInstallation => (BUILDING, "bldg:BuildingInstallation"),
            Self::WallSurface => (BUILDING, "bldg:WallSurface"),
            Self::InteriorWallSurface => (BUILDING, "bldg:InteriorWallSurface"),
            Self::RoofSurface => (BUILDING, "bldg:RoofSurface"),
            Self::GroundSurface => (BUILDING, "bldg:GroundSurface"),
            Self::FloorSurface => (BUILDING, "bldg:FloorSurface"),
            Self::CeilingSurface => (BUILDING, "bldg:CeilingSurface"),
            Self::ClosureSurface => (BUILDING, "bldg:ClosureSurface"),
            Self::OuterFloorSurface => (BUILDING, "bldg:OuterFloorSurface"),
            Self::OuterCeilingSurface => (BUILDING, "bldg:OuterCeilingSurface"),
        }
    }
}

/// Builder for a typed CityGML city object; `properties` must follow CityGML schema order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CityObject {
    /// City object kind.
    pub kind: ObjectKind,
    /// Document-unique `gml:id`.
    pub id: String,
    /// Property elements (attributes, geometry, and child objects) in schema order.
    pub properties: Vec<Element>,
}

impl CityObject {
    /// Creates an empty city object with the given kind and `gml:id`.
    pub fn new(kind: ObjectKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
            properties: vec![],
        }
    }
    /// Converts this city object into a native XML [`Element`].
    pub fn into_element(self) -> Element {
        let (ns, name) = self.kind.xml_name();
        let mut e = Element::new(ns, name);
        e.set_attribute(GML, "gml:id", self.id);
        for p in self.properties {
            e.push(p);
        }
        e
    }
}

/// A typed GML polygon in the coordinates and units of its declared CRS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Polygon {
    /// Optional `gml:id`.
    pub id: Option<String>,
    /// Closed exterior boundary ring.
    pub exterior: Vec<Point3>,
    /// Closed interior hole rings.
    pub interiors: Vec<Vec<Point3>>,
}

impl Polygon {
    /// Encodes the polygon as `gml:Polygon` with 3D `gml:pos` elements without reordering axes.
    pub fn to_element(&self) -> Element {
        let mut p = Element::new(GML, "gml:Polygon");
        if let Some(id) = &self.id {
            p.set_attribute(GML, "gml:id", id);
        }
        for (i, ring) in std::iter::once(&self.exterior)
            .chain(self.interiors.iter())
            .enumerate()
        {
            let mut boundary = Element::new(
                GML,
                if i == 0 {
                    "gml:exterior"
                } else {
                    "gml:interior"
                },
            );
            let mut linear = Element::new(GML, "gml:LinearRing");
            for v in ring {
                linear.push(Element::with_text(
                    GML,
                    "gml:pos",
                    format!("{} {} {}", v.x, v.y, v.z),
                ));
            }
            boundary.push(linear);
            p.push(boundary);
        }
        p
    }
}

/// Builds a CityGML generics attribute element, returning an error for unsupported value types.
pub fn generic_attribute(name: &str, value: &Value) -> cadkit_core::Result<Element> {
    let (kind, text) = match value {
        Value::Text(s) => ("gen:stringAttribute", s.clone()),
        Value::Int(n) => ("gen:intAttribute", n.to_string()),
        Value::Float(n) if n.is_finite() => ("gen:doubleAttribute", n.to_string()),
        _ => {
            return Err(cadkit_core::Error::Unsupported(
                "CityGML generic attribute type".into(),
            ));
        }
    };
    let mut e = Element::new(GENERICS, kind);
    e.set_attribute("", "name", name);
    e.push(Element::with_text(GENERICS, "gen:value", text));
    Ok(e)
}

/// A CityGML 2.0 document whose root preserves unknown XML namespaces and extensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CityGmlDocument {
    /// The `core:CityModel` root element.
    pub root: Element,
}

impl Default for CityGmlDocument {
    fn default() -> Self {
        Self::new()
    }
}
impl CityGmlDocument {
    /// Creates an empty `core:CityModel` document with standard CityGML 2.0 namespace declarations.
    pub fn new() -> Self {
        let mut root = Element::new(CORE, "core:CityModel");
        for (prefix, uri) in [
            ("core", CORE),
            ("gml", GML),
            ("bldg", BUILDING),
            ("gen", GENERICS),
            ("grp", GROUPS),
            ("xlink", XLINK),
        ] {
            root.set_attribute(
                "http://www.w3.org/2000/xmlns/",
                &format!("xmlns:{prefix}"),
                uri,
            );
        }
        Self { root }
    }
    /// Wraps `object` in `core:cityObjectMember` and appends it to the root model.
    pub fn add_object(&mut self, object: CityObject) {
        let mut member = Element::new(CORE, "core:cityObjectMember");
        member.push(object.into_element());
        self.root.push(member);
    }
}
