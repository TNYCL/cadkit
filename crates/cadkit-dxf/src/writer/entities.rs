//! Entity serialisation for all supported versions.
//!
//! DXF stores most planar entities in OCS coordinates; the model holds world coordinates,
//! so points are converted back with the entity's normal (`Ocs::to_local`).

use cadkit_core::{
    Attachment, Color, DimensionType, Entity, EntityKind, HAlign, HatchEdge, Lineweight, Point3,
    Props, VAlign, Value as MValue, Vec3, Vertex,
};

use super::curves::{MAX_DEGREE, clamped_knots, ellipse_points, spline_points};
use super::out::Out;
use super::{Writer, clean_name};
use crate::native::{CodeType, code_type};
use crate::ocs::Ocs;

/// Application id carrying the name of a flattened group.
pub(crate) const GROUP_APPID: &str = "CADKIT_GROUP";

/// An IMAGE entity awaiting its IMAGEDEF / reactor objects.
pub(crate) struct ImageUse {
    pub path: String,
    pub def: u64,
    pub reactor: u64,
    pub entity: u64,
    pub size: [u32; 2],
}

/// Standard AutoCAD line weights in hundredths of a millimetre.
const LINEWEIGHTS: [i64; 24] = [
    0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200,
    211,
];

/// Group 370 value for `lw`; `by_layer` is what ByLayer maps to (-1 for entities, -3 for layers).
pub(crate) fn lineweight_code(lw: Lineweight, by_layer: i64) -> i64 {
    match lw {
        Lineweight::ByLayer => by_layer,
        Lineweight::ByBlock => -2,
        Lineweight::Default => -3,
        Lineweight::Millimeters(mm) => {
            let target = if mm.is_finite() {
                (mm * 100.0).round()
            } else {
                0.0
            };
            LINEWEIGHTS
                .iter()
                .copied()
                .min_by_key(|w| ((*w as f64) - target).abs() as i64)
                .unwrap_or(0)
        }
    }
}

/// Group 62 value and optional group 420 for `c` (256 = ByLayer, 0 = ByBlock).
pub(crate) fn color_codes(c: Color) -> (i64, Option<i64>) {
    match c {
        Color::ByLayer => (256, None),
        Color::ByBlock => (0, None),
        Color::Aci { index } => (i64::from(index), None),
        Color::Rgb { r, g, b } => {
            let mut best = (7u8, i64::MAX);
            for (i, (cr, cg, cb)) in cadkit_core::aci::TABLE.iter().enumerate().skip(1) {
                let d = (i64::from(*cr) - i64::from(r)).pow(2)
                    + (i64::from(*cg) - i64::from(g)).pow(2)
                    + (i64::from(*cb) - i64::from(b)).pow(2);
                if d < best.1 {
                    best = (i as u8, d);
                }
            }
            (
                i64::from(best.0),
                Some((i64::from(r) << 16) | (i64::from(g) << 8) | i64::from(b)),
            )
        }
    }
}

/// Default DIMSTYLE body (values of a metric style; strict readers only need the record to exist).
pub(crate) fn dimstyle_defaults(out: &mut Out, r12: bool, text_style_handle: u64) {
    out.int(70, 0);
    for code in [3, 4, 5, 6, 7] {
        out.word(code, "");
    }
    for (code, v) in [
        (40, 1.0),
        (41, 2.5),
        (42, 0.625),
        (43, 3.75),
        (44, 1.25),
        (45, 0.0),
        (46, 0.0),
        (47, 0.0),
        (48, 0.0),
        (140, 2.5),
        (141, 2.5),
        (142, 0.0),
        (143, 0.039_370_078_74),
        (144, 1.0),
        (145, 0.0),
        (146, 1.0),
        (147, 0.625),
    ] {
        out.real(code, v);
    }
    for (code, v) in [
        (71, 0),
        (72, 0),
        (73, 0),
        (74, 0),
        (75, 0),
        (76, 0),
        (77, 1),
        (78, 8),
    ] {
        out.int(code, v);
    }
    if !r12 {
        out.int(79, 3);
    }
    for (code, v) in [
        (170, 0),
        (171, 3),
        (172, 1),
        (173, 0),
        (174, 0),
        (175, 0),
        (176, 0),
        (177, 0),
        (178, 0),
    ] {
        out.int(code, v);
    }
    if !r12 {
        for (code, v) in [
            (179, 2),
            (271, 2),
            (272, 2),
            (273, 2),
            (274, 3),
            (275, 0),
            (276, 0),
            (277, 2),
            (278, 44),
            (279, 0),
            (280, 0),
            (281, 0),
            (282, 0),
            (283, 0),
            (284, 8),
            (285, 0),
            (286, 0),
            (288, 0),
            (289, 3),
        ] {
            out.int(code, v);
        }
        if text_style_handle != 0 {
            out.handle(340, text_style_handle);
        }
        out.int(371, -2);
        out.int(372, -2);
    }
}

/// Everything the common entity header needs.
#[derive(Clone)]
pub(crate) struct Attr {
    pub layer: String,
    pub linetype: Option<String>,
    pub color: Color,
    pub lineweight: Lineweight,
    pub visible: bool,
    pub paper: bool,
    pub owner: u64,
}

fn value_text(v: &MValue) -> String {
    match v {
        MValue::Text(s) => s.clone(),
        MValue::Int(i) => i.to_string(),
        MValue::Float(f) => super::out::fmt_f64(*f),
        MValue::Bool(b) => (if *b { "1" } else { "0" }).to_owned(),
        MValue::Null => String::new(),
        MValue::List(_) | MValue::Bytes(_) => String::new(),
    }
}

fn prop_point(props: &Props, key: &str) -> Option<Point3> {
    match props.get(key) {
        Some(MValue::List(v)) if v.len() == 3 => {
            let f = |i: usize| match v.get(i) {
                Some(MValue::Float(x)) => Some(*x),
                Some(MValue::Int(x)) => Some(*x as f64),
                _ => None,
            };
            Some(Point3::new(f(0)?, f(1)?, f(2)?))
        }
        _ => None,
    }
}

fn prop_int(props: &Props, key: &str) -> Option<i64> {
    match props.get(key) {
        Some(MValue::Int(i)) => Some(*i),
        _ => None,
    }
}

fn prop_f64(props: &Props, key: &str) -> Option<f64> {
    match props.get(key) {
        Some(MValue::Float(f)) => Some(*f),
        Some(MValue::Int(i)) => Some(*i as f64),
        _ => None,
    }
}

fn prop_text<'p>(props: &'p Props, key: &str) -> Option<&'p str> {
    match props.get(key) {
        Some(MValue::Text(s)) => Some(s),
        _ => None,
    }
}

fn v3(v: Vec3) -> Point3 {
    Point3::new(v.x, v.y, v.z)
}

fn ext(out: &mut Out, ocs: &Ocs) {
    if !ocs.is_world() {
        out.point(210, v3(ocs.az));
    }
}

fn same_z(points: &[Point3]) -> bool {
    match points.first() {
        Some(f) => points.iter().all(|p| (p.z - f.z).abs() < 1e-9),
        None => true,
    }
}

fn halign_code(h: HAlign) -> i64 {
    match h {
        HAlign::Left => 0,
        HAlign::Center => 1,
        HAlign::Right => 2,
        HAlign::Aligned => 3,
        HAlign::Middle => 4,
        HAlign::Fit => 5,
    }
}

fn valign_code(v: VAlign) -> i64 {
    match v {
        VAlign::Baseline => 0,
        VAlign::Bottom => 1,
        VAlign::Middle => 2,
        VAlign::Top => 3,
    }
}

fn attachment_code(a: Attachment) -> i64 {
    match a {
        Attachment::TopLeft => 1,
        Attachment::TopCenter => 2,
        Attachment::TopRight => 3,
        Attachment::MiddleLeft => 4,
        Attachment::MiddleCenter => 5,
        Attachment::MiddleRight => 6,
        Attachment::BottomLeft => 7,
        Attachment::BottomCenter => 8,
        Attachment::BottomRight => 9,
    }
}

/// Splits `s` into pieces of at most `max` bytes without cutting a character.
fn chunks(s: &str, max: usize) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = s;
    while rest.len() > max {
        let mut cut = max;
        while cut > 0 && !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        if cut == 0 {
            break;
        }
        let (a, b) = rest.split_at(cut);
        out.push(a);
        rest = b;
    }
    out.push(rest);
    out
}

/// Attributes of a sub-entity (VERTEX, ATTRIB, SEQEND): group 330 is the parent's handle.
fn owned_by(a: &Attr, parent: u64) -> Attr {
    Attr {
        owner: parent,
        ..a.clone()
    }
}

impl Writer<'_> {
    pub fn write_entity_list(&mut self, out: &mut Out, list: &[Entity], owner: u64, paper: bool) {
        for e in list {
            self.write_entity(out, e, owner, paper, None, 0);
        }
    }

    fn attr(&mut self, e: &Entity, owner: u64, paper: bool) -> Attr {
        let layer = self.layers.add(e.layer.as_deref().unwrap_or("0"));
        let linetype = e.linetype.as_deref().and_then(|l| self.linetype_name(l));
        Attr {
            layer,
            linetype,
            color: e.color,
            lineweight: e.lineweight,
            visible: e.visible,
            paper,
            owner,
        }
    }

    /// Writes `0 TYPE`, handle/owner and the common groups; returns the new handle (0 for R12).
    fn head(&mut self, out: &mut Out, etype: &str, a: &Attr, sub: &str) -> u64 {
        out.word(0, etype);
        let mut h = 0;
        if !self.r12 {
            h = self.new_handle();
            out.handle(5, h);
            out.handle(330, a.owner);
            out.word(100, "AcDbEntity");
        }
        if a.paper {
            out.int(67, 1);
        }
        out.text(8, &a.layer);
        if let Some(lt) = &a.linetype {
            out.text(6, lt);
        }
        let (c62, c420) = color_codes(a.color);
        if c62 != 256 {
            out.int(62, c62);
        }
        if !self.r12 {
            let lw = lineweight_code(a.lineweight, -1);
            if lw != -1 {
                out.int(370, lw);
            }
        }
        if self.ge2004 {
            if let Some(v) = c420 {
                out.int(420, v);
            }
        }
        if !a.visible && !self.r12 {
            out.int(60, 1);
        }
        if !self.r12 && !sub.is_empty() {
            out.word(100, sub);
        }
        h
    }

    fn sub(&self, out: &mut Out, name: &str) {
        if !self.r12 {
            out.word(100, name);
        }
    }

    fn xdata(&mut self, out: &mut Out, props: &Props, group: Option<&str>) {
        let mut wrote_group = false;
        for (key, value) in props {
            let Some(app) = key.strip_prefix("dxf.xdata.") else {
                continue;
            };
            let MValue::List(items) = value else { continue };
            if app == GROUP_APPID {
                wrote_group = true;
            }
            out.text(1001, &clean_name(app));
            for item in items {
                let MValue::List(pair) = item else { continue };
                let (Some(MValue::Int(code)), Some(v)) = (pair.first(), pair.get(1)) else {
                    continue;
                };
                let code = *code as i32;
                if !(1000..=1071).contains(&code) || code == 1001 {
                    continue;
                }
                match (code_type(code), v) {
                    (CodeType::Str, _) => out.text(code, &value_text(v)),
                    (CodeType::Double, MValue::Float(f)) => out.real(code, *f),
                    (CodeType::Double, MValue::Int(i)) => out.real(code, *i as f64),
                    (CodeType::Int16 | CodeType::Int32 | CodeType::Int64, MValue::Int(i)) => {
                        out.int(code, *i)
                    }
                    (CodeType::Binary, MValue::Bytes(b)) => out.bytes(code, b),
                    _ => {}
                }
            }
        }
        if let (Some(name), false) = (group, wrote_group) {
            out.text(1001, GROUP_APPID);
            out.text(1000, name);
        }
    }

    fn skip(&mut self, what: &str, why: &str) {
        self.warn("dxf.write.skipped", format!("{what}: {why}"));
    }

    pub fn write_entity(
        &mut self,
        out: &mut Out,
        e: &Entity,
        owner: u64,
        paper: bool,
        group: Option<&str>,
        depth: u32,
    ) {
        let a = self.attr(e, owner, paper);
        match &e.kind {
            EntityKind::Point { position } => {
                self.head(out, "POINT", &a, "AcDbPoint");
                out.point(10, *position);
            }
            EntityKind::Line { start, end } => {
                self.head(out, "LINE", &a, "AcDbLine");
                out.point(10, *start);
                out.point(11, *end);
            }
            EntityKind::Circle {
                center,
                radius,
                normal,
            } => {
                let ocs = Ocs::new(*normal);
                self.head(out, "CIRCLE", &a, "AcDbCircle");
                out.point(10, ocs.to_local(*center));
                out.real(40, *radius);
                ext(out, &ocs);
            }
            EntityKind::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                normal,
            } => {
                let ocs = Ocs::new(*normal);
                self.head(out, "ARC", &a, "AcDbCircle");
                out.point(10, ocs.to_local(*center));
                out.real(40, *radius);
                ext(out, &ocs);
                self.sub(out, "AcDbArc");
                out.real(50, start_angle.to_degrees());
                out.real(51, end_angle.to_degrees());
            }
            EntityKind::Ellipse {
                center,
                major_axis,
                ratio,
                start_param,
                end_param,
                normal,
            } => {
                self.ellipse(
                    out,
                    &a,
                    *center,
                    *major_axis,
                    *ratio,
                    *start_param,
                    *end_param,
                    *normal,
                );
            }
            EntityKind::Polyline {
                vertices,
                closed,
                normal,
            } => self.polyline(out, e, &a, vertices, *closed, *normal),
            EntityKind::Spline {
                degree,
                knots,
                control_points,
                weights,
                fit_points,
                closed,
            } => {
                self.spline(
                    out,
                    e,
                    &a,
                    *degree,
                    knots,
                    control_points,
                    weights,
                    fit_points,
                    *closed,
                );
            }
            EntityKind::Text {
                position,
                end_point,
                height,
                rotation,
                width_factor,
                oblique,
                value,
                style,
                halign,
                valign,
                normal,
            } => {
                self.text(
                    out,
                    &a,
                    *position,
                    *end_point,
                    *height,
                    *rotation,
                    *width_factor,
                    *oblique,
                    value,
                    style.as_deref(),
                    *halign,
                    *valign,
                    *normal,
                );
            }
            EntityKind::MText {
                position,
                height,
                width,
                rotation,
                value,
                plain,
                style,
                attachment,
                line_spacing,
                normal,
            } => {
                self.mtext(
                    out,
                    &a,
                    *position,
                    *height,
                    *width,
                    *rotation,
                    value,
                    plain,
                    style.as_deref(),
                    *attachment,
                    *line_spacing,
                    *normal,
                );
            }
            EntityKind::Insert {
                block,
                position,
                scale,
                rotation,
                normal,
                columns,
                rows,
                column_spacing,
                row_spacing,
            } => {
                let Some(name) = self.blocks.get(block).cloned() else {
                    self.skip("INSERT", &format!("block `{block}` is not defined"));
                    return;
                };
                let ocs = Ocs::new(*normal);
                let insert_handle = self.head(out, "INSERT", &a, "AcDbBlockReference");
                if !e.attributes.is_empty() {
                    out.int(66, 1);
                }
                out.text(2, &name);
                out.point(10, ocs.to_local(*position));
                if scale.x != 1.0 {
                    out.real(41, scale.x);
                }
                if scale.y != 1.0 {
                    out.real(42, scale.y);
                }
                if scale.z != 1.0 {
                    out.real(43, scale.z);
                }
                if *rotation != 0.0 {
                    out.real(50, rotation.to_degrees());
                }
                if *columns > 1 || *rows > 1 {
                    out.int(70, i64::from((*columns).max(1)));
                    out.int(71, i64::from((*rows).max(1)));
                    out.real(44, *column_spacing);
                    out.real(45, *row_spacing);
                }
                ext(out, &ocs);
                self.xdata(out, &e.props, group);
                if !e.attributes.is_empty() {
                    let child = owned_by(&a, insert_handle);
                    for at in &e.attributes {
                        self.head(out, "ATTRIB", &child, "AcDbText");
                        out.point(10, ocs.to_local(at.position.unwrap_or(*position)));
                        out.real(40, 2.5);
                        out.text(1, &value_text(&at.value));
                        self.sub(out, "AcDbAttribute");
                        out.text(2, &at.tag);
                        out.int(70, i64::from(at.invisible));
                        ext(out, &ocs);
                    }
                    self.seqend(out, &child);
                }
                return;
            }
            EntityKind::Hatch {
                loops,
                solid,
                pattern,
                pattern_scale,
                pattern_angle,
                normal,
            } => {
                self.hatch(
                    out,
                    e,
                    &a,
                    loops,
                    *solid,
                    pattern.as_deref(),
                    *pattern_scale,
                    *pattern_angle,
                    *normal,
                    group,
                    depth,
                );
                return;
            }
            EntityKind::Dimension {
                dimension_type,
                points,
                text_position,
                measurement,
                text,
                block,
                style,
            } => {
                self.dimension(
                    out,
                    e,
                    &a,
                    *dimension_type,
                    points,
                    *text_position,
                    *measurement,
                    text.as_deref(),
                    block.as_deref(),
                    style.as_deref(),
                );
            }
            EntityKind::Face { points, filled } => self.face(out, e, &a, points, *filled),
            EntityKind::Polygon {
                exterior,
                interiors,
            } => {
                let normal = cadkit_core::polygon::normal(exterior).unwrap_or(Vec3::Z);
                let loops: Vec<_> = std::iter::once(exterior)
                    .chain(interiors.iter())
                    .enumerate()
                    .map(|(i, ring)| cadkit_core::HatchLoop {
                        external: i == 0,
                        edges: vec![HatchEdge::Polyline {
                            vertices: ring.iter().copied().map(Vertex::at).collect(),
                            closed: true,
                        }],
                    })
                    .collect();
                self.hatch(
                    out, e, &a, &loops, true, None, 1.0, 0.0, normal, group, depth,
                );
                return;
            }
            EntityKind::Leader {
                vertices,
                arrowhead,
            } => {
                if vertices.len() < 2 {
                    self.skip("LEADER", "needs at least two vertices");
                    return;
                }
                if self.r12 {
                    self.polyline3d(out, &a, vertices, false);
                } else {
                    self.head(out, "LEADER", &a, "AcDbLeader");
                    out.text(3, "Standard");
                    out.int(71, i64::from(*arrowhead));
                    out.int(72, prop_int(&e.props, "dxf.leader.path_type").unwrap_or(0));
                    out.int(73, 3);
                    out.int(74, 1);
                    out.int(75, 0);
                    out.int(76, vertices.len() as i64);
                    for v in vertices {
                        out.point(10, *v);
                    }
                    out.point(210, Point3::new(0.0, 0.0, 1.0));
                    out.point(211, Point3::new(1.0, 0.0, 0.0));
                    out.point(212, Point3::default());
                    out.point(213, Point3::default());
                }
            }
            EntityKind::Image {
                path,
                position,
                u_vector,
                v_vector,
                size_px,
            } => {
                if self.r12 {
                    self.skip("IMAGE", "not available in R12");
                    return;
                }
                let Some(path) = path.as_ref().filter(|p| !p.is_empty()) else {
                    self.skip("IMAGE", "no image path");
                    return;
                };
                let size = size_px.unwrap_or([1, 1]);
                let (w, h) = (f64::from(size[0].max(1)), f64::from(size[1].max(1)));
                let handle = self.head(out, "IMAGE", &a, "AcDbRasterImage");
                let def = self.new_handle();
                let reactor = self.new_handle();
                self.images.push(ImageUse {
                    path: path.clone(),
                    def,
                    reactor,
                    entity: handle,
                    size,
                });
                out.int(90, 0);
                out.point(10, *position);
                out.point(
                    11,
                    Point3::new(u_vector.x / w, u_vector.y / w, u_vector.z / w),
                );
                out.point(
                    12,
                    Point3::new(v_vector.x / h, v_vector.y / h, v_vector.z / h),
                );
                out.point2(13, f64::from(size[0]), f64::from(size[1]));
                out.handle(340, def);
                out.int(
                    70,
                    prop_int(&e.props, "dxf.image.display_flags").unwrap_or(7),
                );
                out.int(280, 1);
                out.int(281, 50);
                out.int(282, 50);
                out.int(283, 0);
                out.handle(360, reactor);
            }
            EntityKind::Viewport {
                center,
                width,
                height,
                view_center,
                view_height,
            } => {
                if !a.paper {
                    self.skip("VIEWPORT", "only valid in paper space");
                    return;
                }
                self.viewport_id += 1;
                let id = self.viewport_id;
                self.head(out, "VIEWPORT", &a, "AcDbViewport");
                out.point(10, *center);
                out.real(40, *width);
                out.real(41, *height);
                out.int(68, 1);
                out.int(69, id);
                if !self.r12 {
                    out.point2(12, view_center.x, view_center.y);
                    out.point2(13, 0.0, 0.0);
                    out.point2(14, 0.5, 0.5);
                    out.point2(15, 0.5, 0.5);
                    out.point(
                        16,
                        prop_point(&e.props, "dxf.viewport.view_direction")
                            .unwrap_or(Point3::new(0.0, 0.0, 1.0)),
                    );
                    out.point(
                        17,
                        prop_point(&e.props, "dxf.viewport.view_target").unwrap_or_default(),
                    );
                    out.real(42, 50.0);
                    out.real(43, 0.0);
                    out.real(44, 0.0);
                    out.real(45, *view_height);
                    out.real(50, 0.0);
                    out.real(51, prop_f64(&e.props, "dxf.viewport.twist").unwrap_or(0.0));
                    out.int(72, 1000);
                    out.int(90, 32768);
                    out.text(1, "");
                } else if !e.props.contains_key("dxf.xdata.ACAD") {
                    // R12 keeps the view settings in extended data (application ACAD, MVIEW).
                    out.text(1001, "ACAD");
                    out.text(1000, "MVIEW");
                    out.text(1002, "{");
                    out.int(1070, 16);
                    out.point(
                        1010,
                        prop_point(&e.props, "dxf.viewport.view_target").unwrap_or_default(),
                    );
                    out.point(
                        1010,
                        prop_point(&e.props, "dxf.viewport.view_direction")
                            .unwrap_or(Point3::new(0.0, 0.0, 1.0)),
                    );
                    for v in [
                        prop_f64(&e.props, "dxf.viewport.twist").unwrap_or(0.0),
                        *view_height,
                        view_center.x,
                        view_center.y,
                        50.0,
                        0.0,
                        0.0,
                    ] {
                        out.real(1040, v);
                    }
                    for v in [0, 1000, 1, 1, 0, 0, 0, 0] {
                        out.int(1070, v);
                    }
                    for v in [0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5] {
                        out.real(1040, v);
                    }
                    out.int(1070, 0);
                    out.text(1002, "{");
                    out.text(1002, "}");
                    out.text(1002, "}");
                }
            }
            EntityKind::Mesh { vertices, faces } => self.mesh(out, &a, vertices, faces),
            EntityKind::Group { children, name, .. } => {
                if depth >= 64 {
                    self.skip("GROUP", "nesting too deep");
                    return;
                }
                let label = name.as_deref().or(group);
                for c in children {
                    self.write_entity(out, c, owner, paper, label, depth + 1);
                }
                return;
            }
            EntityKind::Unknown { type_name } => {
                if type_name == "dxf.ATTDEF" && self.attdef(out, e, &a) {
                    self.xdata(out, &e.props, group);
                } else {
                    self.skip(type_name, "no DXF writer for this record");
                }
                return;
            }
        }
        self.xdata(out, &e.props, group);
    }

    fn seqend(&mut self, out: &mut Out, a: &Attr) {
        out.word(0, "SEQEND");
        if !self.r12 {
            let h = self.new_handle();
            out.handle(5, h);
            out.handle(330, a.owner);
            out.word(100, "AcDbEntity");
        }
        if a.paper {
            out.int(67, 1);
        }
        out.text(8, &a.layer);
    }

    fn vertex_head(&mut self, out: &mut Out, a: &Attr, sub: &str) {
        out.word(0, "VERTEX");
        if !self.r12 {
            let h = self.new_handle();
            out.handle(5, h);
            out.handle(330, a.owner);
            out.word(100, "AcDbEntity");
        }
        if a.paper {
            out.int(67, 1);
        }
        out.text(8, &a.layer);
        if !self.r12 {
            out.word(100, "AcDbVertex");
            out.word(100, sub);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn ellipse(
        &mut self,
        out: &mut Out,
        a: &Attr,
        center: Point3,
        major: Vec3,
        ratio: f64,
        start: f64,
        end: f64,
        normal: Vec3,
    ) {
        let finite = [
            center.x, center.y, center.z, major.x, major.y, major.z, start, end,
        ]
        .iter()
        .all(|v| v.is_finite());
        if !finite
            || !(ratio.is_finite() && ratio > 0.0)
            || (major.x == 0.0 && major.y == 0.0 && major.z == 0.0)
        {
            self.skip(
                "ELLIPSE",
                "non-finite value, invalid ratio or zero major axis",
            );
            return;
        }
        let n = normal.normalized().unwrap_or(Vec3::Z);
        if self.r12 {
            let pts = ellipse_points(center, major, ratio, start, end, n, 72);
            let full = (end - start).abs() >= std::f64::consts::TAU - 1e-9;
            self.polyline3d(out, a, &pts, full);
            return;
        }
        let (mut major, mut ratio, mut start, mut end) = (major, ratio, start, end);
        if ratio > 1.0 {
            // DXF wants the major axis first: swap axes and shift the parameters by a quarter turn.
            let minor = Vec3::new(
                (n.y * major.z - n.z * major.y) * ratio,
                (n.z * major.x - n.x * major.z) * ratio,
                (n.x * major.y - n.y * major.x) * ratio,
            );
            major = minor;
            ratio = 1.0 / ratio;
            start -= std::f64::consts::FRAC_PI_2;
            end -= std::f64::consts::FRAC_PI_2;
        }
        // Parameters must start in [0, 2 pi); shift the end by the same amount.
        let delta = start.rem_euclid(std::f64::consts::TAU) - start;
        start += delta;
        end += delta;
        self.head(out, "ELLIPSE", a, "AcDbEllipse");
        out.point(10, center);
        out.point(11, v3(major));
        out.point(210, v3(n));
        out.real(40, ratio.min(1.0));
        out.real(41, start);
        out.real(42, end);
    }

    fn polyline3d(&mut self, out: &mut Out, a: &Attr, pts: &[Point3], closed: bool) {
        if pts.len() < 2 {
            return;
        }
        let parent = self.head(out, "POLYLINE", a, "AcDb3dPolyline");
        let child = owned_by(a, parent);
        out.int(66, 1);
        out.point(10, Point3::default());
        out.int(70, 8 | i64::from(closed));
        for p in pts {
            self.vertex_head(out, &child, "AcDb3dPolylineVertex");
            out.point(10, *p);
            out.int(70, 32);
        }
        self.seqend(out, &child);
    }

    fn polyline(
        &mut self,
        out: &mut Out,
        e: &Entity,
        a: &Attr,
        vertices: &[Vertex],
        closed: bool,
        normal: Vec3,
    ) {
        if vertices.is_empty() {
            self.skip("POLYLINE", "no vertices");
            return;
        }
        let ocs = Ocs::new(normal);
        let is_3d_prop = matches!(e.props.get("dxf.polyline_3d"), Some(MValue::Bool(true)));
        let local: Vec<Point3> = vertices.iter().map(|v| ocs.to_local(v.position)).collect();
        let flat = same_z(&local);
        let plinegen = matches!(e.props.get("dxf.plinegen"), Some(MValue::Bool(true)));
        if is_3d_prop || !flat {
            let pts: Vec<Point3> = vertices.iter().map(|v| v.position).collect();
            self.polyline3d(out, a, &pts, closed);
            return;
        }
        let elevation = local.first().map_or(0.0, |p| p.z);
        if self.r12 {
            self.head(out, "POLYLINE", a, "");
            out.int(66, 1);
            out.point(10, Point3::new(0.0, 0.0, elevation));
            out.int(70, i64::from(closed) | if plinegen { 128 } else { 0 });
            ext(out, &ocs);
            for (v, p) in vertices.iter().zip(&local) {
                self.vertex_head(out, a, "");
                out.point(10, *p);
                if v.start_width != 0.0 {
                    out.real(40, v.start_width);
                }
                if v.end_width != 0.0 {
                    out.real(41, v.end_width);
                }
                if v.bulge != 0.0 {
                    out.real(42, v.bulge);
                }
            }
            self.seqend(out, a);
            return;
        }
        self.head(out, "LWPOLYLINE", a, "AcDbPolyline");
        out.int(90, vertices.len() as i64);
        out.int(70, i64::from(closed) | if plinegen { 128 } else { 0 });
        if elevation != 0.0 {
            out.real(38, elevation);
        }
        for (v, p) in vertices.iter().zip(&local) {
            out.point2(10, p.x, p.y);
            if v.start_width != 0.0 {
                out.real(40, v.start_width);
            }
            if v.end_width != 0.0 {
                out.real(41, v.end_width);
            }
            if v.bulge != 0.0 {
                out.real(42, v.bulge);
            }
        }
        ext(out, &ocs);
    }

    #[allow(clippy::too_many_arguments)]
    fn spline(
        &mut self,
        out: &mut Out,
        e: &Entity,
        a: &Attr,
        degree: u32,
        knots: &[f64],
        ctrl: &[Point3],
        weights: &[f64],
        fit: &[Point3],
        closed: bool,
    ) {
        let d = degree as usize;
        // Degrees from foreign formats can be absurd; no real spline exceeds MAX_DEGREE.
        let have_ctrl = (1..=MAX_DEGREE).contains(&d) && ctrl.len() > d;
        let knots_ok = knots.len() == ctrl.len().saturating_add(d).saturating_add(1);
        let knot_vec: Vec<f64> = if knots_ok {
            knots.to_vec()
        } else if have_ctrl {
            clamped_knots(ctrl.len(), d)
        } else {
            Vec::new()
        };
        if self.r12 {
            let mut pts = if have_ctrl {
                spline_points(d, &knot_vec, ctrl, weights, 8 * ctrl.len().max(8))
            } else {
                Vec::new()
            };
            if pts.len() < 2 {
                pts = if fit.len() >= 2 {
                    fit.to_vec()
                } else {
                    ctrl.to_vec()
                };
            }
            if pts.len() < 2 {
                self.skip("SPLINE", "no usable points");
                return;
            }
            self.polyline3d(out, a, &pts, closed);
            return;
        }
        if !have_ctrl && fit.len() < 2 {
            self.skip("SPLINE", "no usable control or fit points");
            return;
        }
        let mut flags = prop_int(&e.props, "dxf.spline.flags").unwrap_or(0) & !(4 | 16);
        if closed {
            if flags & 3 == 0 {
                flags |= 1;
            }
        } else {
            flags &= !3;
        }
        let weights_ok = have_ctrl && weights.len() == ctrl.len();
        if weights_ok {
            flags |= 4;
        }
        self.head(out, "SPLINE", a, "AcDbSpline");
        out.int(70, flags);
        out.int(71, if have_ctrl { degree as i64 } else { 3 });
        out.int(72, if have_ctrl { knot_vec.len() } else { 0 } as i64);
        out.int(73, if have_ctrl { ctrl.len() } else { 0 } as i64);
        out.int(74, fit.len() as i64);
        out.real(42, 1e-7);
        out.real(43, 1e-7);
        out.real(44, 1e-10);
        if have_ctrl {
            for k in &knot_vec {
                out.real(40, *k);
            }
            if weights_ok {
                for w in weights {
                    out.real(41, *w);
                }
            }
            for p in ctrl {
                out.point(10, *p);
            }
        }
        for p in fit {
            out.point(11, *p);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        out: &mut Out,
        a: &Attr,
        position: Point3,
        end_point: Option<Point3>,
        height: f64,
        rotation: f64,
        width_factor: f64,
        oblique: f64,
        value: &str,
        style: Option<&str>,
        halign: HAlign,
        valign: VAlign,
        normal: Vec3,
    ) {
        let ocs = Ocs::new(normal);
        let default_align = halign == HAlign::Left && valign == VAlign::Baseline;
        let two_point = matches!(halign, HAlign::Aligned | HAlign::Fit);
        let pos = ocs.to_local(position);
        // Group 10 is the baseline start for default and Aligned/Fit text; for justified text
        // the justification point is written as both group 10 and 11.
        let (p1, p2) = if default_align {
            (pos, None)
        } else if two_point {
            let p2 = end_point.map(|p| ocs.to_local(p)).unwrap_or(Point3::new(
                pos.x + height.max(1.0) * value.chars().count().max(1) as f64 * 0.6,
                pos.y,
                pos.z,
            ));
            (pos, Some(p2))
        } else {
            (pos, Some(pos))
        };
        self.head(out, "TEXT", a, "AcDbText");
        out.point(10, p1);
        out.real(40, height);
        out.text(1, value);
        if rotation != 0.0 {
            out.real(50, rotation.to_degrees());
        }
        if width_factor != 1.0 {
            out.real(41, width_factor);
        }
        if oblique != 0.0 {
            out.real(51, oblique.to_degrees());
        }
        if let Some(s) = style
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| self.styles.get(s).cloned())
        {
            out.text(7, &s);
        }
        if self.r12 {
            if halign_code(halign) != 0 {
                out.int(72, halign_code(halign));
            }
            if valign_code(valign) != 0 {
                out.int(73, valign_code(valign));
            }
        } else if halign_code(halign) != 0 {
            out.int(72, halign_code(halign));
        }
        if let Some(p2) = p2 {
            out.point(11, p2);
        }
        ext(out, &ocs);
        if !self.r12 {
            out.word(100, "AcDbText");
            if valign_code(valign) != 0 {
                out.int(73, valign_code(valign));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn mtext(
        &mut self,
        out: &mut Out,
        a: &Attr,
        position: Point3,
        height: f64,
        width: f64,
        rotation: f64,
        value: &str,
        plain: &str,
        style: Option<&str>,
        attachment: Attachment,
        line_spacing: f64,
        normal: Vec3,
    ) {
        let ocs = Ocs::new(normal);
        // Rebuild markup from plain text when only that is available (other formats).
        let markup: String = if value.is_empty() && !plain.is_empty() {
            plain.replace("\r\n", "\n").replace('\n', "\\P")
        } else {
            value.replace("\r\n", "\\P").replace('\n', "\\P")
        };
        let style = style
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| self.styles.get(s).cloned());
        if self.r12 {
            let text = if plain.is_empty() {
                cadkit_core::text::plain_text(value)
            } else {
                plain.to_owned()
            };
            let (h, v) = match attachment {
                Attachment::TopLeft => (0, 3),
                Attachment::TopCenter => (1, 3),
                Attachment::TopRight => (2, 3),
                Attachment::MiddleLeft => (0, 2),
                Attachment::MiddleCenter => (1, 2),
                Attachment::MiddleRight => (2, 2),
                Attachment::BottomLeft => (0, 1),
                Attachment::BottomCenter => (1, 1),
                Attachment::BottomRight => (2, 1),
            };
            let step = height * 5.0 / 3.0
                * if line_spacing > 0.0 {
                    line_spacing
                } else {
                    1.0
                };
            let (s, c) = rotation.sin_cos();
            let pos = ocs.to_local(position);
            for (i, line) in text.split('\n').enumerate() {
                let off = -(i as f64) * step;
                let p = Point3::new(pos.x - s * off, pos.y + c * off, pos.z);
                self.head(out, "TEXT", a, "");
                out.point(10, p);
                out.real(40, height);
                out.text(1, line);
                if rotation != 0.0 {
                    out.real(50, rotation.to_degrees());
                }
                if let Some(st) = &style {
                    out.text(7, st);
                }
                if h != 0 {
                    out.int(72, h);
                }
                if v != 0 {
                    out.int(73, v);
                }
                if h != 0 || v != 0 {
                    out.point(11, p);
                }
                ext(out, &ocs);
            }
            return;
        }
        self.head(out, "MTEXT", a, "AcDbMText");
        out.point(10, position);
        out.real(40, height);
        out.real(41, width);
        out.int(71, attachment_code(attachment));
        out.int(72, 1);
        let parts = chunks(&markup, 250);
        let last = parts.len().saturating_sub(1);
        for (i, part) in parts.iter().enumerate() {
            out.text(if i == last { 1 } else { 3 }, part);
        }
        if let Some(st) = &style {
            out.text(7, st);
        }
        if !ocs.is_world() {
            out.point(210, v3(ocs.az));
        }
        if rotation != 0.0 || !ocs.is_world() {
            let (s, c) = rotation.sin_cos();
            let dir = Point3::new(
                ocs.ax.x * c + ocs.ay.x * s,
                ocs.ax.y * c + ocs.ay.y * s,
                ocs.ax.z * c + ocs.ay.z * s,
            );
            out.point(11, dir);
        }
        if line_spacing != 1.0 && line_spacing > 0.0 {
            out.int(73, 1);
            out.real(44, line_spacing);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn dimension(
        &mut self,
        out: &mut Out,
        e: &Entity,
        a: &Attr,
        ty: DimensionType,
        points: &[Point3],
        text_position: Option<Point3>,
        measurement: Option<f64>,
        text: Option<&str>,
        block: Option<&str>,
        style: Option<&str>,
    ) {
        let (code, sub, default_roles): (i64, &str, &[&str]) = match ty {
            DimensionType::Linear => (0, "AcDbAlignedDimension", &["10", "13", "14"]),
            DimensionType::Aligned => (1, "AcDbAlignedDimension", &["10", "13", "14"]),
            DimensionType::Angular => (
                2,
                "AcDb2LineAngularDimension",
                &["10", "13", "14", "15", "16"],
            ),
            DimensionType::Diameter => (3, "AcDbDiametricDimension", &["10", "15"]),
            DimensionType::Radius => (4, "AcDbRadialDimension", &["10", "15"]),
            DimensionType::Angular3Point => {
                (5, "AcDb3PointAngularDimension", &["10", "13", "14", "15"])
            }
            DimensionType::Ordinate => (6, "AcDbOrdinateDimension", &["10", "13", "14"]),
            DimensionType::ArcLength | DimensionType::Other => {
                self.skip(
                    "DIMENSION",
                    "arc-length and unclassified dimensions have no writer",
                );
                return;
            }
        };
        let roles: Vec<String> = match e.props.get("dxf.dim.roles") {
            Some(MValue::List(l)) if l.len() == points.len() => l.iter().map(value_text).collect(),
            _ => default_roles.iter().map(|s| (*s).to_owned()).collect(),
        };
        let get = |role: &str| -> Option<Point3> {
            roles
                .iter()
                .position(|r| r == role)
                .and_then(|i| points.get(i))
                .copied()
        };
        let def = get("10")
            .or_else(|| points.first().copied())
            .unwrap_or_default();
        let block_name = block.and_then(|b| self.blocks.get(b).cloned());
        let flags = match prop_int(&e.props, "dxf.dim.flags") {
            Some(f) if f & 7 == code => f,
            _ => {
                code | if block_name.is_some() && !self.r12 {
                    32
                } else {
                    0
                }
            }
        };
        let dimstyle = style
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| self.dimstyles.get(s).cloned())
            .unwrap_or_else(|| {
                if self.r12 {
                    "STANDARD".to_owned()
                } else {
                    "Standard".to_owned()
                }
            });
        self.head(out, "DIMENSION", a, "AcDbDimension");
        if let Some(b) = &block_name {
            out.text(2, b);
        }
        out.point(10, def);
        out.point(11, text_position.unwrap_or(def));
        if let Some(p) = get("12") {
            out.point(12, p);
        }
        out.int(70, flags);
        if let Some(v) = prop_int(&e.props, "dxf.dim.attachment") {
            out.int(71, v);
        }
        if let Some(t) = text {
            out.text(1, t);
        }
        if let Some(v) = prop_f64(&e.props, "dxf.dim.line_spacing") {
            out.real(41, v);
        }
        out.text(3, &dimstyle);
        if let Some(m) = measurement.filter(|_| self.ge2007) {
            out.real(42, m);
        }
        if let Some(v) = prop_f64(&e.props, "dxf.dim.text_rotation") {
            out.real(53, v);
        }
        let p = |role: &str| get(role).unwrap_or(def);
        self.sub(out, sub);
        match ty {
            DimensionType::Linear | DimensionType::Aligned => {
                out.point(13, p("13"));
                out.point(14, p("14"));
                if ty == DimensionType::Linear {
                    if let Some(v) = prop_f64(&e.props, "dxf.dim.rotation") {
                        out.real(50, v);
                    }
                    self.sub(out, "AcDbRotatedDimension");
                }
            }
            DimensionType::Angular => {
                out.point(13, p("13"));
                out.point(14, p("14"));
                out.point(15, p("15"));
                out.point(16, p("16"));
            }
            DimensionType::Angular3Point => {
                out.point(13, p("13"));
                out.point(14, p("14"));
                out.point(15, p("15"));
            }
            DimensionType::Diameter | DimensionType::Radius => {
                out.point(15, p("15"));
                out.real(40, 0.0);
            }
            DimensionType::Ordinate => {
                out.point(13, p("13"));
                out.point(14, p("14"));
            }
            _ => {}
        }
    }

    fn face(&mut self, out: &mut Out, e: &Entity, a: &Attr, pts: &[Point3], filled: bool) {
        if !(3..=4).contains(&pts.len()) {
            self.skip("FACE", "needs 3 or 4 corner points");
            return;
        }
        let tri = pts.len() == 3;
        let (Some(&p1), Some(&p2), Some(&p3)) = (pts.first(), pts.get(1), pts.get(2)) else {
            return;
        };
        let p4 = if tri {
            p3
        } else {
            pts.get(3).copied().unwrap_or(p3)
        };
        let want = prop_text(&e.props, "dxf.type");
        let planar_xy = same_z(pts);
        let use_solid = filled && planar_xy && want != Some("3DFACE");
        if use_solid {
            let etype = if want == Some("TRACE") {
                "TRACE"
            } else {
                "SOLID"
            };
            self.head(out, etype, a, "AcDbTrace");
            // Corners are stored in "Z" order: 1, 2, 4, 3.
            out.point(10, p1);
            out.point(11, p2);
            out.point(12, p4);
            out.point(13, p3);
        } else {
            self.head(out, "3DFACE", a, "AcDbFace");
            out.point(10, p1);
            out.point(11, p2);
            out.point(12, p3);
            out.point(13, p4);
            if let Some(f) = prop_int(&e.props, "dxf.invisible_edges") {
                out.int(70, f);
            }
        }
    }

    fn mesh(&mut self, out: &mut Out, a: &Attr, vertices: &[Point3], faces: &[Vec<u32>]) {
        let n = vertices.len();
        let mut tris: Vec<Vec<usize>> = Vec::new();
        for f in faces {
            let idx: Vec<usize> = f.iter().map(|&i| i as usize).collect();
            if idx.len() < 3 || idx.iter().any(|&i| i >= n) {
                continue;
            }
            if idx.len() <= 4 {
                tris.push(idx);
            } else if let Some(&first) = idx.first() {
                for w in idx.get(1..).unwrap_or(&[]).windows(2) {
                    if let (Some(&b), Some(&c)) = (w.first(), w.get(1)) {
                        tris.push(vec![first, b, c]);
                    }
                }
            }
        }
        if n == 0 || tris.is_empty() {
            self.skip("MESH", "no vertices or faces");
            return;
        }
        let parent = self.head(out, "POLYLINE", a, "AcDbPolyFaceMesh");
        let child = owned_by(a, parent);
        out.int(66, 1);
        out.point(10, Point3::default());
        out.int(70, 64);
        out.int(71, n as i64);
        out.int(72, tris.len() as i64);
        for v in vertices {
            self.vertex_head(out, &child, "AcDbPolyFaceMeshVertex");
            out.point(10, *v);
            out.int(70, 192);
        }
        for t in &tris {
            self.vertex_head(out, &child, "AcDbFaceRecord");
            out.point(10, Point3::default());
            out.int(70, 128);
            for (k, i) in t.iter().enumerate() {
                out.int(71 + k as i32, *i as i64 + 1);
            }
        }
        self.seqend(out, &child);
    }

    fn attdef(&mut self, out: &mut Out, e: &Entity, a: &Attr) -> bool {
        let Some(tag) = prop_text(&e.props, "dxf.attdef.tag") else {
            return false;
        };
        let pos = prop_point(&e.props, "dxf.attdef.position").unwrap_or_default();
        self.head(out, "ATTDEF", a, "AcDbText");
        out.point(10, pos);
        out.real(40, prop_f64(&e.props, "dxf.attdef.height").unwrap_or(2.5));
        out.text(1, prop_text(&e.props, "dxf.attdef.default").unwrap_or(""));
        if let Some(s) =
            prop_text(&e.props, "dxf.attdef.style").and_then(|s| self.styles.get(s).cloned())
        {
            out.text(7, &s);
        }
        self.sub(out, "AcDbAttributeDefinition");
        out.text(3, prop_text(&e.props, "dxf.attdef.prompt").unwrap_or(""));
        out.text(2, tag);
        out.int(70, prop_int(&e.props, "dxf.attdef.flags").unwrap_or(0));
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn hatch(
        &mut self,
        out: &mut Out,
        e: &Entity,
        a: &Attr,
        loops: &[cadkit_core::HatchLoop],
        solid: bool,
        pattern: Option<&str>,
        scale: f64,
        angle: f64,
        normal: Vec3,
        group: Option<&str>,
        depth: u32,
    ) {
        if loops.iter().all(|l| l.edges.is_empty()) {
            self.skip("HATCH", "no boundary loops");
            return;
        }
        if self.r12 {
            // R12 has no HATCH: emit the boundary geometry itself.
            for l in loops {
                for edge in &l.edges {
                    let kind = match edge {
                        HatchEdge::Line { start, end } => EntityKind::Line {
                            start: *start,
                            end: *end,
                        },
                        HatchEdge::Arc {
                            center,
                            radius,
                            start_angle,
                            end_angle,
                            ccw,
                        } => {
                            let (s, en) = if *ccw {
                                (*start_angle, *end_angle)
                            } else {
                                (*end_angle, *start_angle)
                            };
                            EntityKind::Arc {
                                center: *center,
                                radius: *radius,
                                start_angle: s,
                                end_angle: en,
                                normal,
                            }
                        }
                        HatchEdge::Ellipse {
                            center,
                            major_axis,
                            ratio,
                            start_param,
                            end_param,
                            ccw,
                        } => {
                            let (s, en) = if *ccw {
                                (*start_param, *end_param)
                            } else {
                                (*end_param, *start_param)
                            };
                            EntityKind::Ellipse {
                                center: *center,
                                major_axis: *major_axis,
                                ratio: *ratio,
                                start_param: s,
                                end_param: en,
                                normal,
                            }
                        }
                        HatchEdge::Spline {
                            degree,
                            knots,
                            control_points,
                            weights,
                        } => EntityKind::Spline {
                            degree: *degree,
                            knots: knots.clone(),
                            control_points: control_points.clone(),
                            weights: weights.clone(),
                            fit_points: Vec::new(),
                            closed: false,
                        },
                        HatchEdge::Polyline { vertices, closed } => EntityKind::Polyline {
                            vertices: vertices.clone(),
                            closed: *closed,
                            normal,
                        },
                    };
                    let mut child = Entity::new(kind);
                    child.layer = e.layer.clone();
                    child.color = e.color;
                    child.linetype = e.linetype.clone();
                    child.lineweight = e.lineweight;
                    child.visible = e.visible;
                    self.write_entity(out, &child, a.owner, a.paper, group, depth + 1);
                }
            }
            return;
        }
        let ocs = Ocs::new(normal);
        let to_local = |p: Point3| ocs.to_local(p);
        // Elevation = OCS z of the first boundary point.
        let first_point = loops
            .iter()
            .flat_map(|l| l.edges.iter())
            .find_map(|e| match e {
                HatchEdge::Line { start, .. } => Some(*start),
                HatchEdge::Arc { center, .. } | HatchEdge::Ellipse { center, .. } => Some(*center),
                HatchEdge::Spline { control_points, .. } => control_points.first().copied(),
                HatchEdge::Polyline { vertices, .. } => vertices.first().map(|v| v.position),
            });
        let elevation = first_point.map_or(0.0, |p| to_local(p).z);
        self.head(out, "HATCH", a, "AcDbHatch");
        out.point(10, Point3::new(0.0, 0.0, elevation));
        out.point(210, v3(ocs.az));
        let name =
            pattern
                .filter(|p| !p.is_empty())
                .unwrap_or(if solid { "SOLID" } else { "ANSI31" });
        out.text(2, name);
        out.int(70, i64::from(solid));
        out.int(71, 0);
        let loops: Vec<&cadkit_core::HatchLoop> =
            loops.iter().filter(|l| !l.edges.is_empty()).collect();
        out.int(91, loops.len() as i64);
        for l in loops {
            let polyline = match l.edges.as_slice() {
                [HatchEdge::Polyline { vertices, closed }] => Some((vertices, *closed)),
                _ => None,
            };
            let ext_flag = i64::from(l.external);
            if let Some((vertices, closed)) = polyline {
                out.int(92, ext_flag | 2);
                let has_bulge = vertices.iter().any(|v| v.bulge != 0.0);
                out.int(72, i64::from(has_bulge));
                out.int(73, i64::from(closed));
                out.int(93, vertices.len() as i64);
                for v in vertices {
                    let p = to_local(v.position);
                    out.point2(10, p.x, p.y);
                    if has_bulge {
                        out.real(42, v.bulge);
                    }
                }
            } else {
                out.int(92, ext_flag);
                // Polyline edges nested among other edges are expanded to line segments.
                let mut count = 0usize;
                let mut body = Out::new(out.utf8);
                for edge in &l.edges {
                    match edge {
                        HatchEdge::Line { start, end } => {
                            let (s, en) = (to_local(*start), to_local(*end));
                            body.int(72, 1);
                            body.point2(10, s.x, s.y);
                            body.point2(11, en.x, en.y);
                            count += 1;
                        }
                        HatchEdge::Arc {
                            center,
                            radius,
                            start_angle,
                            end_angle,
                            ccw,
                        } => {
                            let c = to_local(*center);
                            body.int(72, 2);
                            body.point2(10, c.x, c.y);
                            body.real(40, *radius);
                            body.real(50, start_angle.to_degrees());
                            body.real(51, end_angle.to_degrees());
                            body.int(73, i64::from(*ccw));
                            count += 1;
                        }
                        HatchEdge::Ellipse {
                            center,
                            major_axis,
                            ratio,
                            start_param,
                            end_param,
                            ccw,
                        } => {
                            let c = to_local(*center);
                            let m = to_local(v3(*major_axis));
                            body.int(72, 3);
                            body.point2(10, c.x, c.y);
                            body.point2(11, m.x, m.y);
                            body.real(40, *ratio);
                            body.real(50, start_param.to_degrees());
                            body.real(51, end_param.to_degrees());
                            body.int(73, i64::from(*ccw));
                            count += 1;
                        }
                        HatchEdge::Spline {
                            degree,
                            knots,
                            control_points,
                            weights,
                        } => {
                            let d = *degree as usize;
                            if !(1..=MAX_DEGREE).contains(&d) || control_points.len() <= d {
                                self.warn(
                                    "dxf.write.skipped",
                                    "HATCH spline edge: degree out of range or too few control points"
                                        .to_owned(),
                                );
                                continue;
                            }
                            let kv: Vec<f64> = if knots.len() == control_points.len() + d + 1 {
                                knots.clone()
                            } else {
                                clamped_knots(control_points.len(), d)
                            };
                            let rational =
                                weights.len() == control_points.len() && !weights.is_empty();
                            body.int(72, 4);
                            body.int(94, i64::from(*degree));
                            body.int(73, i64::from(rational));
                            body.int(74, 0);
                            body.int(95, kv.len() as i64);
                            body.int(96, control_points.len() as i64);
                            for k in &kv {
                                body.real(40, *k);
                            }
                            for (i, p) in control_points.iter().enumerate() {
                                let p = to_local(*p);
                                body.point2(10, p.x, p.y);
                                if rational {
                                    body.real(42, weights.get(i).copied().unwrap_or(1.0));
                                }
                            }
                            body.int(97, 0);
                            count += 1;
                        }
                        HatchEdge::Polyline { vertices, closed } => {
                            let n = vertices.len();
                            let segs = if *closed { n } else { n.saturating_sub(1) };
                            for i in 0..segs {
                                let (Some(p), Some(q)) =
                                    (vertices.get(i), vertices.get((i + 1) % n))
                                else {
                                    continue;
                                };
                                let (s, en) = (to_local(p.position), to_local(q.position));
                                body.int(72, 1);
                                body.point2(10, s.x, s.y);
                                body.point2(11, en.x, en.y);
                                count += 1;
                            }
                        }
                    }
                }
                out.int(93, count as i64);
                out.buf.push_str(&body.buf);
            }
            out.int(97, 0);
        }
        out.int(75, prop_int(&e.props, "dxf.hatch.style").unwrap_or(0));
        out.int(76, 1);
        if !solid {
            out.real(52, angle.to_degrees());
            out.real(41, if scale > 0.0 { scale } else { 1.0 });
            out.int(77, 0);
            match e.props.get("dxf.hatch.pattern_lines") {
                Some(MValue::List(lines)) if !lines.is_empty() => {
                    out.int(78, lines.len() as i64);
                    for line in lines {
                        let MValue::List(row) = line else { continue };
                        let nums: Vec<f64> = row
                            .iter()
                            .map(|v| match v {
                                MValue::Float(f) => *f,
                                MValue::Int(i) => *i as f64,
                                _ => 0.0,
                            })
                            .collect();
                        out.real(53, nums.first().copied().unwrap_or(0.0));
                        for (k, code) in [43, 44, 45, 46].into_iter().enumerate() {
                            out.real(code, nums.get(k + 1).copied().unwrap_or(0.0));
                        }
                        let dashes = nums.get(5..).unwrap_or(&[]);
                        out.int(79, dashes.len() as i64);
                        for d in dashes {
                            out.real(49, *d);
                        }
                    }
                }
                _ => {
                    // Without stored pattern lines emit one generic line family at 45 degrees.
                    out.int(78, 1);
                    out.real(53, 45.0);
                    out.real(43, 0.0);
                    out.real(44, 0.0);
                    out.real(45, -2.245 * scale.max(1e-9));
                    out.real(46, 2.245 * scale.max(1e-9));
                    out.int(79, 0);
                }
            }
        }
        out.real(47, 1.0);
        out.int(98, 0);
        self.xdata(out, &e.props, group);
    }
}
