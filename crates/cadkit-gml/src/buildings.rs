//! CityGML 2.0 buildings with thematic boundary surfaces, built from the planar faces of
//! one model of a neutral [`Document`].
//!
//! Every face becomes one `bldg:WallSurface`, `bldg:RoofSurface` or
//! `bldg:GroundSurface` by the tilt of its normal; see [`BuildingOptions`] for the rule.
//! Coordinates are written unchanged in the caller's CRS.

use crate::native::*;
use crate::surface::{self, Reject, Surface};
use crate::{ExportOptions, ValidationOptions};
use cadkit_core::ops::{WalkContext, WalkOptions};
use cadkit_core::{Document, Entity, EntityKind, Error, LimitKind, Point3, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

/// How faces are grouped into `bldg:Building` objects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BuildingGrouping {
    /// All faces of the model form one building.
    #[default]
    Single,
    /// One building per effective layer (an entity on layer `"0"` inside a block takes
    /// the layer of the inserting entity).
    Layer,
    /// One building per top-level entity of the model: a cell or group, a block
    /// reference with everything it draws, or a single face drawn at the top level.
    TopLevel,
    /// One building per connected component: faces that share a vertex within
    /// [`BuildingOptions::weld_tolerance`], found with a spatial hash grid.
    Connected,
}

/// Options of [`buildings_from_document`] and [`write_buildings`].
///
/// Classification uses the tilt of a face, the angle between its normal and the
/// vertical axis (0° horizontal, 90° vertical), which does not depend on winding:
///
/// - tilt ≥ [`Self::wall_min_tilt_degrees`]: `WallSurface`;
/// - tilt ≤ [`Self::horizontal_max_tilt_degrees`]: `GroundSurface` when the face's lowest
///   point lies within [`Self::ground_tolerance`] of the lowest point of its building,
///   `RoofSurface` otherwise;
/// - in between (sloped): `RoofSurface` when the face's lowest point lies above that
///   band; a sloped face reaching the base (a ramp, a battered wall, an A-frame roof)
///   stays unclassified: it is written into the building's `lodNMultiSurface` and reported.
///
/// Both thresholds are inclusive; a face drawn exactly at one (to within 1e-9 in the
/// cosine of the tilt, the rounding of a normal at map coordinates) belongs to it. The
/// base is the lowest point of each building, so a building of a single horizontal
/// face (a roof drawn as its own top-level entity under [`BuildingGrouping::TopLevel`])
/// is a `GroundSurface`.
///
/// Ground faces are wound so their normal points down, roof faces up; walls keep their
/// drawn winding. Interior floors, soffits and overhang undersides are horizontal faces
/// above the base and so become roofs: the rule knows geometry, not use.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BuildingOptions {
    /// CRS identifier, LoD (2, 3 or 4: CityGML 2.0 has boundary surfaces only from LoD 2),
    /// limits, and the surface options [`ExportOptions::closed_polylines_as_surfaces`]
    /// and planarity tolerances. No coordinate is reprojected.
    pub export: ExportOptions,
    /// Grouping of faces into buildings (default [`BuildingGrouping::Single`]).
    #[serde(default)]
    pub grouping: BuildingGrouping,
    /// Smallest tilt, in degrees, of a wall (default 80).
    #[serde(default = "default_wall_min_tilt")]
    pub wall_min_tilt_degrees: f64,
    /// Largest tilt, in degrees, of a horizontal face (default 10).
    #[serde(default = "default_horizontal_max_tilt")]
    pub horizontal_max_tilt_degrees: f64,
    /// Height band above a building's lowest point that counts as its base, in
    /// coordinate units (default 0.01, 1 cm in a metric CRS).
    #[serde(default = "default_ground_tolerance")]
    pub ground_tolerance: f64,
    /// Distance within which two vertices are shared for
    /// [`BuildingGrouping::Connected`], in coordinate units (default 0.001); 0 means equal.
    #[serde(default = "default_weld_tolerance")]
    pub weld_tolerance: f64,
    /// Work budget, applied separately to the model walk (as
    /// [`WalkOptions::max_work`]), to face validation and to vertex welding, and as the
    /// geometry budget of the validation run by [`write_buildings`]
    /// (default 100,000,000).
    #[serde(default = "default_max_work")]
    pub max_work: u64,
}

fn default_wall_min_tilt() -> f64 {
    80.0
}
fn default_horizontal_max_tilt() -> f64 {
    10.0
}
fn default_ground_tolerance() -> f64 {
    0.01
}
fn default_weld_tolerance() -> f64 {
    0.001
}
fn default_max_work() -> u64 {
    100_000_000
}

impl BuildingOptions {
    /// Building options with the defaults above around `export`.
    #[must_use]
    pub fn new(export: ExportOptions) -> Self {
        Self {
            export,
            grouping: BuildingGrouping::default(),
            wall_min_tilt_degrees: default_wall_min_tilt(),
            horizontal_max_tilt_degrees: default_horizontal_max_tilt(),
            ground_tolerance: default_ground_tolerance(),
            weld_tolerance: default_weld_tolerance(),
            max_work: default_max_work(),
        }
    }

    /// Sets [`Self::grouping`].
    #[must_use]
    pub fn with_grouping(mut self, grouping: BuildingGrouping) -> Self {
        self.grouping = grouping;
        self
    }

    /// Sets [`Self::wall_min_tilt_degrees`].
    #[must_use]
    pub fn with_wall_min_tilt_degrees(mut self, degrees: f64) -> Self {
        self.wall_min_tilt_degrees = degrees;
        self
    }

    /// Sets [`Self::horizontal_max_tilt_degrees`].
    #[must_use]
    pub fn with_horizontal_max_tilt_degrees(mut self, degrees: f64) -> Self {
        self.horizontal_max_tilt_degrees = degrees;
        self
    }

    /// Sets [`Self::ground_tolerance`] (coordinate units).
    #[must_use]
    pub fn with_ground_tolerance(mut self, tolerance: f64) -> Self {
        self.ground_tolerance = tolerance;
        self
    }

    /// Sets [`Self::weld_tolerance`] (coordinate units).
    #[must_use]
    pub fn with_weld_tolerance(mut self, tolerance: f64) -> Self {
        self.weld_tolerance = tolerance;
        self
    }

    /// Sets [`Self::max_work`].
    #[must_use]
    pub fn with_max_work(mut self, max_work: u64) -> Self {
        self.max_work = max_work;
        self
    }

    fn check(&self) -> Result<()> {
        let e = &self.export;
        if e.srs_name.trim().is_empty() || !(2..=4).contains(&e.lod) {
            return Err(Error::invalid(
                0,
                "CityGML buildings require an explicit CRS and LoD 2, 3 or 4",
            ));
        }
        e.planarity().check()?;
        let (wall, flat) = (self.wall_min_tilt_degrees, self.horizontal_max_tilt_degrees);
        if !(flat.is_finite() && wall.is_finite() && 0.0 <= flat && flat < wall && wall <= 90.0) {
            return Err(Error::invalid(
                0,
                "tilt thresholds need 0 <= horizontal_max_tilt_degrees < wall_min_tilt_degrees <= 90",
            ));
        }
        if !(self.ground_tolerance.is_finite() && self.ground_tolerance >= 0.0)
            || !(self.weld_tolerance.is_finite() && self.weld_tolerance >= 0.0)
        {
            return Err(Error::invalid(
                0,
                "ground and weld tolerances must be finite and not negative",
            ));
        }
        Ok(())
    }
}

/// Per-building counts in [`BuildingReport::buildings`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BuildingSummary {
    /// `gml:id` of the `bldg:Building`.
    pub id: String,
    /// The layer of [`BuildingGrouping::Layer`].
    pub layer: Option<String>,
    /// The model entity index of [`BuildingGrouping::TopLevel`].
    pub top_level: Option<usize>,
    /// `WallSurface` objects.
    pub wall_surfaces: u64,
    /// `RoofSurface` objects.
    pub roof_surfaces: u64,
    /// `GroundSurface` objects.
    pub ground_surfaces: u64,
    /// Faces written into the building's `lodNMultiSurface` without a class.
    pub unclassified_faces: u64,
}

/// A face that was skipped or kept without a class, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BuildingIssue {
    /// Stable code. Skipped: `gml.face_degenerate`, `gml.face_not_planar`,
    /// `gml.face_self_intersecting`, `gml.face_invalid` (non-finite coordinates or a bad
    /// hole), `gml.face_curved` (bulges), `gml.face_wide` (polyline widths),
    /// `gml.face_index_out_of_range` (mesh face). Kept unclassified:
    /// `gml.face_sloped_at_base`.
    pub code: String,
    /// Source entity as [`WalkContext::path`]; resolve it with [`Document::entity_at`].
    /// Every cell of an insert array has the same path.
    pub path: Vec<usize>,
    /// [`WalkContext::model_path_len`]: the leading part of `path` that addresses an entity
    /// of the model itself (the outermost block reference for geometry inside a block).
    pub model_path_len: usize,
    /// Index of the face in a `Mesh` entity.
    pub mesh_face: Option<usize>,
    /// The entity's native id ([`Entity::id`]).
    pub entity_id: Option<u64>,
    /// Effective layer name.
    pub layer: String,
    /// Neutral entity kind, as [`EntityKind::type_name`].
    pub entity_type: String,
    /// `gml:id` of the building that holds the face, when it was written.
    pub building_id: Option<String>,
    /// `gml:id` of the written `gml:Polygon`.
    pub polygon_id: Option<String>,
}

/// What [`buildings_from_document`] produced and left out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BuildingReport {
    /// Source model index.
    pub model: usize,
    /// Written buildings in output order.
    pub buildings: Vec<BuildingSummary>,
    /// `WallSurface` objects in all buildings.
    pub wall_surfaces: u64,
    /// `RoofSurface` objects in all buildings.
    pub roof_surfaces: u64,
    /// `GroundSurface` objects in all buildings.
    pub ground_surfaces: u64,
    /// Faces written without a class.
    pub unclassified_faces: u64,
    /// Face candidates not written.
    pub skipped_faces: u64,
    /// Skipped faces per issue code.
    pub skipped_by_code: BTreeMap<String, u64>,
    /// Visited entities that are not face candidates (lines, open polylines, text, ...).
    pub ignored_entities: u64,
    /// Closed polylines not considered because
    /// [`ExportOptions::closed_polylines_as_surfaces`] is off.
    pub closed_polylines_ignored: u64,
    /// Skipped faces in traversal order, then per building in output order its
    /// unclassified faces (and, rarely, a face whose rewound ring no longer passes the
    /// planarity check, skipped as `gml.face_not_planar`).
    pub issues: Vec<BuildingIssue>,
    /// More than min(`max_objects`, 100,000) issues: the rest are only counted.
    pub issues_truncated: bool,
}

/// Where a face candidate came from.
struct Origin<'a> {
    entity: &'a Entity,
    path: Vec<usize>,
    model_path_len: usize,
    layer: usize,
}

/// A face's ring before checks, with its mesh face index.
type Candidate = (Option<usize>, std::result::Result<Vec<Point3>, Reject>);

struct Face {
    surface: Surface,
    origin: usize,
    mesh_face: Option<usize>,
    group: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Wall,
    Roof,
    Ground,
    Unclassified,
}

struct Collector<'a> {
    options: &'a BuildingOptions,
    work: u64,
    layers: Vec<String>,
    layer_index: HashMap<String, usize>,
    origins: Vec<Origin<'a>>,
    faces: Vec<Face>,
    positions: u64,
    report: BuildingReport,
    issue_limit: usize,
    error: Option<Error>,
}

impl<'a> Collector<'a> {
    fn visit(&mut self, e: &'a Entity, ctx: &WalkContext<'a>) -> bool {
        match self.entity(e, ctx) {
            Ok(()) => true,
            Err(err) => {
                self.error = Some(err);
                false
            }
        }
    }

    fn entity(&mut self, e: &'a Entity, ctx: &WalkContext<'a>) -> Result<()> {
        let rings: Vec<Candidate> = match &e.kind {
            EntityKind::Polygon { exterior, .. } => vec![(None, Ok(exterior.clone()))],
            EntityKind::Face { points, .. } => vec![(None, Ok(points.clone()))],
            EntityKind::Mesh { vertices, faces } => faces
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let ring = f
                        .iter()
                        .map(|&v| vertices.get(v as usize).copied())
                        .collect::<Option<Vec<_>>>()
                        .ok_or(Reject::IndexOutOfRange);
                    (Some(i), ring)
                })
                .collect(),
            EntityKind::Polyline {
                vertices, closed, ..
            } => match surface::polyline_ring(vertices, *closed) {
                None => {
                    self.report.ignored_entities += 1;
                    return Ok(());
                }
                Some(_) if !self.options.export.closed_polylines_as_surfaces => {
                    self.report.closed_polylines_ignored += 1;
                    return Ok(());
                }
                Some(ring) => vec![(None, ring)],
            },
            _ => {
                self.report.ignored_entities += 1;
                return Ok(());
            }
        };
        let layer = self.layer(ctx.layer_name(e));
        let origin = self.origins.len();
        self.origins.push(Origin {
            entity: e,
            path: ctx.path(),
            model_path_len: ctx.model_path_len(),
            layer,
        });
        let holes: &[Vec<Point3>] = match &e.kind {
            EntityKind::Polygon { interiors, .. } => interiors,
            _ => &[],
        };
        let limits = &self.options.export.limits;
        for (mesh_face, ring) in rings {
            let ring = match ring {
                Ok(ring) => ring,
                Err(reject) => {
                    self.skip(reject, origin, mesh_face);
                    continue;
                }
            };
            let count = holes
                .iter()
                .fold(ring.len(), |n, h| n.saturating_add(h.len()));
            self.positions = self.positions.saturating_add(count as u64);
            if self.positions > limits.max_total_vertices {
                return Err(Error::limit(
                    LimitKind::TotalVertices,
                    "CityGML building face vertices".into(),
                ));
            }
            if self.faces.len() as u64 >= limits.max_objects {
                return Err(Error::limit(
                    LimitKind::Objects,
                    "CityGML building faces".into(),
                ));
            }
            let tf = &ctx.transform;
            let exterior = ring.into_iter().map(|p| tf.apply_point(p)).collect();
            let interiors = holes
                .iter()
                .map(|h| h.iter().map(|&p| tf.apply_point(p)).collect())
                .collect();
            match surface::prepare(
                exterior,
                interiors,
                self.options.export.planarity(),
                &mut self.work,
            )? {
                Ok(surface) => {
                    let group = match self.options.grouping {
                        BuildingGrouping::Layer => layer,
                        BuildingGrouping::TopLevel => self
                            .origins
                            .get(origin)
                            .and_then(|o| o.path.first().copied())
                            .unwrap_or(0),
                        BuildingGrouping::Single | BuildingGrouping::Connected => 0,
                    };
                    self.faces.push(Face {
                        surface,
                        origin,
                        mesh_face,
                        group,
                    });
                }
                Err(reject) => self.skip(reject, origin, mesh_face),
            }
        }
        Ok(())
    }

    fn layer(&mut self, name: &str) -> usize {
        if let Some(&i) = self.layer_index.get(name) {
            return i;
        }
        let i = self.layers.len();
        self.layers.push(name.to_owned());
        self.layer_index.insert(name.to_owned(), i);
        i
    }

    fn skip(&mut self, reject: Reject, origin: usize, mesh_face: Option<usize>) {
        self.report.skipped_faces += 1;
        *self
            .report
            .skipped_by_code
            .entry(reject.code().to_owned())
            .or_default() += 1;
        self.issue(reject.code(), origin, mesh_face, None, None);
    }

    fn issue(
        &mut self,
        code: &str,
        origin: usize,
        mesh_face: Option<usize>,
        building_id: Option<String>,
        polygon_id: Option<String>,
    ) {
        if self.report.issues.len() >= self.issue_limit {
            self.report.issues_truncated = true;
            return;
        }
        let Some(o) = self.origins.get(origin) else {
            return;
        };
        self.report.issues.push(BuildingIssue {
            code: code.into(),
            path: o.path.clone(),
            model_path_len: o.model_path_len,
            mesh_face,
            entity_id: o.entity.id,
            layer: self.layers.get(o.layer).cloned().unwrap_or_default(),
            entity_type: o.entity.kind.type_name().into(),
            building_id,
            polygon_id,
        });
    }

    /// Union-find over faces that share a welded vertex; the group of a face becomes the
    /// smallest face index of its component, so groups follow traversal order.
    fn connect(&mut self) -> Result<()> {
        let mut parent: Vec<usize> = (0..self.faces.len()).collect();
        let mut welder = crate::geometry::VertexWelder::new(self.options.weld_tolerance);
        let mut owner: Vec<usize> = vec![];
        let mut work = self.options.max_work;
        for (index, face) in self.faces.iter().enumerate() {
            let s = &face.surface;
            for ring in std::iter::once(&s.exterior).chain(s.interiors.iter()) {
                // The closing point repeats the first one.
                let open = ring.split_last().map_or(&[][..], |(_, rest)| rest);
                for &p in open {
                    let before = welder.points.len();
                    let rep = welder.intern(p, &mut work)? as usize;
                    if rep == before {
                        owner.push(index);
                    } else if let Some(&other) = owner.get(rep) {
                        union(&mut parent, index, other);
                    }
                }
            }
        }
        for index in 0..self.faces.len() {
            let root = find(&mut parent, index);
            if let Some(face) = self.faces.get_mut(index) {
                face.group = root;
            }
        }
        Ok(())
    }
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while let Some(&p) = parent.get(x) {
        if p == x {
            break;
        }
        let grand = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(x) {
            *slot = grand;
        }
        x = grand;
    }
    x
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    let (low, high) = if ra <= rb { (ra, rb) } else { (rb, ra) };
    if let Some(slot) = parent.get_mut(high) {
        *slot = low;
    }
}

fn xyz(out: &mut String, p: Point3) {
    // f64 Display is the shortest text that parses back to the same value.
    let _ = write!(out, "{} {} {}", p.x, p.y, p.z);
}

fn envelope(srs: &str, lo: Point3, hi: Point3) -> Element {
    let mut corners = [String::new(), String::new()];
    let [low, high] = &mut corners;
    xyz(low, lo);
    xyz(high, hi);
    let [low, high] = corners;
    let mut env = Element::new(GML, "gml:Envelope");
    env.set_attribute("", "srsName", srs);
    env.set_attribute("", "srsDimension", "3");
    env.push(Element::with_text(GML, "gml:lowerCorner", low));
    env.push(Element::with_text(GML, "gml:upperCorner", high));
    let mut bounded = Element::new(GML, "gml:boundedBy");
    bounded.push(env);
    bounded
}

fn polygon(id: &str, s: &Surface) -> Element {
    let mut p = Element::new(GML, "gml:Polygon");
    p.set_attribute(GML, "gml:id", id);
    for (i, ring) in std::iter::once(&s.exterior)
        .chain(s.interiors.iter())
        .enumerate()
    {
        let mut text = String::with_capacity(ring.len().saturating_mul(48));
        for (j, &point) in ring.iter().enumerate() {
            if j > 0 {
                text.push(' ');
            }
            xyz(&mut text, point);
        }
        let mut list = Element::with_text(GML, "gml:posList", text);
        list.set_attribute("", "srsDimension", "3");
        let mut linear = Element::new(GML, "gml:LinearRing");
        linear.push(list);
        let mut boundary = Element::new(
            GML,
            if i == 0 {
                "gml:exterior"
            } else {
                "gml:interior"
            },
        );
        boundary.push(linear);
        p.push(boundary);
    }
    p
}

fn multi_surface(lod: u8, polygons: Vec<Element>) -> Element {
    let mut multi = Element::new(GML, "gml:MultiSurface");
    for p in polygons {
        let mut member = Element::new(GML, "gml:surfaceMember");
        member.push(p);
        multi.push(member);
    }
    let mut property = Element::new(BUILDING, &format!("bldg:lod{lod}MultiSurface"));
    property.push(multi);
    property
}

fn extend(bounds: &mut Option<(Point3, Point3)>, s: &Surface) {
    for p in std::iter::once(&s.exterior)
        .chain(s.interiors.iter())
        .flatten()
    {
        let (lo, hi) = bounds.get_or_insert((*p, *p));
        *lo = Point3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        *hi = Point3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
}

/// Rounding margin of the tilt thresholds, in cosine. The normal of a vertical face at
/// map coordinates (1e6 to 1e7) is vertical only to about 1e-11; the margin is about
/// 6e-8 degrees at a 90 degree threshold and 0.003 degrees at 0 degrees, below drawing
/// precision.
const TILT_COSINE_MARGIN: f64 = 1e-9;

/// Elements written per face (boundedBy, surface, lodNMultiSurface, MultiSurface,
/// surfaceMember, Polygon, exterior, LinearRing, posList and its text) and per building
/// (member, Building, envelope with corners and texts, lodNMultiSurface, MultiSurface),
/// and per hole (interior, LinearRing, posList and its text).
const NODES_PER_FACE: u64 = 10;
const NODES_PER_BUILDING: u64 = 10;
const NODES_PER_HOLE: u64 = 4;

/// Builds CityGML 2.0 buildings from the surfaces of `source.models[model]`: `Polygon`
/// (holes kept), `Face` and `Mesh` faces and, with
/// [`ExportOptions::closed_polylines_as_surfaces`], closed straight polylines, including
/// geometry drawn through block references and groups (as [`Document::walk`] visits it;
/// hidden layers and invisible entities are skipped). Faces that are degenerate, not
/// planar within the export tolerances or self-intersecting are skipped and reported, so
/// the result passes [`crate::validate`] with default geometry checks.
///
/// Each building gets a `gml:boundedBy` envelope, its faces as
/// `bldg:boundedBy/bldg:{Wall,Roof,Ground}Surface/bldg:lodNMultiSurface` (one surface per
/// face) and unclassified faces in its own `bldg:lodNMultiSurface`; the model gets a 3D
/// envelope in [`ExportOptions::srs_name`]. Ids are `cadkit_bldg_{n}`, surfaces
/// `cadkit_bldg_{n}_s{k}` and polygons `cadkit_bldg_{n}_p{k}`, numbered in traversal
/// order, so equal input gives equal output. No TKGM attribute, name, storey or class is
/// invented. A model without faces gives an empty `CityModel` and a report that says why.
pub fn buildings_from_document(
    source: &Document,
    model: usize,
    options: &BuildingOptions,
) -> Result<(CityGmlDocument, BuildingReport)> {
    options.check()?;
    if source.models.get(model).is_none() {
        return Err(Error::invalid(0, "model index out of range"));
    }
    let limits = options.export.limits;
    cadkit_core::export::validate_limits(source, &limits)?;
    let mut c = Collector {
        options,
        work: options.max_work,
        layers: vec![],
        layer_index: HashMap::new(),
        origins: vec![],
        faces: vec![],
        positions: 0,
        report: BuildingReport {
            model,
            ..BuildingReport::default()
        },
        issue_limit: limits.max_objects.min(100_000) as usize,
        error: None,
    };
    let walk = WalkOptions::new()
        .with_max_depth(limits.max_depth)
        .with_max_work(options.max_work);
    let stats = source.walk_while(model, &walk, |e, ctx| c.visit(e, ctx));
    if let Some(error) = c.error.take() {
        return Err(error);
    }
    if stats.truncated {
        return Err(Error::limit(
            LimitKind::Work,
            "CityGML building model walk".into(),
        ));
    }
    if options.grouping == BuildingGrouping::Connected {
        c.connect()?;
    }

    // Buildings in order of their first face.
    let mut group_index: HashMap<usize, usize> = HashMap::new();
    let mut members: Vec<Vec<usize>> = vec![];
    for (index, face) in c.faces.iter().enumerate() {
        let next = members.len();
        let b = *group_index.entry(face.group).or_insert(next);
        if b == next {
            members.push(vec![]);
        }
        if let Some(list) = members.get_mut(b) {
            list.push(index);
        }
    }
    let holes = c.faces.iter().fold(0u64, |n, f| {
        n.saturating_add(f.surface.interiors.len() as u64)
    });
    let nodes = (members.len() as u64)
        .saturating_mul(NODES_PER_BUILDING)
        .saturating_add((c.faces.len() as u64).saturating_mul(NODES_PER_FACE))
        .saturating_add(holes.saturating_mul(NODES_PER_HOLE))
        .saturating_add(8);
    if nodes > limits.max_objects {
        return Err(Error::limit(
            LimitKind::Objects,
            "CityGML building XML nodes".into(),
        ));
    }

    let cos_wall = options.wall_min_tilt_degrees.to_radians().cos();
    let cos_flat = options.horizontal_max_tilt_degrees.to_radians().cos();
    let lod = options.export.lod;
    let srs = options.export.srs_name.as_str();
    let mut doc = CityGmlDocument::new();
    let mut objects = vec![];
    let mut all_bounds = None;
    let mut faces: Vec<Option<Face>> = std::mem::take(&mut c.faces).into_iter().map(Some).collect();
    for list in members {
        let base = list
            .iter()
            .filter_map(|&i| faces.get(i).and_then(Option::as_ref))
            .map(|f| f.surface.z_min)
            .fold(f64::INFINITY, f64::min);
        let band = base + options.ground_tolerance;
        let mut kept: Vec<(Face, Class)> = Vec::with_capacity(list.len());
        for i in list {
            let Some(mut face) = faces.get_mut(i).and_then(Option::take) else {
                continue;
            };
            // |n.z| is the cosine of the tilt; compare cosines, not angles. The margin
            // keeps a face drawn exactly at a threshold on the inclusive side, which
            // rounding of the normal and of cos() would otherwise decide.
            let up = face.surface.normal.z.abs();
            let class = if up <= cos_wall + TILT_COSINE_MARGIN {
                Class::Wall
            } else if up >= cos_flat - TILT_COSINE_MARGIN {
                if face.surface.z_min <= band {
                    Class::Ground
                } else {
                    Class::Roof
                }
            } else if face.surface.z_min > band {
                Class::Roof
            } else {
                Class::Unclassified
            };
            let flip = match class {
                Class::Ground => face.surface.normal.z > 0.0,
                Class::Roof => face.surface.normal.z < 0.0,
                Class::Wall | Class::Unclassified => false,
            };
            if flip && !face.surface.reverse() {
                c.skip(Reject::NotPlanar, face.origin, face.mesh_face);
                continue;
            }
            kept.push((face, class));
        }
        if kept.is_empty() {
            continue;
        }
        let n = objects.len() + 1;
        let id = format!("cadkit_bldg_{n}");
        let first = kept.first().map(|(f, _)| f.origin).unwrap_or(0);
        let origin = c.origins.get(first);
        let mut summary = BuildingSummary {
            id: id.clone(),
            layer: (options.grouping == BuildingGrouping::Layer)
                .then(|| origin.and_then(|o| c.layers.get(o.layer).cloned()))
                .flatten(),
            top_level: (options.grouping == BuildingGrouping::TopLevel)
                .then(|| origin.and_then(|o| o.path.first().copied()))
                .flatten(),
            ..BuildingSummary::default()
        };
        let mut bounds = None;
        let mut surfaces = vec![];
        let mut loose = vec![];
        for (k, (face, class)) in kept.into_iter().enumerate() {
            extend(&mut bounds, &face.surface);
            let polygon_id = format!("{id}_p{}", k + 1);
            let element = polygon(&polygon_id, &face.surface);
            let kind = match class {
                Class::Wall => {
                    summary.wall_surfaces += 1;
                    ObjectKind::WallSurface
                }
                Class::Roof => {
                    summary.roof_surfaces += 1;
                    ObjectKind::RoofSurface
                }
                Class::Ground => {
                    summary.ground_surfaces += 1;
                    ObjectKind::GroundSurface
                }
                Class::Unclassified => {
                    summary.unclassified_faces += 1;
                    loose.push(element);
                    c.issue(
                        "gml.face_sloped_at_base",
                        face.origin,
                        face.mesh_face,
                        Some(id.clone()),
                        Some(polygon_id),
                    );
                    continue;
                }
            };
            let mut object = CityObject::new(kind, format!("{id}_s{}", k + 1));
            object.properties.push(multi_surface(lod, vec![element]));
            let mut bounded = Element::new(BUILDING, "bldg:boundedBy");
            bounded.push(object.into_element());
            surfaces.push(bounded);
        }
        let mut building = CityObject::new(ObjectKind::Building, id);
        if let Some((lo, hi)) = bounds {
            building.properties.push(envelope(srs, lo, hi));
            let all = all_bounds.get_or_insert((lo, hi));
            all.0 = Point3::new(all.0.x.min(lo.x), all.0.y.min(lo.y), all.0.z.min(lo.z));
            all.1 = Point3::new(all.1.x.max(hi.x), all.1.y.max(hi.y), all.1.z.max(hi.z));
        }
        // CityGML 2.0 _AbstractBuilding order: lod2MultiSurface precedes boundedBy, which
        // precedes lod3MultiSurface and lod4MultiSurface.
        let loose = (!loose.is_empty()).then(|| multi_surface(lod, loose));
        if lod == 2 {
            building.properties.extend(loose);
            building.properties.extend(surfaces);
        } else {
            building.properties.extend(surfaces);
            building.properties.extend(loose);
        }
        c.report.wall_surfaces += summary.wall_surfaces;
        c.report.roof_surfaces += summary.roof_surfaces;
        c.report.ground_surfaces += summary.ground_surfaces;
        c.report.unclassified_faces += summary.unclassified_faces;
        c.report.buildings.push(summary);
        objects.push(building);
    }
    if let Some((lo, hi)) = all_bounds {
        doc.root.push(envelope(srs, lo, hi));
    }
    for object in objects {
        doc.add_object(object);
    }
    Ok((doc, c.report))
}

/// [`buildings_from_document`] serialized as UTF-8 XML after [`crate::validate`] with
/// geometry checks, a planarity tolerance of at least
/// [`ExportOptions::max_planarity_deviation`] and a geometry budget of at least
/// [`BuildingOptions::max_work`].
pub fn write_buildings(
    source: &Document,
    model: usize,
    options: &BuildingOptions,
) -> Result<(Vec<u8>, BuildingReport)> {
    let (doc, report) = buildings_from_document(source, model, options)?;
    let defaults = ValidationOptions::default();
    let validation = ValidationOptions {
        limits: options.export.limits,
        planarity_tolerance: defaults
            .planarity_tolerance
            .max(options.export.max_planarity_deviation),
        max_geometry_work: defaults.max_geometry_work.max(options.max_work),
        ..defaults
    };
    Ok((crate::write(&doc, &validation)?, report))
}
