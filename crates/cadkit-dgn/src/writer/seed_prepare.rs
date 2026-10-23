//! Pre-parsed DGN V8 seed and pre-export document preparation.

use std::collections::BTreeSet;

use cadkit_core::{
    Attribute, Color, Document, Entity, EntityKind, Error, GroupKind, HAlign, HatchEdge, HatchLoop,
    Lineweight, ModelKind, Point3, Props, ReadOptions, Result, Transform, Units, VAlign, Value,
    Vec3, Vertex,
};

use super::{container, raster_template, tables::Tables};
use crate::{
    le,
    native::v8::{self, ModelHeader, RawElement, V8File, V8Model},
};

/// How [`V8Seed::prepare_document`] reconciles 2D and 3D model dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum DimensionPolicy {
    /// Require the source model dimension to match the seed; fail otherwise.
    Strict,
    /// Promote a 2D source model to a 3D seed (`z = 0.0` preserved); reject 3D -> 2D
    /// when any vertex has a non-zero Z coordinate.
    #[default]
    Promote2dTo3d,
    /// Project 3D geometry onto `z = 0.0` (and normalize tilted normals to `+Z`) when
    /// targeting a 2D seed, and promote 2D models to 3D when targeting a 3D seed.
    ProjectToSeed,
}

/// Options for [`V8Seed::prepare_document`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SeedPrepareOptions {
    /// Specific model index to select; `None` selects the first [`ModelKind::Model`].
    pub model_index: Option<usize>,
    /// Policy for reconciling 2D and 3D model dimensions with the seed.
    pub dimension_policy: DimensionPolicy,
    /// Explode [`EntityKind::Insert`] block references into concrete entities.
    pub explode_inserts: bool,
    /// Convert coordinates and linear dimensions to the seed's master unit when both
    /// source and seed carry known metric scales.
    pub convert_units: bool,
    /// Normalize cross-format symbology (`ByBlock`, off-palette RGB/ACI colors, millimeter
    /// lineweights, custom linetypes, unknown font styles), polyline widths, `MText` /
    /// `Leader` / `Hatch` geometry, and `Text` alignment/length so DGN V8 export succeeds.
    pub normalize_for_dgn: bool,
}

impl Default for SeedPrepareOptions {
    fn default() -> Self {
        Self {
            model_index: None,
            dimension_policy: DimensionPolicy::Promote2dTo3d,
            explode_inserts: true,
            convert_units: true,
            normalize_for_dgn: true,
        }
    }
}

impl SeedPrepareOptions {
    /// Selects a specific model index to export.
    #[must_use]
    pub fn with_model_index(mut self, index: Option<usize>) -> Self {
        self.model_index = index;
        self
    }

    /// Sets the 2D/3D dimension reconciliation policy.
    #[must_use]
    pub fn with_dimension_policy(mut self, policy: DimensionPolicy) -> Self {
        self.dimension_policy = policy;
        self
    }

    /// Enables or disables exploding [`EntityKind::Insert`] block references.
    #[must_use]
    pub fn with_explode_inserts(mut self, explode: bool) -> Self {
        self.explode_inserts = explode;
        self
    }

    /// Enables or disables automatic unit conversion to the seed's master unit.
    #[must_use]
    pub fn with_convert_units(mut self, convert: bool) -> Self {
        self.convert_units = convert;
        self
    }

    /// Enables or disables cross-format symbology and entity normalization for DGN V8.
    #[must_use]
    pub fn with_normalize_for_dgn(mut self, normalize: bool) -> Self {
        self.normalize_for_dgn = normalize;
        self
    }
}

/// Pre-parsed single-model DGN V8 seed file for repeated [`super::write_v8_with_seed`]
/// calls and pre-export document preparation ([`V8Seed::prepare_document`]).
#[derive(Debug, Clone)]
pub struct V8Seed {
    pub(super) bytes: Vec<u8>,
    pub(super) native: V8File,
    pub(super) header_prefix: [u8; 0x14],
    pub(super) header_data: Vec<u8>,
    pub(super) mh_path: String,
    pub(super) mh_prefix: Vec<u8>,
    pub(super) max_id: u64,
    pub(super) seed_timestamp_ms: f64,
    pub(super) nonempty: bool,
    has_raster_template: bool,
    units: Units,
    palette: [[u8; 3]; 256],
    known_fonts: BTreeSet<String>,
    known_font_numbers: BTreeSet<u32>,
}

impl V8Seed {
    /// Parses and validates a single-model DGN V8 seed using default [`ReadOptions`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Self::parse(bytes, &ReadOptions::default())
    }

    /// Parses and validates a single-model DGN V8 seed with explicit [`ReadOptions`].
    pub fn parse(bytes: &[u8], options: &ReadOptions) -> Result<Self> {
        let mut read = options.clone();
        read.keep_raw = true;
        let native = v8::read(bytes, &read)?;
        if native.models.len() != 1 {
            return Err(Error::Unsupported(
                "DGN writer requires a single-model seed".into(),
            ));
        }
        let model = native
            .models
            .first()
            .ok_or_else(|| Error::invalid(0, "missing seed model"))?;
        let header = model
            .header
            .as_ref()
            .ok_or_else(|| Error::invalid(0, "missing seed model header"))?;
        if !header.uor_per_master.is_finite() || header.uor_per_master <= 0.0 {
            return Err(Error::invalid(0, "invalid seed UOR scale"));
        }
        if model
            .graphic_pages
            .iter()
            .chain(model.control_pages.iter())
            .any(|p| !p.complete)
            || native.named_pages.iter().any(|p| !p.complete)
        {
            return Err(Error::invalid(
                0,
                "incomplete seed pages cannot be used for writing",
            ));
        }
        let decoded: BTreeSet<&str> = model
            .graphic_pages
            .iter()
            .chain(&model.control_pages)
            .chain(&native.named_pages)
            .map(|p| p.path.as_str())
            .chain(
                model
                    .graphic_aux
                    .iter()
                    .chain(&model.control_aux)
                    .map(|p| p.path.as_str()),
            )
            .collect();
        for stream in &native.streams {
            let model_stream = stream
                .path
                .starts_with(&format!("Dgn-Md/{}/Dgn^G/", model.storage))
                || stream
                    .path
                    .starts_with(&format!("Dgn-Md/{}/Dgn^C/", model.storage))
                || stream
                    .path
                    .starts_with(&format!("Dgn-Md/{}/Dgn^GA/", model.storage))
                || stream
                    .path
                    .starts_with(&format!("Dgn-Md/{}/Dgn^CA/", model.storage));
            if model_stream || stream.path.starts_with("Dgn^Nm/") {
                let page = stream.path.rsplit('/').next().is_some_and(|n| {
                    n.strip_prefix('$')
                        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
                });
                if page && !decoded.contains(stream.path.as_str()) {
                    return Err(Error::invalid(
                        0,
                        format!("seed page {} could not be decoded", stream.path),
                    ));
                }
            }
        }
        let nonempty = model
            .graphic_pages
            .iter()
            .chain(model.control_pages.iter())
            .any(|p| !p.elements.is_empty())
            || !model.graphic_aux.is_empty()
            || !model.control_aux.is_empty();
        let max_elem_id = native
            .named_pages
            .iter()
            .chain(model.graphic_pages.iter())
            .chain(model.control_pages.iter())
            .flat_map(|p| &p.elements)
            .filter_map(RawElement::id)
            .max()
            .unwrap_or(0);
        let header_stream = container::stream(bytes, "Dgn~H", read.limits.max_input_bytes)?;
        let prefix_slice = header_stream
            .get(..0x14)
            .ok_or_else(|| Error::invalid(0, "short DGN file header"))?;
        let mut header_prefix = [0u8; 0x14];
        header_prefix.copy_from_slice(prefix_slice);
        let seed_header = crate::zlib::inflate(
            header_stream
                .get(0x14..)
                .ok_or_else(|| Error::invalid(0, "short DGN file header"))?,
            usize::try_from(read.limits.max_decompressed_bytes).unwrap_or(usize::MAX),
        );
        if seed_header.problem.is_some() || seed_header.data.len() != 1576 {
            return Err(Error::Unsupported(
                "unrecognized DGN file header layout".into(),
            ));
        }
        let max_id = max_elem_id.max(le::u64_at(&seed_header.data, 0x128).unwrap_or(0));
        let seed_timestamp_ms = le::f64_at(&seed_header.data, 0x130)
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(0.0);
        let mh_path = format!("Dgn-Md/{}/Dgn~Mh", model.storage);
        let mh_source = container::stream(bytes, &mh_path, read.limits.max_input_bytes)?;
        let inflated_header = crate::zlib::inflate(
            &mh_source,
            usize::try_from(read.limits.max_decompressed_bytes).unwrap_or(usize::MAX),
        );
        if inflated_header.problem.is_some() {
            return Err(Error::invalid(0, "incomplete model header stream"));
        }
        let mh_prefix = inflated_header
            .data
            .strip_suffix(header.raw.as_slice())
            .ok_or_else(|| Error::invalid(0, "model header suffix mismatch"))?
            .to_vec();
        let has_raster_template = raster_template::Template::capture(model).is_some();
        let units = crate::map::v8::units_of(Some(header)).0;
        let mut tables = Tables::new(&native.named_pages, max_id, &read)?;
        tables.observe_model_fonts(
            model
                .graphic_pages
                .iter()
                .chain(&model.control_pages)
                .flat_map(|p| &p.elements),
            &read,
        );
        let palette = tables.palette;
        let known_font_numbers: BTreeSet<u32> = tables
            .fonts
            .values()
            .copied()
            .chain(tables.referenced_fonts.iter().copied())
            .chain(std::iter::once(0))
            .collect();
        let known_fonts: BTreeSet<String> = tables.fonts.into_keys().collect();

        Ok(Self {
            bytes: bytes.to_vec(),
            native,
            header_prefix,
            header_data: seed_header.data,
            mh_path,
            mh_prefix,
            max_id,
            seed_timestamp_ms,
            nonempty,
            has_raster_template,
            units,
            palette,
            known_fonts,
            known_font_numbers,
        })
    }

    /// Raw bytes of the underlying DGN V8 seed file.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Whether the seed's single design model is 3D (`false` = 2D).
    pub fn is_3d(&self) -> bool {
        self.model_header().is_some_and(|h| h.is_3d)
    }

    /// Master units of the seed's design model.
    pub fn units(&self) -> Units {
        self.units
    }

    /// Whether the seed model already contains graphic or control records (requiring
    /// [`super::WriteOptions::clear_seed_model`] when exporting).
    pub fn is_nonempty(&self) -> bool {
        self.nonempty
    }

    /// Whether the seed holds a complete raster attachment template.
    pub fn has_raster_template(&self) -> bool {
        self.has_raster_template
    }

    pub(super) fn model(&self) -> Result<&V8Model> {
        self.native
            .models
            .first()
            .ok_or_else(|| Error::invalid(0, "missing seed model"))
    }

    pub(super) fn model_header(&self) -> Option<&ModelHeader> {
        self.native.models.first().and_then(|m| m.header.as_ref())
    }

    /// Prepares an arbitrary [`Document`] so it conforms to this seed's model count,
    /// units, 2D/3D dimension, block references, and DGN V8 symbology constraints.
    pub fn prepare_document(
        &self,
        doc: &Document,
        options: &SeedPrepareOptions,
    ) -> Result<Document> {
        let header = self
            .model_header()
            .ok_or_else(|| Error::invalid(0, "missing seed model header"))?;
        let mut out = doc.clone();

        // 1. Select the single target design model while keeping blocks intact for explosion.
        let selected_idx = match options.model_index {
            Some(idx) => {
                if idx >= out.models.len() {
                    return Err(Error::invalid(
                        0,
                        format!("model index {idx} out of bounds ({})", out.models.len()),
                    ));
                }
                idx
            }
            None => out
                .models
                .iter()
                .position(|m| m.kind == ModelKind::Model)
                .ok_or_else(|| {
                    Error::Unsupported("document contains no design model to export".into())
                })?,
        };
        let mut chosen = out
            .models
            .get(selected_idx)
            .cloned()
            .ok_or_else(|| Error::invalid(0, "missing selected model"))?;
        chosen.kind = ModelKind::Model;
        out.models = vec![chosen];

        // 2. Explode block inserts before unit/dimension/symbology normalization.
        if options.explode_inserts && !out.blocks.is_empty() {
            let _ = out.explode_inserts(64);
            out.blocks.clear();
        }

        // 3. Reconcile units with the seed's master unit.
        if options.convert_units {
            match (out.units.meters_per_unit(), header.master.meters()) {
                (Some(src_m), Some(dst_m)) if src_m.is_finite() && src_m > 0.0 => {
                    let factor = src_m / dst_m;
                    if !factor.is_finite() || factor <= 0.0 {
                        return Err(Error::invalid(0, "non-finite unit conversion factor"));
                    }
                    if (factor - 1.0).abs() > 1e-12 {
                        if self.units.unit.meters().is_some() {
                            let _ = out.convert_units(self.units.unit)?;
                        } else {
                            out.transform(&Transform::uniform_scale(factor));
                            for lt in &mut out.linetypes {
                                for dash in &mut lt.pattern {
                                    *dash *= factor;
                                }
                            }
                            for ts in &mut out.text_styles {
                                if ts.height > 0.0 {
                                    ts.height *= factor;
                                }
                            }
                        }
                        if let Some(m) = out.models.first_mut() {
                            for entity in &mut m.entities {
                                scale_dgn_props(entity, factor);
                            }
                        }
                    }
                    out.units = self.units;
                }
                (None, Some(_)) => {
                    out.units = self.units;
                }
                _ => {}
            }
        }

        // 4. Reconcile 2D/3D model dimension according to policy.
        if let Some(m) = out.models.first_mut() {
            if m.is_3d == header.is_3d {
                if !header.is_3d && options.dimension_policy == DimensionPolicy::ProjectToSeed {
                    for e in &mut m.entities {
                        project_entity_2d(e);
                    }
                }
            } else {
                match (m.is_3d, header.is_3d, options.dimension_policy) {
                    (
                        false,
                        true,
                        DimensionPolicy::Promote2dTo3d | DimensionPolicy::ProjectToSeed,
                    ) => {
                        m.is_3d = true;
                    }
                    (true, false, DimensionPolicy::Promote2dTo3d) => {
                        if m.entities.iter().any(entity_has_nonzero_z) {
                            return Err(Error::Unsupported(
                                "3D model contains nonzero Z coordinates; use DimensionPolicy::ProjectToSeed to flatten".into(),
                            ));
                        }
                        for e in &mut m.entities {
                            project_entity_2d(e);
                        }
                        m.is_3d = false;
                    }
                    (true, false, DimensionPolicy::ProjectToSeed) => {
                        for e in &mut m.entities {
                            project_entity_2d(e);
                        }
                        m.is_3d = false;
                    }
                    _ => {
                        return Err(Error::invalid(0, "source and seed model dimensions differ"));
                    }
                }
            }
        }

        // 5. Normalize symbology and cross-format entities for DGN V8 export.
        if options.normalize_for_dgn {
            for layer in &mut out.layers {
                normalize_layer_props(layer, &self.palette);
            }
            if let Some(m) = out.models.first_mut() {
                let mut normalized = Vec::with_capacity(m.entities.len());
                for entity in std::mem::take(&mut m.entities) {
                    if let Some(e) = normalize_entity_for_dgn(
                        entity,
                        &self.palette,
                        &self.known_fonts,
                        &self.known_font_numbers,
                        &out.linetypes,
                    ) {
                        normalized.push(e);
                    }
                }
                m.entities = normalized;
            }
        }

        Ok(out)
    }
}

fn scale_dgn_props(entity: &mut Entity, factor: f64) {
    if let Some(Value::Float(len)) = entity.props.get_mut("dgn.text_length") {
        *len *= factor;
    }
    for attr in &mut entity.attributes {
        if let Some(Value::Float(len)) = attr.props.get_mut("dgn.text_length") {
            *len *= factor;
        }
    }
    if let EntityKind::Group { children, .. } = &mut entity.kind {
        for child in children {
            scale_dgn_props(child, factor);
        }
    }
}

fn hatch_loop_has_nonzero_z(lp: &HatchLoop) -> bool {
    let nz = |p: Point3| p.z.abs() > 1e-12;
    lp.edges.iter().any(|edge| match edge {
        HatchEdge::Line { start, end } => nz(*start) || nz(*end),
        HatchEdge::Arc { center, .. } | HatchEdge::Ellipse { center, .. } => nz(*center),
        HatchEdge::Spline { control_points, .. } => control_points.iter().copied().any(nz),
        HatchEdge::Polyline { vertices, .. } => vertices.iter().any(|v| nz(v.position)),
    })
}

fn entity_has_nonzero_z(entity: &Entity) -> bool {
    let nz = |p: Point3| p.z.abs() > 1e-12;
    if entity.attributes.iter().any(|a| a.position.is_some_and(nz)) {
        return true;
    }
    match &entity.kind {
        EntityKind::Point { position } => nz(*position),
        EntityKind::Line { start, end } => nz(*start) || nz(*end),
        EntityKind::Polyline {
            vertices, normal, ..
        } => vertices.iter().any(|v| nz(v.position)) || normal.z.abs() < 0.999_999,
        EntityKind::Circle { center, normal, .. } | EntityKind::Arc { center, normal, .. } => {
            nz(*center) || normal.z.abs() < 0.999_999
        }
        EntityKind::Ellipse {
            center,
            major_axis,
            normal,
            ..
        } => nz(*center) || major_axis.z.abs() > 1e-12 || normal.z.abs() < 0.999_999,
        EntityKind::Spline {
            control_points,
            fit_points,
            ..
        } => control_points.iter().copied().any(nz) || fit_points.iter().copied().any(nz),
        EntityKind::Text {
            position,
            end_point,
            normal,
            ..
        } => nz(*position) || end_point.is_some_and(nz) || normal.z < 0.999_999,
        EntityKind::MText {
            position, normal, ..
        } => nz(*position) || normal.z < 0.999_999,
        EntityKind::Face { points, .. }
        | EntityKind::Leader {
            vertices: points, ..
        } => points.iter().copied().any(nz),
        EntityKind::Polygon {
            exterior,
            interiors,
        } => {
            exterior.iter().copied().any(nz)
                || interiors.iter().flat_map(|r| r.iter().copied()).any(nz)
        }
        EntityKind::Mesh { vertices, .. } => vertices.iter().copied().any(nz),
        EntityKind::Hatch { loops, normal, .. } => {
            normal.z.abs() < 0.999_999 || loops.iter().any(hatch_loop_has_nonzero_z)
        }
        EntityKind::Insert {
            position, normal, ..
        } => nz(*position) || normal.z < 0.999_999,
        EntityKind::Image {
            position,
            u_vector,
            v_vector,
            ..
        } => nz(*position) || u_vector.z.abs() > 1e-12 || v_vector.z.abs() > 1e-12,
        EntityKind::Viewport { center, .. } => nz(*center),
        EntityKind::Group {
            origin, children, ..
        } => origin.is_some_and(nz) || children.iter().any(entity_has_nonzero_z),
        EntityKind::Dimension { .. } | EntityKind::Unknown { .. } => false,
    }
}

fn project_entity_2d(entity: &mut Entity) {
    let flat_pt = |p: &mut Point3| p.z = 0.0;
    for attr in &mut entity.attributes {
        if let Some(pos) = &mut attr.position {
            flat_pt(pos);
        }
    }
    match &mut entity.kind {
        EntityKind::Point { position } => flat_pt(position),
        EntityKind::Line { start, end } => {
            flat_pt(start);
            flat_pt(end);
        }
        EntityKind::Polyline {
            vertices, normal, ..
        } => {
            for v in vertices {
                flat_pt(&mut v.position);
            }
            *normal = if normal.z < 0.0 {
                Vec3::new(0.0, 0.0, -1.0)
            } else {
                Vec3::Z
            };
        }
        EntityKind::Circle { center, normal, .. } => {
            flat_pt(center);
            *normal = Vec3::Z;
        }
        EntityKind::Arc { center, normal, .. } => {
            flat_pt(center);
            *normal = if normal.z < 0.0 {
                Vec3::new(0.0, 0.0, -1.0)
            } else {
                Vec3::Z
            };
        }
        EntityKind::Ellipse {
            center,
            major_axis,
            normal,
            ..
        } => {
            flat_pt(center);
            major_axis.z = 0.0;
            if major_axis.length() <= 1e-12 {
                *major_axis = Vec3::X;
            }
            *normal = Vec3::Z;
        }
        EntityKind::Spline {
            control_points,
            fit_points,
            ..
        } => {
            for p in control_points {
                flat_pt(p);
            }
            for p in fit_points {
                flat_pt(p);
            }
        }
        EntityKind::Text {
            position,
            end_point,
            normal,
            ..
        } => {
            flat_pt(position);
            if let Some(ep) = end_point {
                flat_pt(ep);
            }
            *normal = Vec3::Z;
        }
        EntityKind::MText {
            position, normal, ..
        } => {
            flat_pt(position);
            *normal = Vec3::Z;
        }
        EntityKind::Face { points, .. }
        | EntityKind::Leader {
            vertices: points, ..
        } => {
            for p in points {
                flat_pt(p);
            }
        }
        EntityKind::Polygon {
            exterior,
            interiors,
        } => {
            for p in exterior {
                flat_pt(p);
            }
            for ring in interiors {
                for p in ring {
                    flat_pt(p);
                }
            }
        }
        EntityKind::Mesh { vertices, .. } => {
            for p in vertices {
                flat_pt(p);
            }
        }
        EntityKind::Hatch { loops, normal, .. } => {
            *normal = Vec3::Z;
            for lp in loops {
                for edge in &mut lp.edges {
                    match edge {
                        HatchEdge::Line { start, end } => {
                            flat_pt(start);
                            flat_pt(end);
                        }
                        HatchEdge::Arc { center, .. } => flat_pt(center),
                        HatchEdge::Ellipse {
                            center, major_axis, ..
                        } => {
                            flat_pt(center);
                            major_axis.z = 0.0;
                        }
                        HatchEdge::Spline { control_points, .. } => {
                            for p in control_points {
                                flat_pt(p);
                            }
                        }
                        HatchEdge::Polyline { vertices, .. } => {
                            for v in vertices {
                                flat_pt(&mut v.position);
                            }
                        }
                    }
                }
            }
        }
        EntityKind::Insert {
            position, normal, ..
        } => {
            flat_pt(position);
            *normal = Vec3::Z;
        }
        EntityKind::Image {
            position,
            u_vector,
            v_vector,
            ..
        } => {
            flat_pt(position);
            u_vector.z = 0.0;
            v_vector.z = 0.0;
        }
        EntityKind::Viewport {
            center,
            view_center,
            ..
        } => {
            flat_pt(center);
            flat_pt(view_center);
        }
        EntityKind::Group {
            origin, children, ..
        } => {
            if let Some(o) = origin {
                flat_pt(o);
            }
            for c in children {
                project_entity_2d(c);
            }
        }
        EntityKind::Dimension { .. } | EntityKind::Unknown { .. } => {}
    }
}

fn sample_hatch_loop(lp: &HatchLoop) -> Vec<Point3> {
    let mut out = Vec::new();
    for edge in &lp.edges {
        match edge {
            HatchEdge::Line { start, end } => {
                if out
                    .last()
                    .is_none_or(|p: &Point3| p.distance_to(*start) > 1e-9)
                {
                    out.push(*start);
                }
                out.push(*end);
            }
            HatchEdge::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                ccw,
            } => {
                let sweep = if *ccw {
                    cadkit_core::geom_ops::ccw_sweep(*start_angle, *end_angle)
                } else {
                    -cadkit_core::geom_ops::ccw_sweep(*end_angle, *start_angle)
                };
                for i in 0..=16 {
                    let a = *start_angle + sweep * (i as f64 / 16.0);
                    let pt = Point3::new(
                        center.x + radius * a.cos(),
                        center.y + radius * a.sin(),
                        center.z,
                    );
                    if out.last().is_none_or(|p: &Point3| p.distance_to(pt) > 1e-9) {
                        out.push(pt);
                    }
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
                let sweep = if *ccw {
                    cadkit_core::geom_ops::ccw_sweep(*start_param, *end_param)
                } else {
                    -cadkit_core::geom_ops::ccw_sweep(*end_param, *start_param)
                };
                for i in 0..=16 {
                    let t = *start_param + sweep * (i as f64 / 16.0);
                    let pt = cadkit_core::geom_ops::ellipse_point(
                        *center,
                        *major_axis,
                        *ratio,
                        Vec3::Z,
                        t,
                    );
                    if out.last().is_none_or(|p: &Point3| p.distance_to(pt) > 1e-9) {
                        out.push(pt);
                    }
                }
            }
            HatchEdge::Spline {
                degree,
                knots,
                control_points,
                weights,
            } => {
                for pt in
                    cadkit_core::geom_ops::nurbs_flatten(*degree, knots, control_points, weights, 8)
                {
                    if out.last().is_none_or(|p: &Point3| p.distance_to(pt) > 1e-9) {
                        out.push(pt);
                    }
                }
            }
            HatchEdge::Polyline { vertices, .. } => {
                for v in vertices {
                    if out
                        .last()
                        .is_none_or(|p: &Point3| p.distance_to(v.position) > 1e-9)
                    {
                        out.push(v.position);
                    }
                }
            }
        }
    }
    if out.len() > 2 && out.first() == out.last() {
        out.pop();
    }
    out
}

fn mm_to_dgn_weight(mm: f64) -> i64 {
    if !mm.is_finite() || mm <= 0.1 {
        0
    } else {
        ((mm / 0.1 - 1.0) * 0.5).round().clamp(0.0, 31.0) as i64
    }
}

fn nearest_palette_index(palette: &[[u8; 3]; 256], rgb: [u8; 3]) -> i64 {
    let mut best_idx = 0usize;
    let mut best_dist = u32::MAX;
    for (i, &[pr, pg, pb]) in palette.iter().enumerate() {
        let dr = i32::from(rgb[0]) - i32::from(pr);
        let dg = i32::from(rgb[1]) - i32::from(pg);
        let db = i32::from(rgb[2]) - i32::from(pb);
        let dist = (dr * dr + dg * dg + db * db) as u32;
        if dist < best_dist {
            best_dist = dist;
            best_idx = i;
            if dist == 0 {
                break;
            }
        }
    }
    best_idx as i64
}

fn resolve_color_to_palette(color: &mut Color, props: &mut Props, palette: &[[u8; 3]; 256]) {
    if matches!(props.get("dgn.color_index"), Some(Value::Int(n)) if !(0..=255).contains(n)) {
        props.remove("dgn.color_index");
    }
    if *color == Color::ByBlock {
        *color = Color::ByLayer;
    }
    if props.contains_key("dgn.color_index") {
        return;
    }
    let rgb = match *color {
        Color::Rgb { r, g, b } => Some([r, g, b]),
        Color::Aci { index } => cadkit_core::aci::TABLE
            .get(usize::from(index))
            .map(|&(r, g, b)| [r, g, b]),
        Color::ByLayer | Color::ByBlock => None,
    };
    if let Some(rgb) = rgb {
        let idx = nearest_palette_index(palette, rgb);
        props.insert("dgn.color_index".into(), Value::Int(idx));
    }
}

fn normalize_weight(lineweight: &mut Lineweight, props: &mut Props) {
    if matches!(props.get("dgn.weight"), Some(Value::Int(n)) if !(0..=31).contains(n)) {
        props.remove("dgn.weight");
    }
    match *lineweight {
        Lineweight::ByBlock => {
            *lineweight = Lineweight::ByLayer;
        }
        Lineweight::Millimeters(mm) => {
            props
                .entry("dgn.weight".into())
                .or_insert_with(|| Value::Int(mm_to_dgn_weight(mm)));
        }
        Lineweight::ByLayer | Lineweight::Default => {}
    }
}

fn normalize_layer_props(layer: &mut cadkit_core::Layer, palette: &[[u8; 3]; 256]) {
    resolve_color_to_palette(&mut layer.color, &mut layer.props, palette);
    normalize_weight(&mut layer.lineweight, &mut layer.props);
}

fn infer_dgn_style(name: &str, linetypes: &[cadkit_core::Linetype]) -> i64 {
    let lower = name.trim().to_ascii_lowercase();
    if lower.is_empty() || lower == "continuous" || lower == "bylayer" || lower == "dgn_solid" {
        return 0;
    }
    if let Some(rest) = lower.strip_prefix("dgn_style_") {
        if let Ok(n) = rest.parse::<u32>() {
            if n <= 7 {
                return i64::from(n);
            }
        }
    }
    match lower.as_str() {
        "dgn_dotted" | "dot" | "dot2" | "dotx2" | "dotted" => return 1,
        "dgn_medium_dash" | "dashed" | "dashed2" | "hidden" | "hidden2" => return 2,
        "dgn_long_dash" | "dashedx2" | "hiddenx2" => return 3,
        "dgn_dot_dash" | "dashdot" | "dashdot2" | "center" | "center2" => return 4,
        "dgn_short_dash" => return 5,
        "dgn_dash_double_dot" | "divide" | "divide2" | "phantom" | "phantom2" => return 6,
        "dgn_long_dash_short_dash" | "border" | "border2" => return 7,
        _ => {}
    }
    if let Some(lt) = linetypes
        .iter()
        .find(|lt| lt.name.eq_ignore_ascii_case(name))
    {
        if let Some(Value::Int(s)) = lt.props.get("dgn.style") {
            if (0..=7).contains(s) {
                return *s;
            }
        }
        let dots = lt
            .pattern
            .iter()
            .filter(|&&v| v.is_finite() && v.abs() <= 1e-9)
            .count();
        let dashes = lt
            .pattern
            .iter()
            .filter(|&&v| v.is_finite() && v > 1e-9)
            .count();
        return match (dashes, dots) {
            (0, 0) => 0,
            (0, _) => 1,
            (1, 1) => 4,
            (1, d) if d >= 2 => 6,
            (d, 0) if d >= 2 => 7,
            _ => 2,
        };
    }
    2
}

fn normalize_attribute_for_dgn(
    attr: &mut Attribute,
    known_fonts: &BTreeSet<String>,
    known_font_numbers: &BTreeSet<u32>,
) {
    if matches!(attr.props.get("dgn.color_index"), Some(Value::Int(n)) if !(0..=255).contains(n)) {
        attr.props.remove("dgn.color_index");
    }
    if matches!(attr.props.get("dgn.weight"), Some(Value::Int(n)) if !(0..=31).contains(n)) {
        attr.props.remove("dgn.weight");
    }
    if let Some(Value::Int(n)) = attr.props.get("dgn.font_number") {
        let valid = u32::try_from(*n).is_ok_and(|f| known_font_numbers.contains(&f));
        if !valid {
            attr.props.remove("dgn.font_number");
        }
    }
    if let Some(display) = &mut attr.display {
        if let Some(style) = &display.style {
            let dgn_font_ok = style
                .strip_prefix("DGN_FONT_")
                .and_then(|s| s.parse::<u32>().ok())
                .is_some_and(|n| known_font_numbers.contains(&n));
            if !known_fonts.contains(style) && !dgn_font_ok {
                display.style = None;
            }
        }
        normalize_alignment(&mut display.halign, &mut display.valign);
    }
}

fn normalize_alignment(halign: &mut HAlign, valign: &mut VAlign) {
    match *halign {
        HAlign::Middle => {
            *halign = HAlign::Center;
            if *valign == VAlign::Baseline {
                *valign = VAlign::Middle;
            }
        }
        HAlign::Aligned | HAlign::Fit => {
            *halign = HAlign::Left;
        }
        HAlign::Left | HAlign::Center | HAlign::Right => {}
    }
    if *valign == VAlign::Bottom {
        *valign = VAlign::Baseline;
    }
}

fn normalize_entity_for_dgn(
    mut entity: Entity,
    palette: &[[u8; 3]; 256],
    known_fonts: &BTreeSet<String>,
    known_font_numbers: &BTreeSet<u32>,
    linetypes: &[cadkit_core::Linetype],
) -> Option<Entity> {
    resolve_color_to_palette(&mut entity.color, &mut entity.props, palette);
    normalize_weight(&mut entity.lineweight, &mut entity.props);
    if let Some(lt_name) = entity.linetype.as_deref() {
        if !lt_name.eq_ignore_ascii_case("continuous")
            && !lt_name.eq_ignore_ascii_case("bylayer")
            && !entity.props.contains_key("dgn.style")
        {
            let style = infer_dgn_style(lt_name, linetypes);
            entity.props.insert("dgn.style".into(), Value::Int(style));
        }
    }
    for attr in &mut entity.attributes {
        normalize_attribute_for_dgn(attr, known_fonts, known_font_numbers);
    }
    entity.attributes.retain(|a| match a.value {
        Value::Text(ref s) => !s.contains('\0'),
        Value::Int(n) => n.unsigned_abs() <= (1u64 << f64::MANTISSA_DIGITS),
        Value::Float(f) => f.is_finite(),
        _ => false,
    });

    match &mut entity.kind {
        EntityKind::Polyline { vertices, .. } => {
            for v in vertices {
                v.start_width = 0.0;
                v.end_width = 0.0;
            }
            Some(entity)
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
            ..
        } => {
            *oblique = 0.0;
            if matches!(*halign, HAlign::Aligned | HAlign::Fit) {
                if let Some(ep) = end_point.take() {
                    let dx = ep.x - position.x;
                    let dy = ep.y - position.y;
                    let span = dx.hypot(dy);
                    if span > 1e-9 {
                        *rotation = dy.atan2(dx);
                        let chars = value.chars().count().max(1) as f64;
                        if *halign == HAlign::Aligned {
                            *height = (span / (chars * 0.6)).max(1e-6);
                            *width_factor = 1.0;
                        } else if *height > 1e-9 {
                            *width_factor = (span / (chars * *height * 0.6)).max(1e-3);
                        }
                    }
                }
            }
            *end_point = None;
            normalize_alignment(halign, valign);
            if let Some(Value::Int(n)) = entity.props.get("dgn.font_number") {
                let valid = u32::try_from(*n).is_ok_and(|f| known_font_numbers.contains(&f));
                if !valid {
                    entity.props.remove("dgn.font_number");
                }
            }
            if let Some(s) = style {
                let dgn_font_ok = s
                    .strip_prefix("DGN_FONT_")
                    .and_then(|rest| rest.parse::<u32>().ok())
                    .is_some_and(|n| known_font_numbers.contains(&n));
                if !known_fonts.contains(s) && !dgn_font_ok {
                    *style = None;
                }
            }
            if !height.is_finite() || *height <= 0.0 {
                *height = 1.0;
            }
            if !width_factor.is_finite() || *width_factor <= 0.0 {
                *width_factor = 1.0;
            }
            if matches!(*halign, HAlign::Center | HAlign::Right)
                && !entity.props.contains_key("dgn.text_length")
            {
                let est = (value.chars().count().max(1) as f64) * *height * *width_factor * 0.6;
                entity
                    .props
                    .insert("dgn.text_length".into(), Value::Float(est.max(1e-6)));
            }
            Some(entity)
        }
        EntityKind::MText {
            position,
            height,
            rotation,
            plain,
            value,
            style,
            attachment,
            line_spacing,
            normal,
            ..
        } => {
            let text_src = if !plain.is_empty() {
                plain.as_str()
            } else {
                value.as_str()
            };
            let rot = *rotation;
            let (mut halign, mut valign) = match attachment {
                cadkit_core::Attachment::TopLeft => (HAlign::Left, VAlign::Top),
                cadkit_core::Attachment::TopCenter => (HAlign::Center, VAlign::Top),
                cadkit_core::Attachment::TopRight => (HAlign::Right, VAlign::Top),
                cadkit_core::Attachment::MiddleLeft => (HAlign::Left, VAlign::Middle),
                cadkit_core::Attachment::MiddleCenter => (HAlign::Center, VAlign::Middle),
                cadkit_core::Attachment::MiddleRight => (HAlign::Right, VAlign::Middle),
                cadkit_core::Attachment::BottomLeft => (HAlign::Left, VAlign::Baseline),
                cadkit_core::Attachment::BottomCenter => (HAlign::Center, VAlign::Baseline),
                cadkit_core::Attachment::BottomRight => (HAlign::Right, VAlign::Baseline),
            };
            normalize_alignment(&mut halign, &mut valign);
            let h = if height.is_finite() && *height > 0.0 {
                *height
            } else {
                1.0
            };
            let spacing = if line_spacing.is_finite() && *line_spacing > 0.0 {
                *line_spacing * 1.4
            } else {
                1.4
            };
            let resolved_style = style.clone().filter(|s| known_fonts.contains(s));
            let lines: Vec<&str> = text_src.split('\n').collect();
            if lines.len() <= 1 {
                let text_val = lines.first().copied().unwrap_or("").to_owned();
                if matches!(halign, HAlign::Center | HAlign::Right) {
                    let est = (text_val.chars().count().max(1) as f64) * h * 0.6;
                    entity
                        .props
                        .insert("dgn.text_length".into(), Value::Float(est.max(1e-6)));
                }
                entity.kind = EntityKind::Text {
                    position: *position,
                    end_point: None,
                    height: h,
                    rotation: rot,
                    width_factor: 1.0,
                    oblique: 0.0,
                    value: text_val,
                    style: resolved_style,
                    halign,
                    valign,
                    normal: *normal,
                };
                Some(entity)
            } else {
                let (ax, ay, _) = cadkit_core::geom_ops::arbitrary_axes(*normal);
                let down = ax
                    .scaled(rot.sin())
                    .plus(ay.scaled(-rot.cos()))
                    .scaled(h * spacing);
                let mut children = Vec::with_capacity(lines.len());
                for (idx, line) in lines.into_iter().enumerate() {
                    let line_pos = position.translated(down.scaled(idx as f64));
                    let mut child = Entity::new(EntityKind::Text {
                        position: line_pos,
                        end_point: None,
                        height: h,
                        rotation: rot,
                        width_factor: 1.0,
                        oblique: 0.0,
                        value: line.to_owned(),
                        style: resolved_style.clone(),
                        halign,
                        valign,
                        normal: *normal,
                    });
                    child.layer = entity.layer.clone();
                    child.color = entity.color;
                    child.lineweight = entity.lineweight;
                    child.visible = entity.visible;
                    for key in ["dgn.color_index", "dgn.weight", "dgn.style"] {
                        if let Some(v) = entity.props.get(key) {
                            child.props.insert(key.into(), v.clone());
                        }
                    }
                    if matches!(halign, HAlign::Center | HAlign::Right) {
                        let est = (line.chars().count().max(1) as f64) * h * 0.6;
                        child
                            .props
                            .insert("dgn.text_length".into(), Value::Float(est.max(1e-6)));
                    }
                    children.push(child);
                }
                let origin = *position;
                entity.kind = EntityKind::Group {
                    group_kind: GroupKind::TextNode,
                    name: None,
                    origin: Some(origin),
                    children,
                };
                Some(entity)
            }
        }
        EntityKind::Leader { vertices, .. } => {
            if vertices.len() < 2 {
                return None;
            }
            let verts = vertices.iter().copied().map(Vertex::at).collect();
            entity.kind = EntityKind::Polyline {
                vertices: verts,
                closed: false,
                normal: Vec3::Z,
            };
            Some(entity)
        }
        EntityKind::Hatch { loops, normal, .. } => {
            let mut children = Vec::new();
            for lp in loops {
                let pts = sample_hatch_loop(lp);
                if pts.len() >= 3 {
                    let mut child = Entity::new(EntityKind::Polyline {
                        vertices: pts.into_iter().map(Vertex::at).collect(),
                        closed: true,
                        normal: *normal,
                    });
                    child.layer = entity.layer.clone();
                    child.color = entity.color;
                    child.lineweight = entity.lineweight;
                    child.visible = entity.visible;
                    for key in ["dgn.color_index", "dgn.weight", "dgn.style"] {
                        if let Some(v) = entity.props.get(key) {
                            child.props.insert(key.into(), v.clone());
                        }
                    }
                    children.push(child);
                }
            }
            match children.len() {
                0 => None,
                1 => {
                    let mut single = children.pop()?;
                    single.attributes = std::mem::take(&mut entity.attributes);
                    Some(single)
                }
                _ => {
                    entity.kind = EntityKind::Group {
                        group_kind: GroupKind::Cell,
                        name: None,
                        origin: None,
                        children,
                    };
                    Some(entity)
                }
            }
        }
        EntityKind::Group { children, .. } => {
            let mut next = Vec::with_capacity(children.len());
            for child in std::mem::take(children) {
                if let Some(c) = normalize_entity_for_dgn(
                    child,
                    palette,
                    known_fonts,
                    known_font_numbers,
                    linetypes,
                ) {
                    next.push(c);
                }
            }
            if next.is_empty() && entity.attributes.is_empty() {
                None
            } else {
                *children = next;
                Some(entity)
            }
        }
        EntityKind::Unknown { type_name } if type_name == "dgn.type_37" => {
            (entity.attributes.len() == 1).then_some(entity)
        }
        EntityKind::Insert { .. }
        | EntityKind::Dimension { .. }
        | EntityKind::Viewport { .. }
        | EntityKind::Unknown { .. } => None,
        EntityKind::Point { .. }
        | EntityKind::Line { .. }
        | EntityKind::Circle { .. }
        | EntityKind::Arc { .. }
        | EntityKind::Ellipse { .. }
        | EntityKind::Spline { .. }
        | EntityKind::Face { .. }
        | EntityKind::Polygon { .. }
        | EntityKind::Mesh { .. }
        | EntityKind::Image { .. } => Some(entity),
    }
}
