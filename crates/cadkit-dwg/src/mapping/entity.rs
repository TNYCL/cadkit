//! Per-entity mapping: native entity data → [`EntityKind`] (in WCS) plus `dwg.` props.
//!
//! Conventions shared with the DXF reader so both produce the same model:
//! - OCS points → WCS with the arbitrary-axis algorithm; normals normalized.
//! - TEXT `position`: baseline start for left/baseline and aligned/fit text (whose end
//!   is `end_point`), the justification point otherwise.
//! - SOLID/TRACE corners are reordered to outline order 1, 2, 4, 3; a fourth corner
//!   equal to the third gives a triangle.
//! - DIMENSION points, in this order and only when present: 10, 13, 14, 15, 16, then 12
//!   when it is not the origin. Their roles are listed in `dwg.dim_roles`.
//! - Types without a model kind become `Unknown { "dwg.<NAME>" }` with what was decoded
//!   in props.

use cadkit_core::{
    Attachment, Attribute, DimensionType, Entity, EntityKind, HAlign, HatchEdge, HatchLoop, Point3,
    Props, VAlign, Value, Vec3, Vertex,
};

use super::{Mapper, is_unit_z, list3, ocs_to_wcs, p3, v3};
use crate::native::{
    Dimension, DwgObject, Face, FaceKind, Hatch, HatchEdgeData, Image, MText, ObjectData, Polyline,
    PolylineKind, Text,
};
use cadkit_core::text::plain_text;

/// VERTEX record types.
const VERTEX_TYPES: [u16; 5] = [0x0A, 0x0B, 0x0C, 0x0D, 0x0E];

/// A normal as the model wants it: normalized, +Z when degenerate.
pub(super) fn normal(a: [f64; 3]) -> Vec3 {
    v3(a).normalized().unwrap_or(Vec3::Z)
}

fn point_value(p: Point3) -> Value {
    Value::List(vec![
        Value::Float(p.x),
        Value::Float(p.y),
        Value::Float(p.z),
    ])
}

fn halign(v: i16) -> HAlign {
    match v {
        1 => HAlign::Center,
        2 => HAlign::Right,
        3 => HAlign::Aligned,
        4 => HAlign::Middle,
        5 => HAlign::Fit,
        _ => HAlign::Left,
    }
}

fn valign(v: i16) -> VAlign {
    match v {
        1 => VAlign::Bottom,
        2 => VAlign::Middle,
        3 => VAlign::Top,
        _ => VAlign::Baseline,
    }
}

fn attachment(v: i16) -> Attachment {
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

/// Angle of a WCS direction measured in the OCS of `normal_vec` (MTEXT X axis).
fn ocs_angle(normal_vec: [f64; 3], dir: [f64; 3]) -> f64 {
    let (ax, ay, _) = cadkit_core::geom_ops::arbitrary_axes(v3(normal_vec));
    let d = v3(dir);
    d.dot(ay).atan2(d.dot(ax))
}

impl Mapper<'_> {
    /// Maps one entity, or `None` for records the model folds into their parent or
    /// does not represent (BLOCK, ENDBLK, SEQEND, VERTEX_*).
    pub(super) fn map_entity(&mut self, obj: &DwgObject) -> Option<Entity> {
        let mut props = Props::new();
        let mut attributes = Vec::new();
        let kind = match &obj.data {
            ObjectData::Block { .. }
            | ObjectData::EndBlock
            | ObjectData::Seqend
            | ObjectData::Vertex(_)
            | ObjectData::PfaceFace(_) => {
                return None;
            }
            ObjectData::Line(l) => {
                thickness(&mut props, l.thickness);
                extrusion(&mut props, l.extrusion);
                EntityKind::Line {
                    start: p3(l.start),
                    end: p3(l.end),
                }
            }
            ObjectData::Point(p) => {
                thickness(&mut props, p.thickness);
                extrusion(&mut props, p.extrusion);
                if p.x_axis_angle != 0.0 {
                    props.insert("dwg.x_axis_angle".into(), Value::Float(p.x_axis_angle));
                }
                EntityKind::Point {
                    position: p3(p.position),
                }
            }
            ObjectData::Circle(c) => {
                thickness(&mut props, c.thickness);
                EntityKind::Circle {
                    center: ocs_to_wcs(c.extrusion, c.center),
                    radius: c.radius,
                    normal: normal(c.extrusion),
                }
            }
            ObjectData::Arc(a) => {
                thickness(&mut props, a.thickness);
                EntityKind::Arc {
                    center: ocs_to_wcs(a.extrusion, a.center),
                    radius: a.radius,
                    start_angle: a.start_angle,
                    end_angle: a.end_angle,
                    normal: normal(a.extrusion),
                }
            }
            ObjectData::Ellipse(e) => EntityKind::Ellipse {
                center: p3(e.center),
                major_axis: v3(e.major_axis),
                ratio: e.ratio,
                start_param: e.start,
                end_param: e.end,
                normal: normal(e.extrusion),
            },
            ObjectData::Spline(sp) => {
                props.insert(
                    "dwg.spline_scenario".into(),
                    Value::Int(i64::from(sp.scenario)),
                );
                if let Some(t) = sp.start_tangent {
                    props.insert("dwg.start_tangent".into(), list3(t));
                }
                if let Some(t) = sp.end_tangent {
                    props.insert("dwg.end_tangent".into(), list3(t));
                }
                EntityKind::Spline {
                    degree: u32::try_from(sp.degree).unwrap_or(0),
                    knots: sp.knots.clone(),
                    control_points: sp.control_points.iter().map(|p| p3(*p)).collect(),
                    weights: sp.weights.clone(),
                    fit_points: sp.fit_points.iter().map(|p| p3(*p)).collect(),
                    closed: sp.closed || sp.periodic,
                }
            }
            ObjectData::LwPolyline(p) => {
                let vertices = p
                    .points
                    .iter()
                    .enumerate()
                    .map(|(i, xy)| {
                        let (sw, ew) = p
                            .widths
                            .get(i)
                            .map_or((p.const_width, p.const_width), |w| (w[0], w[1]));
                        Vertex {
                            position: ocs_to_wcs(p.extrusion, [xy[0], xy[1], p.elevation]),
                            bulge: p.bulges.get(i).copied().unwrap_or(0.0),
                            start_width: sw,
                            end_width: ew,
                        }
                    })
                    .collect();
                thickness(&mut props, p.thickness);
                if p.flags & 0x100 != 0 {
                    props.insert("dwg.plinegen".into(), Value::Bool(true));
                }
                if !p.vertex_ids.is_empty() {
                    props.insert(
                        "dwg.vertex_ids".into(),
                        Value::List(
                            p.vertex_ids
                                .iter()
                                .map(|&i| Value::Int(i64::from(i)))
                                .collect(),
                        ),
                    );
                }
                EntityKind::Polyline {
                    vertices,
                    closed: p.closed(),
                    normal: normal(p.extrusion),
                }
            }
            ObjectData::Polyline(p) => self.polyline_kind(p, &mut props),
            ObjectData::Face(f) => face_kind(f, &mut props),
            ObjectData::Text(t) => self.text_kind(t, &mut props),
            ObjectData::MText(m) => self.mtext_kind(m),
            ObjectData::AttDef(a) => {
                // Attribute definitions are templates, not drawn geometry (as the DXF reader).
                let ins = ocs_to_wcs(
                    a.text.extrusion,
                    [a.text.insertion[0], a.text.insertion[1], a.text.elevation],
                );
                props.insert("dwg.tag".into(), Value::Text(a.tag.clone()));
                let value = a
                    .mtext
                    .as_ref()
                    .map_or_else(|| a.text.value.clone(), |m| m.value.clone());
                props.insert("dwg.default".into(), Value::Text(value));
                if let Some(prompt) = &a.prompt {
                    props.insert("dwg.prompt".into(), Value::Text(prompt.clone()));
                }
                props.insert("dwg.flags".into(), Value::Int(i64::from(a.flags)));
                props.insert("dwg.insertion_point".into(), point_value(ins));
                props.insert("dwg.height".into(), Value::Float(a.text.height));
                EntityKind::Unknown {
                    type_name: "dwg.ATTDEF".into(),
                }
            }
            ObjectData::Insert(ins) => {
                let block = ins
                    .block_header
                    .and_then(|h| self.names.blocks.get(&h))
                    .cloned()
                    .unwrap_or_default();
                let (columns, rows, column_spacing, row_spacing) =
                    ins.array.map_or((1, 1, 0.0, 0.0), |a| {
                        (
                            u32::try_from(a.columns).unwrap_or(1).max(1),
                            u32::try_from(a.rows).unwrap_or(1).max(1),
                            a.column_spacing,
                            a.row_spacing,
                        )
                    });
                attributes = self.attributes(&ins.attribs, ins.attribs_are_range);
                EntityKind::Insert {
                    block,
                    position: ocs_to_wcs(ins.extrusion, ins.position),
                    scale: v3(ins.scale),
                    rotation: ins.rotation,
                    normal: normal(ins.extrusion),
                    columns,
                    rows,
                    column_spacing,
                    row_spacing,
                }
            }
            ObjectData::Dimension(d) => self.dimension_kind(d, &mut props),
            ObjectData::Leader(l) => {
                props.insert("dwg.path_type".into(), Value::Int(i64::from(l.path_type)));
                if let Some(style) = l.dimstyle.and_then(|h| self.names.dimstyles.get(&h)) {
                    props.insert("dwg.dimstyle".into(), Value::Text(style.clone()));
                }
                EntityKind::Leader {
                    vertices: l.points.iter().map(|p| p3(*p)).collect(),
                    arrowhead: l.arrowhead,
                }
            }
            ObjectData::Hatch(h) => hatch_kind(h, &mut props),
            ObjectData::Image(im) if obj.type_name == "IMAGE" => self.image_kind(im, &mut props),
            ObjectData::Viewport(vp) => {
                props.insert(
                    "dwg.viewport_status".into(),
                    Value::Int(i64::from(vp.status)),
                );
                props.insert("dwg.view_direction".into(), list3(vp.view_direction));
                props.insert("dwg.view_target".into(), list3(vp.view_target));
                if vp.twist != 0.0 {
                    props.insert("dwg.twist".into(), Value::Float(vp.twist));
                }
                if !vp.frozen_layers.is_empty() {
                    let names = vp
                        .frozen_layers
                        .iter()
                        .filter_map(|h| self.names.layers.get(h))
                        .map(|n| Value::Text(n.clone()))
                        .collect();
                    props.insert("dwg.frozen_layers".into(), Value::List(names));
                }
                EntityKind::Viewport {
                    center: p3(vp.center),
                    width: vp.width,
                    height: vp.height,
                    view_center: Point3::new(vp.view_center[0], vp.view_center[1], 0.0),
                    view_height: vp.view_height,
                }
            }
            other => {
                self.unknown_props(obj, other, &mut props);
                *self.unsupported.entry(obj.type_name.clone()).or_default() += 1;
                EntityKind::Unknown {
                    type_name: format!("dwg.{}", obj.type_name),
                }
            }
        };
        let mut entity = self.base_entity(obj, kind);
        entity.props.extend(props);
        entity.attributes = attributes;
        Some(entity)
    }

    /// Props for the entity types that stay `Unknown` in the model.
    fn unknown_props(&self, obj: &DwgObject, data: &ObjectData, props: &mut Props) {
        props.insert("dwg.type_code".into(), Value::Int(i64::from(obj.type_code)));
        if let Some(err) = &obj.error {
            props.insert("dwg.decode_error".into(), Value::Text(err.clone()));
        }
        match data {
            ObjectData::RayLine(r) => {
                props.insert("dwg.point".into(), list3(r.point));
                props.insert("dwg.direction".into(), list3(r.direction));
            }
            ObjectData::Shape(sh) => {
                props.insert(
                    "dwg.insertion_point".into(),
                    point_value(ocs_to_wcs(sh.extrusion, sh.insertion)),
                );
                props.insert("dwg.size".into(), Value::Float(sh.size));
                props.insert("dwg.rotation".into(), Value::Float(sh.rotation));
                props.insert("dwg.shape_index".into(), Value::Int(i64::from(sh.index)));
                if let Some(style) = sh.style.and_then(|h| self.names.styles.get(&h)) {
                    props.insert("dwg.style".into(), Value::Text(style.clone()));
                }
            }
            ObjectData::Tolerance(t) => {
                props.insert("dwg.insertion_point".into(), list3(t.insertion));
                props.insert("dwg.direction".into(), list3(t.direction));
                props.insert("dwg.text".into(), Value::Text(t.text.clone()));
                if let Some(style) = t.dimstyle.and_then(|h| self.names.dimstyles.get(&h)) {
                    props.insert("dwg.dimstyle".into(), Value::Text(style.clone()));
                }
            }
            ObjectData::MLine(m) => {
                props.insert(
                    "dwg.vertices".into(),
                    Value::List(m.vertices.iter().map(|v| list3(*v)).collect()),
                );
                props.insert("dwg.scale".into(), Value::Float(m.scale));
                props.insert("dwg.closed".into(), Value::Bool(m.flags == 3));
                if let Some(style) = m.style.and_then(|h| self.names.mlinestyles.get(&h)) {
                    props.insert("dwg.mlinestyle".into(), Value::Text(style.clone()));
                }
            }
            ObjectData::Ole2Frame(o) => {
                props.insert(
                    "dwg.ole_data_size".into(),
                    Value::Int(i64::from(o.data_size)),
                );
            }
            ObjectData::Acis(a) => {
                props.insert("dwg.acis_empty".into(), Value::Bool(a.empty));
                props.insert("dwg.acis_version".into(), Value::Int(i64::from(a.version)));
                if let Some(n) = a.sat_bytes {
                    props.insert("dwg.acis_sat_bytes".into(), Value::Int(n as i64));
                }
            }
            ObjectData::Image(im) => {
                // WIPEOUT: the frame geometry of an image without a file.
                props.insert("dwg.insertion_point".into(), list3(im.insertion));
                props.insert("dwg.u_vector".into(), list3(im.u));
                props.insert("dwg.v_vector".into(), list3(im.v));
                props.insert(
                    "dwg.clip_vertices".into(),
                    Value::List(
                        im.clip_vertices
                            .iter()
                            .map(|p| Value::List(vec![Value::Float(p[0]), Value::Float(p[1])]))
                            .collect(),
                    ),
                );
            }
            _ => {}
        }
    }

    /// INSERT attributes from their ATTRIB records.
    fn attributes(&mut self, list: &[u64], is_range: bool) -> Vec<Attribute> {
        let mut out = Vec::new();
        for h in self.child_handles(list, is_range, &[0x02]) {
            let Some(att) = self.file.objects.get(&h) else {
                continue;
            };
            match &att.data {
                ObjectData::Attrib(a) => {
                    let pos = ocs_to_wcs(
                        a.text.extrusion,
                        [a.text.insertion[0], a.text.insertion[1], a.text.elevation],
                    );
                    // Multi-line attributes keep their text in the embedded MTEXT.
                    let value = a
                        .mtext
                        .as_ref()
                        .map_or_else(|| a.text.value.clone(), |m| m.value.clone());
                    out.push(Attribute {
                        tag: a.tag.clone(),
                        value: Value::Text(value),
                        set: None,
                        position: Some(pos),
                        invisible: a.flags & 1 != 0,
                    });
                }
                _ => {
                    *self
                        .unsupported
                        .entry(format!("{} (attribute)", att.type_name))
                        .or_default() += 1
                }
            }
        }
        out
    }

    fn text_kind(&self, t: &Text, props: &mut Props) -> EntityKind {
        let ins = ocs_to_wcs(t.extrusion, [t.insertion[0], t.insertion[1], t.elevation]);
        let align = t
            .alignment
            .map(|a| ocs_to_wcs(t.extrusion, [a[0], a[1], t.elevation]));
        let (h, v) = (halign(t.halign), valign(t.valign));
        let two_point = matches!(h, HAlign::Aligned | HAlign::Fit);
        let position = match (h, v, align) {
            (HAlign::Left, VAlign::Baseline, _) | (_, _, None) => ins,
            _ if two_point => ins,
            (_, _, Some(a)) => a,
        };
        let end_point = if two_point { align } else { None };
        props.insert("dwg.insertion_point".into(), point_value(ins));
        if let Some(a) = align {
            props.insert("dwg.alignment_point".into(), point_value(a));
        }
        thickness(props, t.thickness);
        if t.generation != 0 {
            props.insert("dwg.generation".into(), Value::Int(i64::from(t.generation)));
        }
        EntityKind::Text {
            position,
            end_point,
            height: t.height,
            rotation: t.rotation,
            width_factor: t.width_factor,
            oblique: t.oblique,
            value: t.value.clone(),
            style: t.style.and_then(|h| self.names.styles.get(&h)).cloned(),
            halign: h,
            valign: v,
            normal: normal(t.extrusion),
        }
    }

    fn mtext_kind(&self, m: &MText) -> EntityKind {
        EntityKind::MText {
            position: p3(m.insertion),
            height: m.height,
            width: m.rect_width,
            rotation: ocs_angle(m.extrusion, m.x_axis),
            plain: plain_text(&m.value),
            value: m.value.clone(),
            style: m.style.and_then(|h| self.names.styles.get(&h)).cloned(),
            attachment: attachment(m.attachment),
            line_spacing: m.line_spacing.unwrap_or(1.0),
            normal: normal(m.extrusion),
        }
    }

    /// POLYLINE_2D/3D → Polyline, POLYLINE_PFACE/MESH → Mesh, from the VERTEX records.
    fn polyline_kind(&mut self, p: &Polyline, props: &mut Props) -> EntityKind {
        let children = self.child_handles(&p.vertices, p.vertices_are_range, &VERTEX_TYPES);
        let mut vertices = Vec::new();
        let mut faces = Vec::new();
        for h in children {
            let Some(o) = self.file.objects.get(&h) else {
                continue;
            };
            match &o.data {
                ObjectData::Vertex(v) => vertices.push((o.type_code, v.clone())),
                ObjectData::PfaceFace(f) => faces.push(f.indices),
                _ => {}
            }
        }
        match p.kind {
            PolylineKind::PolyFace => {
                let points: Vec<Point3> = vertices
                    .iter()
                    .filter(|(t, _)| *t == 0x0D)
                    .map(|(_, v)| p3(v.position))
                    .collect();
                let n = points.len() as i64;
                let faces = faces
                    .iter()
                    .map(|idx| {
                        idx.iter()
                            .map(|&i| i64::from(i).abs())
                            .filter(|&i| i >= 1 && i <= n)
                            .map(|i| (i - 1) as u32)
                            .collect::<Vec<_>>()
                    })
                    .filter(|f| f.len() >= 3)
                    .collect();
                props.insert("dwg.polyface".into(), Value::Bool(true));
                EntityKind::Mesh {
                    vertices: points,
                    faces,
                }
            }
            PolylineKind::Mesh => {
                let points: Vec<Point3> = vertices
                    .iter()
                    .filter(|(t, v)| *t == 0x0C && v.flags & 16 == 0)
                    .map(|(_, v)| p3(v.position))
                    .collect();
                let (m, n) = (
                    usize::try_from(p.m).unwrap_or(0),
                    usize::try_from(p.n).unwrap_or(0),
                );
                let mut grid = Vec::new();
                if m >= 2 && n >= 2 && m.checked_mul(n).is_some_and(|t| t <= points.len()) {
                    let rows = if p.flags & 1 != 0 { m } else { m - 1 };
                    let cols = if p.flags & 32 != 0 { n } else { n - 1 };
                    for i in 0..rows {
                        for j in 0..cols {
                            let (i2, j2) = ((i + 1) % m, (j + 1) % n);
                            grid.push(vec![
                                (i * n + j) as u32,
                                (i2 * n + j) as u32,
                                (i2 * n + j2) as u32,
                                (i * n + j2) as u32,
                            ]);
                        }
                    }
                }
                props.insert(
                    "dwg.polygon_mesh".into(),
                    Value::List(vec![Value::Int(i64::from(p.m)), Value::Int(i64::from(p.n))]),
                );
                EntityKind::Mesh {
                    vertices: points,
                    faces: grid,
                }
            }
            PolylineKind::TwoD | PolylineKind::ThreeD => {
                let three_d = p.kind == PolylineKind::ThreeD;
                let list = vertices
                    .iter()
                    .filter(|(_, v)| v.flags & 16 == 0)
                    .map(|(_, v)| {
                        let position = if three_d {
                            p3(v.position)
                        } else {
                            let z = if p.elevation != 0.0 {
                                p.elevation
                            } else {
                                v.position[2]
                            };
                            ocs_to_wcs(p.extrusion, [v.position[0], v.position[1], z])
                        };
                        Vertex {
                            position,
                            bulge: v.bulge,
                            start_width: v.start_width,
                            end_width: v.end_width,
                        }
                    })
                    .collect();
                thickness(props, p.thickness);
                if three_d {
                    props.insert("dwg.polyline_3d".into(), Value::Bool(true));
                }
                if p.flags & 2 != 0 {
                    props.insert("dwg.polyline_fit".into(), Value::Text("curve".into()));
                } else if p.flags & 4 != 0 {
                    props.insert("dwg.polyline_fit".into(), Value::Text("spline".into()));
                }
                if p.flags & 128 != 0 {
                    props.insert("dwg.plinegen".into(), Value::Bool(true));
                }
                EntityKind::Polyline {
                    vertices: list,
                    closed: p.flags & 1 != 0,
                    normal: if three_d {
                        Vec3::Z
                    } else {
                        normal(p.extrusion)
                    },
                }
            }
        }
    }

    fn dimension_kind(&self, d: &Dimension, props: &mut Props) -> EntityKind {
        let ocs = |xy: [f64; 2]| ocs_to_wcs(d.extrusion, [xy[0], xy[1], d.elevation]);
        let mut points = vec![p3(d.pt10)];
        let mut roles = vec![Value::Text("10".into())];
        for (label, p) in [("13", d.pt13), ("14", d.pt14), ("15", d.pt15)] {
            if let Some(p) = p {
                points.push(p3(p));
                roles.push(Value::Text(label.into()));
            }
        }
        // Point 16 (angular 2-line arc point) is an OCS point like 11 and 12.
        if let Some(p) = d.pt16 {
            points.push(ocs(p));
            roles.push(Value::Text("16".into()));
        }
        if d.pt12 != [0.0, 0.0] {
            points.push(ocs(d.pt12));
            roles.push(Value::Text("12".into()));
        }
        props.insert("dwg.dim_roles".into(), Value::List(roles));
        let dimension_type = match d.type_code {
            0x14 => DimensionType::Ordinate,
            0x15 => DimensionType::Linear,
            0x16 => DimensionType::Aligned,
            0x17 => DimensionType::Angular3Point,
            0x18 => DimensionType::Angular,
            0x19 => DimensionType::Radius,
            0x1A => DimensionType::Diameter,
            _ => DimensionType::Other,
        };
        for (key, value) in [
            ("dwg.dim_rotation", d.rotation),
            ("dwg.dim_ext_line_rotation", d.ext_line_rotation),
            ("dwg.dim_leader_length", d.leader_length),
            (
                "dwg.dim_text_rotation",
                Some(d.text_rotation).filter(|r| *r != 0.0),
            ),
        ] {
            if let Some(v) = value {
                props.insert(key.into(), Value::Float(v));
            }
        }
        if let Some(f) = d.flags2 {
            props.insert("dwg.dim_ordinate_x".into(), Value::Bool(f & 1 != 0));
        }
        props.insert(
            "dwg.dim_user_text_position".into(),
            Value::Bool(d.flags1 & 1 == 0),
        );
        EntityKind::Dimension {
            dimension_type,
            points,
            text_position: Some(ocs(d.text_midpoint)),
            measurement: d.measurement,
            text: Some(d.user_text.clone()).filter(|t| !t.is_empty()),
            block: d.block.and_then(|h| self.names.blocks.get(&h)).cloned(),
            style: d
                .dimstyle
                .and_then(|h| self.names.dimstyles.get(&h))
                .cloned(),
        }
    }

    fn image_kind(&self, im: &Image, props: &mut Props) -> EntityKind {
        props.insert("dwg.image_flags".into(), Value::Int(i64::from(im.flags)));
        if im.clipping {
            props.insert(
                "dwg.clip_vertices".into(),
                Value::List(
                    im.clip_vertices
                        .iter()
                        .map(|p| Value::List(vec![Value::Float(p[0]), Value::Float(p[1])]))
                        .collect(),
                ),
            );
        }
        let size_px = (im.size[0] >= 0.0
            && im.size[1] >= 0.0
            && im.size[0] <= f64::from(u32::MAX)
            && im.size[1] <= f64::from(u32::MAX))
        .then(|| [im.size[0] as u32, im.size[1] as u32]);
        EntityKind::Image {
            path: im
                .imagedef
                .and_then(|h| self.names.imagedefs.get(&h))
                .cloned(),
            position: p3(im.insertion),
            u_vector: v3(im.u).scaled(im.size[0]),
            v_vector: v3(im.v).scaled(im.size[1]),
            size_px,
        }
    }
}

fn thickness(props: &mut Props, t: f64) {
    if t != 0.0 {
        props.insert("dwg.thickness".into(), Value::Float(t));
    }
}

fn extrusion(props: &mut Props, e: [f64; 3]) {
    if !is_unit_z(e) {
        props.insert("dwg.extrusion".into(), list3(e));
    }
}

fn face_kind(f: &Face, props: &mut Props) -> EntityKind {
    let filled = f.kind != FaceKind::Face3d;
    let conv = |p: [f64; 3]| {
        if filled {
            ocs_to_wcs(f.extrusion, p)
        } else {
            p3(p)
        }
    };
    let [c1, c2, c3, c4] = f.corners.map(conv);
    let points = match (c4 != c3, filled) {
        (true, true) => vec![c1, c2, c4, c3],
        (true, false) => vec![c1, c2, c3, c4],
        _ => vec![c1, c2, c3],
    };
    thickness(props, f.thickness);
    if f.invisible_edges != 0 {
        props.insert(
            "dwg.invisible_edges".into(),
            Value::Int(i64::from(f.invisible_edges)),
        );
    }
    let name = match f.kind {
        FaceKind::Solid => "SOLID",
        FaceKind::Trace => "TRACE",
        FaceKind::Face3d => "3DFACE",
    };
    props.insert("dwg.face_type".into(), Value::Text(name.into()));
    EntityKind::Face { points, filled }
}

fn hatch_kind(h: &Hatch, props: &mut Props) -> EntityKind {
    let to_world = |p: [f64; 2]| ocs_to_wcs(h.extrusion, [p[0], p[1], h.elevation]);
    let dir_world = |p: [f64; 2]| {
        let w = ocs_to_wcs(h.extrusion, [p[0], p[1], 0.0]);
        Vec3::new(w.x, w.y, w.z)
    };
    let loops = h
        .paths
        .iter()
        .map(|path| {
            let edges = if path.flags & 2 != 0 {
                let vertices = path
                    .polyline
                    .iter()
                    .enumerate()
                    .map(|(i, p)| Vertex {
                        position: to_world(*p),
                        bulge: path.bulges.get(i).copied().unwrap_or(0.0),
                        ..Vertex::default()
                    })
                    .collect();
                vec![HatchEdge::Polyline {
                    vertices,
                    closed: path.closed,
                }]
            } else {
                path.edges
                    .iter()
                    .map(|e| match e {
                        HatchEdgeData::Line { start, end } => HatchEdge::Line {
                            start: to_world(*start),
                            end: to_world(*end),
                        },
                        HatchEdgeData::Arc {
                            center,
                            radius,
                            start,
                            end,
                            ccw,
                        } => HatchEdge::Arc {
                            center: to_world(*center),
                            radius: *radius,
                            start_angle: *start,
                            end_angle: *end,
                            ccw: *ccw,
                        },
                        HatchEdgeData::Ellipse {
                            center,
                            major_axis,
                            ratio,
                            start,
                            end,
                            ccw,
                        } => HatchEdge::Ellipse {
                            center: to_world(*center),
                            major_axis: dir_world(*major_axis),
                            ratio: *ratio,
                            start_param: *start,
                            end_param: *end,
                            ccw: *ccw,
                        },
                        HatchEdgeData::Spline {
                            degree,
                            knots,
                            control_points,
                            weights,
                            ..
                        } => HatchEdge::Spline {
                            degree: u32::try_from(*degree).unwrap_or(0),
                            knots: knots.clone(),
                            control_points: control_points.iter().map(|p| to_world(*p)).collect(),
                            weights: weights.clone(),
                        },
                    })
                    .collect()
            };
            HatchLoop {
                edges,
                external: path.flags & 1 != 0 || path.flags & 16 != 0,
            }
        })
        .collect();
    props.insert("dwg.hatch_style".into(), Value::Int(i64::from(h.style)));
    props.insert(
        "dwg.hatch_pattern_type".into(),
        Value::Int(i64::from(h.pattern_type)),
    );
    if h.associative {
        props.insert("dwg.hatch_associative".into(), Value::Bool(true));
    }
    if h.gradient == Some(true) {
        props.insert(
            "dwg.hatch_gradient".into(),
            Value::Text(h.gradient_name.clone()),
        );
    }
    if !h.lines.is_empty() {
        let lines = h
            .lines
            .iter()
            .map(|l| {
                let mut row = vec![
                    Value::Float(l.angle),
                    Value::Float(l.base[0]),
                    Value::Float(l.base[1]),
                    Value::Float(l.offset[0]),
                    Value::Float(l.offset[1]),
                ];
                row.extend(l.dashes.iter().map(|d| Value::Float(*d)));
                Value::List(row)
            })
            .collect();
        props.insert("dwg.hatch_pattern_lines".into(), Value::List(lines));
    }
    EntityKind::Hatch {
        loops,
        solid: h.solid,
        pattern: Some(h.pattern.clone()).filter(|p| !p.is_empty()),
        pattern_scale: if h.solid { 1.0 } else { h.scale },
        pattern_angle: if h.solid { 0.0 } else { h.angle },
        normal: normal(h.extrusion),
    }
}
