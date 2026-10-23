//! Validates resource budgets and float finiteness on in-memory documents before writing.

use crate::{
    Attribute, Document, Entity, EntityKind, Error, HatchEdge, Limits, Lineweight, Point3, Props,
    Result, Value, Vec3, Vertex,
};

/// Parses a JSON document after checking the input byte limit and validates document budgets.
pub fn document_from_json(text: &str, limits: &Limits) -> Result<Document> {
    if text.len() as u64 > limits.max_input_bytes {
        return Err(Error::LimitExceeded("document JSON input bytes".into()));
    }
    let doc = serde_json::from_str(text).map_err(|e| Error::invalid(0, e.to_string()))?;
    validate_limits(&doc, limits)?;
    Ok(doc)
}

struct Budget<'a> {
    limits: &'a Limits,
    objects: u64,
    total_vertices: u64,
    bytes: u64,
}

impl<'a> Budget<'a> {
    fn new(limits: &'a Limits) -> Self {
        Self {
            limits,
            objects: 0,
            total_vertices: 0,
            bytes: 0,
        }
    }

    fn add_objects(&mut self, n: usize, label: &'static str) -> Result<()> {
        self.objects = self.objects.saturating_add(n as u64);
        if self.objects > self.limits.max_objects {
            return Err(Error::LimitExceeded(label.into()));
        }
        Ok(())
    }

    fn add_bytes(&mut self, n: u64) -> Result<()> {
        self.bytes = self.bytes.saturating_add(n);
        if self.bytes > self.limits.max_decompressed_bytes {
            return Err(Error::LimitExceeded("export byte budget".into()));
        }
        Ok(())
    }

    fn check_str(&mut self, s: &str) -> Result<()> {
        if s.len() as u64 > u64::from(self.limits.max_string_bytes) {
            return Err(Error::LimitExceeded("export string".into()));
        }
        self.add_bytes(s.len() as u64)
    }

    fn check_opt_str(&mut self, s: &Option<String>) -> Result<()> {
        if let Some(s) = s {
            self.check_str(s)?;
        }
        Ok(())
    }

    fn check_f64(v: f64, label: &'static str) -> Result<()> {
        if !v.is_finite() {
            return Err(Error::invalid(0, format!("non-finite {label}")));
        }
        Ok(())
    }

    fn check_point(p: Point3, label: &'static str) -> Result<()> {
        if !p.is_finite() {
            return Err(Error::invalid(0, format!("non-finite {label}")));
        }
        Ok(())
    }

    fn check_vec(v: Vec3, label: &'static str) -> Result<()> {
        if !(v.x.is_finite() && v.y.is_finite() && v.z.is_finite()) {
            return Err(Error::invalid(0, format!("non-finite {label}")));
        }
        Ok(())
    }

    fn check_lineweight(lw: Lineweight) -> Result<()> {
        if let Lineweight::Millimeters(mm) = lw {
            Self::check_f64(mm, "lineweight")?;
        }
        Ok(())
    }

    fn check_vertex(v: &Vertex) -> Result<()> {
        Self::check_point(v.position, "vertex position")?;
        Self::check_f64(v.bulge, "vertex bulge")?;
        Self::check_f64(v.start_width, "vertex width")?;
        Self::check_f64(v.end_width, "vertex width")
    }

    fn check_props(&mut self, props: &Props) -> Result<()> {
        self.add_bytes((props.len() as u64).saturating_mul(16))?;
        for (k, v) in props {
            self.check_str(k)?;
            self.check_value(v)?;
        }
        Ok(())
    }

    fn check_value(&mut self, root: &Value) -> Result<()> {
        let max_depth = self.limits.max_depth.min(256);
        let mut todo = vec![(root, 0u32)];
        let mut nodes = 1u64;
        while let Some((v, d)) = todo.pop() {
            if d >= max_depth {
                return Err(Error::LimitExceeded("export property depth".into()));
            }
            self.add_bytes(8)?;
            match v {
                Value::Float(value) => Self::check_f64(*value, "metadata value")?,
                Value::List(values) => {
                    nodes = nodes.saturating_add(values.len() as u64);
                    if nodes > self.limits.max_objects {
                        return Err(Error::LimitExceeded("export property objects".into()));
                    }
                    todo.extend(values.iter().map(|v| (v, d + 1)));
                }
                Value::Text(s) => self.check_str(s)?,
                Value::Bytes(b) => self.add_bytes(b.len() as u64)?,
                Value::Null | Value::Bool(_) | Value::Int(_) => {}
            }
        }
        Ok(())
    }

    fn check_attribute(&mut self, a: &Attribute) -> Result<()> {
        self.add_bytes(32)?;
        self.check_str(&a.tag)?;
        self.check_opt_str(&a.set)?;
        self.check_opt_str(&a.layer)?;
        if let Some(pos) = a.position {
            Self::check_point(pos, "attribute position")?;
        }
        if let Some(disp) = &a.display {
            Self::check_f64(disp.height, "attribute height")?;
            Self::check_f64(disp.width, "attribute width")?;
            Self::check_f64(disp.rotation, "attribute rotation")?;
            self.check_opt_str(&disp.style)?;
        }
        self.check_value(&a.value)?;
        self.check_props(&a.props)
    }

    fn check_spline_arrays(
        &mut self,
        knots: &[f64],
        control_points: &[Point3],
        weights: &[f64],
        fit_points: &[Point3],
    ) -> Result<u64> {
        let max_v = u64::from(self.limits.max_vertices);
        let max_knots = max_v.saturating_mul(2).saturating_add(32);
        if knots.len() as u64 > max_knots || weights.len() as u64 > max_v {
            return Err(Error::LimitExceeded("export entity vertices".into()));
        }
        for &k in knots {
            Self::check_f64(k, "spline knot")?;
        }
        for &w in weights {
            Self::check_f64(w, "spline weight")?;
        }
        for &p in control_points {
            Self::check_point(p, "spline control point")?;
        }
        for &p in fit_points {
            Self::check_point(p, "spline fit point")?;
        }
        let n = (control_points.len() as u64)
            .saturating_add(fit_points.len() as u64)
            .max(1);
        self.add_bytes(
            (knots.len().saturating_add(weights.len()) as u64)
                .saturating_mul(8)
                .saturating_add(n.saturating_mul(24)),
        )?;
        Ok(n)
    }

    fn check_hatch_edge(&mut self, edge: &HatchEdge) -> Result<u64> {
        match edge {
            HatchEdge::Line { start, end } => {
                Self::check_point(*start, "hatch line")?;
                Self::check_point(*end, "hatch line")?;
                self.add_bytes(48)?;
                Ok(2)
            }
            HatchEdge::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                ..
            } => {
                Self::check_point(*center, "hatch arc")?;
                Self::check_f64(*radius, "hatch arc")?;
                Self::check_f64(*start_angle, "hatch arc")?;
                Self::check_f64(*end_angle, "hatch arc")?;
                self.add_bytes(56)?;
                Ok(1)
            }
            HatchEdge::Ellipse {
                center,
                major_axis,
                ratio,
                start_param,
                end_param,
                ..
            } => {
                Self::check_point(*center, "hatch ellipse")?;
                Self::check_vec(*major_axis, "hatch ellipse")?;
                Self::check_f64(*ratio, "hatch ellipse")?;
                Self::check_f64(*start_param, "hatch ellipse")?;
                Self::check_f64(*end_param, "hatch ellipse")?;
                self.add_bytes(72)?;
                Ok(1)
            }
            HatchEdge::Spline {
                knots,
                control_points,
                weights,
                ..
            } => self.check_spline_arrays(knots, control_points, weights, &[]),
            HatchEdge::Polyline { vertices, .. } => {
                for v in vertices {
                    Self::check_vertex(v)?;
                }
                self.add_bytes((vertices.len() as u64).saturating_mul(48))?;
                Ok(vertices.len() as u64)
            }
        }
    }

    fn check_entity<'e>(
        &mut self,
        e: &'e Entity,
        depth: u32,
        todo: &mut Vec<(&'e Entity, u32)>,
    ) -> Result<()> {
        if depth >= self.limits.max_depth.min(256) {
            return Err(Error::LimitExceeded("export depth".into()));
        }
        self.add_bytes(64)?;
        self.check_opt_str(&e.layer)?;
        self.check_opt_str(&e.linetype)?;
        Self::check_lineweight(e.lineweight)?;
        if let Some(raw) = &e.raw {
            self.add_bytes(raw.bytes.len() as u64)?;
        }

        let n = match &e.kind {
            EntityKind::Point { position } => {
                Self::check_point(*position, "point")?;
                1
            }
            EntityKind::Line { start, end } => {
                Self::check_point(*start, "line")?;
                Self::check_point(*end, "line")?;
                2
            }
            EntityKind::Polyline {
                vertices, normal, ..
            } => {
                Self::check_vec(*normal, "polyline normal")?;
                for v in vertices {
                    Self::check_vertex(v)?;
                }
                self.add_bytes((vertices.len() as u64).saturating_mul(48))?;
                vertices.len() as u64
            }
            EntityKind::Circle {
                center,
                radius,
                normal,
            } => {
                Self::check_point(*center, "circle")?;
                Self::check_f64(*radius, "circle radius")?;
                Self::check_vec(*normal, "circle normal")?;
                1
            }
            EntityKind::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                normal,
            } => {
                Self::check_point(*center, "arc")?;
                Self::check_f64(*radius, "arc radius")?;
                Self::check_f64(*start_angle, "arc angle")?;
                Self::check_f64(*end_angle, "arc angle")?;
                Self::check_vec(*normal, "arc normal")?;
                1
            }
            EntityKind::Ellipse {
                center,
                major_axis,
                ratio,
                start_param,
                end_param,
                normal,
            } => {
                Self::check_point(*center, "ellipse")?;
                Self::check_vec(*major_axis, "ellipse axis")?;
                Self::check_f64(*ratio, "ellipse ratio")?;
                Self::check_f64(*start_param, "ellipse param")?;
                Self::check_f64(*end_param, "ellipse param")?;
                Self::check_vec(*normal, "ellipse normal")?;
                1
            }
            EntityKind::Spline {
                knots,
                control_points,
                weights,
                fit_points,
                ..
            } => self.check_spline_arrays(knots, control_points, weights, fit_points)?,
            EntityKind::Text {
                position,
                end_point,
                height,
                rotation,
                width_factor,
                oblique,
                value,
                style,
                normal,
                ..
            } => {
                Self::check_point(*position, "text position")?;
                if let Some(ep) = end_point {
                    Self::check_point(*ep, "text end point")?;
                }
                Self::check_f64(*height, "text height")?;
                Self::check_f64(*rotation, "text rotation")?;
                Self::check_f64(*width_factor, "text width factor")?;
                Self::check_f64(*oblique, "text oblique")?;
                Self::check_vec(*normal, "text normal")?;
                self.check_str(value)?;
                self.check_opt_str(style)?;
                1
            }
            EntityKind::MText {
                position,
                height,
                width,
                rotation,
                value,
                plain,
                style,
                line_spacing,
                normal,
                ..
            } => {
                Self::check_point(*position, "mtext position")?;
                Self::check_f64(*height, "mtext height")?;
                Self::check_f64(*width, "mtext width")?;
                Self::check_f64(*rotation, "mtext rotation")?;
                Self::check_f64(*line_spacing, "mtext line spacing")?;
                Self::check_vec(*normal, "mtext normal")?;
                self.check_str(value)?;
                self.check_str(plain)?;
                self.check_opt_str(style)?;
                1
            }
            EntityKind::Insert {
                block,
                position,
                scale,
                rotation,
                normal,
                column_spacing,
                row_spacing,
                ..
            } => {
                self.check_str(block)?;
                Self::check_point(*position, "insert position")?;
                Self::check_vec(*scale, "insert scale")?;
                Self::check_f64(*rotation, "insert rotation")?;
                Self::check_vec(*normal, "insert normal")?;
                Self::check_f64(*column_spacing, "insert spacing")?;
                Self::check_f64(*row_spacing, "insert spacing")?;
                1
            }
            EntityKind::Hatch {
                loops,
                pattern,
                pattern_scale,
                pattern_angle,
                normal,
                ..
            } => {
                if loops.len() as u64 > self.limits.max_objects {
                    return Err(Error::LimitExceeded("export hatch loops".into()));
                }
                self.check_opt_str(pattern)?;
                Self::check_f64(*pattern_scale, "hatch pattern scale")?;
                Self::check_f64(*pattern_angle, "hatch pattern angle")?;
                Self::check_vec(*normal, "hatch normal")?;
                let mut total = 0u64;
                for l in loops {
                    if l.edges.len() as u64 > self.limits.max_objects {
                        return Err(Error::LimitExceeded("export hatch edges".into()));
                    }
                    for edge in &l.edges {
                        total = total.saturating_add(self.check_hatch_edge(edge)?);
                    }
                }
                total
            }
            EntityKind::Dimension {
                points,
                text_position,
                measurement,
                text,
                block,
                style,
                ..
            } => {
                for &p in points {
                    Self::check_point(p, "dimension point")?;
                }
                if let Some(tp) = text_position {
                    Self::check_point(*tp, "dimension text position")?;
                }
                if let Some(m) = measurement {
                    Self::check_f64(*m, "dimension measurement")?;
                }
                self.check_opt_str(text)?;
                self.check_opt_str(block)?;
                self.check_opt_str(style)?;
                self.add_bytes((points.len() as u64).saturating_mul(24))?;
                (points.len() as u64).saturating_add(u64::from(text_position.is_some()))
            }
            EntityKind::Face { points, .. } => {
                for &p in points {
                    Self::check_point(p, "face point")?;
                }
                self.add_bytes((points.len() as u64).saturating_mul(24))?;
                points.len() as u64
            }
            EntityKind::Polygon {
                exterior,
                interiors,
            } => {
                if interiors.len() as u64 > self.limits.max_objects {
                    return Err(Error::LimitExceeded("export rings".into()));
                }
                for &p in exterior {
                    Self::check_point(p, "polygon point")?;
                }
                let mut total = exterior.len() as u64;
                for ring in interiors {
                    for &p in ring {
                        Self::check_point(p, "polygon point")?;
                    }
                    total = total.saturating_add(ring.len() as u64);
                }
                self.add_bytes(total.saturating_mul(24))?;
                total
            }
            EntityKind::Leader { vertices, .. } => {
                for &p in vertices {
                    Self::check_point(p, "leader vertex")?;
                }
                self.add_bytes((vertices.len() as u64).saturating_mul(24))?;
                vertices.len() as u64
            }
            EntityKind::Image {
                path,
                position,
                u_vector,
                v_vector,
                ..
            } => {
                self.check_opt_str(path)?;
                Self::check_point(*position, "image position")?;
                Self::check_vec(*u_vector, "image vector")?;
                Self::check_vec(*v_vector, "image vector")?;
                1
            }
            EntityKind::Viewport {
                center,
                width,
                height,
                view_center,
                view_height,
            } => {
                Self::check_point(*center, "viewport center")?;
                Self::check_f64(*width, "viewport width")?;
                Self::check_f64(*height, "viewport height")?;
                Self::check_point(*view_center, "viewport view center")?;
                Self::check_f64(*view_height, "viewport view height")?;
                1
            }
            EntityKind::Mesh { vertices, faces } => {
                if faces.len() as u64 > self.limits.max_objects {
                    return Err(Error::LimitExceeded("export faces".into()));
                }
                for &p in vertices {
                    Self::check_point(p, "mesh vertex")?;
                }
                let mut total = vertices.len() as u64;
                let mut indices = 0u64;
                for f in faces {
                    let fl = f.len() as u64;
                    total = total.saturating_add(fl);
                    indices = indices.saturating_add(fl);
                }
                self.add_bytes(
                    (vertices.len() as u64)
                        .saturating_mul(24)
                        .saturating_add(indices.saturating_mul(4)),
                )?;
                total
            }
            EntityKind::Group {
                name,
                origin,
                children,
                ..
            } => {
                self.check_opt_str(name)?;
                if let Some(o) = origin {
                    Self::check_point(*o, "group origin")?;
                }
                self.add_objects(children.len(), "export objects")?;
                todo.extend(children.iter().map(|c| (c, depth + 1)));
                0
            }
            EntityKind::Unknown { type_name } => {
                self.check_str(type_name)?;
                1
            }
        };

        if n > u64::from(self.limits.max_vertices) {
            return Err(Error::LimitExceeded("export entity vertices".into()));
        }
        self.total_vertices = self
            .total_vertices
            .saturating_add(n)
            .saturating_add(e.attributes.len() as u64);
        if self.total_vertices > self.limits.max_total_vertices {
            return Err(Error::LimitExceeded("export total vertices".into()));
        }

        for a in &e.attributes {
            self.check_attribute(a)?;
        }
        self.check_props(&e.props)
    }
}

/// Enforces depth, object, geometry, string, finite-float, and memory size limits directly on the
/// in-memory AST without serializing or cloning the document.
pub fn validate_limits(doc: &Document, limits: &Limits) -> Result<()> {
    let mut b = Budget::new(limits);
    b.add_bytes(128)?;
    b.check_str(&doc.source.version)?;
    b.check_opt_str(&doc.source.application)?;
    b.check_opt_str(&doc.source.codepage)?;
    if let Some(mpu) = doc.units.meters_per_unit {
        Budget::check_f64(mpu, "meters_per_unit")?;
    }

    b.add_objects(doc.models.len(), "export objects")?;
    b.add_objects(doc.blocks.len(), "export objects")?;
    if doc.layers.len() as u64 > limits.max_objects
        || doc.linetypes.len() as u64 > limits.max_objects
        || doc.text_styles.len() as u64 > limits.max_objects
        || doc.warnings.len() as u64 > limits.max_objects
    {
        return Err(Error::LimitExceeded("export table entries".into()));
    }

    for layer in &doc.layers {
        b.add_bytes(48)?;
        b.check_str(&layer.name)?;
        b.check_opt_str(&layer.linetype)?;
        b.check_opt_str(&layer.description)?;
        Budget::check_lineweight(layer.lineweight)?;
        b.check_props(&layer.props)?;
    }

    for lt in &doc.linetypes {
        b.add_bytes(32)?;
        b.check_str(&lt.name)?;
        b.check_opt_str(&lt.description)?;
        if lt.pattern.len() as u64 > u64::from(limits.max_vertices) {
            return Err(Error::LimitExceeded("export linetype pattern".into()));
        }
        for &seg in &lt.pattern {
            Budget::check_f64(seg, "linetype segment")?;
        }
        b.add_bytes((lt.pattern.len() as u64).saturating_mul(8))?;
        b.check_props(&lt.props)?;
    }

    for ts in &doc.text_styles {
        b.add_bytes(48)?;
        b.check_str(&ts.name)?;
        b.check_opt_str(&ts.font)?;
        Budget::check_f64(ts.height, "text style height")?;
        Budget::check_f64(ts.width_factor, "text style width factor")?;
        Budget::check_f64(ts.oblique, "text style oblique")?;
        b.check_props(&ts.props)?;
    }

    for w in &doc.warnings {
        b.add_bytes(32)?;
        b.check_str(&w.code)?;
        b.check_str(&w.message)?;
    }

    b.check_props(&doc.props)?;

    let mut todo = Vec::new();
    for m in &doc.models {
        b.add_bytes(48)?;
        b.check_str(&m.name)?;
        b.check_props(&m.props)?;
        b.add_objects(m.entities.len(), "export objects")?;
        todo.extend(m.entities.iter().map(|e| (e, 0u32)));
    }
    for blk in &doc.blocks {
        b.add_bytes(64)?;
        b.check_str(&blk.name)?;
        b.check_opt_str(&blk.xref_path)?;
        b.check_opt_str(&blk.description)?;
        Budget::check_point(blk.base_point, "block base point")?;
        b.check_props(&blk.props)?;
        b.add_objects(blk.entities.len(), "export objects")?;
        todo.extend(blk.entities.iter().map(|e| (e, 0u32)));
    }

    while let Some((e, depth)) = todo.pop() {
        b.check_entity(e, depth, &mut todo)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AttributeDisplay, Color, HAlign, HatchLoop, Model, VAlign};

    #[test]
    fn spline_hatch_leader_dimension_vertices_and_byte_budget_yield_limit_exceeded() {
        let spline = Entity::new(EntityKind::Spline {
            degree: 3,
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0, 2.0],
            control_points: vec![Point3::xy(0.0, 0.0); 5],
            weights: Vec::new(),
            fit_points: Vec::new(),
            closed: false,
        });
        let mut doc = Document::default();
        doc.models.push(Model {
            name: "Model".into(),
            entities: vec![spline],
            ..Model::default()
        });
        let limits = Limits {
            max_vertices: 4,
            ..Limits::default()
        };
        assert!(matches!(
            validate_limits(&doc, &limits),
            Err(Error::LimitExceeded(_))
        ));

        let hatch = Entity::new(EntityKind::Hatch {
            loops: vec![HatchLoop {
                edges: vec![HatchEdge::Polyline {
                    vertices: vec![Vertex::at(Point3::xy(0.0, 0.0)); 6],
                    closed: true,
                }],
                external: true,
            }],
            solid: true,
            pattern: None,
            pattern_scale: 1.0,
            pattern_angle: 0.0,
            normal: Vec3::Z,
        });
        doc.models[0].entities = vec![hatch];
        assert!(matches!(
            validate_limits(&doc, &limits),
            Err(Error::LimitExceeded(_))
        ));

        doc.models[0].entities = vec![Entity::new(EntityKind::Leader {
            vertices: vec![Point3::xy(0.0, 0.0); 5],
            arrowhead: true,
        })];
        assert!(matches!(
            validate_limits(&doc, &limits),
            Err(Error::LimitExceeded(_))
        ));

        let byte_limits = Limits {
            max_decompressed_bytes: 64,
            ..Limits::default()
        };
        assert!(matches!(
            validate_limits(&doc, &byte_limits),
            Err(Error::LimitExceeded(msg)) if msg == "export byte budget"
        ));
    }

    #[test]
    fn rejects_non_finite_floats_everywhere() {
        let mut doc = Document::default();
        doc.models.push(Model {
            name: "Model".into(),
            entities: vec![Entity::new(EntityKind::Line {
                start: Point3::new(f64::NAN, 0.0, 0.0),
                end: Point3::xy(1.0, 1.0),
            })],
            ..Model::default()
        });
        assert!(matches!(
            validate_limits(&doc, &Limits::default()),
            Err(Error::Invalid { .. })
        ));

        doc.models[0].entities = vec![Entity::new(EntityKind::Dimension {
            dimension_type: crate::DimensionType::Linear,
            points: vec![Point3::xy(0.0, 0.0)],
            text_position: None,
            measurement: Some(f64::INFINITY),
            text: None,
            block: None,
            style: None,
        })];
        assert!(matches!(
            validate_limits(&doc, &Limits::default()),
            Err(Error::Invalid { .. })
        ));
    }

    #[test]
    fn v036_serde_json_wire_format_is_byte_identical() {
        // Golden v0.3.6 wire compatibility check: Attribute (with & without optional display/layer),
        // Value (Int before Float), Color, Lineweight, HAlign, and VAlign must serialize identically.
        let basic_attr = Attribute {
            tag: "ROOM_NO".into(),
            value: Value::Int(101),
            set: Some("ROOM".into()),
            position: Some(Point3::new(1.5, 2.5, 0.0)),
            invisible: false,
            layer: None,
            display: None,
            props: Props::new(),
        };
        assert_eq!(
            serde_json::to_string(&basic_attr).unwrap(),
            r#"{"tag":"ROOM_NO","value":101,"set":"ROOM","position":{"x":1.5,"y":2.5,"z":0.0},"invisible":false}"#
        );

        let rich_attr = Attribute {
            layer: Some("TAGS".into()),
            display: Some(AttributeDisplay {
                height: 2.5,
                width: 2.5,
                style: Some("ROMANS".into()),
                halign: HAlign::Center,
                valign: VAlign::Middle,
                rotation: 0.0,
            }),
            ..basic_attr
        };
        let rich_json = serde_json::to_string(&rich_attr).unwrap();
        assert_eq!(
            rich_json,
            r#"{"tag":"ROOM_NO","value":101,"set":"ROOM","position":{"x":1.5,"y":2.5,"z":0.0},"invisible":false,"layer":"TAGS","display":{"height":2.5,"width":2.5,"style":"ROMANS","halign":"center","valign":"middle","rotation":0.0}}"#
        );
        let round: Attribute = serde_json::from_str(&rich_json).unwrap();
        assert_eq!(round, rich_attr);

        // Untagged Value deserializes integers as Value::Int rather than Value::Float.
        let parsed_val: Value = serde_json::from_str("42").unwrap();
        assert_eq!(parsed_val, Value::Int(42));
        assert_eq!(
            serde_json::to_string(&Color::Aci { index: 3 }).unwrap(),
            r#"{"type":"aci","index":3}"#
        );
        assert_eq!(
            serde_json::to_string(&Lineweight::Millimeters(0.25)).unwrap(),
            r#"{"type":"millimeters","value":0.25}"#
        );
    }
}
