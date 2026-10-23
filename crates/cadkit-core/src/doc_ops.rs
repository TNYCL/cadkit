//! High-level in-place [`Document`] and [`Entity`] operations: affine transforms, unit
//! conversion, insert explosion, group flattening, curve tessellation, layer filtering, and
//! unused-definition purging.

use std::collections::{HashMap, HashSet};
use std::f64::consts::TAU;

use crate::error::{Error, Result};
use crate::geom::{Point3, Vec3};
use crate::geom_ops::{
    EllipseArc, Transform, arbitrary_axes, conjugate_to_axes, interpolate_fit_points, nurbs_flatten,
};
use crate::model::{
    Attribute, Block, Color, Document, Entity, EntityKind, HatchEdge, HatchLoop, LengthUnit,
    Lineweight, ModelKind, Units, Vertex,
};
use crate::ops::{
    DEFAULT_MAX_WORK, SPLINE_SAMPLES, WalkStats, circle_arc, full_ellipse_arc, segment_arc,
};

/// Counts of unreferenced definitions removed by [`Document::purge_unused`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PurgeStats {
    /// Unused [`Block`] definitions removed.
    pub blocks: usize,
    /// Unused [`crate::Layer`] definitions removed.
    pub layers: usize,
    /// Unused [`crate::Linetype`] definitions removed.
    pub linetypes: usize,
    /// Unused [`crate::TextStyle`] definitions removed.
    pub text_styles: usize,
}

/// Maps a transformed [`EllipseArc`] back into [`EntityKind::Circle`], [`EntityKind::Arc`], or
/// [`EntityKind::Ellipse`] depending on whether the transformed axes stay circular.
fn arc_or_ellipse_kind(ea: &EllipseArc, fallback_normal: Vec3, full_loop: bool) -> EntityKind {
    let raw_cross = ea.u.cross(ea.v);
    let mut new_normal = raw_cross
        .normalized()
        .or_else(|| fallback_normal.normalized())
        .unwrap_or(Vec3::Z);
    let (mut n_ax, mut n_ay, _) = arbitrary_axes(new_normal);
    let u2_check = (ea.u.dot(n_ax), ea.u.dot(n_ay));
    let v2_check = (ea.v.dot(n_ax), ea.v.dot(n_ay));
    let det = u2_check.0 * v2_check.1 - u2_check.1 * v2_check.0;
    if det < 0.0 && ea.sweep() >= 0.0 || det > 0.0 && ea.sweep() < 0.0 {
        new_normal = -new_normal;
        (n_ax, n_ay, _) = arbitrary_axes(new_normal);
    }

    let u2 = (ea.u.dot(n_ax), ea.u.dot(n_ay));
    let v2 = (ea.v.dot(n_ax), ea.v.dot(n_ay));
    let ru = u2.0.hypot(u2.1);
    let rv = v2.0.hypot(v2.1);
    let ortho = u2.0 * v2.0 + u2.1 * v2.1;
    let scale_ref = ru.max(rv).max(1.0);
    let is_circular =
        (ru - rv).abs() <= 1e-9 * scale_ref && ortho.abs() <= 1e-9 * scale_ref * scale_ref;

    if is_circular {
        let radius = 0.5 * (ru + rv);
        if full_loop {
            return EntityKind::Circle {
                center: ea.center,
                radius,
                normal: new_normal,
            };
        }
        let p0 = ea.point(ea.t0) - ea.center;
        let p1 = ea.point(ea.t1) - ea.center;
        let start_angle = p0.dot(n_ay).atan2(p0.dot(n_ax)).rem_euclid(TAU);
        let end_angle = p1.dot(n_ay).atan2(p1.dot(n_ax)).rem_euclid(TAU);
        return EntityKind::Arc {
            center: ea.center,
            radius,
            start_angle,
            end_angle,
            normal: new_normal,
        };
    }

    let (semi_major, semi_minor, rot) = conjugate_to_axes(u2, v2);
    let major_dir = n_ax * rot.cos() + n_ay * rot.sin();
    let major_axis = major_dir * semi_major;
    let ratio = if semi_major > 1e-12 {
        (semi_minor / semi_major).clamp(1e-9, 1.0)
    } else {
        1.0
    };
    if full_loop {
        return EntityKind::Ellipse {
            center: ea.center,
            major_axis,
            ratio,
            start_param: 0.0,
            end_param: TAU,
            normal: new_normal,
        };
    }
    let minor_dir = new_normal.cross(major_dir).normalized().unwrap_or(Vec3::Y);
    let param_of = |p: Point3| -> f64 {
        let d = p - ea.center;
        let x = d.dot(major_dir) / semi_major.max(1e-12);
        let y = d.dot(minor_dir) / semi_minor.max(1e-12);
        y.atan2(x).rem_euclid(TAU)
    };
    EntityKind::Ellipse {
        center: ea.center,
        major_axis,
        ratio,
        start_param: param_of(ea.point(ea.t0)),
        end_param: param_of(ea.point(ea.t1)),
        normal: new_normal,
    }
}

fn transform_plane_frame(normal: Vec3, rotation: f64, tf: &Transform) -> (Vec3, f64, f64, f64) {
    let (ax, ay, az) = arbitrary_axes(normal);
    let (s, c) = rotation.sin_cos();
    let dir_x = tf.apply_vector(ax * c + ay * s);
    let dir_y = tf.apply_vector(ay * c - ax * s);
    let mut new_normal = dir_x
        .cross(dir_y)
        .normalized()
        .or_else(|| tf.apply_vector(az).normalized())
        .unwrap_or(Vec3::Z);
    if dir_x.cross(dir_y).dot(new_normal) < 0.0 {
        new_normal = -new_normal;
    }
    let (n_ax, n_ay, _) = arbitrary_axes(new_normal);
    let new_rot = dir_x.dot(n_ay).atan2(dir_x.dot(n_ax));
    let sx = dir_x.length();
    let sy = dir_y.length();
    (new_normal, new_rot, sx, sy)
}

fn transform_hatch_edge(
    edge: &HatchEdge,
    old_normal: Vec3,
    new_normal: Vec3,
    tf: &Transform,
) -> HatchEdge {
    let (old_ax, old_ay, _) = arbitrary_axes(old_normal);
    let tx = tf.apply_vector(old_ax);
    let ty = tf.apply_vector(old_ay);
    let flipped = tx.cross(ty).dot(new_normal) < 0.0;
    let width_scale = 0.5 * (tx.length() + ty.length());

    match edge {
        HatchEdge::Line { start, end } => HatchEdge::Line {
            start: tf.apply_point(*start),
            end: tf.apply_point(*end),
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
            let ea = circle_arc(*center, *radius, s, en, old_normal, tf);
            match arc_or_ellipse_kind(
                &ea,
                new_normal,
                (*end_angle - *start_angle).abs() >= TAU - 1e-9,
            ) {
                EntityKind::Circle { center, radius, .. } => HatchEdge::Arc {
                    center,
                    radius,
                    start_angle: 0.0,
                    end_angle: TAU,
                    ccw: !flipped ^ !*ccw,
                },
                EntityKind::Arc {
                    center,
                    radius,
                    start_angle: ns,
                    end_angle: ne,
                    ..
                } => {
                    let out_ccw = *ccw ^ flipped;
                    let (a0, a1) = if out_ccw { (ns, ne) } else { (ne, ns) };
                    HatchEdge::Arc {
                        center,
                        radius,
                        start_angle: a0,
                        end_angle: a1,
                        ccw: out_ccw,
                    }
                }
                EntityKind::Ellipse {
                    center,
                    major_axis,
                    ratio,
                    start_param,
                    end_param,
                    ..
                } => {
                    let out_ccw = *ccw ^ flipped;
                    let (p0, p1) = if out_ccw {
                        (start_param, end_param)
                    } else {
                        (end_param, start_param)
                    };
                    HatchEdge::Ellipse {
                        center,
                        major_axis,
                        ratio,
                        start_param: p0,
                        end_param: p1,
                        ccw: out_ccw,
                    }
                }
                _ => edge.clone(),
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
            let ea = full_ellipse_arc(*center, *major_axis, *ratio, old_normal, s, en, tf);
            let full = (*end_param - *start_param).abs() >= TAU - 1e-9;
            match arc_or_ellipse_kind(&ea, new_normal, full) {
                EntityKind::Circle { center, radius, .. } => HatchEdge::Ellipse {
                    center,
                    major_axis: arbitrary_axes(new_normal).0 * radius,
                    ratio: 1.0,
                    start_param: 0.0,
                    end_param: TAU,
                    ccw: *ccw ^ flipped,
                },
                EntityKind::Ellipse {
                    center,
                    major_axis,
                    ratio,
                    start_param: ns,
                    end_param: ne,
                    ..
                } => {
                    let out_ccw = *ccw ^ flipped;
                    let (p0, p1) = if full {
                        (0.0, TAU)
                    } else if out_ccw {
                        (ns, ne)
                    } else {
                        (ne, ns)
                    };
                    HatchEdge::Ellipse {
                        center,
                        major_axis,
                        ratio,
                        start_param: p0,
                        end_param: p1,
                        ccw: out_ccw,
                    }
                }
                _ => edge.clone(),
            }
        }
        HatchEdge::Spline {
            degree,
            knots,
            control_points,
            weights,
        } => HatchEdge::Spline {
            degree: *degree,
            knots: knots.clone(),
            control_points: control_points.iter().map(|p| tf.apply_point(*p)).collect(),
            weights: weights.clone(),
        },
        HatchEdge::Polyline { vertices, closed } => HatchEdge::Polyline {
            vertices: vertices
                .iter()
                .map(|v| Vertex {
                    position: tf.apply_point(v.position),
                    bulge: if flipped { -v.bulge } else { v.bulge },
                    start_width: v.start_width * width_scale,
                    end_width: v.end_width * width_scale,
                })
                .collect(),
            closed: *closed,
        },
    }
}

fn transform_attribute(attr: &Attribute, tf: &Transform) -> Attribute {
    let mut out = attr.clone();
    if let Some(pos) = out.position {
        out.position = Some(tf.apply_point(pos));
    }
    if let Some(disp) = &mut out.display {
        let (_, new_rot, sx, sy) = transform_plane_frame(Vec3::Z, disp.rotation, tf);
        let h_scale = if sy > 0.0 && sy.is_finite() { sy } else { 1.0 };
        let w_scale = if sx > 0.0 && sx.is_finite() {
            sx
        } else {
            h_scale
        };
        disp.height *= h_scale;
        disp.width *= w_scale;
        disp.rotation = new_rot;
    }
    out
}

impl Entity {
    /// Returns a copy of this entity with its geometry and displayed attributes mapped by `tf`.
    ///
    /// Circles and circular arcs that undergo anisotropic scaling are promoted to exact
    /// [`EntityKind::Ellipse`] entities.
    pub fn transformed(&self, tf: &Transform) -> Entity {
        let kind = match &self.kind {
            EntityKind::Point { position } => EntityKind::Point {
                position: tf.apply_point(*position),
            },
            EntityKind::Line { start, end } => EntityKind::Line {
                start: tf.apply_point(*start),
                end: tf.apply_point(*end),
            },
            EntityKind::Polyline {
                vertices,
                closed,
                normal,
            } => {
                let (new_normal, _, sx, sy) = transform_plane_frame(*normal, 0.0, tf);
                let (old_ax, old_ay, _) = arbitrary_axes(*normal);
                let flipped = tf
                    .apply_vector(old_ax)
                    .cross(tf.apply_vector(old_ay))
                    .dot(new_normal)
                    < 0.0;
                let w_scale = 0.5 * (sx + sy);
                EntityKind::Polyline {
                    vertices: vertices
                        .iter()
                        .map(|v| Vertex {
                            position: tf.apply_point(v.position),
                            bulge: if flipped { -v.bulge } else { v.bulge },
                            start_width: v.start_width * w_scale,
                            end_width: v.end_width * w_scale,
                        })
                        .collect(),
                    closed: *closed,
                    normal: new_normal,
                }
            }
            EntityKind::Circle {
                center,
                radius,
                normal,
            } => {
                let ea = circle_arc(*center, *radius, 0.0, 0.0, *normal, tf);
                arc_or_ellipse_kind(&ea, tf.apply_vector(*normal), true)
            }
            EntityKind::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                normal,
            } => {
                let ea = circle_arc(*center, *radius, *start_angle, *end_angle, *normal, tf);
                let full = (*end_angle - *start_angle).abs() >= TAU - 1e-9;
                arc_or_ellipse_kind(&ea, tf.apply_vector(*normal), full)
            }
            EntityKind::Ellipse {
                center,
                major_axis,
                ratio,
                start_param,
                end_param,
                normal,
            } => {
                let ea = full_ellipse_arc(
                    *center,
                    *major_axis,
                    *ratio,
                    *normal,
                    *start_param,
                    *end_param,
                    tf,
                );
                let full = (*end_param - *start_param).abs() >= TAU - 1e-9;
                arc_or_ellipse_kind(&ea, tf.apply_vector(*normal), full)
            }
            EntityKind::Spline {
                degree,
                knots,
                control_points,
                weights,
                fit_points,
                closed,
            } => EntityKind::Spline {
                degree: *degree,
                knots: knots.clone(),
                control_points: control_points.iter().map(|p| tf.apply_point(*p)).collect(),
                weights: weights.clone(),
                fit_points: fit_points.iter().map(|p| tf.apply_point(*p)).collect(),
                closed: *closed,
            },
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
                let (new_normal, new_rot, sx, sy) = transform_plane_frame(*normal, *rotation, tf);
                let h_scale = if sy > 0.0 && sy.is_finite() { sy } else { 1.0 };
                let wf_scale = if sy > 1e-12 && sx > 0.0 && sx.is_finite() {
                    sx / sy
                } else {
                    1.0
                };
                EntityKind::Text {
                    position: tf.apply_point(*position),
                    end_point: end_point.map(|p| tf.apply_point(p)),
                    height: *height * h_scale,
                    rotation: new_rot,
                    width_factor: *width_factor * wf_scale,
                    oblique: *oblique,
                    value: value.clone(),
                    style: style.clone(),
                    halign: *halign,
                    valign: *valign,
                    normal: new_normal,
                }
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
                let (new_normal, new_rot, sx, sy) = transform_plane_frame(*normal, *rotation, tf);
                let h_scale = if sy > 0.0 && sy.is_finite() { sy } else { 1.0 };
                let w_scale = if sx > 0.0 && sx.is_finite() {
                    sx
                } else {
                    h_scale
                };
                EntityKind::MText {
                    position: tf.apply_point(*position),
                    height: *height * h_scale,
                    width: *width * w_scale,
                    rotation: new_rot,
                    value: value.clone(),
                    plain: plain.clone(),
                    style: style.clone(),
                    attachment: *attachment,
                    line_spacing: *line_spacing,
                    normal: new_normal,
                }
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
                let (new_normal, new_rot, sx, sy) = transform_plane_frame(*normal, *rotation, tf);
                let sz = tf
                    .apply_vector(normal.normalized().unwrap_or(Vec3::Z))
                    .length();
                let det_sign = if tf.determinant() < 0.0 { -1.0 } else { 1.0 };
                EntityKind::Insert {
                    block: block.clone(),
                    position: tf.apply_point(*position),
                    scale: Vec3::new(
                        scale.x * sx * det_sign,
                        scale.y * sy,
                        scale.z * sz.max(1e-12),
                    ),
                    rotation: new_rot,
                    normal: new_normal,
                    columns: *columns,
                    rows: *rows,
                    column_spacing: *column_spacing * sx,
                    row_spacing: *row_spacing * sy,
                }
            }
            EntityKind::Hatch {
                loops,
                solid,
                pattern,
                pattern_scale,
                pattern_angle,
                normal,
            } => {
                let (new_normal, new_angle, sx, sy) =
                    transform_plane_frame(*normal, *pattern_angle, tf);
                let p_scale = 0.5 * (sx + sy);
                EntityKind::Hatch {
                    loops: loops
                        .iter()
                        .map(|l| HatchLoop {
                            edges: l
                                .edges
                                .iter()
                                .map(|e| transform_hatch_edge(e, *normal, new_normal, tf))
                                .collect(),
                            external: l.external,
                        })
                        .collect(),
                    solid: *solid,
                    pattern: pattern.clone(),
                    pattern_scale: if *solid {
                        1.0
                    } else {
                        *pattern_scale * p_scale
                    },
                    pattern_angle: if *solid { 0.0 } else { new_angle },
                    normal: new_normal,
                }
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
                let linear_scale = (tf.x.length() + tf.y.length() + tf.z.length()) / 3.0;
                let scaled_measurement = measurement.map(|m| match dimension_type {
                    crate::DimensionType::Angular | crate::DimensionType::Angular3Point => m,
                    _ => m * linear_scale,
                });
                EntityKind::Dimension {
                    dimension_type: *dimension_type,
                    points: points.iter().map(|p| tf.apply_point(*p)).collect(),
                    text_position: text_position.map(|p| tf.apply_point(p)),
                    measurement: scaled_measurement,
                    text: text.clone(),
                    block: block.clone(),
                    style: style.clone(),
                }
            }
            EntityKind::Face { points, filled } => EntityKind::Face {
                points: points.iter().map(|p| tf.apply_point(*p)).collect(),
                filled: *filled,
            },
            EntityKind::Polygon {
                exterior,
                interiors,
            } => {
                let flip = tf.determinant() < 0.0;
                let map_ring = |ring: &[Point3]| -> Vec<Point3> {
                    let mut out: Vec<Point3> = ring.iter().map(|p| tf.apply_point(*p)).collect();
                    if flip {
                        out.reverse();
                    }
                    out
                };
                EntityKind::Polygon {
                    exterior: map_ring(exterior),
                    interiors: interiors.iter().map(|r| map_ring(r)).collect(),
                }
            }
            EntityKind::Leader {
                vertices,
                arrowhead,
            } => EntityKind::Leader {
                vertices: vertices.iter().map(|p| tf.apply_point(*p)).collect(),
                arrowhead: *arrowhead,
            },
            EntityKind::Image {
                path,
                position,
                u_vector,
                v_vector,
                size_px,
            } => EntityKind::Image {
                path: path.clone(),
                position: tf.apply_point(*position),
                u_vector: tf.apply_vector(*u_vector),
                v_vector: tf.apply_vector(*v_vector),
                size_px: *size_px,
            },
            EntityKind::Viewport {
                center,
                width,
                height,
                view_center,
                view_height,
            } => EntityKind::Viewport {
                center: tf.apply_point(*center),
                width: *width,
                height: *height,
                view_center: tf.apply_point(*view_center),
                view_height: *view_height * tf.y.length().max(1e-12),
            },
            EntityKind::Mesh { vertices, faces } => EntityKind::Mesh {
                vertices: vertices.iter().map(|p| tf.apply_point(*p)).collect(),
                faces: faces.clone(),
            },
            EntityKind::Group {
                group_kind,
                name,
                origin,
                children,
            } => EntityKind::Group {
                group_kind: *group_kind,
                name: name.clone(),
                origin: origin.map(|p| tf.apply_point(p)),
                children: children.iter().map(|c| c.transformed(tf)).collect(),
            },
            EntityKind::Unknown { type_name } => EntityKind::Unknown {
                type_name: type_name.clone(),
            },
        };

        Entity {
            id: self.id,
            layer: self.layer.clone(),
            color: self.color,
            linetype: self.linetype.clone(),
            lineweight: self.lineweight,
            visible: self.visible,
            kind,
            attributes: self
                .attributes
                .iter()
                .map(|a| transform_attribute(a, tf))
                .collect(),
            props: self.props.clone(),
            raw: None,
        }
    }
}

/// Scales an entity's coordinates and linear extents by `factor` during unit conversion while
/// keeping [`EntityKind::Insert::scale`] dimensionless (since [`Block`] definitions are scaled too).
fn scale_entity_units(entity: &Entity, factor: f64, tf: &Transform) -> Entity {
    match &entity.kind {
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
        } => Entity {
            id: entity.id,
            layer: entity.layer.clone(),
            color: entity.color,
            linetype: entity.linetype.clone(),
            lineweight: entity.lineweight,
            visible: entity.visible,
            kind: EntityKind::Insert {
                block: block.clone(),
                position: tf.apply_point(*position),
                scale: *scale,
                rotation: *rotation,
                normal: *normal,
                columns: *columns,
                rows: *rows,
                column_spacing: *column_spacing * factor,
                row_spacing: *row_spacing * factor,
            },
            attributes: entity
                .attributes
                .iter()
                .map(|a| transform_attribute(a, tf))
                .collect(),
            props: entity.props.clone(),
            raw: None,
        },
        EntityKind::Group {
            group_kind,
            name,
            origin,
            children,
        } => Entity {
            id: entity.id,
            layer: entity.layer.clone(),
            color: entity.color,
            linetype: entity.linetype.clone(),
            lineweight: entity.lineweight,
            visible: entity.visible,
            kind: EntityKind::Group {
                group_kind: *group_kind,
                name: name.clone(),
                origin: origin.map(|p| tf.apply_point(p)),
                children: children
                    .iter()
                    .map(|c| scale_entity_units(c, factor, tf))
                    .collect(),
            },
            attributes: entity
                .attributes
                .iter()
                .map(|a| transform_attribute(a, tf))
                .collect(),
            props: entity.props.clone(),
            raw: None,
        },
        _ => entity.transformed(tf),
    }
}

fn tessellate_arc(arc: &EllipseArc, max_sagitta: f64) -> Vec<Point3> {
    let sweep = arc.sweep().abs();
    let r = arc.u.length().max(arc.v.length());
    let tol = if max_sagitta.is_finite() && max_sagitta > 0.0 {
        max_sagitta
    } else {
        (r * 0.01).max(1e-3)
    };
    let steps = if r > tol && r > 1e-12 {
        let half_theta = (1.0 - tol / r).clamp(-1.0, 1.0).acos();
        let step_angle = (2.0 * half_theta).max(1e-6);
        ((sweep / step_angle).ceil() as usize).clamp(4, 4096)
    } else {
        8
    };
    let mut pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = arc.t0 + (arc.t1 - arc.t0) * (i as f64) / (steps as f64);
        pts.push(arc.point(t));
    }
    pts
}

fn tessellate_entity_in_place(entity: &mut Entity, tolerance: f64) {
    let id = Transform::identity();
    match &mut entity.kind {
        EntityKind::Circle {
            center,
            radius,
            normal,
        } => {
            let ea = circle_arc(*center, *radius, 0.0, 0.0, *normal, &id);
            let mut pts = tessellate_arc(&ea, tolerance);
            if pts.len() > 2 {
                pts.pop();
            }
            entity.kind = EntityKind::Polyline {
                vertices: pts.into_iter().map(Vertex::at).collect(),
                closed: true,
                normal: *normal,
            };
        }
        EntityKind::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            normal,
        } => {
            let ea = circle_arc(*center, *radius, *start_angle, *end_angle, *normal, &id);
            let pts = tessellate_arc(&ea, tolerance);
            entity.kind = EntityKind::Polyline {
                vertices: pts.into_iter().map(Vertex::at).collect(),
                closed: false,
                normal: *normal,
            };
        }
        EntityKind::Ellipse {
            center,
            major_axis,
            ratio,
            start_param,
            end_param,
            normal,
        } => {
            let ea = full_ellipse_arc(
                *center,
                *major_axis,
                *ratio,
                *normal,
                *start_param,
                *end_param,
                &id,
            );
            let closed = (*end_param - *start_param).abs() >= TAU - 1e-9;
            let mut pts = tessellate_arc(&ea, tolerance);
            if closed && pts.len() > 2 {
                pts.pop();
            }
            entity.kind = EntityKind::Polyline {
                vertices: pts.into_iter().map(Vertex::at).collect(),
                closed,
                normal: *normal,
            };
        }
        EntityKind::Spline {
            degree,
            knots,
            control_points,
            weights,
            fit_points,
            closed,
        } => {
            let samples = if tolerance.is_finite() && tolerance > 0.0 {
                SPLINE_SAMPLES.max(24)
            } else {
                SPLINE_SAMPLES
            };
            let pts = if control_points.len() >= 2 {
                nurbs_flatten(*degree, knots, control_points, weights, samples)
            } else if let Some((deg, iknots, ictrl)) = interpolate_fit_points(fit_points, *closed) {
                nurbs_flatten(deg, &iknots, &ictrl, &[], samples)
            } else {
                fit_points.clone()
            };
            if pts.len() >= 2 {
                entity.kind = EntityKind::Polyline {
                    vertices: pts.into_iter().map(Vertex::at).collect(),
                    closed: *closed,
                    normal: Vec3::Z,
                };
            }
        }
        EntityKind::Polyline {
            vertices,
            closed,
            normal,
        } => {
            if vertices.iter().any(|v| v.bulge.abs() > 1e-12) && vertices.len() >= 2 {
                let n = vertices.len();
                let segs = if *closed { n } else { n - 1 };
                let mut out: Vec<Vertex> = Vec::with_capacity(n * 2);
                for i in 0..segs {
                    let (Some(a), Some(b)) = (vertices.get(i), vertices.get((i + 1) % n)) else {
                        continue;
                    };
                    if let Some(arc) = segment_arc(a, b, *normal, &id) {
                        let pts = tessellate_arc(&arc, tolerance);
                        let end_idx = pts.len().saturating_sub(1);
                        for pt in pts.into_iter().take(end_idx) {
                            out.push(Vertex {
                                position: pt,
                                bulge: 0.0,
                                start_width: a.start_width,
                                end_width: a.end_width,
                            });
                        }
                    } else {
                        out.push(Vertex { bulge: 0.0, ..*a });
                    }
                }
                if !*closed && let Some(last) = vertices.last() {
                    out.push(Vertex {
                        bulge: 0.0,
                        ..*last
                    });
                }
                *vertices = out;
            }
        }
        EntityKind::Group { children, .. } => {
            for child in children {
                tessellate_entity_in_place(child, tolerance);
            }
        }
        _ => {}
    }
}

fn flatten_list(
    entities: Vec<Entity>,
    inherited_layer: Option<&str>,
    inherited_color: Color,
    inherited_lw: Lineweight,
    depth: u32,
    out: &mut Vec<Entity>,
) {
    for mut e in entities {
        if matches!(e.layer.as_deref(), None | Some("0"))
            && let Some(l) = inherited_layer
        {
            e.layer = Some(l.to_owned());
        }
        if e.color == Color::ByBlock && inherited_color != Color::ByBlock {
            e.color = inherited_color;
        }
        if e.lineweight == Lineweight::ByBlock && inherited_lw != Lineweight::ByBlock {
            e.lineweight = inherited_lw;
        }
        match e.kind {
            EntityKind::Group { children, .. } if depth < 64 => {
                let child_layer = e.layer.as_deref().or(inherited_layer);
                flatten_list(children, child_layer, e.color, e.lineweight, depth + 1, out);
            }
            _ => out.push(e),
        }
    }
}

fn retain_layer_list(entities: &mut Vec<Entity>, keep: &mut impl FnMut(&str) -> bool) {
    entities.retain_mut(|e| {
        if let EntityKind::Group { children, .. } = &mut e.kind {
            retain_layer_list(children, keep);
            if children.is_empty() {
                return false;
            }
        }
        let layer = e.layer.as_deref().unwrap_or("0");
        keep(layer)
    });
}

fn collect_refs<'a>(
    entities: &'a [Entity],
    blocks: &mut Vec<&'a str>,
    layers: &mut HashSet<&'a str>,
    linetypes: &mut HashSet<&'a str>,
    styles: &mut HashSet<&'a str>,
) {
    let mut stack: Vec<&'a [Entity]> = vec![entities];
    while let Some(slice) = stack.pop() {
        for e in slice {
            layers.insert(e.layer.as_deref().unwrap_or("0"));
            if let Some(lt) = e.linetype.as_deref() {
                linetypes.insert(lt);
            }
            for attr in &e.attributes {
                if let Some(al) = attr.layer.as_deref() {
                    layers.insert(al);
                }
                if let Some(disp) = &attr.display
                    && let Some(st) = disp.style.as_deref()
                {
                    styles.insert(st);
                }
            }
            match &e.kind {
                EntityKind::Insert { block, .. } => blocks.push(block.as_str()),
                EntityKind::Dimension { block, style, .. } => {
                    if let Some(b) = block.as_deref() {
                        blocks.push(b);
                    }
                    if let Some(s) = style.as_deref() {
                        styles.insert(s);
                    }
                }
                EntityKind::Text { style, .. } | EntityKind::MText { style, .. } => {
                    if let Some(s) = style.as_deref() {
                        styles.insert(s);
                    }
                }
                EntityKind::Group { children, .. } if !children.is_empty() => {
                    stack.push(children);
                }
                _ => {}
            }
        }
    }
}

struct Exploder<'a> {
    blocks: HashMap<&'a str, &'a Block>,
    max_depth: u32,
    work_left: u64,
    work_spent: u64,
    truncated: bool,
    stack: Vec<&'a str>,
}

impl<'a> Exploder<'a> {
    fn charge(&mut self, cost: u64) -> bool {
        if self.truncated {
            return false;
        }
        if cost > self.work_left {
            self.work_left = 0;
            self.truncated = true;
            return false;
        }
        self.work_left -= cost;
        self.work_spent += cost;
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn expand_list<'b>(
        &mut self,
        list: &'b [Entity],
        tf: &Transform,
        depth: u32,
        inherited_layer: Option<&'b str>,
        block_color: Option<Color>,
        block_lw: Option<Lineweight>,
        out: &mut Vec<Entity>,
    ) {
        for e in list {
            if !self.charge(1) {
                out.push(e.transformed(tf));
                continue;
            }
            let effective_layer: Option<&'b str> = match e.layer.as_deref() {
                None | Some("0") => inherited_layer.or(e.layer.as_deref()),
                Some(l) => Some(l),
            };
            let resolved_color = match e.color {
                Color::ByBlock => block_color.unwrap_or(Color::ByLayer),
                other => other,
            };
            let resolved_lw = match e.lineweight {
                Lineweight::ByBlock => block_lw.unwrap_or(Lineweight::ByLayer),
                other => other,
            };

            match &e.kind {
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
                    let Some(def) = self.blocks.get(block.as_str()).copied() else {
                        let mut fallback = e.transformed(tf);
                        fallback.layer = effective_layer.map(str::to_owned);
                        fallback.color = resolved_color;
                        fallback.lineweight = resolved_lw;
                        out.push(fallback);
                        continue;
                    };
                    if depth >= self.max_depth || self.stack.contains(&def.name.as_str()) {
                        let mut fallback = e.transformed(tf);
                        fallback.layer = effective_layer.map(str::to_owned);
                        fallback.color = resolved_color;
                        fallback.lineweight = resolved_lw;
                        out.push(fallback);
                        continue;
                    }
                    let child_inherited = match e.layer.as_deref() {
                        None | Some("0") => inherited_layer,
                        Some(l) => Some(l),
                    };
                    self.stack.push(def.name.as_str());
                    'cells: for r in 0..(*rows).max(1) {
                        for c in 0..(*columns).max(1) {
                            if !self.charge(1) {
                                break 'cells;
                            }
                            let offset = Vec3::new(
                                f64::from(c) * *column_spacing,
                                f64::from(r) * *row_spacing,
                                0.0,
                            );
                            let local = Transform::from_insert_offset(
                                *position,
                                *scale,
                                *rotation,
                                *normal,
                                def.base_point,
                                offset,
                            );
                            let child_tf = tf.compose(&local);
                            self.expand_list(
                                &def.entities,
                                &child_tf,
                                depth + 1,
                                child_inherited,
                                Some(resolved_color),
                                Some(resolved_lw),
                                out,
                            );
                        }
                    }
                    self.stack.pop();

                    // Visible attributes with display geometry become standalone text entities.
                    for attr in &e.attributes {
                        if attr.invisible {
                            continue;
                        }
                        let (Some(pos), Some(disp), Some(val)) =
                            (attr.position, attr.display.as_ref(), attr.value.as_str())
                        else {
                            continue;
                        };
                        if val.is_empty() || disp.height <= 0.0 {
                            continue;
                        }
                        let wf = if disp.width > 0.0 {
                            disp.width / disp.height
                        } else {
                            1.0
                        };
                        let attr_entity = Entity {
                            id: None,
                            layer: attr
                                .layer
                                .clone()
                                .or_else(|| effective_layer.map(str::to_owned)),
                            color: resolved_color,
                            linetype: None,
                            lineweight: resolved_lw,
                            visible: e.visible,
                            kind: EntityKind::Text {
                                position: pos,
                                end_point: None,
                                height: disp.height,
                                rotation: disp.rotation,
                                width_factor: wf,
                                oblique: 0.0,
                                value: val.to_owned(),
                                style: disp.style.clone(),
                                halign: disp.halign,
                                valign: disp.valign,
                                normal: Vec3::Z,
                            },
                            attributes: Vec::new(),
                            props: Default::default(),
                            raw: None,
                        };
                        out.push(attr_entity.transformed(tf));
                    }
                }
                _ => {
                    let mut mapped = e.transformed(tf);
                    mapped.layer = effective_layer.map(str::to_owned);
                    mapped.color = resolved_color;
                    mapped.lineweight = resolved_lw;
                    out.push(mapped);
                }
            }
        }
    }
}

impl Document {
    /// Applies an affine [`Transform`] in-place to all design-space models (`ModelKind::Model`,
    /// or all models if no `ModelKind::Model` exists).
    ///
    /// Block definitions remain in block-local coordinates while [`EntityKind::Insert`] entities
    /// absorb the world-space transform into their placement, scale, and rotation.
    pub fn transform(&mut self, tf: &Transform) {
        let has_design = self.models.iter().any(|m| m.kind == ModelKind::Model);
        for model in &mut self.models {
            if has_design && model.kind != ModelKind::Model {
                continue;
            }
            for entity in &mut model.entities {
                *entity = entity.transformed(tf);
            }
        }
    }

    /// Converts all drawing coordinates, block definitions, linetype dash patterns, and fixed
    /// text-style heights from [`Document::units`] to `target`, returning the applied scale factor.
    ///
    /// Returns [`Error::Unsupported`] if either [`Document::units`] or `target` has no known
    /// meter conversion scale (such as [`LengthUnit::Unitless`]).
    pub fn convert_units(&mut self, target: LengthUnit) -> Result<f64> {
        let src_m = self.units.meters_per_unit().ok_or_else(|| {
            Error::Unsupported(
                "cannot convert units of a unitless document; set Document::units first".into(),
            )
        })?;
        let dst_m = target.meters().ok_or_else(|| {
            Error::Unsupported(format!(
                "target unit '{}' does not have a fixed meter scale",
                target.as_str()
            ))
        })?;
        let factor = src_m / dst_m;
        if !factor.is_finite() || factor <= 0.0 {
            return Err(Error::Unsupported(
                "unit conversion produced a non-finite scale factor".into(),
            ));
        }
        if (factor - 1.0).abs() > 1e-15 {
            let tf = Transform::uniform_scale(factor);
            for block in &mut self.blocks {
                block.base_point = tf.apply_point(block.base_point);
                for entity in &mut block.entities {
                    *entity = scale_entity_units(entity, factor, &tf);
                }
            }
            let has_design = self.models.iter().any(|m| m.kind == ModelKind::Model);
            for model in &mut self.models {
                if has_design && model.kind != ModelKind::Model {
                    continue;
                }
                for entity in &mut model.entities {
                    *entity = scale_entity_units(entity, factor, &tf);
                }
            }
            for lt in &mut self.linetypes {
                for dash in &mut lt.pattern {
                    *dash *= factor;
                }
            }
            for ts in &mut self.text_styles {
                if ts.height > 0.0 {
                    ts.height *= factor;
                }
            }
        }
        self.units = Units {
            unit: target,
            meters_per_unit: Some(dst_m),
        };
        Ok(factor)
    }

    /// Expands [`EntityKind::Insert`] block references in every model into world-space entities
    /// up to `max_depth`, resolving `ByBlock` symbology and `"0"` layer inheritance.
    ///
    /// Missing blocks, cyclic references, and inserts deeper than `max_depth` are kept intact.
    /// Work is bounded by [`DEFAULT_MAX_WORK`].
    pub fn explode_inserts(&mut self, max_depth: u32) -> WalkStats {
        let mut block_map: HashMap<&str, &Block> = HashMap::new();
        for b in &self.blocks {
            block_map.entry(b.name.as_str()).or_insert(b);
        }
        let mut exploder = Exploder {
            blocks: block_map,
            max_depth,
            work_left: DEFAULT_MAX_WORK,
            work_spent: 0,
            truncated: false,
            stack: Vec::new(),
        };
        let old_models: Vec<Vec<Entity>> = self
            .models
            .iter_mut()
            .map(|m| std::mem::take(&mut m.entities))
            .collect();
        let mut new_models = Vec::with_capacity(old_models.len());
        let id = Transform::identity();
        for entities in old_models {
            let mut expanded = Vec::with_capacity(entities.len());
            exploder.expand_list(&entities, &id, 0, None, None, None, &mut expanded);
            new_models.push(expanded);
        }
        drop(exploder.blocks);
        let stats = WalkStats {
            truncated: exploder.truncated,
            work: exploder.work_spent,
        };
        for (model, entities) in self.models.iter_mut().zip(new_models) {
            model.entities = entities;
        }
        stats
    }

    /// Flattens [`EntityKind::Group`] hierarchies into their parent entity lists across all models
    /// and block definitions, propagating inherited layers and `ByBlock` symbology.
    pub fn flatten_groups(&mut self) {
        for model in &mut self.models {
            let old = std::mem::take(&mut model.entities);
            let mut out = Vec::with_capacity(old.len());
            flatten_list(old, None, Color::ByLayer, Lineweight::ByLayer, 0, &mut out);
            model.entities = out;
        }
        for block in &mut self.blocks {
            let old = std::mem::take(&mut block.entities);
            let mut out = Vec::with_capacity(old.len());
            flatten_list(old, None, Color::ByBlock, Lineweight::ByBlock, 0, &mut out);
            block.entities = out;
        }
    }

    /// Converts curved entities (`Circle`, `Arc`, `Ellipse`, `Spline`, and bulged `Polyline`
    /// segments) across all models and blocks into straight-segment [`EntityKind::Polyline`]s
    /// within chordal error `tolerance`.
    pub fn tessellate_curves(&mut self, tolerance: f64) {
        for model in &mut self.models {
            for entity in &mut model.entities {
                tessellate_entity_in_place(entity, tolerance);
            }
        }
        for block in &mut self.blocks {
            for entity in &mut block.entities {
                tessellate_entity_in_place(entity, tolerance);
            }
        }
    }

    /// Retains only layers and model entities whose effective layer name satisfies `keep`.
    pub fn retain_layers(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.layers.retain(|l| keep(&l.name));
        for model in &mut self.models {
            retain_layer_list(&mut model.entities, &mut keep);
        }
    }

    /// Removes unreferenced [`Block`], [`crate::Layer`] (except `"0"`), [`crate::Linetype`], and
    /// [`crate::TextStyle`] table entries, returning counts of purged definitions.
    pub fn purge_unused(&mut self) -> PurgeStats {
        let mut used_blocks: HashSet<&str> = HashSet::new();
        let mut used_layers: HashSet<&str> = HashSet::new();
        let mut used_linetypes: HashSet<&str> = HashSet::new();
        let mut used_styles: HashSet<&str> = HashSet::new();

        let block_lookup: HashMap<&str, &Block> =
            self.blocks.iter().map(|b| (b.name.as_str(), b)).collect();
        let mut pending_blocks: Vec<&str> = Vec::new();

        for model in &self.models {
            collect_refs(
                &model.entities,
                &mut pending_blocks,
                &mut used_layers,
                &mut used_linetypes,
                &mut used_styles,
            );
        }
        while let Some(bname) = pending_blocks.pop() {
            if !used_blocks.insert(bname) {
                continue;
            }
            if let Some(def) = block_lookup.get(bname) {
                collect_refs(
                    &def.entities,
                    &mut pending_blocks,
                    &mut used_layers,
                    &mut used_linetypes,
                    &mut used_styles,
                );
            }
        }

        let used_blocks_owned: HashSet<String> =
            used_blocks.into_iter().map(str::to_owned).collect();
        let used_layers_owned: HashSet<String> =
            used_layers.into_iter().map(str::to_owned).collect();
        let mut used_linetypes_owned: HashSet<String> =
            used_linetypes.into_iter().map(str::to_owned).collect();
        let used_styles_owned: HashSet<String> =
            used_styles.into_iter().map(str::to_owned).collect();

        let blocks_before = self.blocks.len();
        self.blocks.retain(|b| used_blocks_owned.contains(&b.name));

        let layers_before = self.layers.len();
        self.layers
            .retain(|l| l.name == "0" || used_layers_owned.contains(&l.name));
        for l in &self.layers {
            if let Some(lt) = &l.linetype {
                used_linetypes_owned.insert(lt.clone());
            }
        }

        let linetypes_before = self.linetypes.len();
        self.linetypes
            .retain(|lt| used_linetypes_owned.contains(&lt.name));

        let styles_before = self.text_styles.len();
        self.text_styles
            .retain(|ts| used_styles_owned.contains(&ts.name));

        PurgeStats {
            blocks: blocks_before - self.blocks.len(),
            layers: layers_before - self.layers.len(),
            linetypes: linetypes_before - self.linetypes.len(),
            text_styles: styles_before - self.text_styles.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AttributeDisplay, GroupKind, HAlign, Layer, Linetype, TextStyle, VAlign};

    #[test]
    fn anisotropic_circle_promotes_to_ellipse_and_uniform_stays_circle() {
        let c = Entity::circle(Point3::xy(1.0, 2.0), 5.0);
        let uniform = c.transformed(&Transform::uniform_scale(2.0));
        assert_eq!(
            uniform.kind,
            EntityKind::Circle {
                center: Point3::xy(2.0, 4.0),
                radius: 10.0,
                normal: Vec3::Z,
            }
        );

        let stretched = c.transformed(&Transform::scaling(2.0, 1.0, 1.0));
        match stretched.kind {
            EntityKind::Ellipse {
                center,
                major_axis,
                ratio,
                ..
            } => {
                assert_eq!(center, Point3::xy(2.0, 2.0));
                assert!((major_axis.length() - 10.0).abs() < 1e-9);
                assert!((ratio - 0.5).abs() < 1e-9);
            }
            other => panic!("expected Ellipse, got {other:?}"),
        }
    }

    #[test]
    fn convert_units_scales_blocks_and_models_without_double_scaling_inserts() {
        let mut doc = Document::new();
        doc.units = Units::from_insunits(4); // Millimeter
        doc.blocks.push(Block {
            name: "DOOR".into(),
            base_point: Point3::ZERO,
            entities: vec![Entity::line(Point3::ZERO, Point3::xy(900.0, 0.0))],
            ..Block::default()
        });
        doc.linetypes.push(Linetype {
            name: "DASHED".into(),
            pattern: vec![10.0, -5.0],
            ..Linetype::default()
        });
        doc.model_space_mut()
            .entities
            .push(Entity::insert("DOOR", Point3::xy(3000.0, 0.0)));

        let factor = doc.convert_units(LengthUnit::Meter).unwrap();
        assert!((factor - 0.001).abs() < 1e-12);
        assert_eq!(doc.units.unit, LengthUnit::Meter);
        assert_eq!(doc.linetypes[0].pattern, vec![0.01, -0.005]);

        let bb = doc.bbox(0).unwrap();
        assert!((bb.min.x - 3.0).abs() < 1e-9);
        assert!((bb.max.x - 3.9).abs() < 1e-9);
    }

    #[test]
    fn explode_inserts_flatten_groups_tessellate_retain_and_purge() {
        let mut doc = Document::new();
        doc.layers.push(Layer {
            name: "WALLS".into(),
            visible: true,
            ..Layer::default()
        });
        doc.layers.push(Layer {
            name: "UNUSED_LAYER".into(),
            visible: true,
            ..Layer::default()
        });
        doc.linetypes.push(Linetype {
            name: "UNUSED_LT".into(),
            ..Linetype::default()
        });
        doc.text_styles.push(TextStyle {
            name: "UNUSED_STYLE".into(),
            ..TextStyle::default()
        });
        doc.blocks.push(Block {
            name: "SYM".into(),
            entities: vec![Entity::circle(Point3::ZERO, 2.0).with_color(Color::ByBlock)],
            ..Block::default()
        });
        doc.blocks.push(Block {
            name: "UNUSED_BLK".into(),
            ..Block::default()
        });

        let mut ins = Entity::insert("SYM", Point3::xy(10.0, 20.0))
            .on_layer("WALLS")
            .with_color(Color::aci(1));
        ins.attributes.push(Attribute {
            tag: "ID".into(),
            value: "R-1".into(),
            position: Some(Point3::xy(10.0, 22.0)),
            invisible: false,
            display: Some(AttributeDisplay {
                height: 1.5,
                width: 1.5,
                style: None,
                halign: HAlign::Center,
                valign: VAlign::Middle,
                rotation: 0.0,
            }),
            ..Attribute::default()
        });

        let grp = Entity::new(EntityKind::Group {
            group_kind: GroupKind::Cell,
            name: Some("G".into()),
            origin: None,
            children: vec![Entity::line(Point3::ZERO, Point3::xy(1.0, 1.0)).on_layer("OTHER")],
        });
        doc.model_space_mut().entities.extend([ins, grp]);

        let stats = doc.explode_inserts(8);
        assert!(!stats.truncated);
        doc.flatten_groups();
        assert_eq!(doc.model_space().unwrap().entities.len(), 3); // circle + attr text + flattened line

        doc.retain_layers(|l| l == "WALLS");
        assert_eq!(doc.model_space().unwrap().entities.len(), 2);

        doc.tessellate_curves(0.05);
        assert!(matches!(
            doc.model_space().unwrap().entities[0].kind,
            EntityKind::Polyline { closed: true, .. }
        ));

        let purged = doc.purge_unused();
        assert_eq!(purged.blocks, 2); // SYM was exploded, UNUSED_BLK never used!
        assert_eq!(purged.linetypes, 1);
        assert_eq!(purged.text_styles, 1);
    }
}
