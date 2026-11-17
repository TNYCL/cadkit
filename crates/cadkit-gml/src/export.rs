//! Explicit CAD geometry projection and conversion diagnostics.

use crate::native::*;
use cadkit_core::LimitKind;
use cadkit_core::{Document, Entity, EntityKind, Error, Point3, Result, Value};
use serde::{Deserialize, Serialize};

/// Coordinate, LoD, and resource-limit contract for CityGML export.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ExportOptions {
    /// Identifier of the existing CRS of the source coordinates; no coordinate reprojection is performed.
    pub srs_name: String,
    /// Level of Detail (`0..=4`) written on generic city object geometry.
    pub lod: u8,
    /// Resource limits enforced before writing and on the serialized XML output.
    #[serde(default)]
    pub limits: cadkit_core::Limits,
    /// Write closed, straight (bulge- and width-free) polylines with at least three
    /// distinct vertices as `gml:Polygon` surfaces when they are planar and simple.
    /// Others stay `gml:LineString` and are reported. Default `false`: closure alone
    /// does not assert an area, so the historical output is unchanged.
    #[serde(default)]
    pub closed_polylines_as_surfaces: bool,
    /// Largest distance of a ring point from the face plane, as a fraction of the face's
    /// bounding-box diagonal (default `1e-3`), for faces this crate turns into surfaces.
    #[serde(default = "default_planarity_tolerance")]
    pub planarity_tolerance: f64,
    /// Ceiling for that distance in coordinate units (default `0.01`, the default
    /// [`crate::ValidationOptions::planarity_tolerance`]), so accepted surfaces pass
    /// default validation. Larger values need the same validation tolerance on read.
    #[serde(default = "default_max_planarity_deviation")]
    pub max_planarity_deviation: f64,
}

fn default_planarity_tolerance() -> f64 {
    crate::surface::DEFAULT_PLANARITY_TOLERANCE
}

fn default_max_planarity_deviation() -> f64 {
    crate::surface::DEFAULT_MAX_PLANARITY_DEVIATION
}

impl ExportOptions {
    /// Creates CityGML export options for `srs_name` and `lod` (`0..=4`) with default limits.
    #[must_use]
    pub fn new(srs_name: impl Into<String>, lod: u8) -> Self {
        Self {
            srs_name: srs_name.into(),
            lod,
            limits: cadkit_core::Limits::default(),
            closed_polylines_as_surfaces: false,
            planarity_tolerance: default_planarity_tolerance(),
            max_planarity_deviation: default_max_planarity_deviation(),
        }
    }

    /// Replaces the CRS identifier written to `srsName`.
    #[must_use]
    pub fn with_srs_name(mut self, srs_name: impl Into<String>) -> Self {
        self.srs_name = srs_name.into();
        self
    }

    /// Replaces the target Level of Detail (`0..=4`).
    #[must_use]
    pub fn with_lod(mut self, lod: u8) -> Self {
        self.lod = lod;
        self
    }

    /// Replaces the resource limits enforced during export.
    #[must_use]
    pub fn with_limits(mut self, limits: cadkit_core::Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Sets [`Self::closed_polylines_as_surfaces`].
    #[must_use]
    pub fn with_closed_polylines_as_surfaces(mut self, enabled: bool) -> Self {
        self.closed_polylines_as_surfaces = enabled;
        self
    }

    /// Sets [`Self::planarity_tolerance`] (fraction of the face's bounding-box diagonal).
    #[must_use]
    pub fn with_planarity_tolerance(mut self, relative: f64) -> Self {
        self.planarity_tolerance = relative;
        self
    }

    /// Sets [`Self::max_planarity_deviation`] (coordinate units).
    #[must_use]
    pub fn with_max_planarity_deviation(mut self, deviation: f64) -> Self {
        self.max_planarity_deviation = deviation;
        self
    }

    pub(crate) fn planarity(&self) -> crate::surface::Planarity {
        crate::surface::Planarity {
            relative: self.planarity_tolerance,
            absolute: self.max_planarity_deviation,
        }
    }
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
    /// Stable diagnostic code: `gml.metadata_only`, or with
    /// [`ExportOptions::closed_polylines_as_surfaces`] a `gml.face_*` reason
    /// (`gml.face_not_planar`, `gml.face_degenerate`, `gml.face_self_intersecting`,
    /// `gml.face_invalid`) for a closed polyline written as a line string instead.
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
    /// One diagnostic for each metadata-only entity, in source traversal order, followed
    /// per top-level entity by closed polylines that could not become surfaces.
    pub issues: Vec<ExportIssue>,
}

struct Exporter {
    unsupported: UnsupportedGeometry,
    report: ExportReport,
    max_depth: u32,
    surfaces: Option<crate::surface::Planarity>,
    work: u64,
    /// Surface diagnostics kept apart from metadata-only issues: they do not make the
    /// object's geometry partial.
    pending: Vec<ExportIssue>,
}

impl Exporter {
    fn geometry(
        &mut self,
        e: &Entity,
        model: usize,
        path: &mut Vec<usize>,
    ) -> Result<Option<Element>> {
        if path.len() as u64 > u64::from(self.max_depth.min(64)) {
            return Err(Error::limit(
                LimitKind::Depth,
                "CityGML export depth".into(),
            ));
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
        } else if let Some(polygon) = self.polyline_surface(e, model, path)? {
            Ok(polygon)
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

impl Exporter {
    /// The `gml:Polygon` of a closed, straight, planar polyline when surfaces are enabled.
    fn polyline_surface(
        &mut self,
        e: &Entity,
        model: usize,
        path: &[usize],
    ) -> Result<Option<Element>> {
        let (
            Some(planarity),
            EntityKind::Polyline {
                vertices, closed, ..
            },
        ) = (self.surfaces, &e.kind)
        else {
            return Ok(None);
        };
        // Open, curved or wide polylines keep their existing line-string handling.
        let Some(Ok(ring)) = crate::surface::polyline_ring(vertices, *closed) else {
            return Ok(None);
        };
        match crate::surface::prepare(ring, vec![], planarity, &mut self.work)? {
            Ok(surface) => Ok(Some(
                Polygon {
                    id: None,
                    exterior: surface.exterior,
                    interiors: vec![],
                }
                .to_element(),
            )),
            Err(reject) => {
                self.pending.push(ExportIssue {
                    code: reject.code().into(),
                    model,
                    path: path.to_vec(),
                    entity_type: e.kind.type_name().into(),
                });
                Ok(None)
            }
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
            if *closed
                && points.first() != points.last()
                && let Some(first) = points.first().copied()
            {
                points.push(first);
            }
            // Closure alone does not assert a planar surface. CAD paths may cross
            // themselves or leave the plane; only explicit Polygon/Face means area,
            // unless the caller opts in with `closed_polylines_as_surfaces`.
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
            if ring.first() != ring.last()
                && let Some(first) = ring.first().copied()
            {
                ring.push(first);
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
                if ring.first() != ring.last()
                    && let Some(first) = ring.first().copied()
                {
                    ring.push(first);
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
        EntityKind::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            normal,
        } => crate::curves::arc(*center, *radius, *start_angle, *end_angle, *normal),
        other => Err(Error::Unsupported(format!(
            "CityGML export for {} requires explicit geometry/semantic mapping",
            other.type_name()
        ))),
    }
}

/// Builds one `gen:GenericCityObject` per top-level neutral entity, rejecting unsupported geometry.
/// Coordinates are written unchanged; matching the declared CRS and units is the caller's responsibility.
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
    if options.closed_polylines_as_surfaces {
        options.planarity().check()?;
    }
    let mut doc = CityGmlDocument::new();
    let mut exporter = Exporter {
        unsupported,
        report: ExportReport::default(),
        max_depth: options.limits.max_depth,
        surfaces: options
            .closed_polylines_as_surfaces
            .then(|| options.planarity()),
        // The writer validates with the default geometry budget; spend no more here.
        work: crate::ValidationOptions::default().max_geometry_work,
        pending: vec![],
    };
    let mut next = 0u64;
    for (model_index, model) in source.models.iter().enumerate() {
        for (entity_index, e) in model.entities.iter().enumerate() {
            next = next
                .checked_add(1)
                .ok_or_else(|| Error::limit(LimitKind::Objects, "CityGML export objects".into()))?;
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
                match generic_attribute(&a.tag, &a.value) {
                    Ok(attr) => object.properties.push(attr),
                    Err(Error::Unsupported(_))
                        if unsupported == UnsupportedGeometry::MetadataOnly =>
                    {
                        match &a.value {
                            Value::Bool(b) => object
                                .properties
                                .push(generic_attribute(&a.tag, &Value::Text(b.to_string()))?),
                            Value::List(_) => {
                                let json = serde_json::to_string(&a.value)
                                    .map_err(|err| Error::invalid(0, err.to_string()))?;
                                object
                                    .properties
                                    .push(generic_attribute(&a.tag, &Value::Text(json))?);
                            }
                            Value::Null | Value::Bytes(_) => {}
                            _ => {}
                        }
                    }
                    Err(err) => return Err(err),
                }
            }
            // `validate_limits` has already verified that every float in `source` is finite.
            let metadata =
                serde_json::to_string(e).map_err(|err| Error::invalid(0, err.to_string()))?;
            if metadata.len() as u64 > u64::from(options.limits.max_string_bytes) {
                return Err(Error::limit(
                    LimitKind::StringBytes,
                    "CityGML entity metadata bytes".into(),
                ));
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
            exporter.report.issues.append(&mut exporter.pending);
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
