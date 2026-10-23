//! Projects CityGML object graphs into the format-neutral [`Document`] model.

use crate::native::*;
use crate::validate::{dimension, elements, polygon, positions, ring_element_points};
use cadkit_core::{
    Attribute, Color, Document, Entity, EntityKind, Error, Format, GroupKind, Layer, LengthUnit,
    Model, ReadOptions, Result, Units, Value, Vertex,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

/// Options controlling which CityGML geometry is projected into the neutral [`Document`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportOptions {
    /// Optional LoD filter (`0..=4`); `None` imports all LoDs, while `Some(lod)` skips other `lodN*` geometry properties.
    pub lod: Option<u8>,
}

impl ImportOptions {
    /// Sets the optional Level of Detail filter (`0..=4`).
    #[must_use]
    pub fn with_lod(mut self, lod: Option<u8>) -> Self {
        self.lod = lod;
        self
    }
}

fn object(e: &Element) -> bool {
    (e.name.namespace == BUILDING
        && matches!(
            e.name.local(),
            "Building"
                | "BuildingPart"
                | "Room"
                | "Door"
                | "Window"
                | "BuildingInstallation"
                | "IntBuildingInstallation"
                | "BuildingFurniture"
                | "WallSurface"
                | "InteriorWallSurface"
                | "RoofSurface"
                | "GroundSurface"
                | "FloorSurface"
                | "CeilingSurface"
                | "ClosureSurface"
                | "OuterFloorSurface"
                | "OuterCeilingSurface"
        ))
        || e.name.is(GENERICS, "GenericCityObject")
        || e.name.is(GROUPS, "CityObjectGroup")
}

fn thematic_layer_color(name: &str) -> Color {
    match name {
        "RoofSurface" => Color::rgb(198, 92, 72),
        "WallSurface" => Color::rgb(212, 206, 194),
        "InteriorWallSurface" => Color::rgb(188, 196, 204),
        "GroundSurface" => Color::rgb(122, 132, 140),
        "FloorSurface" | "OuterFloorSurface" => Color::rgb(168, 152, 132),
        "CeilingSurface" | "OuterCeilingSurface" => Color::rgb(220, 224, 230),
        "ClosureSurface" => Color::rgb(150, 158, 168),
        "Window" => Color::rgb(96, 182, 228),
        "Door" => Color::rgb(196, 142, 78),
        "Room" => Color::rgb(144, 190, 148),
        "BuildingInstallation" | "IntBuildingInstallation" => Color::rgb(164, 140, 196),
        "BuildingFurniture" => Color::rgb(88, 168, 158),
        "Building" | "BuildingPart" => Color::rgb(180, 186, 196),
        _ => Color::ByLayer,
    }
}

fn assign_default_layer(entity: &mut Entity, layer_name: &str) {
    if entity.layer.is_none() {
        entity.layer = Some(layer_name.to_owned());
    }
    if let EntityKind::Group { children, .. } = &mut entity.kind {
        for child in children {
            if child.layer.is_none() {
                assign_default_layer(child, layer_name);
            }
        }
    }
}

/// Extracts an EPSG numeric code from an OGC URN, HTTP URI, or `EPSG:NNNN` string.
fn parse_epsg_code(srs: &str) -> Option<u32> {
    let trimmed = srs.trim();
    let lower = trimmed.to_ascii_lowercase();
    let idx = lower.rfind("epsg")?;
    let tail = trimmed.get(idx + 4..)?.trim_start_matches([':', '/']);
    let digits_end = tail
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(tail.len());
    tail.get(..digits_end)?.parse::<u32>().ok()
}

/// Infers linear coordinate units when `srsName` identifies a well-known projected or
/// geocentric EPSG coordinate reference system; geographic (degree) and unknown CRSs
/// stay [`LengthUnit::Unitless`].
pub(crate) fn infer_srs_units(srs: &str) -> Option<Units> {
    let code = parse_epsg_code(srs)?;
    let unit = match code {
        // Explicit geographic 2D/3D (degrees): keep unitless.
        4326 | 4258 | 4269 | 4979 | 4937 => return None,
        // WGS 84 / ETRS89 / ED50 / TUREF / Web Mercator / national metric grids:
        3857
        | 4978
        | 2056
        | 2154
        | 2180
        | 23028..=23038
        | 2319..=2325
        | 25828..=25838
        | 27700
        | 28992
        | 3003..=3004
        | 3034..=3035
        | 3040..=3051
        | 31254..=31259
        | 31466..=31469
        | 32601..=32660
        | 32701..=32760
        | 5253..=5259 => LengthUnit::Meter,
        // US State Plane NAD83 (US Survey Feet):
        2225..=2284 => LengthUnit::UsSurveyFoot,
        // US State Plane NAD83 (International Feet):
        2222..=2224 | 2868 | 2992 | 2994 => LengthUnit::Foot,
        _ => return None,
    };
    Some(Units {
        unit,
        meters_per_unit: unit.meters(),
    })
}

struct Importer<'a> {
    ids: HashMap<&'a str, &'a Element>,
    options: &'a ImportOptions,
    read: &'a ReadOptions,
    used_layers: BTreeSet<String>,
    next: u64,
    work: u64,
    vertices: u64,
}
impl Importer<'_> {
    fn decode_surface_patches(&mut self, e: &Element, dim: usize) -> Result<Option<EntityKind>> {
        // Tin elements carrying controlPoints without explicit trianglePatches stay Unknown.
        let mut patches_containers = Vec::new();
        for child in e.elements() {
            if child.name.namespace == GML
                && matches!(
                    child.name.local(),
                    "patches" | "polygonPatches" | "trianglePatches"
                )
            {
                patches_containers.push(child);
            } else if child.name.namespace == GML
                && !matches!(
                    child.name.local(),
                    "name" | "description" | "metaDataProperty"
                )
            {
                return Ok(None);
            }
        }
        if patches_containers.is_empty() {
            return Ok(None);
        }
        let mut patch_entities = Vec::new();
        for container in patches_containers {
            let c_dim = dimension(container, dim)?;
            for patch in container.elements() {
                if !patch.name.is(GML, "PolygonPatch") && !patch.name.is(GML, "Triangle") {
                    return Ok(None);
                }
                let p = match polygon(patch, c_dim, &self.read.limits) {
                    Ok(p) => p,
                    Err(Error::Unsupported(_)) => return Ok(None),
                    Err(err) => return Err(err),
                };
                self.vertices = self.vertices.saturating_add(
                    p.exterior.len() as u64
                        + p.interiors.iter().map(|r| r.len() as u64).sum::<u64>(),
                );
                if self.vertices > self.read.limits.max_total_vertices {
                    return Err(Error::LimitExceeded("GML expanded vertices".into()));
                }
                patch_entities.push(self.entity(
                    patch,
                    EntityKind::Polygon {
                        exterior: p.exterior,
                        interiors: p.interiors,
                    },
                )?);
            }
        }
        match patch_entities.len() {
            0 => Ok(None),
            1 => Ok(patch_entities.pop().map(|ent| ent.kind)),
            _ => Ok(Some(EntityKind::Group {
                group_kind: GroupKind::Other,
                name: e.attribute(GML, "id").map(str::to_owned),
                origin: None,
                children: patch_entities,
            })),
        }
    }

    fn visit(&mut self, e: &Element, depth: u32, inherited: usize) -> Result<Vec<Entity>> {
        if depth >= self.read.limits.max_depth || self.work >= self.read.limits.max_objects {
            return Err(Error::LimitExceeded("GML mapping depth/work".into()));
        }
        self.work += 1;
        let dim = dimension(e, inherited)?;
        if let Some(lod) = self.options.lod {
            let local = e.name.local();
            if local.starts_with("lod")
                && local
                    .as_bytes()
                    .get(3)
                    .is_some_and(|v| v.is_ascii_digit() && *v - b'0' != lod)
            {
                return Ok(vec![]);
            }
        }
        if let Some(id) = e.attribute(XLINK, "href").and_then(|s| s.strip_prefix('#')) {
            // Do not expand semantic group or parent links as geometry.
            if e.name.namespace == GML {
                let target = *self
                    .ids
                    .get(id)
                    .ok_or_else(|| Error::invalid(e.offset, "unresolved geometry reference"))?;
                return self.visit(target, depth + 1, dim);
            }
            return Ok(vec![]);
        }
        let kind = if e.name.is(GML, "Polygon") {
            let p = polygon(e, dim, &self.read.limits)?;
            self.vertices = self.vertices.saturating_add(
                p.exterior.len() as u64 + p.interiors.iter().map(|r| r.len() as u64).sum::<u64>(),
            );
            Some(EntityKind::Polygon {
                exterior: p.exterior,
                interiors: p.interiors,
            })
        } else if e.name.is(GML, "LineString") || e.name.is(GML, "Point") {
            let mut points = vec![];
            for p in e.elements() {
                if p.name.is(GML, "pos") || p.name.is(GML, "posList") {
                    points.extend(positions(p, dim, &self.read.limits)?);
                }
            }
            self.vertices = self.vertices.saturating_add(points.len() as u64);
            if e.name.is(GML, "Point") {
                if points.len() != 1 {
                    return Err(Error::invalid(e.offset, "Point requires one position"));
                }
                Some(EntityKind::Point {
                    position: points
                        .first()
                        .copied()
                        .ok_or_else(|| Error::invalid(e.offset, "empty Point"))?,
                })
            } else {
                if points.len() < 2 {
                    return Err(Error::invalid(
                        e.offset,
                        "LineString requires two positions",
                    ));
                }
                Some(EntityKind::Polyline {
                    vertices: points.into_iter().map(Vertex::at).collect(),
                    closed: false,
                    normal: cadkit_core::Vec3::Z,
                })
            }
        } else if e.name.is(GML, "Curve") {
            match crate::curves::read_curve(e, dim, &self.read.limits)? {
                Some((decoded, vcount)) => {
                    self.vertices = self.vertices.saturating_add(vcount);
                    Some(decoded)
                }
                None => Some(EntityKind::Unknown {
                    type_name: "gml.Curve".into(),
                }),
            }
        } else if e.name.is(GML, "Ring") {
            match ring_element_points(e, dim, &self.read.limits) {
                Ok(mut pts) => {
                    self.vertices = self.vertices.saturating_add(pts.len() as u64);
                    if pts.len() > 3 && pts.first() == pts.last() {
                        pts.pop();
                    }
                    Some(EntityKind::Polyline {
                        vertices: pts.into_iter().map(Vertex::at).collect(),
                        closed: true,
                        normal: cadkit_core::Vec3::Z,
                    })
                }
                Err(Error::Unsupported(_)) => Some(EntityKind::Unknown {
                    type_name: "gml.Ring".into(),
                }),
                Err(err) => return Err(err),
            }
        } else if e.name.namespace == GML
            && matches!(
                e.name.local(),
                "Surface" | "Tin" | "TriangulatedSurface" | "PolyhedralSurface"
            )
        {
            let decoded = self.decode_surface_patches(e, dim)?;
            Some(decoded.unwrap_or_else(|| EntityKind::Unknown {
                type_name: format!("gml.{}", e.name.local()),
            }))
        } else {
            None
        };
        if self.vertices > self.read.limits.max_total_vertices {
            return Err(Error::LimitExceeded("GML expanded vertices".into()));
        }
        if let Some(kind) = kind {
            return Ok(vec![self.entity(e, kind)?]);
        }
        let mut children = vec![];
        for child in e.elements() {
            children.extend(self.visit(child, depth + 1, dim)?);
        }
        if e.name.is(GML, "OrientableSurface") && e.attribute("", "orientation") == Some("-") {
            let mut todo: Vec<_> = children.iter_mut().collect();
            while let Some(entity) = todo.pop() {
                match &mut entity.kind {
                    EntityKind::Polygon {
                        exterior,
                        interiors,
                    } => {
                        exterior.reverse();
                        for ring in interiors {
                            ring.reverse();
                        }
                    }
                    EntityKind::Group { children, .. } => todo.extend(children.iter_mut()),
                    _ => {}
                }
            }
        }
        if object(e) {
            let kind = EntityKind::Group {
                group_kind: GroupKind::Other,
                name: e
                    .elements()
                    .find(|n| n.name.is(GML, "name"))
                    .map(Element::text)
                    .or_else(|| e.attribute(GML, "id").map(str::to_owned)),
                origin: None,
                children,
            };
            let mut entity = self.entity(e, kind)?;
            let mut cadkit_layer: Option<String> = None;
            for a in e
                .elements()
                .filter(|a| a.name.namespace == GENERICS && a.name.local().ends_with("Attribute"))
            {
                if let (Some(name), Some(value)) = (
                    a.attribute("", "name"),
                    a.elements().find(|v| v.name.is(GENERICS, "value")),
                ) {
                    let text = value.text();
                    if name == "cadkit.layer" && a.name.local() == "stringAttribute" {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            cadkit_layer = Some(trimmed.to_owned());
                        }
                    }
                    let v =
                        match a.name.local() {
                            "intAttribute" => Value::Int(text.trim().parse().map_err(|_| {
                                Error::invalid(a.offset, "invalid integer attribute")
                            })?),
                            "doubleAttribute" | "measureAttribute" => {
                                Value::Float(text.trim().parse().map_err(|_| {
                                    Error::invalid(a.offset, "invalid numeric attribute")
                                })?)
                            }
                            _ => Value::Text(text),
                        };
                    entity.attributes.push(Attribute {
                        tag: name.into(),
                        value: v,
                        set: Some(a.name.local().into()),
                        position: None,
                        invisible: true,
                        ..Default::default()
                    });
                }
            }
            let layer_name = cadkit_layer.unwrap_or_else(|| e.name.local().to_owned());
            self.used_layers.insert(layer_name.clone());
            assign_default_layer(&mut entity, &layer_name);
            Ok(vec![entity])
        } else {
            Ok(children)
        }
    }
    fn entity(&mut self, e: &Element, kind: EntityKind) -> Result<Entity> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| Error::LimitExceeded("GML entity ids".into()))?;
        let mut entity = Entity::new(kind);
        entity.id = Some(self.next);
        if let Some(id) = e.attribute(GML, "id") {
            entity.props.insert("gml.id".into(), Value::Text(id.into()));
        }
        entity
            .props
            .insert("gml.type".into(), Value::Text(e.name.local().into()));
        Ok(entity)
    }
}

/// Projects CityGML geometry into a neutral [`Document`]; full LoD ownership, shared references, and XML extensions remain in `source`.
pub fn to_document(
    source: &CityGmlDocument,
    options: &ImportOptions,
    read: &ReadOptions,
) -> Result<Document> {
    if options.lod.is_some_and(|n| n > 4) {
        return Err(Error::invalid(0, "LoD must be between 0 and 4"));
    }
    crate::validate(
        source,
        &crate::ValidationOptions {
            limits: read.limits,
            geometry: false,
            ..Default::default()
        },
    )?;
    let nodes = elements(source, &read.limits)?;
    let mut importer = Importer {
        ids: nodes
            .iter()
            .filter_map(|(e, _)| e.attribute(GML, "id").map(|id| (id, *e)))
            .collect(),
        options,
        read,
        used_layers: BTreeSet::new(),
        next: 0,
        work: 0,
        vertices: 0,
    };
    let entities = importer.visit(&source.root, 0, 3)?;
    let mut doc = Document::default();
    doc.source.format = Format::CityGml;
    doc.source.version = "2.0".into();
    doc.layers = importer
        .used_layers
        .into_iter()
        .map(|name| {
            let color = thematic_layer_color(&name);
            Layer {
                name,
                color,
                visible: true,
                plottable: true,
                ..Layer::default()
            }
        })
        .collect();
    doc.warnings.push(cadkit_core::Warning {code:"gml.neutral_projection".into(), message:"Neutral geometry is a projection; use CityGmlDocument to preserve LoD ownership, shared references and XML extensions".into(), offset:None, object:None});
    doc.models.push(Model {
        name: "CityModel".into(),
        is_3d: true,
        entities,
        ..Default::default()
    });
    if let Some(srs) = nodes.iter().find_map(|(e, _)| e.attribute("", "srsName")) {
        if let Some(units) = infer_srs_units(srs) {
            doc.units = units;
        }
        doc.props
            .insert("gml.srs_name".into(), Value::Text(srs.into()));
    }
    Ok(doc)
}
