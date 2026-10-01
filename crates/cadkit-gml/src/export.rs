//! Explicit CAD geometry projection and conversion diagnostics.

use crate::native::*;
use cadkit_core::{Document, Entity, EntityKind, Error, Point3, Result, Value};
use serde::{Deserialize, Serialize};

/// Yeni CityGML çıktısının koordinat sözleşmesi.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportOptions {
    /// Koordinatların mevcut CRS tanımı; dönüştürme yapılmaz.
    pub srs_name: String,
    /// Genel nesne geometrisinin LoD değeri, 0..=4.
    pub lod: u8,
    /// Yazma öncesi ve XML çıktısı için kaynak sınırları.
    #[serde(default)]
    pub limits: cadkit_core::Limits,
}

/// Handling of geometry without a supported, exact GML representation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedGeometry {
    /// Stop without output. This remains the default of the existing export API.
    #[default]
    Reject,
    /// Retain the source entity JSON as metadata, omit its GML geometry and report it.
    /// This does not embed external image files or resolve block references.
    MetadataOnly,
}

/// A source entity whose geometry is not present in the output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportIssue {
    /// Stable diagnostic code, currently `gml.metadata_only`.
    pub code: String,
    /// Zero-based source model index.
    pub model: usize,
    /// Zero-based top-level entity index followed by nested group child indices.
    pub path: Vec<usize>,
    /// Neutral entity kind; never a file-controlled native type name.
    pub entity_type: String,
}

/// Counts of visited source entities, including group nodes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportReport {
    /// Number of source entities visited.
    pub entities: u64,
    /// Entities represented by GML geometry; groups may have incomplete child geometry.
    pub geometry_entities: u64,
    /// Entities retained only in `cadkit.entity` metadata.
    pub metadata_only_entities: u64,
    /// One diagnostic for each metadata-only entity, in source traversal order.
    pub issues: Vec<ExportIssue>,
}

struct Exporter {
    unsupported: UnsupportedGeometry,
    report: ExportReport,
    max_depth: u32,
}

impl Exporter {
    fn geometry(
        &mut self,
        e: &Entity,
        model: usize,
        path: &mut Vec<usize>,
    ) -> Result<Option<Element>> {
        if path.len() as u64 > u64::from(self.max_depth.min(64)) {
            return Err(Error::LimitExceeded("CityGML export depth".into()));
        }
        self.report.entities += 1;
        let result = if let EntityKind::Group { children, .. } = &e.kind {
            let mut multi = Element::new(GML, "gml:MultiGeometry");
            for (i, child) in children.iter().enumerate() {
                path.push(i);
                if let Some(g) = self.geometry(child, model, path)? {
                    let mut member = Element::new(GML, "gml:geometryMember");
                    member.push(g);
                    multi.push(member);
                }
                path.pop();
            }
            if multi.children.is_empty() {
                Err(Error::Unsupported("CityGML group has no geometry".into()))
            } else {
                Ok(multi)
            }
        } else {
            geometry(&e.kind)
        };
        match result {
            Ok(g) => {
                self.report.geometry_entities += 1;
                Ok(Some(g))
            }
            Err(Error::Unsupported(_)) if self.unsupported == UnsupportedGeometry::MetadataOnly => {
                self.report.metadata_only_entities += 1;
                self.report.issues.push(ExportIssue {
                    code: "gml.metadata_only".into(),
                    model,
                    path: path.clone(),
                    entity_type: e.kind.type_name().into(),
                });
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
}

fn point_element(p: Point3) -> Element {
    Element::with_text(GML, "gml:pos", format!("{} {} {}", p.x, p.y, p.z))
}
fn geometry(kind: &EntityKind) -> Result<Element> {
    match kind {
        EntityKind::Point { position } => {
            let mut e = Element::new(GML, "gml:Point");
            e.push(point_element(*position));
            Ok(e)
        }
        EntityKind::Line { start, end } => {
            let mut e = Element::new(GML, "gml:LineString");
            e.push(point_element(*start));
            e.push(point_element(*end));
            Ok(e)
        }
        EntityKind::Polyline {
            vertices, closed, ..
        } => {
            if vertices.len() < 2 {
                return Err(Error::invalid(
                    0,
                    "CityGML line requires at least two positions",
                ));
            }
            if vertices
                .iter()
                .any(|v| v.bulge != 0.0 || v.start_width != 0.0 || v.end_width != 0.0)
            {
                return Err(Error::Unsupported(
                    "CityGML polyline bulges/widths require explicit conversion".into(),
                ));
            }
            let mut points: Vec<_> = vertices.iter().map(|v| v.position).collect();
            if *closed && points.first() != points.last() {
                if let Some(first) = points.first().copied() {
                    points.push(first);
                }
            }
            // Closure alone does not assert a planar surface. CAD paths may cross
            // themselves or leave the plane; only explicit Polygon/Face means area.
            let mut e = Element::new(GML, "gml:LineString");
            for p in points {
                e.push(point_element(p));
            }
            Ok(e)
        }
        EntityKind::Polygon {
            exterior,
            interiors,
        } => Ok(Polygon {
            id: None,
            exterior: exterior.clone(),
            interiors: interiors.clone(),
        }
        .to_element()),
        EntityKind::Face { points, .. } => {
            let mut ring = points.clone();
            if ring.first() != ring.last() {
                if let Some(first) = ring.first().copied() {
                    ring.push(first);
                }
            }
            Ok(Polygon {
                id: None,
                exterior: ring,
                interiors: vec![],
            }
            .to_element())
        }
        EntityKind::Mesh { vertices, faces } => {
            let mut e = Element::new(GML, "gml:MultiSurface");
            for f in faces {
                let mut ring = vec![];
                for &i in f {
                    ring.push(
                        *vertices
                            .get(i as usize)
                            .ok_or_else(|| Error::invalid(0, "mesh face index outside vertices"))?,
                    );
                }
                if ring.first() != ring.last() {
                    if let Some(first) = ring.first().copied() {
                        ring.push(first);
                    }
                }
                let mut member = Element::new(GML, "gml:surfaceMember");
                member.push(
                    Polygon {
                        id: None,
                        exterior: ring,
                        interiors: vec![],
                    }
                    .to_element(),
                );
                e.push(member);
            }
            Ok(e)
        }
        EntityKind::Circle {
            center,
            radius,
            normal,
        } => crate::curves::circle(*center, *radius, *normal),
        other => Err(Error::Unsupported(format!(
            "CityGML export for {} requires explicit geometry/semantic mapping",
            other.type_name()
        ))),
    }
}

/// Her üst düzey nötr nesne için bir `GenericCityObject` üretir.
/// Koordinatlar değiştirilmez; CRS ve birimler çağıranın sorumluluğundadır.
pub fn from_document(source: &Document, options: &ExportOptions) -> Result<CityGmlDocument> {
    from_document_with_report(source, options, UnsupportedGeometry::Reject).map(|(doc, _)| doc)
}

/// Creates generic objects and reports geometry retained only as metadata.
/// Native building/unit semantics must be supplied through `CityGmlDocument`.
/// Invalid geometry and resource-limit errors always fail, under either policy.
pub fn from_document_with_report(
    source: &Document,
    options: &ExportOptions,
    unsupported: UnsupportedGeometry,
) -> Result<(CityGmlDocument, ExportReport)> {
    if options.srs_name.trim().is_empty() || options.lod > 4 {
        return Err(Error::invalid(
            0,
            "CityGML export requires an explicit CRS and LoD 0..4",
        ));
    }
    cadkit_core::export::validate_limits(source, &options.limits)?;
    let mut doc = CityGmlDocument::new();
    let mut exporter = Exporter {
        unsupported,
        report: ExportReport::default(),
        max_depth: options.limits.max_depth,
    };
    let mut next = 0u64;
    for (model_index, model) in source.models.iter().enumerate() {
        for (entity_index, e) in model.entities.iter().enumerate() {
            next = next
                .checked_add(1)
                .ok_or_else(|| Error::LimitExceeded("CityGML export objects".into()))?;
            let mut object =
                CityObject::new(ObjectKind::GenericCityObject, format!("cadkit_{next}"));
            if let Some(layer) = &e.layer {
                object.properties.push(generic_attribute(
                    "cadkit.layer",
                    &Value::Text(layer.clone()),
                )?);
            }
            if let Some(id) = e.id {
                object.properties.push(generic_attribute(
                    "cadkit.id",
                    &Value::Text(id.to_string()),
                )?);
            }
            for a in &e.attributes {
                object.properties.push(generic_attribute(&a.tag, &a.value)?);
            }
            // serde_json encodes non-finite numbers as null. An in-memory value
            // roundtrip detects that loss even inside optional fields or metadata-only
            // geometry, without decimal parsing or false float-rounding mismatches.
            let value =
                serde_json::to_value(e).map_err(|err| Error::invalid(0, err.to_string()))?;
            let checked: Entity = serde_json::from_value(value)
                .map_err(|_| Error::invalid(0, "entity cannot be preserved as JSON metadata"))?;
            if checked != *e {
                return Err(Error::invalid(
                    0,
                    "entity cannot be preserved as JSON metadata",
                ));
            }
            let metadata =
                serde_json::to_string(e).map_err(|err| Error::invalid(0, err.to_string()))?;
            if metadata.len() as u64 > u64::from(options.limits.max_string_bytes) {
                return Err(Error::LimitExceeded("CityGML entity metadata bytes".into()));
            }
            object
                .properties
                .push(generic_attribute("cadkit.entity", &Value::Text(metadata))?);
            if !e.props.is_empty() {
                let text = serde_json::to_string(&e.props)
                    .map_err(|e| Error::invalid(0, e.to_string()))?;
                object
                    .properties
                    .push(generic_attribute("cadkit.props", &Value::Text(text))?);
            }
            let first_issue = exporter.report.issues.len();
            let geometry = exporter.geometry(e, model_index, &mut vec![entity_index])?;
            let issues = exporter.report.issues.get(first_issue..).unwrap_or(&[]);
            if !issues.is_empty() {
                object.properties.push(generic_attribute(
                    "cadkit.geometry_status",
                    &Value::Text(
                        if geometry.is_some() {
                            "partial"
                        } else {
                            "metadata_only"
                        }
                        .into(),
                    ),
                )?);
                let text =
                    serde_json::to_string(issues).map_err(|e| Error::invalid(0, e.to_string()))?;
                object.properties.push(generic_attribute(
                    "cadkit.export_issues",
                    &Value::Text(text),
                )?);
            }
            if let Some(mut g) = geometry {
                let mut property =
                    Element::new(GENERICS, &format!("gen:lod{}Geometry", options.lod));
                g.set_attribute("", "srsName", &options.srs_name);
                g.set_attribute("", "srsDimension", "3");
                property.push(g);
                object.properties.push(property);
            }
            doc.add_object(object);
        }
    }
    Ok((doc, exporter.report))
}
