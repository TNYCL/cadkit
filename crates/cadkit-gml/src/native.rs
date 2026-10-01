//! XML adlarını ve özgün nesne ilişkilerini koruyan CityGML modeli.

use cadkit_core::{Point3, Value};
use serde::{Deserialize, Serialize};

/// CityGML 2.0 çekirdek ad alanı.
pub const CORE: &str = "http://www.opengis.net/citygml/2.0";
/// GML 3.1.1 ad alanı.
pub const GML: &str = "http://www.opengis.net/gml";
/// CityGML 2.0 bina modülü.
pub const BUILDING: &str = "http://www.opengis.net/citygml/building/2.0";
/// CityGML 2.0 genel nesne modülü.
pub const GENERICS: &str = "http://www.opengis.net/citygml/generics/2.0";
/// CityGML 2.0 grup modülü.
pub const GROUPS: &str = "http://www.opengis.net/citygml/cityobjectgroup/2.0";
/// XML bağlantı ad alanı.
pub const XLINK: &str = "http://www.w3.org/1999/xlink";

/// Önek ve çözümlenmiş ad alanı birlikte saklanır; önek değişikliği anlamı değiştirmez.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Name {
    /// XML'deki önekli ad.
    pub qualified: String,
    /// Çözümlenmiş ad alanı URI'si; boş dize ad alanı bulunmadığını belirtir.
    pub namespace: String,
}

impl Name {
    /// URI ve önekli addan bir XML adı oluşturur.
    pub fn new(namespace: &str, qualified: &str) -> Self {
        Self {
            qualified: qualified.into(),
            namespace: namespace.into(),
        }
    }
    /// Önekten bağımsız yerel ad.
    pub fn local(&self) -> &str {
        self.qualified.rsplit(':').next().unwrap_or(&self.qualified)
    }
    /// Genişletilmiş XML adıyla eşleşme.
    pub fn is(&self, namespace: &str, local: &str) -> bool {
        self.namespace == namespace && self.local() == local
    }
}

/// XML özniteliği; metin değeri entity başvuruları çözüldükten sonra saklanır.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct XmlAttribute {
    /// Özniteliğin genişletilmiş adı.
    pub name: Name,
    /// Unicode metin değeri.
    pub value: String,
}

/// Çocuk sırasını ve karışık metin içeriğini koruyan XML düğümü.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Node {
    /// İç içe XML öğesi.
    Element(Element),
    /// XML metni; boşluklar da korunur.
    Text(String),
    /// XML yorumu.
    Comment(String),
}

/// Ad alanı çözülmüş öğe; bilinmeyen uzantılar da burada korunur.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    /// Öğe adı.
    pub name: Name,
    /// Kaynaktaki sırayla öznitelikler.
    pub attributes: Vec<XmlAttribute>,
    /// Kaynaktaki sırayla çocuklar.
    pub children: Vec<Node>,
    /// Kaynak XML'deki mutlak bayt konumu; yeni öğelerde sıfırdır.
    #[serde(default)]
    pub offset: u64,
}

impl Element {
    /// Boş öğe oluşturur; kullanılan önek belge kökünde tanımlanmalıdır.
    pub fn new(namespace: &str, name: &str) -> Self {
        Self {
            name: Name::new(namespace, name),
            attributes: vec![],
            children: vec![],
            offset: 0,
        }
    }
    /// Öznitelik ekler veya aynı genişletilmiş adı günceller.
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
    /// Genişletilmiş ada göre öznitelik değeri.
    pub fn attribute(&self, namespace: &str, local: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| a.name.is(namespace, local))
            .map(|a| a.value.as_str())
    }
    /// Doğrudan çocuk öğeler.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| {
            if let Node::Element(e) = n {
                Some(e)
            } else {
                None
            }
        })
    }
    /// Doğrudan metin parçalarını birleştirir.
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
    /// Çocuk öğe ekler.
    pub fn push(&mut self, child: Element) {
        self.children.push(Node::Element(child));
    }
    /// Metin içerikli öğe oluşturur.
    pub fn with_text(namespace: &str, name: &str, text: impl Into<String>) -> Self {
        let mut e = Self::new(namespace, name);
        e.children.push(Node::Text(text.into()));
        e
    }
}

/// İlk profilde tipli olarak oluşturulabilen şehir nesneleri.
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
    /// Standart ad alanı ve önekli öğe adı.
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

/// Tipli şehir nesnesi oluşturucusu; özellik sırası CityGML şemasını izlemelidir.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CityObject {
    /// Nesne türü.
    pub kind: ObjectKind,
    /// Belge içinde benzersiz XML kimliği.
    pub id: String,
    /// Şema sırasındaki özellik öğeleri (geometri, alt nesneler ve öznitelikler).
    pub properties: Vec<Element>,
}

impl CityObject {
    /// Boş şehir nesnesi oluşturur.
    pub fn new(kind: ObjectKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
            properties: vec![],
        }
    }
    /// Nesneyi kayıpsız XML modeline dönüştürür.
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

/// Belirtilen CRS birimlerinde tipli GML poligonu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Polygon {
    /// İsteğe bağlı kaynak kimliği.
    pub id: Option<String>,
    /// Kapalı dış halka.
    pub exterior: Vec<Point3>,
    /// Kapalı iç halkalar.
    pub interiors: Vec<Vec<Point3>>,
}

impl Polygon {
    /// Üç bileşenli `gml:pos` öğeleriyle poligon üretir; eksenleri değiştirmez.
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

/// Genel öznitelik oluşturur; desteklenmeyen türler hata verir.
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

/// CityGML 2.0 belgesi; bilinmeyen XML uzantıları da kökte korunur.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CityGmlDocument {
    /// `core:CityModel` kökü.
    pub root: Element,
}

impl Default for CityGmlDocument {
    fn default() -> Self {
        Self::new()
    }
}
impl CityGmlDocument {
    /// Standart önekleri tanımlanmış boş belge oluşturur.
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
    /// Üst düzey şehir nesnesini `cityObjectMember` içine ekler.
    pub fn add_object(&mut self, object: CityObject) {
        let mut member = Element::new(CORE, "core:cityObjectMember");
        member.push(object.into_element());
        self.root.push(member);
    }
}
