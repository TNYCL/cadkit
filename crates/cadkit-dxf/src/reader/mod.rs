//! DXF reader: pair stream -> records -> tables / blocks / entities / objects -> [`Document`].

mod common;
mod entities;
mod hatch;
mod records;
mod tables;

use std::collections::HashMap;

use cadkit_core::text::OemCodepage;
use cadkit_core::{
    Block, Document, Entity, EntityKind, Error, Format, Model, ModelKind, Point3, ReadOptions,
    Result, Summary, Units, Value as MValue,
};

use self::common::{handle, model_value};
use self::entities::{Ctx, DxfEncoding, Parsed};
use self::records::{Group, Record, RecordReader, parse_handle};
use crate::native::{Pair, Tokenizer, Value, encoding_for_codepage};

/// Where an entity read from a block or section belongs.
enum Space {
    /// Decide from owner handle / group 67.
    Auto,
    Model,
    /// A paper-space block, identified by its lower-cased block name.
    Paper(String),
}

struct Placed {
    parsed: Parsed,
    space: Space,
}

struct LayoutInfo {
    name: String,
    handle: Option<u64>,
    tab_order: i64,
    block_record: Option<u64>,
}

struct State<'o> {
    ctx: Ctx<'o>,
    doc: Document,
    is_r12: bool,
    block_records: HashMap<u64, String>,
    layouts: Vec<LayoutInfo>,
    imagedefs: HashMap<u64, String>,
    placed: Vec<Placed>,
    entity_count: u64,
}

/// `*Model_Space` (R13+) or `$MODEL_SPACE` (R12 spelling).
fn is_model_space(name: &str) -> bool {
    name.eq_ignore_ascii_case("*model_space") || name.eq_ignore_ascii_case("$model_space")
}

/// `*Paper_Space`, `*Paper_Space0`, ... (R13+) or `$PAPER_SPACE` (R12 spelling).
fn is_paper_space(name: &str) -> bool {
    name.len() >= 12
        && name.get(..12).is_some_and(|p| {
            p.eq_ignore_ascii_case("*paper_space") || p.eq_ignore_ascii_case("$paper_space")
        })
}

fn resolve_encoding(name: &str) -> Option<DxfEncoding> {
    if let Some(oem) = OemCodepage::from_label(name) {
        return Some(DxfEncoding::Oem(oem));
    }
    encoding_for_codepage(name).map(DxfEncoding::Whatwg)
}

/// Chooses the text encoding: UTF-8 for AC1021+, else `$DWGCODEPAGE`, else the caller's fallback,
/// else windows-1252.
fn pick_encoding(
    version: Option<&str>,
    codepage: Option<&str>,
    fallback: Option<&str>,
    warn: &mut Vec<String>,
) -> DxfEncoding {
    if version.is_some_and(|v| v >= "AC1021") {
        return DxfEncoding::Whatwg(encoding_rs::UTF_8);
    }
    if let Some(cp) = codepage {
        match resolve_encoding(cp) {
            Some(e) => return e,
            None => warn.push(format!("unsupported $DWGCODEPAGE `{cp}`")),
        }
    }
    if let Some(fb) = fallback {
        if let Some(e) = resolve_encoding(fb) {
            return e;
        }
        warn.push(format!("unsupported fallback code page `{fb}`"));
    }
    DxfEncoding::Whatwg(encoding_rs::WINDOWS_1252)
}

/// Scans the HEADER for `$ACADVER` and `$DWGCODEPAGE` before the real decoding pass.
fn prescan(bytes: &[u8]) -> (Option<String>, Option<String>) {
    let mut tok = Tokenizer::new(bytes);
    tok.set_decode_escapes(false);
    let mut version = None;
    let mut codepage = None;
    let mut var = String::new();
    for _ in 0..200_000 {
        let Ok(Some(p)) = tok.next_pair() else { break };
        match (p.code, &p.value) {
            (0, Value::Str(s)) if s.eq_ignore_ascii_case("ENDSEC") => break,
            (9, Value::Str(s)) => var = s.to_ascii_uppercase(),
            (1, Value::Str(s)) if var == "$ACADVER" => version = Some(s.trim().to_owned()),
            (3, Value::Str(s)) if var == "$DWGCODEPAGE" => codepage = Some(s.trim().to_owned()),
            _ => {}
        }
        if version.is_some() && codepage.is_some() {
            break;
        }
    }
    (version, codepage)
}

fn length_unit(code: i64) -> Units {
    Units::from_insunits(i32::try_from(code).unwrap_or(0))
}

/// Inspects the DXF `HEADER` section without decoding `TABLES`, `BLOCKS`, or `ENTITIES`.
pub(crate) fn inspect(bytes: &[u8], options: &ReadOptions) -> Result<Summary> {
    if bytes.len() as u64 > options.limits.max_input_bytes {
        return Err(Error::LimitExceeded(format!(
            "input of {} bytes exceeds the limit of {}",
            bytes.len(),
            options.limits.max_input_bytes
        )));
    }
    if !crate::native::sniff(bytes) {
        return Err(Error::UnknownFormat);
    }
    let mut tok = Tokenizer::new(bytes);
    tok.set_decode_escapes(false);
    tok.set_max_string_bytes(options.limits.max_string_bytes as usize);

    let mut saw_section = false;
    let mut awaiting_section_name = false;
    let mut in_header = false;
    let mut comment: Option<String> = None;
    let mut version: Option<String> = None;
    let mut codepage: Option<String> = None;
    let mut insunits: Option<i64> = None;
    let mut extmin_z: Option<f64> = None;
    let mut extmax_z: Option<f64> = None;
    let mut var = String::new();

    for _ in 0..200_000 {
        let pair = match tok.next_pair() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(e @ Error::LimitExceeded(_)) => return Err(e),
            Err(e) => {
                if !saw_section {
                    return Err(e);
                }
                break;
            }
        };
        if pair.code == 999 {
            if comment.is_none() {
                if let Value::Str(s) = pair.value {
                    comment = Some(s);
                }
            }
            continue;
        }
        if pair.code == 0 {
            var.clear();
            if let Value::Str(s) = &pair.value {
                if s.eq_ignore_ascii_case("SECTION") {
                    saw_section = true;
                    awaiting_section_name = true;
                    continue;
                }
                awaiting_section_name = false;
                if s.eq_ignore_ascii_case("ENDSEC") && in_header {
                    break;
                }
                if s.eq_ignore_ascii_case("EOF") {
                    break;
                }
            }
            continue;
        }
        if awaiting_section_name {
            if pair.code == 2 {
                awaiting_section_name = false;
                if pair
                    .value
                    .as_str()
                    .is_some_and(|s| s.trim().eq_ignore_ascii_case("HEADER"))
                {
                    in_header = true;
                } else {
                    break;
                }
            }
            continue;
        }
        if in_header {
            match (pair.code, &pair.value) {
                (9, Value::Str(s)) => var = s.trim().to_ascii_uppercase(),
                (1, Value::Str(s)) if var == "$ACADVER" => version = Some(s.trim().to_owned()),
                (3, Value::Str(s)) if var == "$DWGCODEPAGE" => {
                    codepage = Some(s.trim().to_owned());
                }
                (70, v) if var == "$INSUNITS" => insunits = v.as_i64(),
                (30, v) if var == "$EXTMIN" => extmin_z = v.as_f64(),
                (30, v) if var == "$EXTMAX" => extmax_z = v.as_f64(),
                _ => {}
            }
        }
    }

    if !saw_section {
        return Err(Error::UnknownFormat);
    }

    let is_3d = match (extmin_z, extmax_z) {
        (Some(z0), Some(z1)) if z0.is_finite() && z1.is_finite() && z0 <= z1 => {
            (z0.abs() > 1e-12 || z1.abs() > 1e-12).then_some(true)
        }
        _ => None,
    };
    let units = insunits
        .map(length_unit)
        .or_else(|| in_header.then(Units::default));

    Ok(Summary::new(Format::Dxf, version.unwrap_or_default())
        .with_is_3d(is_3d)
        .with_units(units)
        .with_codepage(codepage)
        .with_application(comment))
}

/// Most groups one record may hold. A pair costs about 40 bytes in memory, so 8 million caps
/// a single record near 320 MB; the largest real records (an LWPOLYLINE with widths needs
/// 5 groups per vertex, a hatch spline a few per control point) stay far below that. The cap
/// also follows `max_vertices` (8 groups per vertex) and the input size (every group takes at
/// least two bytes), so small inputs can never allocate much.
const MAX_RECORD_PAIRS: usize = 8_000_000;

fn record_pair_cap(max_vertices: u32, input_len: usize) -> usize {
    (max_vertices as usize)
        .saturating_mul(8)
        .saturating_add(4096)
        .min(MAX_RECORD_PAIRS)
        .min(input_len / 2 + 16)
}

/// Reads a DXF file. See the crate documentation for the mapping rules.
pub(crate) fn read(bytes: &[u8], options: &ReadOptions) -> Result<Document> {
    if bytes.len() as u64 > options.limits.max_input_bytes {
        return Err(Error::LimitExceeded(format!(
            "input of {} bytes exceeds the limit of {}",
            bytes.len(),
            options.limits.max_input_bytes
        )));
    }
    if !crate::native::sniff(bytes) {
        return Err(Error::UnknownFormat);
    }
    let (version, codepage) = prescan(bytes);
    let mut enc_warnings = Vec::new();
    let encoding = pick_encoding(
        version.as_deref(),
        codepage.as_deref(),
        options.fallback_codepage.as_deref(),
        &mut enc_warnings,
    );

    let mut tok = Tokenizer::new(bytes);
    match encoding {
        DxfEncoding::Whatwg(enc) => tok.set_encoding(enc),
        DxfEncoding::Oem(oem) => tok.set_oem_codepage(oem),
    }
    tok.set_max_string_bytes(options.limits.max_string_bytes as usize);
    let max_pairs = record_pair_cap(options.limits.max_vertices, bytes.len());
    let mut rr = RecordReader::new(tok, max_pairs);

    let mut st = State {
        ctx: Ctx::new(options, encoding),
        doc: Document::default(),
        is_r12: version.as_deref().is_none_or(|v| v <= "AC1009"),
        block_records: HashMap::new(),
        layouts: Vec::new(),
        imagedefs: HashMap::new(),
        placed: Vec::new(),
        entity_count: 0,
    };
    st.doc.source.format = Format::Dxf;
    st.doc.source.version = version.unwrap_or_default();
    st.doc.source.codepage = codepage;
    for w in enc_warnings {
        st.ctx.warn("dxf.codepage", w, None, None);
    }

    let mut sections = 0u32;
    while let Some(rec) = rr.next_record()? {
        match rec.name.as_str() {
            "SECTION" => {
                sections += 1;
                let name = Group(&rec.pairs)
                    .string(2)
                    .unwrap_or_default()
                    .to_ascii_uppercase();
                st.section(&mut rr, &name, &rec)?;
            }
            "EOF" => break,
            _ => {}
        }
    }
    st.doc.source.application = rr.comment.clone();

    if let Some(err) = rr.take_error() {
        match err {
            Error::LimitExceeded(_) => return Err(err),
            e if sections == 0 => return Err(e),
            e => st.ctx.warn(
                "dxf.truncated",
                format!("input ended early: {e}"),
                Some(rr.offset()),
                None,
            ),
        }
    }
    if rr.stray > 0 {
        st.ctx.warn(
            "dxf.stray_groups",
            format!("{} group(s) outside any record were ignored", rr.stray),
            None,
            None,
        );
    }
    if sections == 0 {
        return Err(Error::UnknownFormat);
    }
    Ok(st.finish())
}

impl State<'_> {
    fn section(&mut self, rr: &mut RecordReader, name: &str, rec: &Record) -> Result<()> {
        match name {
            "HEADER" => self.header(rec),
            "TABLES" => self.tables(rr)?,
            "BLOCKS" => self.blocks(rr)?,
            "ENTITIES" => {
                let list = self.entity_list(rr)?;
                self.placed.extend(list.into_iter().map(|parsed| Placed {
                    parsed,
                    space: Space::Auto,
                }));
            }
            "OBJECTS" => self.objects(rr)?,
            _ => skip_section(rr)?,
        }
        Ok(())
    }

    fn header(&mut self, rec: &Record) {
        let mut vars: Vec<(String, Vec<&Pair>)> = Vec::new();
        for p in &rec.pairs {
            if p.code == 9 {
                vars.push((p.value.as_str().unwrap_or("").to_owned(), Vec::new()));
            } else if let Some((_, items)) = vars.last_mut() {
                items.push(p);
            }
        }
        for (name, items) in vars {
            if name.is_empty() {
                continue;
            }
            if name.eq_ignore_ascii_case("$INSUNITS") {
                if let Some(v) = items.first().and_then(|p| p.value.as_i64()) {
                    self.doc.units = length_unit(v);
                }
            }
            let value = match items.as_slice() {
                [] => continue,
                [one] => model_value(&one.value),
                many => MValue::List(many.iter().map(|p| model_value(&p.value)).collect()),
            };
            self.doc.props.insert(format!("dxf.{name}"), value);
        }
    }

    fn tables(&mut self, rr: &mut RecordReader) -> Result<()> {
        while let Some(rec) = rr.next_record()? {
            match rec.name.as_str() {
                "ENDSEC" | "EOF" | "SECTION" => {
                    rr.push_back(rec);
                    break;
                }
                "LAYER" => self.doc.layers.push(tables::layer(&rec)),
                "LTYPE" => {
                    if let Some(l) = tables::linetype(&rec) {
                        self.doc.linetypes.push(l);
                    }
                }
                "STYLE" => {
                    if let Some(s) = tables::text_style(&rec) {
                        self.doc.text_styles.push(s);
                    }
                }
                "BLOCK_RECORD" => {
                    let g = Group(rec.body());
                    if let (Some(h), Some(name)) = (handle(g), g.string(2)) {
                        self.block_records.insert(h, name);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Reads entities until a section/block terminator, which is pushed back.
    fn entity_list(&mut self, rr: &mut RecordReader) -> Result<Vec<Parsed>> {
        let mut out = Vec::new();
        while let Some(rec) = rr.next_record()? {
            if matches!(
                rec.name.as_str(),
                "ENDSEC" | "ENDBLK" | "BLOCK" | "EOF" | "SECTION"
            ) {
                rr.push_back(rec);
                break;
            }
            let mut children = Vec::new();
            let wants_children = match rec.name.as_str() {
                "POLYLINE" => true,
                "INSERT" => Group(rec.body()).i64_or(66, 0) != 0,
                _ => false,
            };
            if wants_children {
                while let Some(child) = rr.next_record()? {
                    match child.name.as_str() {
                        "VERTEX" | "ATTRIB" => {
                            if children.len() >= self.ctx.opts.limits.max_vertices as usize {
                                return Err(Error::LimitExceeded(
                                    "too many sub-records in one entity".into(),
                                ));
                            }
                            children.push(child);
                        }
                        "SEQEND" => break,
                        _ => {
                            rr.push_back(child);
                            break;
                        }
                    }
                }
            }
            self.entity_count += 1;
            if self.entity_count > self.ctx.opts.limits.max_objects {
                return Err(Error::LimitExceeded(format!(
                    "more than {} entities",
                    self.ctx.opts.limits.max_objects
                )));
            }
            if let Some(p) = entities::convert(&mut self.ctx, &rec, &children)? {
                out.push(p);
            }
        }
        Ok(out)
    }

    fn blocks(&mut self, rr: &mut RecordReader) -> Result<()> {
        while let Some(rec) = rr.next_record()? {
            match rec.name.as_str() {
                "ENDSEC" | "EOF" | "SECTION" => {
                    rr.push_back(rec);
                    break;
                }
                "BLOCK" => {
                    let g = Group(rec.body());
                    let name = g.string(2).or_else(|| g.string(3)).unwrap_or_default();
                    let flags = g.i64_or(70, 0);
                    let entities = self.entity_list(rr)?;
                    // Consume ENDBLK when present.
                    if let Some(end) = rr.next_record()? {
                        if end.name != "ENDBLK" {
                            rr.push_back(end);
                        }
                    }
                    if is_model_space(&name) {
                        self.placed
                            .extend(entities.into_iter().map(|parsed| Placed {
                                parsed,
                                space: Space::Model,
                            }));
                        continue;
                    }
                    if is_paper_space(&name) {
                        let key = name.to_ascii_lowercase();
                        self.placed
                            .extend(entities.into_iter().map(|parsed| Placed {
                                parsed,
                                space: Space::Paper(key.clone()),
                            }));
                        continue;
                    }
                    let mut block = Block {
                        name,
                        id: handle(g),
                        base_point: g.point_or_zero(10),
                        entities: entities.into_iter().map(|p| p.entity).collect(),
                        is_xref: flags & 4 != 0,
                        xref_path: if flags & 4 != 0 {
                            g.string(1).filter(|s| !s.is_empty())
                        } else {
                            None
                        },
                        description: g.string(4).filter(|s| !s.is_empty()),
                        props: Default::default(),
                    };
                    block
                        .props
                        .insert("dxf.block_flags".into(), MValue::Int(flags));
                    if let Some(l) = g.string(8) {
                        block.props.insert("dxf.layer".into(), MValue::Text(l));
                    }
                    common::xdata_props(&rec, &mut block.props);
                    self.doc.blocks.push(block);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn objects(&mut self, rr: &mut RecordReader) -> Result<()> {
        while let Some(rec) = rr.next_record()? {
            match rec.name.as_str() {
                "ENDSEC" | "EOF" | "SECTION" => {
                    rr.push_back(rec);
                    break;
                }
                "LAYOUT" => {
                    // AcDbPlotSettings also has a group 1 (page setup name): read the AcDbLayout part.
                    let at = rec
                        .body()
                        .iter()
                        .position(|p| p.code == 100 && p.value.as_str() == Some("AcDbLayout"))
                        .unwrap_or(0);
                    let g = Group(rec.body().get(at..).unwrap_or(&[]));
                    self.layouts.push(LayoutInfo {
                        name: g.string(1).unwrap_or_default(),
                        handle: handle(Group(rec.body())),
                        tab_order: g.i64_or(71, 0),
                        block_record: g
                            .all(330)
                            .last()
                            .and_then(|v| v.as_str())
                            .and_then(parse_handle),
                    });
                }
                "IMAGEDEF" => {
                    let g = Group(rec.body());
                    if let (Some(h), Some(path)) = (handle(g), g.string(1)) {
                        self.imagedefs.insert(h, path);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Assigns entities to spaces and builds the final document.
    fn finish(mut self) -> Document {
        self.ctx.finish();
        let placed = std::mem::take(&mut self.placed);
        let mut model_entities: Vec<Entity> = Vec::new();
        // (lower-cased key, original block name, entities) in order of first appearance.
        // Entities are taken out (`Option`) once a LAYOUT claims them; `paper_index` makes
        // every lookup O(1) however many layouts a file has.
        let mut papers: Vec<(String, String, Option<Vec<Entity>>)> = Vec::new();
        let mut paper_index: std::collections::HashMap<String, usize> = Default::default();

        for Placed { parsed, space } in placed {
            let dest: Option<(String, String)> = match space {
                Space::Model => None,
                Space::Paper(key) => Some((key.clone(), key)),
                Space::Auto => self.route_auto(&parsed),
            };
            match dest {
                None => model_entities.push(parsed.entity),
                Some((key, orig)) => match paper_index.get(&key).and_then(|&i| papers.get_mut(i)) {
                    Some((_, _, list)) => list.get_or_insert_with(Vec::new).push(parsed.entity),
                    None => {
                        paper_index.insert(key.clone(), papers.len());
                        papers.push((key, orig, Some(vec![parsed.entity])));
                    }
                },
            }
        }

        // Model space.
        let model_layout = self
            .layouts
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case("Model"));
        let mut model = Model {
            name: "Model".to_owned(),
            id: model_layout.and_then(|l| l.handle),
            kind: ModelKind::Model,
            is_3d: false,
            entities: model_entities,
            props: Default::default(),
        };
        model.is_3d = model.entities.iter().any(entity_has_z);

        // Layouts, ordered by tab order.
        let mut layout_idx: Vec<usize> = (0..self.layouts.len())
            .filter(|&i| {
                self.layouts
                    .get(i)
                    .is_some_and(|l| !l.name.eq_ignore_ascii_case("Model"))
            })
            .collect();
        layout_idx.sort_by_key(|&i| self.layouts.get(i).map_or(0, |l| l.tab_order));
        let mut layout_models: Vec<Model> = Vec::new();
        for i in layout_idx {
            let Some(l) = self.layouts.get(i) else {
                continue;
            };
            let key = l
                .block_record
                .and_then(|b| self.block_records.get(&b))
                .map(|n| n.to_ascii_lowercase());
            let entities = key
                .as_ref()
                .and_then(|k| paper_index.get(k))
                .and_then(|&i| papers.get_mut(i))
                .and_then(|(_, _, list)| list.take())
                .unwrap_or_default();
            let mut props = cadkit_core::Props::new();
            props.insert("dxf.layout.tab_order".into(), MValue::Int(l.tab_order));
            if let Some(b) = l.block_record {
                props.insert(
                    "dxf.layout.block_record".into(),
                    MValue::Text(format!("{b:X}")),
                );
            }
            layout_models.push(Model {
                name: l.name.clone(),
                id: l.handle,
                kind: ModelKind::Layout,
                is_3d: false,
                entities,
                props,
            });
        }
        // Paper-space entities without a LAYOUT object (R12, stripped files).
        let mut default_name = Some(if self.is_r12 {
            "Paper Space"
        } else {
            "Layout1"
        });
        for (_, orig, entities) in papers {
            let Some(entities) = entities else { continue };
            let name = match default_name.take() {
                Some(n) if layout_models.is_empty() => n.to_owned(),
                _ => orig.trim_start_matches('*').to_owned(),
            };
            layout_models.push(Model {
                name,
                id: None,
                kind: ModelKind::Layout,
                is_3d: false,
                entities,
                props: Default::default(),
            });
        }

        self.doc.models.push(model);
        self.doc.models.extend(layout_models);

        // Resolve IMAGE -> IMAGEDEF paths.
        let defs = std::mem::take(&mut self.imagedefs);
        let fix = |e: &mut Entity| {
            if let EntityKind::Image { path, .. } = &mut e.kind {
                if let Some(MValue::Text(h)) = e.props.get("dxf.imagedef") {
                    *path = parse_handle(h).and_then(|h| defs.get(&h)).cloned();
                }
            }
        };
        for m in &mut self.doc.models {
            m.entities.iter_mut().for_each(fix);
        }
        for b in &mut self.doc.blocks {
            b.entities.iter_mut().for_each(fix);
        }

        self.doc.warnings = std::mem::take(&mut self.ctx.warnings);
        self.doc
    }

    /// Destination of an entity from ENTITIES: `None` = model space, else paper-space key.
    fn route_auto(&self, parsed: &Parsed) -> Option<(String, String)> {
        if let Some(name) = parsed.owner.and_then(|o| self.block_records.get(&o)) {
            if is_model_space(name) {
                return None;
            }
            if is_paper_space(name) {
                return Some((name.to_ascii_lowercase(), name.clone()));
            }
        }
        if parsed.paper {
            Some(("*paper_space".to_owned(), "*Paper_Space".to_owned()))
        } else {
            None
        }
    }
}

fn skip_section(rr: &mut RecordReader) -> Result<()> {
    while let Some(rec) = rr.next_record()? {
        if matches!(rec.name.as_str(), "ENDSEC" | "EOF" | "SECTION") {
            rr.push_back(rec);
            break;
        }
    }
    Ok(())
}

fn point_nonzero_z(p: &Point3) -> bool {
    p.z != 0.0
}

fn entity_has_z(e: &Entity) -> bool {
    match &e.kind {
        EntityKind::Point { position } => point_nonzero_z(position),
        EntityKind::Line { start, end } => point_nonzero_z(start) || point_nonzero_z(end),
        EntityKind::Polyline { vertices, .. } => {
            vertices.iter().any(|v| point_nonzero_z(&v.position))
        }
        EntityKind::Spline {
            control_points,
            fit_points,
            ..
        } => control_points.iter().chain(fit_points).any(point_nonzero_z),
        EntityKind::Face { points, .. } => points.iter().any(point_nonzero_z),
        EntityKind::Mesh { .. } => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_cap_is_bounded() {
        assert_eq!(record_pair_cap(u32::MAX, usize::MAX), MAX_RECORD_PAIRS);
        assert_eq!(record_pair_cap(100, usize::MAX), 4896);
        assert_eq!(record_pair_cap(u32::MAX, 1000), 516);
    }
}
