//! Seed tabanlı DGN V8 dışa aktarımı; desteklenmeyen veriler hata üretir.

mod container;
mod encode;
mod tables;

use crate::{
    le,
    native::v8::{self, ModelHeader, RawElement},
};
use cadkit_core::{
    Document, Entity, EntityKind, Error, GroupKind, Limits, Point3, ReadOptions, Result, Value,
    Vec3,
};
use encode::*;
use flate2::{Compression, write::ZlibEncoder};
use std::{collections::BTreeMap, io::Write};
use tables::Tables;

/// V8 yazma sözleşmesi; bir çağrı tek model üretir.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct WriteOptions {
    /// Girdi, çıktı, öğe ve koordinat sınırları.
    pub limits: Limits,
    /// 8 bit etiket/metin kodlaması; `None` UTF-16 kullanır.
    pub codepage: Option<String>,
    /// Yeni kayıtların zamanı, Unix başlangıcından milisaniye; varsayılan sıfır.
    pub timestamp_ms: f64,
    /// Dolu seed'in grafik ve kontrol kayıtlarının çıkarılmasına açık izin.
    pub clear_seed_model: bool,
}
impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            limits: Limits::default(),
            codepage: None,
            timestamp_ms: 0.0,
            clear_seed_model: false,
        }
    }
}

/// Akış içeriklerini değiştirmeden geçerli CFB diziniyle yeniden paketler.
pub fn repack_v8(seed: &[u8], options: &ReadOptions) -> Result<Vec<u8>> {
    let native = v8::read(seed, options)?;
    if native.models.is_empty() {
        return Err(Error::invalid(0, "DGN seed has no model"));
    }
    container::rewrite(seed, &BTreeMap::new(), options)
}

fn compressed(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes)?;
    Ok(encoder.finish()?)
}
fn page(records: &[Vec<u8>], version: u32, number: u32) -> Result<Vec<u8>> {
    let mut data = vec![];
    for b in records {
        data.extend(0u32.to_le_bytes());
        data.extend_from_slice(b);
    }
    let mut out = vec![0; 16];
    u32_at(
        &mut out,
        0,
        u32::try_from(records.len())
            .map_err(|_| Error::LimitExceeded("DGN page records".into()))?,
    )?;
    u32_at(&mut out, 4, version)?;
    u32_at(&mut out, 8, number)?;
    u32_at(&mut out, 12, records.len() as u32)?;
    out.extend(compressed(&data)?);
    Ok(out)
}

/// Nötr modelin tek tasarım modelini uygun boyutlu V8 seed'e yazar.
/// Kaynak birimi seed master birimiyle aynı olmalı; Z veya öznitelikler sessizce atılmaz.
pub fn write_v8(doc: &Document, seed: &[u8], options: &WriteOptions) -> Result<Vec<u8>> {
    cadkit_core::export::validate_limits(doc, &options.limits)?;
    if doc
        .units
        .meters_per_unit
        .is_some_and(|v| !v.is_finite() || v <= 0.0)
    {
        return Err(Error::invalid(0, "invalid source unit scale"));
    }
    if !options.timestamp_ms.is_finite() {
        return Err(Error::invalid(0, "DGN timestamp must be finite"));
    }
    if doc.models.len() != 1 {
        return Err(Error::Unsupported(
            "DGN writer requires exactly one source model".into(),
        ));
    }
    let read = ReadOptions {
        limits: options.limits,
        fallback_codepage: options.codepage.clone(),
        keep_raw: true,
    };
    let native = v8::read(seed, &read)?;
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
    let source = doc
        .models
        .first()
        .ok_or_else(|| Error::invalid(0, "missing source model"))?;
    if source.kind != cadkit_core::ModelKind::Model {
        return Err(Error::Unsupported(
            "DGN writer accepts design models only".into(),
        ));
    }
    if source.is_3d != header.is_3d {
        return Err(Error::invalid(0, "source and seed model dimensions differ"));
    }
    if let (Some(source), Some(target)) = (doc.units.meters_per_unit(), header.master.meters()) {
        if !source.is_finite() || (source - target).abs() > target.abs() * 1e-12 {
            return Err(Error::invalid(
                0,
                "source and seed units differ; convert coordinates explicitly",
            ));
        }
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
    let nonempty = model
        .graphic_pages
        .iter()
        .chain(model.control_pages.iter())
        .any(|p| !p.elements.is_empty())
        || !model.graphic_aux.is_empty()
        || !model.control_aux.is_empty();
    if nonempty && !options.clear_seed_model {
        return Err(Error::Unsupported(
            "nonempty seed requires clear_seed_model=true".into(),
        ));
    }
    let max_id = native
        .named_pages
        .iter()
        .chain(model.graphic_pages.iter())
        .chain(model.control_pages.iter())
        .flat_map(|p| &p.elements)
        .filter_map(RawElement::id)
        .max()
        .unwrap_or(0);
    let header_stream = container::stream(seed, "Dgn~H", options.limits.max_input_bytes)?;
    let seed_header = crate::zlib::inflate(
        header_stream
            .get(0x14..)
            .ok_or_else(|| Error::invalid(0, "short DGN file header"))?,
        usize::try_from(options.limits.max_decompressed_bytes).unwrap_or(usize::MAX),
    );
    if seed_header.problem.is_some() || seed_header.data.len() != 1576 {
        return Err(Error::Unsupported(
            "unrecognized DGN file header layout".into(),
        ));
    }
    let max_id = max_id.max(le::u64_at(&seed_header.data, 0x128).unwrap_or(0));
    let mut state = State {
        tables: Tables::new(
            native
                .named_pages
                .iter()
                .flat_map(|p| p.elements.iter().cloned()),
            max_id,
            &read,
        )?,
        model: header,
        options,
        layers: &doc.layers,
        records: vec![],
        tags: vec![],
        work: 0,
        vertices: 0,
        bytes: 0,
    };
    for layer in &doc.layers {
        state
            .tables
            .level(Some(&layer.name), options.timestamp_ms)?;
    }
    for entity in &source.entities {
        state.entity(entity, 0, false)?;
    }
    state.records.append(&mut state.tags);
    let records = state.records;
    let named = state.tables.finish(options.timestamp_ms)?;
    let max_id = records
        .iter()
        .chain(named.iter())
        .filter_map(|r| le::u64_at(r, 16))
        .max()
        .unwrap_or(max_id)
        .max(max_id);
    let mut replacements = BTreeMap::new();
    for stream in &native.streams {
        if stream
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
                .starts_with(&format!("Dgn-Md/{}/Dgn^CA/", model.storage))
            || stream.path.starts_with("Dgn^Nm/")
        {
            replacements.insert(stream.path.clone(), None);
        }
    }
    let version = model
        .graphic_pages
        .first()
        .map(|p| p.header.format_version)
        .filter(|v| matches!(v, 2 | 3))
        .unwrap_or(3);
    put_pages(
        &mut replacements,
        &format!("Dgn-Md/{}/Dgn^G", model.storage),
        &records,
        version,
        options.limits.max_decompressed_bytes,
    )?;
    put_pages(
        &mut replacements,
        "Dgn^Nm",
        &named,
        native
            .named_pages
            .first()
            .map(|p| p.header.format_version)
            .unwrap_or(3),
        options.limits.max_decompressed_bytes,
    )?;
    let mut mh = header.raw.clone();
    let mut low = [i64::MAX; 3];
    let mut high = [i64::MIN; 3];
    for b in &records {
        for (i, (lo, hi)) in low.iter_mut().zip(high.iter_mut()).enumerate() {
            *lo = (*lo).min(le::i64_at(b, 0x38 + i * 8).unwrap_or(0));
            *hi = (*hi).max(le::i64_at(b, 0x50 + i * 8).unwrap_or(0));
        }
    }
    if records.is_empty() {
        low = [0; 3];
        high = [0; 3];
    }
    for (i, n) in low.into_iter().chain(high).enumerate() {
        put(&mut mh, 0x90 + i * 8, &n.to_le_bytes())?;
    }
    let mh_path = format!("Dgn-Md/{}/Dgn~Mh", model.storage);
    let mh_source = container::stream(seed, &mh_path, options.limits.max_input_bytes)?;
    let inflated_header = crate::zlib::inflate(
        &mh_source,
        usize::try_from(options.limits.max_decompressed_bytes).unwrap_or(usize::MAX),
    );
    if inflated_header.problem.is_some() {
        return Err(Error::invalid(0, "incomplete model header stream"));
    }
    let mut full_header = inflated_header
        .data
        .strip_suffix(header.raw.as_slice())
        .ok_or_else(|| Error::invalid(0, "model header suffix mismatch"))?
        .to_vec();
    full_header.extend(mh);
    replacements.insert(mh_path, Some(compressed(&full_header)?));
    let h = container::stream(seed, "Dgn~H", options.limits.max_input_bytes)?;
    let inflated = crate::zlib::inflate(
        h.get(0x14..)
            .ok_or_else(|| Error::invalid(0, "short DGN file header"))?,
        usize::try_from(options.limits.max_decompressed_bytes).unwrap_or(usize::MAX),
    );
    if inflated.problem.is_some() || inflated.data.len() != 1576 {
        return Err(Error::Unsupported(
            "unrecognized DGN file header layout".into(),
        ));
    }
    let mut data = inflated.data;
    u64_at(&mut data, 0x128, max_id)?;
    // Başlığın zaman/revizyon biçimi ek örneklerle doğrulanana kadar seed değerleri korunur.
    let mut h = h
        .get(..0x14)
        .ok_or_else(|| Error::invalid(0, "short DGN header"))?
        .to_vec();
    h.extend(compressed(&data)?);
    replacements.insert("Dgn~H".into(), Some(h));
    container::rewrite(seed, &replacements, &read)
}

#[cfg(test)]
mod tests;

fn put_pages(
    replacements: &mut BTreeMap<String, Option<Vec<u8>>>,
    prefix: &str,
    records: &[Vec<u8>],
    version: u32,
    max: u64,
) -> Result<()> {
    let total = records
        .iter()
        .try_fold(0u64, |sum, r| sum.checked_add(r.len() as u64 + 4))
        .ok_or_else(|| Error::LimitExceeded("DGN output records".into()))?;
    if total > max {
        return Err(Error::LimitExceeded("DGN output records".into()));
    }
    let mut first = 0;
    let mut bytes = 0usize;
    let mut number = 1u32;
    for (i, r) in records.iter().enumerate() {
        if bytes > 0 && bytes.saturating_add(r.len() + 4) > 256 * 1024 {
            replacements.insert(
                format!("{prefix}/${number}"),
                Some(page(records.get(first..i).unwrap_or(&[]), version, number)?),
            );
            first = i;
            bytes = 0;
            number = number
                .checked_add(1)
                .ok_or_else(|| Error::LimitExceeded("DGN page count".into()))?;
        }
        bytes = bytes.saturating_add(r.len() + 4);
    }
    replacements.insert(
        format!("{prefix}/${number}"),
        Some(page(records.get(first..).unwrap_or(&[]), version, number)?),
    );
    Ok(())
}

struct State<'a> {
    tables: Tables,
    model: &'a ModelHeader,
    options: &'a WriteOptions,
    layers: &'a [cadkit_core::Layer],
    records: Vec<Vec<u8>>,
    tags: Vec<Vec<u8>>,
    work: u64,
    vertices: u64,
    bytes: u64,
}
impl State<'_> {
    #[allow(clippy::too_many_arguments)]
    fn header(
        &self,
        kind: u32,
        size: usize,
        id: u64,
        level: u32,
        time: f64,
        entity: &Entity,
        three: bool,
    ) -> Result<Vec<u8>> {
        let mut b = encode::header(kind, size, id, level, time, entity, three)?;
        if !entity.props.contains_key("dgn.color_index") {
            use cadkit_core::Color;
            let color = if entity.color == Color::ByLayer {
                self.layers
                    .iter()
                    .find(|l| Some(l.name.as_str()) == entity.layer.as_deref())
                    .map(|l| l.color)
                    .unwrap_or(Color::ByLayer)
            } else {
                entity.color
            };
            let rgb = match color {
                Color::Aci { index } => {
                    let &(r, g, b) = cadkit_core::aci::TABLE
                        .get(usize::from(index))
                        .ok_or_else(|| Error::invalid(0, "ACI index"))?;
                    Some([r, g, b])
                }
                Color::Rgb { r, g, b } => Some([r, g, b]),
                Color::ByLayer => None,
                Color::ByBlock => {
                    return Err(Error::Unsupported(
                        "resolve ByBlock color before DGN export".into(),
                    ));
                }
            };
            if let Some(rgb) = rgb {
                let index=self.tables.palette.iter().position(|c|*c==rgb).ok_or_else(||Error::Unsupported("RGB color is absent from seed palette; provide dgn.color_index explicitly".into()))?;
                u32_at(&mut b, 0x34, index as u32)?;
            }
        }
        if entity.linetype.as_deref().is_some_and(|s| {
            !s.eq_ignore_ascii_case("continuous") && !s.eq_ignore_ascii_case("bylayer")
        }) && !entity.props.contains_key("dgn.style")
        {
            return Err(Error::Unsupported(
                "custom linetype requires seed dgn.style index".into(),
            ));
        }
        Ok(b)
    }

    fn add(&mut self, b: Vec<u8>, tag: bool) -> Result<()> {
        self.bytes = self.bytes.saturating_add(b.len() as u64);
        if self.bytes > self.options.limits.max_decompressed_bytes
            || self.records.len().saturating_add(self.tags.len()) as u64
                >= self.options.limits.max_objects
        {
            return Err(Error::LimitExceeded("DGN output record budget".into()));
        }
        if tag {
            self.tags.push(b);
        } else {
            self.records.push(b);
        }
        Ok(())
    }
    fn entity(&mut self, e: &Entity, depth: u32, component: bool) -> Result<()> {
        if depth >= self.options.limits.max_depth || self.work >= self.options.limits.max_objects {
            return Err(Error::LimitExceeded("DGN output depth/objects".into()));
        }
        self.work += 1;
        let level = self
            .tables
            .level(e.layer.as_deref(), self.options.timestamp_ms)?;
        let id = self.tables.id()?;
        let three = self.model.is_3d;
        let psize = if three { 24 } else { 16 };
        let flags = if component { 0x4000_0000 } else { 0 };
        let mut links = vec![];
        let mut b = match &e.kind {
            EntityKind::Point { position } => {
                let p = coordinate(*position, self.model)?;
                let mut b = self.header(
                    3 | flags,
                    0x68 + psize * 2,
                    id,
                    level,
                    self.options.timestamp_ms,
                    e,
                    three,
                )?;
                point(&mut b, 0x68, p, three)?;
                point(&mut b, 0x68 + psize, p, three)?;
                range(&mut b, &[p, p])?;
                b
            }
            EntityKind::Line { start, end } => {
                let p = coordinate(*start, self.model)?;
                let q = coordinate(*end, self.model)?;
                let mut b = self.header(
                    3 | flags,
                    0x68 + psize * 2,
                    id,
                    level,
                    self.options.timestamp_ms,
                    e,
                    three,
                )?;
                point(&mut b, 0x68, p, three)?;
                point(&mut b, 0x68 + psize, q, three)?;
                range(&mut b, &[p, q])?;
                b
            }
            EntityKind::Polyline {
                vertices, closed, ..
            } => {
                if vertices
                    .iter()
                    .any(|v| v.bulge != 0.0 || v.start_width != 0.0 || v.end_width != 0.0)
                {
                    return Err(Error::Unsupported(
                        "DGN polyline bulges/widths require explicit conversion".into(),
                    ));
                }
                self.points(
                    e,
                    id,
                    level,
                    flags,
                    &vertices.iter().map(|v| v.position).collect::<Vec<_>>(),
                    *closed,
                )?
            }
            EntityKind::Face { points, .. } => self.points(e, id, level, flags, points, true)?,
            EntityKind::Polygon {
                exterior,
                interiors,
            } => {
                cadkit_core::polygon::validate(exterior, interiors, 1e-6, &mut 20_000_000)?;
                if interiors.is_empty() {
                    self.points(e, id, level, flags, exterior, true)?
                } else {
                    let mut children = vec![];
                    for (i, ring) in std::iter::once(exterior)
                        .chain(interiors.iter())
                        .enumerate()
                    {
                        let mut child = Entity::new(EntityKind::Point {
                            position: Point3::default(),
                        });
                        child.layer = e.layer.clone();
                        child.color = e.color;
                        child.linetype = e.linetype.clone();
                        child.lineweight = e.lineweight;
                        child.visible = e.visible;
                        for key in ["dgn.color_index", "dgn.style", "dgn.weight"] {
                            if let Some(value) = e.props.get(key) {
                                child.props.insert(key.into(), value.clone());
                            }
                        }
                        child.attributes.clear();
                        child.kind = EntityKind::Polyline {
                            vertices: ring.iter().copied().map(cadkit_core::Vertex::at).collect(),
                            closed: true,
                            normal: Vec3::Z,
                        };
                        if i > 0 {
                            child.props.insert("dgn.hole".into(), Value::Bool(true));
                        }
                        children.push(child);
                    }
                    self.group(e, id, level, flags, depth, &children, GroupKind::Cell, None)?;
                    self.attributes(e, id, level)?;
                    return Ok(());
                }
            }
            EntityKind::Group {
                children,
                group_kind,
                name,
                ..
            } => {
                self.group(
                    e,
                    id,
                    level,
                    flags,
                    depth,
                    children,
                    *group_kind,
                    name.as_deref(),
                )?;
                self.attributes(e, id, level)?;
                return Ok(());
            }
            EntityKind::Mesh { vertices, faces } => {
                let mut children = vec![];
                for face in faces {
                    let mut points = vec![];
                    for &i in face {
                        points.push(
                            *vertices.get(i as usize).ok_or_else(|| {
                                Error::invalid(0, "mesh index outside vertex array")
                            })?,
                        );
                    }
                    let mut child = Entity::new(EntityKind::Face {
                        points,
                        filled: true,
                    });
                    child.layer = e.layer.clone();
                    child.color = e.color;
                    children.push(child);
                }
                self.group(e, id, level, flags, depth, &children, GroupKind::Cell, None)?;
                self.attributes(e, id, level)?;
                return Ok(());
            }
            EntityKind::Circle {
                center,
                radius,
                normal,
            } => {
                if normal.normalized().is_none() {
                    return Err(Error::invalid(0, "invalid normal"));
                }
                let (x, y, n) = cadkit_core::geom_ops::arbitrary_axes(*normal);
                self.ellipse(
                    e, id, level, flags, *center, *radius, *radius, x, y, n, None,
                )?
            }
            EntityKind::Arc {
                center,
                radius,
                normal,
                start_angle,
                end_angle,
            } => {
                if normal.normalized().is_none() {
                    return Err(Error::invalid(0, "invalid normal"));
                }
                let (x, y, n) = cadkit_core::geom_ops::arbitrary_axes(*normal);
                self.ellipse(
                    e,
                    id,
                    level,
                    flags,
                    *center,
                    *radius,
                    *radius,
                    x,
                    y,
                    n,
                    Some((
                        *start_angle,
                        cadkit_core::geom_ops::ccw_sweep(*start_angle, *end_angle),
                    )),
                )?
            }
            EntityKind::Ellipse {
                center,
                major_axis,
                ratio,
                start_param,
                end_param,
                normal,
            } => {
                let n = normal
                    .normalized()
                    .ok_or_else(|| Error::invalid(0, "zero ellipse normal"))?;
                let x = major_axis
                    .normalized()
                    .ok_or_else(|| Error::invalid(0, "zero ellipse axis"))?;
                if n.dot(x).abs() > 1e-9 {
                    return Err(Error::invalid(0, "ellipse major axis is not in plane"));
                }
                let y = n.cross(x);
                let a = major_axis.length();
                let arc = ((*end_param - *start_param - std::f64::consts::TAU).abs() > 1e-12)
                    .then_some((*start_param, *end_param - *start_param));
                self.ellipse(e, id, level, flags, *center, a, a * ratio, x, y, n, arc)?
            }
            EntityKind::Text {
                position,
                height,
                rotation: angle,
                width_factor,
                oblique,
                value,
                style,
                halign,
                valign,
                normal,
                ..
            } => {
                if *oblique != 0.0
                    || *halign != cadkit_core::HAlign::Left
                    || *valign != cadkit_core::VAlign::Baseline
                {
                    return Err(Error::Unsupported(
                        "DGN text requires baseline-left alignment without oblique".into(),
                    ));
                }
                if !height.is_finite()
                    || *height <= 0.0
                    || !width_factor.is_finite()
                    || *width_factor <= 0.0
                    || normal.normalized().is_none()
                {
                    return Err(Error::invalid(0, "invalid text dimensions or normal"));
                }
                let font = match e.props.get("dgn.font_number") {
                    Some(Value::Int(n)) => {
                        u32::try_from(*n).map_err(|_| Error::invalid(0, "invalid DGN font"))?
                    }
                    _ => match style {
                        Some(name) => *self.tables.fonts.get(name).ok_or_else(|| {
                            Error::Unsupported("text font is absent from seed".into())
                        })?,
                        None => self.tables.fonts.values().next().copied().unwrap_or(0),
                    },
                };
                let value = self.text(value)?;
                let offset = if three { 0xca } else { 0xaa };
                let origin = coordinate(*position, self.model)?;
                let mut b = self.header(
                    17 | flags,
                    offset + value.len(),
                    id,
                    level,
                    self.options.timestamp_ms,
                    e,
                    three,
                )?;
                u32_at(&mut b, 0x68, font)?;
                put(&mut b, 0x6c, &2u16.to_le_bytes())?;
                put(
                    &mut b,
                    0x6e,
                    &u16::try_from(value.len())
                        .map_err(|_| Error::LimitExceeded("DGN text length".into()))?
                        .to_le_bytes(),
                )?;
                f64_at(
                    &mut b,
                    0x70,
                    height * width_factor * self.model.uor_per_master / 0.006,
                )?;
                f64_at(&mut b, 0x78, height * self.model.uor_per_master / 0.006)?;
                if normal.normalized().is_none() {
                    return Err(Error::invalid(0, "invalid normal"));
                }
                let (x, y, n) = cadkit_core::geom_ops::arbitrary_axes(*normal);
                let a = x.scaled(angle.cos()).plus(y.scaled(angle.sin()));
                let c = x.scaled(-angle.sin()).plus(y.scaled(angle.cos()));
                rotation(&mut b, 0x90, a, c, n, three)?;
                point(&mut b, if three { 0xb0 } else { 0x98 }, origin, three)?;
                put(&mut b, offset, &value)?;
                range(&mut b, &[origin])?;
                links.extend(self.codepage_link()?);
                b
            }
            other => {
                return Err(Error::Unsupported(format!(
                    "DGN V8 writer: {} requires explicit conversion",
                    other.type_name()
                )));
            }
        };
        // Kaynakta çözümlenmiş delik bayrağı yeni geometriye de taşınır.
        if matches!(e.props.get("dgn.hole"), Some(Value::Bool(true))) {
            let props = le::u32_at(&b, 0x28).unwrap_or(0) | 0x8000;
            u32_at(&mut b, 0x28, props)?;
        }
        self.add(finish(b, &links)?, false)?;
        self.attributes(e, id, level)
    }
    fn points(
        &mut self,
        e: &Entity,
        id: u64,
        level: u32,
        flags: u32,
        points: &[Point3],
        closed: bool,
    ) -> Result<Vec<u8>> {
        if points.len() < if closed { 3 } else { 2 } {
            return Err(Error::invalid(0, "too few DGN vertices"));
        }
        let count = points.len() + usize::from(closed && points.first() != points.last());
        self.vertices = self.vertices.saturating_add(count as u64);
        if count as u64 > u64::from(self.options.limits.max_vertices)
            || self.vertices > self.options.limits.max_total_vertices
        {
            return Err(Error::LimitExceeded("DGN output vertices".into()));
        }
        let mut coordinates: Vec<_> = points
            .iter()
            .map(|p| coordinate(*p, self.model))
            .collect::<Result<_>>()?;
        if count > points.len() {
            if let Some(first) = coordinates.first().copied() {
                coordinates.push(first);
            }
        }
        let size = if self.model.is_3d { 24 } else { 16 };
        let mut b = self.header(
            if closed { 6 } else { 4 } | flags,
            0x70 + count * size,
            id,
            level,
            self.options.timestamp_ms,
            e,
            self.model.is_3d,
        )?;
        u32_at(&mut b, 0x68, count as u32)?;
        for (i, p) in coordinates.iter().enumerate() {
            point(&mut b, 0x70 + i * size, *p, self.model.is_3d)?;
        }
        range(&mut b, &coordinates)?;
        Ok(b)
    }
    #[allow(clippy::too_many_arguments)]
    fn ellipse(
        &mut self,
        e: &Entity,
        id: u64,
        level: u32,
        flags: u32,
        center: Point3,
        a: f64,
        b: f64,
        x: Vec3,
        y: Vec3,
        n: Vec3,
        arc: Option<(f64, f64)>,
    ) -> Result<Vec<u8>> {
        if !(a > 0.0 && b > 0.0 && a.is_finite() && b.is_finite()) {
            return Err(Error::invalid(0, "invalid DGN ellipse radii"));
        }
        let three = self.model.is_3d;
        let p = coordinate(center, self.model)?;
        let (axes, rot, origin, size) = if arc.is_some() {
            (
                0x78,
                0x88,
                if three { 0xa8 } else { 0x90 },
                if three { 0xc0 } else { 0xa0 },
            )
        } else {
            (
                0x68,
                0x78,
                if three { 0x98 } else { 0x80 },
                if three { 0xb0 } else { 0x90 },
            )
        };
        let mut out = self.header(
            if arc.is_some() { 16 } else { 15 } | flags,
            size,
            id,
            level,
            self.options.timestamp_ms,
            e,
            three,
        )?;
        if let Some((start, sweep)) = arc {
            f64_at(&mut out, 0x68, start)?;
            f64_at(&mut out, 0x70, sweep)?;
        }
        f64_at(&mut out, axes, a * self.model.uor_per_master)?;
        f64_at(&mut out, axes + 8, b * self.model.uor_per_master)?;
        rotation(&mut out, rot, x, y, n, three)?;
        point(&mut out, origin, p, three)?;
        let bounds = [
            Point3::new(
                center.x - (a * x.x).hypot(b * y.x),
                center.y - (a * x.y).hypot(b * y.y),
                center.z - (a * x.z).hypot(b * y.z),
            ),
            Point3::new(
                center.x + (a * x.x).hypot(b * y.x),
                center.y + (a * x.y).hypot(b * y.y),
                center.z + (a * x.z).hypot(b * y.z),
            ),
        ];
        range(
            &mut out,
            &[
                coordinate(bounds[0], self.model)?,
                coordinate(bounds[1], self.model)?,
            ],
        )?;
        Ok(out)
    }
    #[allow(clippy::too_many_arguments)]
    fn group(
        &mut self,
        e: &Entity,
        id: u64,
        level: u32,
        flags: u32,
        depth: u32,
        children: &[Entity],
        kind: GroupKind,
        name: Option<&str>,
    ) -> Result<()> {
        if children.len() as u64 > self.options.limits.max_objects {
            return Err(Error::LimitExceeded("DGN group children".into()));
        }
        let cell = !matches!(kind, GroupKind::ComplexChain | GroupKind::ComplexShape);
        let t = if cell {
            2
        } else if kind == GroupKind::ComplexShape {
            14
        } else {
            12
        };
        let size = if cell {
            if self.model.is_3d { 0x100 } else { 0xc0 }
        } else {
            0x70
        };
        let mut b = self.header(
            t | flags | 0x2000_0000,
            size,
            id,
            level,
            self.options.timestamp_ms,
            e,
            self.model.is_3d,
        )?;
        u32_at(&mut b, 0x68, children.len() as u32)?;
        if cell {
            let off = if self.model.is_3d { 0xa0 } else { 0x90 };
            let width = if self.model.is_3d { 3 } else { 2 };
            for i in 0..width {
                f64_at(&mut b, off + 8 * (i * width + i), 1.0)?;
            }
        }
        let links = if let Some(name) = name {
            string_link(1, name)?
        } else {
            vec![]
        };
        let index = self.records.len();
        self.add(finish(b, &links)?, false)?;
        let first = self.records.len();
        for child in children {
            self.entity(child, depth + 1, true)?;
        }
        let mut corners = vec![];
        for child in self.records.get(first..).unwrap_or(&[]) {
            let p = [
                le::i64_at(child, 0x38).unwrap_or(0) as f64,
                le::i64_at(child, 0x40).unwrap_or(0) as f64,
                le::i64_at(child, 0x48).unwrap_or(0) as f64,
            ];
            let q = [
                le::i64_at(child, 0x50).unwrap_or(0) as f64,
                le::i64_at(child, 0x58).unwrap_or(0) as f64,
                le::i64_at(child, 0x60).unwrap_or(0) as f64,
            ];
            corners.extend([p, q]);
        }
        if let Some(parent) = self.records.get_mut(index) {
            range(parent, &corners)?;
            if cell {
                let low = [
                    le::i64_at(parent, 0x38).unwrap_or(0) as f64,
                    le::i64_at(parent, 0x40).unwrap_or(0) as f64,
                    le::i64_at(parent, 0x48).unwrap_or(0) as f64,
                ];
                let high = [
                    le::i64_at(parent, 0x50).unwrap_or(0) as f64,
                    le::i64_at(parent, 0x58).unwrap_or(0) as f64,
                    le::i64_at(parent, 0x60).unwrap_or(0) as f64,
                ];
                point(parent, 0x70, low, self.model.is_3d)?;
                point(
                    parent,
                    if self.model.is_3d { 0x88 } else { 0x80 },
                    high,
                    self.model.is_3d,
                )?;
            }
        }
        Ok(())
    }
    fn text(&self, value: &str) -> Result<Vec<u8>> {
        if value.len() as u64 > u64::from(self.options.limits.max_string_bytes)
            || value.contains('\0')
        {
            return Err(Error::LimitExceeded("DGN string bytes/NUL".into()));
        }
        if let Some(label) = &self.options.codepage {
            let enc = encoding_rs::Encoding::for_label(label.as_bytes())
                .ok_or_else(|| Error::Unsupported("unknown DGN codepage".into()))?;
            let (bytes, _, errors) = enc.encode(value);
            if errors {
                return Err(Error::Unsupported(
                    "text cannot be represented in selected DGN codepage".into(),
                ));
            }
            let mut bytes = bytes.into_owned();
            bytes.push(0);
            Ok(bytes)
        } else {
            utf16(value)
        }
    }
    fn codepage_link(&self) -> Result<Vec<u8>> {
        let Some(label) = &self.options.codepage else {
            return Ok(vec![]);
        };
        let number = label
            .strip_prefix("windows-")
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|n| (1250..=1258).contains(n))
            .ok_or_else(|| {
                Error::Unsupported("DGN writer codepage must be windows-1250..1258".into())
            })?;
        let mut b = vec![0; 16];
        put(&mut b, 0, &[7, 0x10])?;
        put(&mut b, 2, &0x80d4u16.to_le_bytes())?;
        u32_at(&mut b, 12, number)?;
        Ok(b)
    }
    fn attributes(&mut self, e: &Entity, target: u64, level: u32) -> Result<()> {
        for a in &e.attributes {
            let (set, index, kind) = self.tables.tag(a)?;
            let id = self.tables.id()?;
            let value = match &a.value {
                Value::Text(s) => self.text(s)?,
                Value::Int(n) => i32::try_from(*n)
                    .map_err(|_| Error::invalid(0, "DGN integer tag exceeds i32"))?
                    .to_le_bytes()
                    .to_vec(),
                Value::Float(n) if n.is_finite() => n.to_le_bytes().to_vec(),
                _ => return Err(Error::Unsupported("DGN tag type".into())),
            };
            let mut b = self.header(
                37,
                0x140 + value.len(),
                id,
                level,
                self.options.timestamp_ms,
                e,
                self.model.is_3d,
            )?;
            let flags = le::u32_at(&b, 0x28).unwrap_or(0) | if a.invisible { 0x80 } else { 0 };
            u32_at(&mut b, 0x28, flags)?;
            let p = coordinate(a.position.unwrap_or(Point3::default()), self.model)?;
            point(&mut b, 0xa0, p, true)?;
            range(&mut b, &[p])?;
            put(&mut b, 0xd0, &index.to_le_bytes())?;
            put(&mut b, 0xd2, &kind.to_le_bytes())?;
            u32_at(
                &mut b,
                0x138,
                u32::try_from(value.len())
                    .map_err(|_| Error::LimitExceeded("DGN tag value".into()))?,
            )?;
            put(&mut b, 0x140, &value)?;
            let mut links = dependency(0x2717, set)?;
            links.extend(dependency(0x2710, target)?);
            links.extend(self.codepage_link()?);
            self.add(finish(b, &links)?, true)?;
        }
        Ok(())
    }
}
