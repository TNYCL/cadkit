//! Kimlik, referans, sayısal veri ve geometri denetimleri.

use crate::native::{CityGmlDocument, Element, GENERICS, GML, Node, Polygon, XLINK};
use cadkit_core::{Error, Limits, Point3, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Yapısal ve geometrik denetim seçenekleri.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ValidationOptions {
    /// Kaynak tüketim sınırları.
    pub limits: Limits,
    /// Poligon kesişimleri ve düzlemsellik denetlensin.
    pub geometry: bool,
    /// Boundary intersection tolerance, in coordinate units.
    pub tolerance: f64,
    /// Maximum ring-point distance from the exterior plane, in coordinate units.
    /// Defaults to 0.01 (1 cm in a metric projected CRS), allowing rounded source
    /// coordinates. Geographic coordinates require an explicit, appropriate tolerance.
    pub planarity_tolerance: f64,
    /// Tüm belge için kenar karşılaştırma bütçesi.
    pub max_geometry_work: u64,
}
impl Default for ValidationOptions {
    fn default() -> Self {
        Self {
            limits: Limits::default(),
            geometry: true,
            tolerance: 1e-6,
            planarity_tolerance: 0.01,
            max_geometry_work: 20_000_000,
        }
    }
}

/// Yapısal sayaçlar; müşteri değerlerini içermez.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    /// XML öğe sayısı.
    pub elements: u64,
    /// Benzersiz GML kimlikleri.
    pub ids: u64,
    /// Çözümlenmiş yerel referanslar.
    pub references: u64,
    /// Poligonlar.
    pub polygons: u64,
    /// İç halkalar.
    pub interiors: u64,
    /// Koordinat konumları.
    pub positions: u64,
    /// Tipli genel öznitelikler.
    pub attributes: u64,
}

pub(crate) fn elements<'a>(
    doc: &'a CityGmlDocument,
    limits: &Limits,
) -> Result<Vec<(&'a Element, usize)>> {
    let mut result = vec![];
    let mut stack = vec![(&doc.root, 0u32, 3usize)];
    let mut bytes = 0u64;
    let mut nodes = 1u64;
    while let Some((e, depth, inherited)) = stack.pop() {
        if depth >= limits.max_depth.min(256) || result.len() as u64 >= limits.max_objects {
            return Err(Error::LimitExceeded("CityGML elements/depth".into()));
        }
        for name in [&e.name.qualified, &e.name.namespace] {
            check_string(name, e.offset, limits, &mut bytes)?;
        }
        if !e.name.qualified.split(':').all(xml_id) || e.name.qualified.matches(':').count() > 1 {
            return Err(Error::invalid(e.offset, "invalid XML qualified name"));
        }
        let dimension = dimension(e, inherited)?;
        if e.attributes.len() > 256 {
            return Err(Error::LimitExceeded("XML attributes per element".into()));
        }
        for a in &e.attributes {
            if !a.name.qualified.split(':').all(xml_id) || a.name.qualified.matches(':').count() > 1
            {
                return Err(Error::invalid(e.offset, "invalid XML attribute name"));
            }
            for text in [&a.value, &a.name.qualified, &a.name.namespace] {
                check_string(text, e.offset, limits, &mut bytes)?;
            }
        }
        nodes = nodes.saturating_add(e.children.len() as u64);
        if nodes > limits.max_objects {
            return Err(Error::LimitExceeded("CityGML tree nodes".into()));
        }
        for child in &e.children {
            match child {
                Node::Element(child) => stack.push((child, depth + 1, dimension)),
                Node::Text(t) | Node::Comment(t) => {
                    check_string(t, e.offset, limits, &mut bytes)?;
                }
            }
        }
        if nodes > limits.max_objects || bytes > limits.max_decompressed_bytes {
            return Err(Error::LimitExceeded("CityGML tree budget".into()));
        }
        result.push((e, dimension));
    }
    Ok(result)
}

fn check_string(s: &str, offset: u64, limits: &Limits, bytes: &mut u64) -> Result<()> {
    if s.len() as u64 > u64::from(limits.max_string_bytes) {
        return Err(Error::LimitExceeded("XML string bytes".into()));
    }
    crate::xml::check_chars(s, offset)?;
    *bytes = bytes.saturating_add(s.len() as u64);
    if *bytes > limits.max_decompressed_bytes {
        return Err(Error::LimitExceeded("XML decoded bytes".into()));
    }
    Ok(())
}

pub(crate) fn dimension(e: &Element, inherited: usize) -> Result<usize> {
    match e.attribute("", "srsDimension") {
        Some("2") => Ok(2),
        Some("3") => Ok(3),
        Some(_) => Err(Error::invalid(
            e.offset,
            "only coordinate dimensions 2 and 3 are supported",
        )),
        None => Ok(inherited),
    }
}

/// `pos` veya `posList` içeriğini, bildirilen eksen sırasını değiştirmeden okur.
pub(crate) fn positions(e: &Element, inherited: usize, limits: &Limits) -> Result<Vec<Point3>> {
    let dim = dimension(e, inherited)?;
    let text = e.text();
    let mut coordinates = vec![];
    for token in text.split_whitespace() {
        if coordinates.len() as u64 >= u64::from(limits.max_vertices).saturating_mul(dim as u64) {
            return Err(Error::LimitExceeded("GML coordinates per entity".into()));
        }
        let v = token
            .parse::<f64>()
            .map_err(|_| Error::invalid(e.offset, "invalid GML coordinate"))?;
        if !v.is_finite() {
            return Err(Error::invalid(e.offset, "non-finite GML coordinate"));
        }
        coordinates.push(v);
    }
    if coordinates.is_empty()
        || coordinates.len() % dim != 0
        || (e.name.is(GML, "pos") && coordinates.len() != dim)
    {
        return Err(Error::invalid(
            e.offset,
            "GML coordinate dimension mismatch",
        ));
    }
    if let Some(count) = e.attribute("", "count") {
        let n = count
            .parse::<usize>()
            .map_err(|_| Error::invalid(e.offset, "invalid posList count"))?;
        if n != coordinates.len() / dim {
            return Err(Error::invalid(e.offset, "posList count mismatch"));
        }
    }
    Ok(coordinates
        .chunks_exact(dim)
        .map(|p| {
            Point3::new(
                p.first().copied().unwrap_or(0.0),
                p.get(1).copied().unwrap_or(0.0),
                p.get(2).copied().unwrap_or(0.0),
            )
        })
        .collect())
}

/// Bir GML poligonunu tipli dış/iç halkalara dönüştürür.
pub fn polygon(e: &Element, inherited: usize, limits: &Limits) -> Result<Polygon> {
    let dim = dimension(e, inherited)?;
    let mut exterior = None;
    let mut interiors = vec![];
    for boundary in e.elements() {
        if !(boundary.name.is(GML, "exterior") || boundary.name.is(GML, "interior")) {
            continue;
        }
        let ring = boundary
            .elements()
            .find(|e| e.name.is(GML, "LinearRing"))
            .ok_or_else(|| {
                Error::Unsupported("polygon boundary requires an inline LinearRing".into())
            })?;
        let dim = dimension(ring, dimension(boundary, dim)?)?;
        let mut points = vec![];
        for p in ring.elements() {
            if p.name.is(GML, "pos") || p.name.is(GML, "posList") {
                let values = positions(p, dim, limits)?;
                if points.len().saturating_add(values.len()) as u64 > u64::from(limits.max_vertices)
                {
                    return Err(Error::LimitExceeded("GML ring vertices".into()));
                }
                points.extend(values);
            } else {
                return Err(Error::Unsupported(format!(
                    "LinearRing child {}",
                    p.name.local()
                )));
            }
        }
        if points.len() < 4 || points.first() != points.last() {
            return Err(Error::invalid(
                ring.offset,
                "LinearRing must be closed and contain at least four positions",
            ));
        }
        if boundary.name.is(GML, "exterior") {
            if exterior.replace(points).is_some() {
                return Err(Error::invalid(e.offset, "polygon has multiple exteriors"));
            }
        } else {
            interiors.push(points);
        }
    }
    Ok(Polygon {
        id: e.attribute(GML, "id").map(str::to_owned),
        exterior: exterior.ok_or_else(|| Error::invalid(e.offset, "polygon has no exterior"))?,
        interiors,
    })
}

/// Şema doğrulamasından bağımsız yapısal ve geometrik kontrolleri çalıştırır.
/// XSD doğrulaması ve katılar arası hacim kesişimi bu işlevin dışındadır.
pub fn validate(doc: &CityGmlDocument, options: &ValidationOptions) -> Result<ValidationReport> {
    if !doc.root.name.is(crate::native::CORE, "CityModel") {
        return Err(Error::UnknownFormat);
    }
    let nodes = elements(doc, &options.limits)?;
    let mut report = ValidationReport::default();
    let mut ids = HashMap::new();
    let mut references = vec![];
    for (e, dim) in &nodes {
        report.elements += 1;
        if let Some(id) = e.attribute(GML, "id") {
            if !xml_id(id) {
                return Err(Error::invalid(e.offset, "invalid gml:id"));
            }
            if ids.insert(id, *e).is_some() {
                return Err(Error::invalid(e.offset, "duplicate gml:id"));
            }
            report.ids += 1;
        }
        if let Some(href) = e.attribute(XLINK, "href") {
            let id = href
                .strip_prefix('#')
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    Error::Unsupported("external GML references are not resolved".into())
                })?;
            references.push((id, e.offset));
            report.references += 1;
        }
        if e.name.is(GML, "pos") || e.name.is(GML, "posList") {
            report.positions = report
                .positions
                .saturating_add(positions(e, *dim, &options.limits)?.len() as u64);
            if report.positions > options.limits.max_total_vertices {
                return Err(Error::LimitExceeded("GML total positions".into()));
            }
        }
        if e.name.is(GML, "Polygon") {
            let p = polygon(e, *dim, &options.limits)?;
            report.polygons += 1;
            report.interiors += p.interiors.len() as u64;
        }
        if e.name.is(GML, "Circle") {
            // Native XML can retain coordinate encodings outside the neutral subset.
            if let Err(error) = crate::curves::read_segment(e, *dim, &options.limits) {
                if !matches!(error, Error::Unsupported(_)) {
                    return Err(error);
                }
            }
        }
        if e.name.namespace == GENERICS
            && matches!(
                e.name.local(),
                "intAttribute"
                    | "doubleAttribute"
                    | "stringAttribute"
                    | "dateAttribute"
                    | "uriAttribute"
                    | "measureAttribute"
            )
        {
            report.attributes += 1;
            let value = e
                .elements()
                .find(|v| v.name.is(GENERICS, "value"))
                .ok_or_else(|| Error::invalid(e.offset, "generic attribute has no value"))?
                .text();
            let valid = match e.name.local() {
                "intAttribute" => value.trim().parse::<i64>().is_ok(),
                "doubleAttribute" | "measureAttribute" => {
                    value.trim().parse::<f64>().is_ok_and(f64::is_finite)
                }
                _ => true,
            };
            if !valid {
                return Err(Error::invalid(e.offset, "invalid typed generic attribute"));
            }
        }
    }
    for (id, offset) in references {
        if !ids.contains_key(id) {
            return Err(Error::invalid(offset, "unresolved internal GML reference"));
        }
    }
    // Geometrinin başvurular üzerinden kendisini genişletmesi sınırlı kalmalıdır.
    let mut graph: HashMap<&str, Vec<&str>> = HashMap::new();
    let geometry = |e: &Element| e.name.namespace == GML;
    for (&id, &e) in &ids {
        if !geometry(e) {
            continue;
        }
        let mut todo = vec![e];
        while let Some(n) = todo.pop() {
            if let Some(href) = n.attribute(XLINK, "href").and_then(|h| h.strip_prefix('#')) {
                graph.entry(id).or_default().push(href);
            }
            for child in n.elements() {
                if let Some(child_id) = child.attribute(GML, "id") {
                    graph.entry(id).or_default().push(child_id);
                } else {
                    todo.push(child);
                }
            }
        }
    }
    let mut done = HashSet::new();
    for &start in graph.keys() {
        let mut todo = vec![(start, false)];
        let mut active = HashSet::new();
        while let Some((id, exit)) = todo.pop() {
            if exit {
                active.remove(id);
                done.insert(id);
                continue;
            }
            if done.contains(id) {
                continue;
            }
            if !active.insert(id) {
                return Err(Error::invalid(0, "cyclic GML geometry reference"));
            }
            if active.len() as u64 > u64::from(options.limits.max_depth) {
                return Err(Error::LimitExceeded("GML reference depth".into()));
            }
            todo.push((id, true));
            if let Some(next) = graph.get(id) {
                for &n in next {
                    todo.push((n, false));
                }
            }
        }
    }
    if options.geometry {
        if let Some(issue) = crate::geometry::inspect(&nodes, options)?.first() {
            return Err(Error::invalid(issue.offset, &issue.message));
        }
    }
    Ok(report)
}

fn xml_id(s: &str) -> bool {
    fn start(c: char) -> bool {
        matches!(c, 'A'..='Z' | '_' | 'a'..='z' | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}' | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}' | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}' | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}' | '\u{10000}'..='\u{EFFFF}')
    }
    let mut chars = s.chars();
    chars.next().is_some_and(start) && chars.all(|c| {
        start(c)
            || matches!(c,'-'|'.'|'0'..='9'|'\u{B7}'|'\u{300}'..='\u{36F}'|'\u{203F}'..='\u{2040}')
    })
}
