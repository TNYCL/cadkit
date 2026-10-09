//! Entity records to model entities.
//!
//! Coordinate policy (see `docs/dxf/NOTES.md`): geometry is converted to world coordinates
//! with the arbitrary-axis algorithm; `normal` fields keep the extrusion direction, and
//! angles stay relative to the OCS X axis derived from it.

use std::collections::BTreeMap;

use cadkit_core::{
    Attachment, Attribute, DimensionType, Entity, EntityKind, Error, HAlign, Point3, Props,
    ReadOptions, Result, VAlign, Value as MValue, Vec3, Vertex, Warning,
};

use super::common::{
    color, handle, lineweight, model_value, normalize_linetype, point_value, xdata_props,
};
use super::hatch;
use super::records::{Group, Record, parse_handle};
use crate::native::Pair;
use crate::ocs::{Ocs, normalize_normal};
use cadkit_core::text::plain_text;

/// Reader-wide mutable context for entity decoding.
pub(crate) struct Ctx<'o> {
    pub opts: &'o ReadOptions,
    pub warnings: Vec<Warning>,
    /// Unknown entity type -> (index of its warning, count).
    pub unknown: BTreeMap<String, (usize, u64)>,
}

impl<'o> Ctx<'o> {
    pub fn new(opts: &'o ReadOptions) -> Self {
        Self {
            opts,
            warnings: Vec::new(),
            unknown: BTreeMap::new(),
        }
    }

    pub fn warn(&mut self, code: &str, message: String, offset: Option<u64>, object: Option<u64>) {
        self.warnings.push(Warning {
            code: code.to_owned(),
            message,
            offset,
            object,
        });
    }

    fn max_vertices(&self) -> u32 {
        self.opts.limits.max_vertices
    }

    fn check_count(&self, n: usize, what: &str) -> Result<()> {
        if n > self.max_vertices() as usize {
            return Err(Error::LimitExceeded(format!(
                "{what} count exceeds {}",
                self.max_vertices()
            )));
        }
        Ok(())
    }

    /// Rewrites the per-type "unknown entity" warnings with their final counts.
    pub fn finish(&mut self) {
        for (name, (idx, count)) in &self.unknown {
            if let Some(w) = self.warnings.get_mut(*idx) {
                w.message = format!(
                    "entity type {name} is not modelled ({count} occurrence(s)); kept as dxf.{name} Unknown"
                );
            }
        }
    }
}

/// A decoded entity plus the placement hints needed to assign it to a space.
pub(crate) struct Parsed {
    pub entity: Entity,
    /// Owner handle (group 330): usually the block record of its space.
    pub owner: Option<u64>,
    /// Paper-space flag (group 67).
    pub paper: bool,
}

fn normal_of(g: Group) -> Vec3 {
    match (g.f64(210), g.f64(220), g.f64(230)) {
        (None, None, None) => Vec3::Z,
        (x, y, z) => normalize_normal(Vec3::new(
            x.unwrap_or(0.0),
            y.unwrap_or(0.0),
            z.unwrap_or(1.0),
        )),
    }
}

/// Collects points stored as `base`, `base + 10`, `base + 20`; each `base` group starts a point.
fn collect_points(pairs: &[Pair], base: i32, ctx: &Ctx) -> Result<Vec<Point3>> {
    let mut pts: Vec<Point3> = Vec::new();
    for p in pairs {
        let v = p.value.as_f64().unwrap_or(0.0);
        if p.code == base {
            ctx.check_count(pts.len(), "point")?;
            pts.push(Point3::new(v, 0.0, 0.0));
        } else if p.code == base + 10 {
            if let Some(last) = pts.last_mut() {
                last.y = v;
            }
        } else if p.code == base + 20 {
            if let Some(last) = pts.last_mut() {
                last.z = v;
            }
        }
    }
    Ok(pts)
}

fn halign(v: i64) -> HAlign {
    match v {
        1 => HAlign::Center,
        2 => HAlign::Right,
        3 => HAlign::Aligned,
        4 => HAlign::Middle,
        5 => HAlign::Fit,
        _ => HAlign::Left,
    }
}

fn valign(v: i64) -> VAlign {
    match v {
        1 => VAlign::Bottom,
        2 => VAlign::Middle,
        3 => VAlign::Top,
        _ => VAlign::Baseline,
    }
}

fn attachment(v: i64) -> Attachment {
    match v {
        2 => Attachment::TopCenter,
        3 => Attachment::TopRight,
        4 => Attachment::MiddleLeft,
        5 => Attachment::MiddleCenter,
        6 => Attachment::MiddleRight,
        7 => Attachment::BottomLeft,
        8 => Attachment::BottomCenter,
        9 => Attachment::BottomRight,
        _ => Attachment::TopLeft,
    }
}

fn dimension_type(flags: i64) -> DimensionType {
    match flags & 7 {
        0 => DimensionType::Linear,
        1 => DimensionType::Aligned,
        2 => DimensionType::Angular,
        3 => DimensionType::Diameter,
        4 => DimensionType::Radius,
        5 => DimensionType::Angular3Point,
        6 => DimensionType::Ordinate,
        7 => DimensionType::ArcLength,
        _ => DimensionType::Other,
    }
}

/// Converts one entity record. `children` are its VERTEX (POLYLINE) or ATTRIB (INSERT)
/// records. Returns `Ok(None)` for records that are skipped silently.
pub(crate) fn convert(ctx: &mut Ctx, rec: &Record, children: &[Record]) -> Result<Option<Parsed>> {
    let g = Group(rec.body());
    let normal = normal_of(g);
    let ocs = Ocs::new(normal);
    let mut props = Props::new();

    let kind = match rec.name.as_str() {
        "POINT" => EntityKind::Point {
            position: g.point_or_zero(10),
        },
        "LINE" => EntityKind::Line {
            start: g.point_or_zero(10),
            end: g.point_or_zero(11),
        },
        "CIRCLE" => EntityKind::Circle {
            center: ocs.to_world(g.point_or_zero(10)),
            radius: g.f64_or(40, 0.0),
            normal: ocs.az,
        },
        "ARC" => EntityKind::Arc {
            center: ocs.to_world(g.point_or_zero(10)),
            radius: g.f64_or(40, 0.0),
            start_angle: g.f64_or(50, 0.0).to_radians(),
            end_angle: g.f64_or(51, 360.0).to_radians(),
            normal: ocs.az,
        },
        "ELLIPSE" => EntityKind::Ellipse {
            center: g.point_or_zero(10),
            major_axis: {
                let m = g.point(11).unwrap_or(Point3::new(1.0, 0.0, 0.0));
                Vec3::new(m.x, m.y, m.z)
            },
            ratio: g.f64_or(40, 1.0),
            start_param: g.f64_or(41, 0.0),
            end_param: g.f64_or(42, std::f64::consts::TAU),
            normal: ocs.az,
        },
        "LWPOLYLINE" => lwpolyline(ctx, g, &ocs, &mut props)?,
        "POLYLINE" => polyline(ctx, g, &ocs, children, &mut props)?,
        "SPLINE" => spline(ctx, g, &mut props)?,
        "TEXT" => text(g, &ocs),
        "MTEXT" => mtext(g, &ocs),
        "INSERT" => EntityKind::Insert {
            block: g.string(2).unwrap_or_default(),
            position: ocs.to_world(g.point_or_zero(10)),
            scale: Vec3::new(g.f64_or(41, 1.0), g.f64_or(42, 1.0), g.f64_or(43, 1.0)),
            rotation: g.f64_or(50, 0.0).to_radians(),
            normal: ocs.az,
            columns: g.i64_or(70, 1).clamp(1, i64::from(u32::MAX)) as u32,
            rows: g.i64_or(71, 1).clamp(1, i64::from(u32::MAX)) as u32,
            column_spacing: g.f64_or(44, 0.0),
            row_spacing: g.f64_or(45, 0.0),
        },
        "HATCH" => {
            let h = hatch::parse(g, &ocs, ctx.max_vertices(), &mut props)?;
            EntityKind::Hatch {
                loops: h.loops,
                solid: h.solid,
                pattern: h.pattern,
                pattern_scale: h.pattern_scale,
                pattern_angle: h.pattern_angle,
                normal: ocs.az,
            }
        }
        "DIMENSION" => dimension(g, &ocs, &mut props),
        "SOLID" | "TRACE" | "3DFACE" => face(g, &ocs, &rec.name, &mut props),
        "LEADER" => leader(ctx, g, &mut props)?,
        "IMAGE" => image(g, &mut props),
        "VIEWPORT" => viewport(g, &mut props),
        "ATTDEF" => attdef(rec, g, &ocs, &mut props),
        // Sub-records that only make sense inside their parent.
        "VERTEX" | "SEQEND" | "ATTRIB" => return Ok(None),
        other => {
            if other.is_empty() {
                return Ok(None);
            }
            let name = other.to_owned();
            match ctx.unknown.get_mut(&name) {
                Some((_, count)) => *count += 1,
                None => {
                    let idx = ctx.warnings.len();
                    ctx.warn(
                        "dxf.unknown_entity",
                        format!("entity type {name} is not modelled"),
                        Some(rec.offset),
                        handle(g),
                    );
                    ctx.unknown.insert(name.clone(), (idx, 1));
                }
            }
            props.insert("dxf.codes".into(), codes_value(rec.body()));
            EntityKind::Unknown {
                type_name: format!("dxf.{name}"),
            }
        }
    };

    let mut entity = Entity::new(kind);
    entity.id = handle(g);
    entity.layer = Some(g.string(8).unwrap_or_else(|| "0".to_owned()));
    entity.linetype = normalize_linetype(g.string(6));
    entity.color = color(g);
    entity.lineweight = lineweight(g);
    entity.visible = g.i64_or(60, 0) == 0;
    if let Some(t) = g.f64(39).filter(|t| *t != 0.0) {
        props.insert("dxf.thickness".into(), MValue::Float(t));
    }
    if let Some(s) = g.f64(48).filter(|s| *s != 1.0) {
        props.insert("dxf.linetype_scale".into(), MValue::Float(s));
    }
    xdata_props(rec, &mut props);
    entity.props = props;

    if let EntityKind::Insert { .. } = entity.kind {
        for child in children.iter().filter(|c| c.name == "ATTRIB") {
            entity.attributes.push(attribute(child));
        }
    }

    let owner = g.text(330).and_then(parse_handle);
    let paper = g.i64_or(67, 0) != 0;
    Ok(Some(Parsed {
        entity,
        owner,
        paper,
    }))
}

fn codes_value(pairs: &[Pair]) -> MValue {
    MValue::List(
        pairs
            .iter()
            .map(|p| MValue::List(vec![MValue::Int(i64::from(p.code)), model_value(&p.value)]))
            .collect(),
    )
}

/// Text of a multi-line ATTRIB/ATTDEF: the embedded MTEXT (chunks 3 then 1) when present and
/// non-empty, else the single-line value (group 1 of the main part).
fn embedded_or(rec: &Record, g: &Group, code: i32) -> String {
    let emb = Group(rec.embedded());
    let mut text = String::new();
    for v in emb.all(3) {
        text.push_str(v.as_str().unwrap_or(""));
    }
    text.push_str(emb.text(1).unwrap_or(""));
    if text.is_empty() {
        g.string(code).unwrap_or_default()
    } else {
        text
    }
}

fn attribute(rec: &Record) -> Attribute {
    let g = Group(rec.body());
    let ocs = Ocs::new(normal_of(g));
    let flags = g.i64_or(70, 0);
    Attribute {
        tag: g.string(2).unwrap_or_default(),
        value: MValue::Text(embedded_or(rec, &g, 1)),
        set: None,
        position: Some(ocs.to_world(g.point_or_zero(10))),
        invisible: flags & 1 != 0,
        ..Default::default()
    }
}

fn lwpolyline(ctx: &Ctx, g: Group, ocs: &Ocs, props: &mut Props) -> Result<EntityKind> {
    let flags = g.i64_or(70, 0);
    let elevation = g.f64_or(38, 0.0);
    let const_width = g.f64_or(43, 0.0);
    /// One vertex under construction: x, y, start width, end width, bulge.
    type RawVertex = (f64, f64, Option<f64>, Option<f64>, f64);
    let mut raw: Vec<RawVertex> = Vec::new();
    for p in g.0 {
        let v = p.value.as_f64().unwrap_or(0.0);
        match p.code {
            10 => {
                ctx.check_count(raw.len(), "vertex")?;
                raw.push((v, 0.0, None, None, 0.0));
            }
            20 => {
                if let Some(l) = raw.last_mut() {
                    l.1 = v;
                }
            }
            40 => {
                if let Some(l) = raw.last_mut() {
                    l.2 = Some(v);
                }
            }
            41 => {
                if let Some(l) = raw.last_mut() {
                    l.3 = Some(v);
                }
            }
            42 => {
                if let Some(l) = raw.last_mut() {
                    l.4 = v;
                }
            }
            _ => {}
        }
    }
    let vertices = raw
        .into_iter()
        .map(|(x, y, sw, ew, bulge)| Vertex {
            position: ocs.to_world(Point3::new(x, y, elevation)),
            bulge,
            start_width: sw.unwrap_or(const_width),
            end_width: ew.unwrap_or(const_width),
        })
        .collect();
    if flags & 128 != 0 {
        props.insert("dxf.plinegen".into(), MValue::Bool(true));
    }
    Ok(EntityKind::Polyline {
        vertices,
        closed: flags & 1 != 0,
        normal: ocs.az,
    })
}

fn polyline(
    ctx: &Ctx,
    g: Group,
    ocs: &Ocs,
    children: &[Record],
    props: &mut Props,
) -> Result<EntityKind> {
    let flags = g.i64_or(70, 0);
    let vertex_records: Vec<&Record> = children.iter().filter(|c| c.name == "VERTEX").collect();
    ctx.check_count(vertex_records.len(), "vertex")?;

    if flags & 64 != 0 {
        // Polyface mesh: vertex records (flags 64|128) then face records (flag 128 only).
        let mut vertices = Vec::new();
        let mut face_recs = Vec::new();
        for v in &vertex_records {
            let vg = Group(v.body());
            let vf = vg.i64_or(70, 0);
            if vf & 64 != 0 {
                vertices.push(vg.point_or_zero(10));
            } else if vf & 128 != 0 {
                face_recs.push(vg);
            }
        }
        let n = vertices.len() as i64;
        let mut faces = Vec::new();
        for fg in face_recs {
            let idx: Vec<u32> = [71, 72, 73, 74]
                .iter()
                .filter_map(|&c| fg.i64(c))
                .map(i64::abs)
                .filter(|&i| i >= 1 && i <= n)
                .map(|i| (i - 1) as u32)
                .collect();
            if idx.len() >= 3 {
                faces.push(idx);
            }
        }
        props.insert("dxf.polyface".into(), MValue::Bool(true));
        return Ok(EntityKind::Mesh { vertices, faces });
    }

    if flags & 16 != 0 {
        // Polygon mesh: M x N grid of vertices stored row by row.
        let m = g.i64_or(71, 0).max(0) as usize;
        let n = g.i64_or(72, 0).max(0) as usize;
        let vertices: Vec<Point3> = vertex_records
            .iter()
            .filter(|v| Group(v.body()).i64_or(70, 0) & 16 == 0)
            .map(|v| Group(v.body()).point_or_zero(10))
            .collect();
        let mut faces = Vec::new();
        if m >= 2 && n >= 2 && m.checked_mul(n).is_some_and(|t| t <= vertices.len()) {
            let closed_m = flags & 1 != 0;
            let closed_n = flags & 32 != 0;
            let rows = if closed_m { m } else { m - 1 };
            let cols = if closed_n { n } else { n - 1 };
            ctx.check_count(rows.saturating_mul(cols), "mesh face")?;
            for i in 0..rows {
                for j in 0..cols {
                    let i2 = (i + 1) % m;
                    let j2 = (j + 1) % n;
                    faces.push(vec![
                        (i * n + j) as u32,
                        (i2 * n + j) as u32,
                        (i2 * n + j2) as u32,
                        (i * n + j2) as u32,
                    ]);
                }
            }
        }
        props.insert(
            "dxf.polygon_mesh".into(),
            MValue::List(vec![MValue::Int(m as i64), MValue::Int(n as i64)]),
        );
        return Ok(EntityKind::Mesh { vertices, faces });
    }

    let is_3d = flags & 8 != 0;
    let elevation = g.f64_or(30, 0.0);
    let default_start = g.f64_or(40, 0.0);
    let default_end = g.f64_or(41, 0.0);
    let mut vertices = Vec::with_capacity(vertex_records.len());
    for v in &vertex_records {
        let vg = Group(v.body());
        let vf = vg.i64_or(70, 0);
        if vf & 16 != 0 {
            continue; // spline frame control point
        }
        let p = vg.point_or_zero(10);
        let position = if is_3d {
            p
        } else {
            let z = if elevation != 0.0 { elevation } else { p.z };
            ocs.to_world(Point3::new(p.x, p.y, z))
        };
        vertices.push(Vertex {
            position,
            bulge: vg.f64_or(42, 0.0),
            start_width: vg.f64_or(40, default_start),
            end_width: vg.f64_or(41, default_end),
        });
    }
    if is_3d {
        props.insert("dxf.polyline_3d".into(), MValue::Bool(true));
    }
    if flags & 2 != 0 {
        props.insert("dxf.polyline_fit".into(), MValue::Text("curve".into()));
    } else if flags & 4 != 0 {
        props.insert("dxf.polyline_fit".into(), MValue::Text("spline".into()));
    }
    if flags & 128 != 0 {
        props.insert("dxf.plinegen".into(), MValue::Bool(true));
    }
    Ok(EntityKind::Polyline {
        vertices,
        closed: flags & 1 != 0,
        normal: if is_3d { Vec3::Z } else { ocs.az },
    })
}

fn spline(ctx: &Ctx, g: Group, props: &mut Props) -> Result<EntityKind> {
    let flags = g.i64_or(70, 0);
    let degree = g.i64_or(71, 3).clamp(0, 1000) as u32;
    let knots: Vec<f64> = g.all(40).filter_map(|v| v.as_f64()).collect();
    let weights: Vec<f64> = g.all(41).filter_map(|v| v.as_f64()).collect();
    ctx.check_count(knots.len(), "knot")?;
    ctx.check_count(weights.len(), "weight")?;
    let control_points = collect_points(g.0, 10, ctx)?;
    let fit_points = collect_points(g.0, 11, ctx)?;
    props.insert("dxf.spline.flags".into(), MValue::Int(flags));
    Ok(EntityKind::Spline {
        degree,
        knots,
        control_points,
        weights,
        fit_points,
        closed: flags & 3 != 0,
    })
}

fn text(g: Group, ocs: &Ocs) -> EntityKind {
    let h = halign(g.i64_or(72, 0));
    let v = valign(g.i64_or(73, 0));
    let p1 = ocs.to_world(g.point_or_zero(10));
    let p2 = g.point(11).map(|p| ocs.to_world(p));
    let default_align = h == HAlign::Left && v == VAlign::Baseline;
    let two_point = matches!(h, HAlign::Aligned | HAlign::Fit);
    // `position` is the anchor implied by the alignment: group 10 for left/baseline and
    // Aligned/Fit text, group 11 (justification point) otherwise.
    let position = if default_align || two_point {
        p1
    } else {
        p2.unwrap_or(p1)
    };
    let end_point = if two_point { p2 } else { None };
    EntityKind::Text {
        position,
        end_point,
        height: g.f64_or(40, 0.0),
        rotation: g.f64_or(50, 0.0).to_radians(),
        width_factor: g.f64_or(41, 1.0),
        oblique: g.f64_or(51, 0.0).to_radians(),
        value: g.string(1).unwrap_or_default(),
        style: g.string(7),
        halign: h,
        valign: v,
        normal: ocs.az,
    }
}

fn mtext(g: Group, ocs: &Ocs) -> EntityKind {
    // Continuation chunks (3) precede the final chunk (1).
    let mut value = String::new();
    for v in g.all(3) {
        value.push_str(v.as_str().unwrap_or(""));
    }
    value.push_str(g.text(1).unwrap_or(""));
    let rotation = match g.point(11) {
        Some(d) => {
            let local = ocs.to_local(d);
            local.y.atan2(local.x)
        }
        None => g.f64_or(50, 0.0).to_radians(),
    };
    EntityKind::MText {
        position: g.point_or_zero(10),
        height: g.f64_or(40, 0.0),
        width: g.f64_or(41, 0.0),
        rotation,
        plain: plain_text(&value),
        value,
        style: g.string(7),
        attachment: attachment(g.i64_or(71, 1)),
        line_spacing: g.f64_or(44, 1.0),
        normal: ocs.az,
    }
}

fn dimension(g: Group, ocs: &Ocs, props: &mut Props) -> EntityKind {
    let flags = g.i64_or(70, 0);
    // (role label, group code base, in OCS)
    let roles: [(&str, i32, bool); 6] = [
        ("10", 10, false),
        ("13", 13, false),
        ("14", 14, false),
        ("15", 15, false),
        ("16", 16, true),
        ("12", 12, true),
    ];
    let mut points = Vec::new();
    let mut labels = Vec::new();
    for (label, base, in_ocs) in roles {
        let found = g.point(base);
        let p = match (found, label) {
            (Some(p), _) => p,
            (None, "10") => Point3::default(),
            _ => continue,
        };
        points.push(if in_ocs { ocs.to_world(p) } else { p });
        labels.push(MValue::Text(label.to_owned()));
    }
    props.insert("dxf.dim.roles".into(), MValue::List(labels));
    props.insert("dxf.dim.flags".into(), MValue::Int(flags));
    for (code, key) in [
        (50, "dxf.dim.rotation"),
        (52, "dxf.dim.oblique"),
        (53, "dxf.dim.text_rotation"),
        (41, "dxf.dim.line_spacing"),
    ] {
        if let Some(v) = g.f64(code) {
            props.insert(key.into(), MValue::Float(v));
        }
    }
    if let Some(v) = g.i64(71) {
        props.insert("dxf.dim.attachment".into(), MValue::Int(v));
    }
    EntityKind::Dimension {
        dimension_type: dimension_type(flags),
        points,
        text_position: g.point(11).map(|p| ocs.to_world(p)),
        measurement: g.f64(42),
        text: g.string(1).filter(|s| !s.is_empty()),
        block: g.string(2).filter(|s| !s.is_empty()),
        style: g.string(3),
    }
}

fn face(g: Group, ocs: &Ocs, name: &str, props: &mut Props) -> EntityKind {
    let solid = name != "3DFACE";
    let conv = |p: Point3| if solid { ocs.to_world(p) } else { p };
    let p1 = conv(g.point_or_zero(10));
    let p2 = conv(g.point_or_zero(11));
    let p3 = conv(g.point_or_zero(12));
    let p4 = g.point(13).map(conv);
    let points = match p4 {
        Some(p4) if p4 != p3 => {
            // SOLID/TRACE corners are stored in "Z" order: the outline is 1, 2, 4, 3.
            if solid {
                vec![p1, p2, p4, p3]
            } else {
                vec![p1, p2, p3, p4]
            }
        }
        _ => vec![p1, p2, p3],
    };
    props.insert("dxf.type".into(), MValue::Text(name.to_owned()));
    if let Some(f) = g.i64(70).filter(|f| *f != 0) {
        props.insert("dxf.invisible_edges".into(), MValue::Int(f));
    }
    EntityKind::Face {
        points,
        filled: solid,
    }
}

fn leader(ctx: &Ctx, g: Group, props: &mut Props) -> Result<EntityKind> {
    let vertices = collect_points(g.0, 10, ctx)?;
    if let Some(s) = g.string(3) {
        props.insert("dxf.leader.dimstyle".into(), MValue::Text(s));
    }
    if let Some(v) = g.i64(72) {
        props.insert("dxf.leader.path_type".into(), MValue::Int(v));
    }
    Ok(EntityKind::Leader {
        vertices,
        arrowhead: g.i64_or(71, 1) != 0,
    })
}

fn image(g: Group, props: &mut Props) -> EntityKind {
    let u = g.point(11).unwrap_or(Point3::new(1.0, 0.0, 0.0));
    let v = g.point(12).unwrap_or(Point3::new(0.0, 1.0, 0.0));
    let size = g.point(13);
    let (w, h) = size.map_or((1.0, 1.0), |s| (s.x, s.y));
    if let Some(hd) = g.text(340) {
        props.insert("dxf.imagedef".into(), MValue::Text(hd.to_owned()));
    }
    if let Some(fl) = g.i64(70) {
        props.insert("dxf.image.display_flags".into(), MValue::Int(fl));
    }
    let size_px = size
        .filter(|s| {
            s.x >= 0.0 && s.y >= 0.0 && s.x <= f64::from(u32::MAX) && s.y <= f64::from(u32::MAX)
        })
        .map(|s| [s.x as u32, s.y as u32]);
    EntityKind::Image {
        path: None,
        position: g.point_or_zero(10),
        u_vector: Vec3::new(u.x * w, u.y * w, u.z * w),
        v_vector: Vec3::new(v.x * h, v.y * h, v.z * h),
        size_px,
    }
}

fn viewport(g: Group, props: &mut Props) -> EntityKind {
    if let Some(v) = g.i64(69) {
        props.insert("dxf.viewport.id".into(), MValue::Int(v));
    }
    if let Some(v) = g.i64(68) {
        props.insert("dxf.viewport.status".into(), MValue::Int(v));
    }
    if let Some(p) = g.point(16) {
        props.insert("dxf.viewport.view_direction".into(), point_value(p));
    }
    if let Some(p) = g.point(17) {
        props.insert("dxf.viewport.view_target".into(), point_value(p));
    }
    if let Some(v) = g.f64(51) {
        props.insert("dxf.viewport.twist".into(), MValue::Float(v));
    }
    EntityKind::Viewport {
        center: g.point_or_zero(10),
        width: g.f64_or(40, 0.0),
        height: g.f64_or(41, 0.0),
        view_center: g.point_or_zero(12),
        view_height: g.f64_or(45, 0.0),
    }
}

/// ATTDEF has no model counterpart: it becomes `Unknown { "dxf.ATTDEF" }` with its data in props.
fn attdef(rec: &Record, g: Group, ocs: &Ocs, props: &mut Props) -> EntityKind {
    props.insert(
        "dxf.attdef.tag".into(),
        MValue::Text(g.string(2).unwrap_or_default()),
    );
    props.insert(
        "dxf.attdef.prompt".into(),
        MValue::Text(g.string(3).unwrap_or_default()),
    );
    props.insert(
        "dxf.attdef.default".into(),
        MValue::Text(embedded_or(rec, &g, 1)),
    );
    props.insert(
        "dxf.attdef.position".into(),
        point_value(ocs.to_world(g.point_or_zero(10))),
    );
    props.insert("dxf.attdef.height".into(), MValue::Float(g.f64_or(40, 0.0)));
    props.insert("dxf.attdef.flags".into(), MValue::Int(g.i64_or(70, 0)));
    if let Some(s) = g.string(7) {
        props.insert("dxf.attdef.style".into(), MValue::Text(s));
    }
    EntityKind::Unknown {
        type_name: "dxf.ATTDEF".into(),
    }
}
