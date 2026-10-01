//! V8 file -> `Document`.

use std::collections::{HashMap, HashSet};

use cadkit_core::{
    Block, Document, Format, Layer, LengthUnit, Model, ModelKind, Props, ReadOptions, Result,
    SourceInfo, Units, Value,
};

use crate::budget::Budget;
use crate::le;
use crate::map::{Item, Mapper, Node, Xf, length_unit, tree_v8, unit_from_label};
use crate::native::decode_v8::{self, DEP_TAG_TARGET};
use crate::native::element::{ElementData, LevelData};
use crate::native::linkage;
use crate::native::v8::{self, ModelHeader, RawElement, V8File};
use crate::text;

/// Dependency application id linking a raster frame to its attachment (type 92 -> 94).
const DEP_RASTER_FRAME: u16 = 0x271b;

pub(crate) fn read(bytes: &[u8], options: &ReadOptions) -> Result<Document> {
    let mut budget = Budget::new(options.limits);
    let enc = text::encoding_for(options.fallback_codepage.as_deref());
    let file = v8::read_with(bytes, &mut budget, enc)?;
    Ok(map_file(&file, options, enc))
}

fn items<'a>(
    elements: impl Iterator<Item = &'a RawElement>,
    enc: &'static encoding_rs::Encoding,
    options: &ReadOptions,
) -> Vec<Item<'a>> {
    elements
        .map(|r| Item {
            el: decode_v8::decode(r, enc, &options.limits),
            raw: &r.bytes,
            offset: r.offset as u64,
        })
        .collect()
}

fn map_file(file: &V8File, options: &ReadOptions, enc: &'static encoding_rs::Encoding) -> Document {
    let mut m = Mapper::new(options.keep_raw, options.limits.max_depth);
    let named = items(
        file.named_pages.iter().flat_map(|p| p.elements.iter()),
        enc,
        options,
    );

    // File-level tables.
    let mut levels: Vec<LevelData> = Vec::new();
    for it in &named {
        match &it.el.data {
            ElementData::Level(l) => levels.push(l.clone()),
            ElementData::Font { number, name } if !name.is_empty() => {
                m.fonts.insert(*number, name.clone());
            }
            ElementData::TagSet(ts) => {
                if let Some(id) = it.el.header.id {
                    m.tag_sets.insert(id, ts.clone());
                }
            }
            ElementData::ColorTable { colors } => {
                for (dst, src) in m.palette.iter_mut().zip(colors.iter()) {
                    *dst = *src;
                }
            }
            _ => {}
        }
    }
    let mut layers = Vec::new();
    let mut names: HashSet<String> = HashSet::new();
    for l in &levels {
        let base = l
            .name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("Level {}", l.id));
        let name = if names.contains(&base) {
            format!("{base} ({})", l.id)
        } else {
            base
        };
        names.insert(name.clone());
        m.levels.insert(l.id, name.clone());
        let mut props = Props::new();
        props.insert("dgn.level_flags".into(), Value::Int(i64::from(l.flags)));
        if l.parent != u32::MAX {
            props.insert("dgn.parent_level".into(), Value::Int(i64::from(l.parent)));
        }
        layers.push(Layer {
            name,
            id: Some(u64::from(l.id)),
            visible: true,
            plottable: true,
            description: l.description.clone(),
            props,
            ..Layer::default()
        });
    }

    // Units come from the first model with a header; other models are rescaled into them.
    let first = file.models.iter().find_map(|md| md.header.as_ref());
    let (units, doc_mpu) = units_of(first);

    // Shared cell definitions are file-level; their geometry is origin-relative.
    let def_xf = first
        .map_or(
            Xf {
                origin: [0.0; 3],
                scale: 1.0,
            },
            |h| xf_of(h, doc_mpu),
        )
        .relative();
    let named_tree = tree_v8(&named, options.limits.max_depth);
    let mut blocks: Vec<Block> = Vec::new();
    let mut block_names: HashSet<String> = HashSet::new();
    for node in &named_tree {
        let Some(it) = named.get(node.idx) else {
            continue;
        };
        let ElementData::SharedCellDefinition(c) = &it.el.data else {
            continue;
        };
        let base = linkage::string(&it.el.linkages, 1)
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("DGN_SHARED_CELL_{}", it.el.header.id.unwrap_or(0)));
        let name = if block_names.contains(&base) {
            format!("{base}#{}", it.el.header.id.unwrap_or(0))
        } else {
            base
        };
        block_names.insert(name.clone());
        let entities = node
            .children
            .iter()
            .filter_map(|ch| m.entity(&named, ch, &def_xf, 1))
            .collect();
        let mut props = Props::new();
        props.insert("dgn.type".into(), Value::Int(34));
        props.insert(
            "dgn.matrix".into(),
            Value::List(c.matrix.iter().map(|v| Value::Float(*v)).collect()),
        );
        blocks.push(Block {
            name,
            id: it.el.header.id,
            base_point: def_xf.pt(c.origin),
            entities,
            is_xref: false,
            xref_path: None,
            description: linkage::string(&it.el.linkages, 2),
            props,
        });
    }

    let mut models = Vec::new();
    for md in &file.models {
        let header = md.header.as_ref();
        let xf = header.map_or(
            Xf {
                origin: [0.0; 3],
                scale: 1.0,
            },
            |h| xf_of(h, doc_mpu),
        );
        // XAttributes by element id.
        m.xattributes.clear();
        for rec in md.graphic_aux.iter().flat_map(|p| p.records.iter()) {
            m.xattributes
                .entry(rec.element_id)
                .or_default()
                .push(rec.kind);
        }
        // Raster attachments: 90 holds the file, 92 links a frame (94) to a 90.
        let control = items(
            md.control_pages.iter().flat_map(|p| p.elements.iter()),
            enc,
            options,
        );
        let mut references: HashMap<u64, (Option<String>, Option<String>)> = HashMap::new();
        for it in &control {
            if let (
                ElementData::RasterReference {
                    file_name,
                    full_path,
                },
                Some(id),
            ) = (&it.el.data, it.el.header.id)
            {
                references.insert(id, (file_name.clone(), full_path.clone()));
            }
        }
        m.raster_paths.clear();
        for it in &control {
            if it.el.header.type_code != 92 {
                continue;
            }
            if let (Some(frame), Some(reference)) = (
                linkage::dependency_root(&it.el.linkages, DEP_RASTER_FRAME),
                le::u64_at(it.raw, 0x30),
            ) {
                if let Some(paths) = references.get(&reference) {
                    m.raster_paths.insert(frame, paths.clone());
                }
            }
        }

        let graphic = items(
            md.graphic_pages.iter().flat_map(|p| p.elements.iter()),
            enc,
            options,
        );
        // Frames that store the same extent corner for the same file: their extents are then
        // not image footprints (private sample: 8 frames, one file, one corner).
        m.shared_raster_extent.clear();
        let mut by_extent: HashMap<(i64, i64, Option<String>), Vec<u64>> = HashMap::new();
        for it in &graphic {
            if let (
                ElementData::RasterFrame {
                    corner: Some([cx, cy]),
                    ..
                },
                Some(id),
            ) = (&it.el.data, it.el.header.id)
            {
                let file = m
                    .raster_paths
                    .get(&id)
                    .and_then(|(n, f)| f.clone().or_else(|| n.clone()));
                by_extent
                    .entry((cx.round() as i64, cy.round() as i64, file))
                    .or_default()
                    .push(id);
            }
        }
        let shared: Vec<u64> = by_extent
            .into_values()
            .filter(|v| v.len() > 1)
            .flatten()
            .collect();
        if !shared.is_empty() {
            m.warn(
                "dgn.raster_extent_shared",
                format!(
                    "{} raster frames share one stored extent corner and file; their \
                     rectangles (as stored, equal to the element ranges) are probably not \
                     image footprints",
                    shared.len()
                ),
                None,
                None,
            );
            m.shared_raster_extent.extend(shared);
        }
        m.pending_tags.clear();
        m.attached.clear();
        for (i, it) in graphic.iter().enumerate() {
            if let ElementData::Tag(t) = &it.el.data {
                if let Some(target) = t.target.filter(|&tg| Some(tg) != it.el.header.id) {
                    let attr = m.attribute(t, &it.el.header, &xf);
                    m.pending_tags.entry(target).or_default().push((i, attr));
                    m.attached.insert(i);
                }
            }
        }
        let tree = tree_v8(&graphic, options.limits.max_depth);
        let mut entities = Vec::with_capacity(tree.len());
        for node in &tree {
            if m.attached.contains(&node.idx) {
                continue;
            }
            if let Some(e) = m.entity(&graphic, node, &xf, 0) {
                entities.push(e);
            }
        }
        // Tags whose target was not found stay visible as standalone entities.
        let mut leftovers: Vec<usize> = m
            .pending_tags
            .drain()
            .flat_map(|(_, v)| v.into_iter().map(|(i, _)| i))
            .collect();
        leftovers.sort_unstable();
        if !leftovers.is_empty() {
            let n = leftovers.len();
            m.warn(
                "dgn.tag_target_missing",
                format!("{n} tags reference elements that were not found"),
                None,
                None,
            );
        }
        for i in leftovers {
            m.attached.remove(&i);
            if let Some(e) = m.entity(
                &graphic,
                &Node {
                    idx: i,
                    children: Vec::new(),
                },
                &xf,
                0,
            ) {
                entities.push(e);
            }
        }
        let tags = graphic
            .iter()
            .filter(|it| matches!(it.el.data, ElementData::Tag(_)))
            .count();
        let linked = graphic
            .iter()
            .filter(|it| linkage::dependency_root(&it.el.linkages, DEP_TAG_TARGET).is_some())
            .count();

        let index = md.index_entry.as_ref();
        let name = index
            .map(|e| e.name.clone())
            .filter(|n| !n.is_empty())
            .or_else(|| header.and_then(|h| h.name.clone()))
            .unwrap_or_else(|| md.storage.clone());
        let mut props = Props::new();
        props.insert("dgn.storage".into(), Value::Text(md.storage.clone()));
        if let Some(e) = index {
            props.insert(
                "dgn.model_number".into(),
                Value::Int(i64::from(e.model_number)),
            );
            props.insert(
                "dgn.model_index_flags".into(),
                Value::Int(i64::from(e.flags)),
            );
            if !e.description.is_empty() {
                props.insert("dgn.description".into(), Value::Text(e.description.clone()));
            }
        }
        if let Some(h) = header {
            props.insert("dgn.uor_per_master".into(), Value::Float(h.uor_per_master));
            props.insert(
                "dgn.global_origin".into(),
                Value::List(h.global_origin.iter().map(|v| Value::Float(*v)).collect()),
            );
            props.insert(
                "dgn.model_type_word".into(),
                Value::Int(i64::from(h.type_word)),
            );
            if let Some(l) = &h.master.label {
                props.insert("dgn.master_unit".into(), Value::Text(l.clone()));
            }
            if let Some(l) = &h.sub.label {
                props.insert("dgn.sub_unit".into(), Value::Text(l.clone()));
            }
            if (xf.scale * h.uor_per_master - 1.0).abs() > 1e-12 {
                props.insert(
                    "dgn.rescaled_to_document_units".into(),
                    Value::Float(xf.scale * h.uor_per_master),
                );
            }
        }
        props.insert("dgn.element_count".into(), Value::Int(graphic.len() as i64));
        props.insert("dgn.tag_count".into(), Value::Int(tags as i64));
        props.insert("dgn.tags_with_target".into(), Value::Int(linked as i64));
        props.insert(
            "dgn.control_element_count".into(),
            Value::Int(control.len() as i64),
        );
        let is_3d = header.map_or_else(|| graphic.iter().any(|it| it.el.header.is_3d), |h| h.is_3d);
        let id = md.storage.trim_start_matches('#').parse::<u64>().ok();
        models.push(Model {
            name,
            id,
            kind: ModelKind::Model,
            is_3d,
            entities,
            props,
        });
    }

    // Layers for levels referenced without a table entry.
    let declared: HashSet<u32> = levels.iter().map(|l| l.id).collect();
    for id in m.used_levels.clone() {
        if declared.contains(&id) {
            continue;
        }
        let name = m.level_name(id);
        if names.insert(name.clone()) {
            layers.push(Layer {
                name,
                id: Some(u64::from(id)),
                visible: true,
                plottable: true,
                ..Layer::default()
            });
        }
    }

    let mut props = Props::new();
    props.insert(
        "dgn.cfb_version".into(),
        Value::Int(i64::from(file.cfb_version)),
    );
    props.insert(
        "dgn.stream_count".into(),
        Value::Int(file.streams.len() as i64),
    );
    props.insert(
        "dgn.named_element_count".into(),
        Value::Int(named.len() as i64),
    );
    props.insert(
        "dgn.tag_set_count".into(),
        Value::Int(m.tag_sets.len() as i64),
    );
    if let Some(sets) = m.tag_sets_value() {
        props.insert("dgn.tag_sets".into(), sets);
    }
    if let Some(cp) = file.summary.codepage {
        props.insert("dgn.summary_codepage".into(), Value::Int(i64::from(cp)));
    }
    if let Some(h) = first {
        if let (Some(mm), Some(sm)) = (h.master.meters(), h.sub.meters()) {
            props.insert("dgn.sub_units_per_master".into(), Value::Float(mm / sm));
        }
    }
    let text_styles = m.text_styles();
    let linetypes = m.linetypes();
    let mut warnings = file.warnings.clone();
    warnings.extend(m.finish_warnings());
    Document {
        source: SourceInfo {
            format: Format::DgnV8,
            version: "V8".into(),
            application: file.summary.application.clone(),
            codepage: Some(enc.name().to_ascii_lowercase()),
        },
        units,
        layers,
        linetypes,
        text_styles,
        blocks,
        models,
        props,
        warnings,
    }
}

/// Document units from a model header: the master unit definition, else its label.
fn units_of(h: Option<&ModelHeader>) -> (Units, Option<f64>) {
    let Some(h) = h else {
        return (Units::default(), None);
    };
    if let Some(m) = h.master.meters() {
        return (
            Units {
                unit: length_unit(m),
                meters_per_unit: Some(m),
            },
            Some(m),
        );
    }
    if let Some((unit, m)) = h.master.label.as_deref().and_then(unit_from_label) {
        return (
            Units {
                unit,
                meters_per_unit: Some(m),
            },
            Some(m),
        );
    }
    (
        Units {
            unit: LengthUnit::Unitless,
            meters_per_unit: None,
        },
        None,
    )
}

/// UOR -> document units for one model.
fn xf_of(h: &ModelHeader, doc_mpu: Option<f64>) -> Xf {
    let mut scale = 1.0 / h.uor_per_master;
    if let (Some(doc), Some(model)) = (doc_mpu, h.master.meters()) {
        let factor = model / doc;
        if factor.is_finite() && factor > 0.0 {
            scale *= factor;
        }
    }
    Xf {
        origin: h.global_origin,
        scale,
    }
}

#[cfg(test)]
mod tests {
    use cadkit_core::{Color, EntityKind, ReadOptions};

    use crate::cfb::tests::build;
    use crate::native::decode_v8::tests::{f64s, header};
    use crate::native::v8::tests::{element, model_header_stream, page};
    use crate::zlib;

    fn string_linkage(id: u32, text: &str) -> Vec<u8> {
        let mut payload = text.as_bytes().to_vec();
        payload.push(0);
        let total = (12 + payload.len()).div_ceil(2) * 2;
        let mut l = vec![(total / 2 - 1) as u8, 0x10, 0xd2, 0x56];
        l.extend_from_slice(&id.to_le_bytes());
        l.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        l.extend_from_slice(&payload);
        l.resize(total, 0);
        l
    }

    fn dependency(app: u16, root_type: u8, id: u64) -> Vec<u8> {
        let mut l = vec![0, 0x10, 0xd0, 0x56];
        l.extend_from_slice(&app.to_le_bytes());
        l.extend_from_slice(&[1, 0, 0, root_type, 1, 0]);
        if root_type != 2 {
            l.extend_from_slice(&[0; 8]);
        }
        l.extend_from_slice(&id.to_le_bytes());
        l.extend_from_slice(&[0; 4]);
        l[0] = (l.len() / 2 - 1) as u8;
        l
    }

    /// Element body from 0x0c for non-graphic records: level/table slot, id, then `rest`.
    fn record(slot: u32, id: u64, rest_from_0x20: &[u8]) -> Vec<u8> {
        let mut b = slot.to_le_bytes().to_vec();
        b.extend_from_slice(&id.to_le_bytes());
        b.extend_from_slice(&[0; 8]);
        b.extend_from_slice(rest_from_0x20);
        b
    }

    fn synthetic_v8() -> Vec<u8> {
        // Model index with one model "Model A".
        let mut mix = Vec::new();
        for v in [0xaa00_ba11u32, 4, 1, 0] {
            mix.extend_from_slice(&v.to_le_bytes());
        }
        let name: Vec<u8> = "Model A"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        mix.extend_from_slice(&0x0001_0000u32.to_le_bytes());
        mix.extend_from_slice(&0u32.to_le_bytes());
        mix.extend_from_slice(&0u64.to_le_bytes());
        mix.extend_from_slice(&((32 + name.len()) as u16).to_le_bytes());
        mix.extend_from_slice(&(name.len() as u16).to_le_bytes());
        mix.extend_from_slice(&[0; 12]);
        mix.extend_from_slice(&name);

        // A 2D line on level 5, color 3, and a hidden tag pointing at it.
        let mut line = header(5, 10, 0, 3);
        line.extend(f64s(&[5.0, 5.0, 1005.0, 2005.0]));
        let mut tag = header(5, 11, 0x0080, 0);
        tag.resize(0x140 - 0x0c, 0);
        tag[0xd0 - 0x0c..0xd4 - 0x0c].copy_from_slice(&[1, 0, 1, 0]);
        tag[0x138 - 0x0c..0x13c - 0x0c].copy_from_slice(&4u32.to_le_bytes());
        tag.extend_from_slice(b"101\0");
        let mut links = dependency(0x2717, 2, 20);
        links.extend(dependency(0x2710, 10, 10));
        let graphic = page(&[element(3, &line, &[]), element(37, &tag, &links)]);

        // File-level tables: level table with "Walls" (id 5) and tag set "Room".
        let table = element(0x2000_0060, &record(1, 30, &1u32.to_le_bytes()), &[]);
        let mut lvl = 5u32.to_le_bytes().to_vec();
        lvl.extend_from_slice(&u32::MAX.to_le_bytes());
        lvl.resize(0x48, 0);
        let level = element(
            0x4000_005f,
            &record(1, 31, &lvl),
            &string_linkage(1, "Walls"),
        );
        let mut defs = Vec::new();
        defs.extend_from_slice(b"Number");
        defs.push(0);
        defs.extend_from_slice(&1u16.to_le_bytes());
        defs.push(0);
        defs.extend_from_slice(&1u16.to_le_bytes());
        defs.extend_from_slice(&[0; 5]);
        defs.push(0);
        let mut set = vec![0u8; 8];
        set.extend_from_slice(b"teSt");
        set.extend_from_slice(&[0; 8]);
        set.extend_from_slice(&(defs.len() as u32).to_le_bytes());
        set.extend_from_slice(&(defs.len() as u32).to_le_bytes());
        set.extend_from_slice(&defs);
        let tagset = element(
            0x2000_0027,
            &record(0, 20, &set),
            &string_linkage(1, "Room"),
        );
        let named = page(&[table, level, tagset]);

        let mut h = vec![0u8; 0x14];
        h.extend_from_slice(&zlib::compress(&[0u8; 64]));
        build(&[
            ("Dgn~H", h),
            ("Dgn~S", vec![0xff; 32]),
            ("Dgn^Ix/Dgn~Mix", zlib::compress(&mix)),
            (
                "Dgn-Md/#000000/Dgn~Mh",
                model_header_stream(false, 1000.0, "Model A"),
            ),
            ("Dgn-Md/#000000/Dgn^G/$1", graphic),
            ("Dgn^Nm/$1", named),
        ])
    }

    #[test]
    fn synthetic_file_maps_levels_tags_and_units() {
        let bytes = synthetic_v8();
        assert!(crate::sniff(&bytes));
        let doc = crate::read(&bytes, &ReadOptions::default()).unwrap();
        assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
        assert_eq!(doc.units.meters_per_unit, Some(1.0));
        assert_eq!(doc.models.len(), 1);
        let m = &doc.models[0];
        assert_eq!(m.name, "Model A");
        assert!(!m.is_3d);
        assert_eq!(m.entities.len(), 1, "the tag is attached, not drawn");
        let line = &m.entities[0];
        assert_eq!(line.layer.as_deref(), Some("Walls"));
        assert_eq!(line.color, Color::Rgb { r: 255, g: 0, b: 0 });
        // (uor - global origin 5) / 1000 UOR per master.
        let EntityKind::Line { start, end } = line.kind else {
            panic!("{:?}", line.kind)
        };
        assert_eq!((start.x, start.y, end.x, end.y), (0.0, 0.005, 1.0, 2.005));
        assert_eq!(line.attributes.len(), 1);
        let a = &line.attributes[0];
        assert_eq!(a.tag, "Number");
        assert_eq!(a.set.as_deref(), Some("Room"));
        assert_eq!(a.value, cadkit_core::Value::Text("101".into()));
        assert!(a.invisible);
        assert!(
            doc.layers
                .iter()
                .any(|l| l.name == "Walls" && l.id == Some(5))
        );
    }

    #[test]
    fn repeated_ids_do_not_amplify_xattributes() {
        // 40 lines that all claim element id 10, and 200 XAttribute records for id 10.
        let mut line = header(5, 10, 0, 3);
        line.extend(f64s(&[0.0, 0.0, 10.0, 10.0]));
        let lines: Vec<Vec<u8>> = (0..40).map(|_| element(3, &line, &[])).collect();
        let mut aux = Vec::new();
        for k in 0..200u32 {
            aux.extend_from_slice(&0xa11b_u32.to_le_bytes());
            aux.extend_from_slice(&4u32.to_le_bytes());
            aux.extend_from_slice(&(0xaa + k).to_le_bytes());
            aux.extend_from_slice(&0u32.to_le_bytes());
            aux.extend_from_slice(&10u64.to_le_bytes());
            aux.extend_from_slice(&0u32.to_le_bytes());
            aux.extend_from_slice(&[1, 2, 3, 4]);
        }
        let mut aux_page = Vec::new();
        for v in [200u32, 2, 1, 200] {
            aux_page.extend_from_slice(&v.to_le_bytes());
        }
        aux_page.extend_from_slice(&zlib::compress(&aux));
        let mut h = vec![0u8; 0x14];
        h.extend_from_slice(&zlib::compress(&[0u8; 64]));
        let bytes = build(&[
            ("Dgn~H", h),
            (
                "Dgn-Md/#000000/Dgn~Mh",
                model_header_stream(false, 1000.0, "M"),
            ),
            ("Dgn-Md/#000000/Dgn^G/$1", page(&lines)),
            ("Dgn-Md/#000000/Dgn^GA/$1", aux_page),
        ]);
        let doc = crate::read(&bytes, &ReadOptions::default()).unwrap();
        let entities = &doc.models[0].entities;
        assert_eq!(entities.len(), 40);
        let carrying: Vec<_> = entities
            .iter()
            .filter(|e| e.props.contains_key("dgn.xattribute_count"))
            .collect();
        assert_eq!(carrying.len(), 1, "attached once per id");
        let e = carrying[0];
        assert_eq!(
            e.props.get("dgn.xattribute_count"),
            Some(&cadkit_core::Value::Int(200))
        );
        let Some(cadkit_core::Value::List(kinds)) = e.props.get("dgn.xattribute_kinds") else {
            panic!()
        };
        assert_eq!(kinds.len(), 64, "kind list is capped");
    }

    #[test]
    fn other_compound_files_are_not_dgn() {
        let doc = build(&[
            ("WordDocument", vec![1, 2, 3]),
            ("\u{5}SummaryInformation", vec![0; 48]),
        ]);
        assert!(!crate::sniff(&doc));
        assert!(crate::read(&doc, &ReadOptions::default()).is_err());
    }
}
