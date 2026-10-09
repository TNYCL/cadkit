//! CityGML ilişkilerinden nötr geometriye ve açık geometri eşlemesiyle geri dönüşüm.

use crate::native::*;
use crate::validate::{dimension, elements, polygon, positions};
use cadkit_core::{
    Attribute, Document, Entity, EntityKind, Error, Format, GroupKind, Model, ReadOptions, Result,
    Value, Vertex,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Nötr görünüme alınacak ayrıntı düzeyi.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportOptions {
    /// `None` bütün LoD'ları tutar; seçilen LoD dışındaki geometri özellikleri atlanır.
    pub lod: Option<u8>,
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

struct Importer<'a> {
    ids: HashMap<&'a str, &'a Element>,
    options: &'a ImportOptions,
    read: &'a ReadOptions,
    next: u64,
    work: u64,
    vertices: u64,
}
impl Importer<'_> {
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
            // Semantik grup/parent bağlantıları geometri olarak genişletilmez.
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
            let circle = crate::curves::read_circle(e, dim, &self.read.limits)?;
            if circle.is_some() {
                self.vertices = self.vertices.saturating_add(3);
            }
            Some(circle.unwrap_or_else(|| EntityKind::Unknown {
                type_name: "gml.Curve".into(),
            }))
        } else if e.name.namespace == GML
            && matches!(
                e.name.local(),
                "Surface" | "Tin" | "TriangulatedSurface" | "PolyhedralSurface" | "Ring"
            )
        {
            Some(EntityKind::Unknown {
                type_name: format!("gml.{}", e.name.local()),
            })
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
            for a in e
                .elements()
                .filter(|a| a.name.namespace == GENERICS && a.name.local().ends_with("Attribute"))
            {
                if let (Some(name), Some(value)) = (
                    a.attribute("", "name"),
                    a.elements().find(|v| v.name.is(GENERICS, "value")),
                ) {
                    let text = value.text();
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

/// Geometriyi dönüştürür; özgün nesne ağı ve XML ayrıntıları kaynak belgede kalır.
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
        next: 0,
        work: 0,
        vertices: 0,
    };
    let entities = importer.visit(&source.root, 0, 3)?;
    let mut doc = Document::default();
    doc.source.format = Format::CityGml;
    doc.source.version = "2.0".into();
    doc.warnings.push(cadkit_core::Warning {code:"gml.neutral_projection".into(), message:"Neutral geometry is a projection; use CityGmlDocument to preserve LoD ownership, shared references and XML extensions".into(), offset:None, object:None});
    doc.models.push(Model {
        name: "CityModel".into(),
        is_3d: true,
        entities,
        ..Default::default()
    });
    if let Some(srs) = nodes.iter().find_map(|(e, _)| e.attribute("", "srsName")) {
        doc.props
            .insert("gml.srs_name".into(), Value::Text(srs.into()));
    }
    Ok(doc)
}
