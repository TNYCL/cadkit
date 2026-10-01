//! V7 file -> `Document`.

use std::collections::BTreeSet;

use cadkit_core::{
    Block, Document, Format, Layer, LengthUnit, Model, ModelKind, Props, ReadOptions, Result,
    SourceInfo, Units, Value,
};

use crate::budget::Budget;
use crate::map::{Item, Mapper, Xf, tree_v7, unit_from_label};
use crate::native::element::ElementData;
use crate::native::v7::{self, V7File, description_end};
use crate::text;

pub(crate) fn read(bytes: &[u8], options: &ReadOptions) -> Result<Document> {
    let mut budget = Budget::new(options.limits);
    let enc = text::encoding_for(options.fallback_codepage.as_deref());
    let file = v7::read_with(bytes, &mut budget, enc)?;
    Ok(map_file(&file, options, enc))
}

/// Records that are design data rather than drawn elements.
fn is_control(item: &Item<'_>) -> bool {
    let h = &item.el.header;
    !h.has_display_header
        || matches!(h.type_code, 5 | 8 | 66)
        || matches!(
            item.el.data,
            ElementData::Tcb(_) | ElementData::ColorTable { .. } | ElementData::TagSet(_)
        )
}

fn map_file(file: &V7File, options: &ReadOptions, enc: &'static encoding_rs::Encoding) -> Document {
    let mut m = Mapper::new(options.keep_raw, options.limits.max_depth);
    let mut warnings = file.warnings.clone();

    let all: Vec<(Item<'_>, Option<u64>)> = file
        .records
        .iter()
        .map(|r| {
            let item = Item {
                el: v7::decode(r, file.is_3d, enc, &options.limits),
                raw: &r.bytes,
                offset: r.offset as u64,
            };
            (item, description_end(r).map(|e| e as u64))
        })
        .collect();
    let deleted = all.iter().filter(|(i, _)| i.el.header.deleted).count();

    // Tables: the last color table wins (GDAL dgnlib), tag sets by set number.
    for (it, _) in &all {
        match &it.el.data {
            ElementData::ColorTable { colors } if !it.el.header.deleted => {
                for (dst, src) in m.palette.iter_mut().zip(colors.iter()) {
                    *dst = *src;
                }
            }
            ElementData::TagSet(ts) => {
                if let Some(n) = ts.number {
                    m.tag_sets.insert(u64::from(n), ts.clone());
                }
            }
            _ => {}
        }
    }

    // Units: UOR per master = sub-units per master * UOR per sub-unit.
    let tcb = file.tcb.as_ref();
    let upm = tcb
        .map(|t| f64::from(t.subunits_per_master) * f64::from(t.uor_per_subunit))
        .filter(|v| v.is_finite() && *v > 0.0);
    if upm.is_none() {
        warnings.push(cadkit_core::Warning {
            code: "dgn.v7_units".into(),
            message: "design header has no usable units; coordinates are in UOR".into(),
            offset: None,
            object: None,
        });
    }
    let xf = Xf {
        origin: tcb.map_or([0.0; 3], |t| t.origin),
        scale: upm.map_or(1.0, |u| 1.0 / u),
    };
    let units = match tcb.and_then(|t| unit_from_label(&t.master_label)) {
        Some((unit, meters)) => Units {
            unit,
            meters_per_unit: Some(meters),
        },
        None => Units {
            unit: LengthUnit::Unitless,
            meters_per_unit: None,
        },
    };

    // Drawn records only: deleted and control records do not take part in the hierarchy.
    let (items, ends): (Vec<Item<'_>>, Vec<Option<u64>>) = all
        .into_iter()
        .filter(|(i, _)| !i.el.header.deleted && !is_control(i))
        .unzip();
    let tree = tree_v7(&items, &ends, options.limits.max_depth);

    let mut blocks = Vec::new();
    let mut entities = Vec::new();
    for node in &tree {
        let Some(it) = items.get(node.idx) else {
            continue;
        };
        if let ElementData::SharedCellDefinition(c) = &it.el.data {
            let rel = xf.relative();
            let name = c
                .name
                .clone()
                .unwrap_or_else(|| format!("DGN_SHARED_CELL_{}", it.offset));
            let entities = node
                .children
                .iter()
                .filter_map(|ch| m.entity(&items, ch, &rel, 1))
                .collect();
            let mut props = Props::new();
            props.insert("dgn.type".into(), Value::Int(34));
            blocks.push(Block {
                name,
                base_point: rel.pt(c.origin),
                entities,
                props,
                ..Block::default()
            });
            continue;
        }
        if let Some(e) = m.entity(&items, node, &xf, 0) {
            entities.push(e);
        }
    }

    let levels: BTreeSet<u32> = m.used_levels.clone();
    let layers = levels
        .into_iter()
        .map(|l| Layer {
            name: m.level_name(l),
            id: Some(u64::from(l)),
            visible: true,
            plottable: true,
            ..Layer::default()
        })
        .collect();

    let mut model_props = Props::new();
    if let Some(u) = upm {
        model_props.insert("dgn.uor_per_master".into(), Value::Float(u));
    }
    if let Some(t) = tcb {
        model_props.insert(
            "dgn.master_unit".into(),
            Value::Text(t.master_label.clone()),
        );
        model_props.insert("dgn.sub_unit".into(), Value::Text(t.sub_label.clone()));
        model_props.insert(
            "dgn.sub_units_per_master".into(),
            Value::Int(i64::from(t.subunits_per_master)),
        );
        model_props.insert(
            "dgn.global_origin".into(),
            Value::List(t.origin.iter().map(|v| Value::Float(*v)).collect()),
        );
    }
    model_props.insert("dgn.element_count".into(), Value::Int(items.len() as i64));
    let mut props = Props::new();
    props.insert(
        "dgn.record_count".into(),
        Value::Int(file.records.len() as i64),
    );
    props.insert("dgn.deleted_count".into(), Value::Int(deleted as i64));
    props.insert("dgn.end_offset".into(), Value::Int(file.end_offset as i64));
    if let Some(sets) = m.tag_sets_value() {
        props.insert("dgn.tag_sets".into(), sets);
    }
    let text_styles = m.text_styles();
    let linetypes = m.linetypes();
    warnings.extend(m.finish_warnings());
    Document {
        source: SourceInfo {
            format: Format::DgnV7,
            version: "V7".into(),
            application: None,
            codepage: Some(enc.name().to_ascii_lowercase()),
        },
        units,
        layers,
        linetypes,
        text_styles,
        blocks,
        models: vec![Model {
            name: "Default".into(),
            id: Some(0),
            kind: ModelKind::Model,
            is_3d: file.is_3d,
            entities,
            props: model_props,
        }],
        props,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use cadkit_core::{EntityKind, GroupKind, ReadOptions};

    use crate::native::v7::tests::{disp, mid, record};

    fn tcb() -> Vec<u8> {
        let mut t = record(9, 8, &[0u8; 1532]);
        t[1112..1116].copy_from_slice(&mid(10));
        t[1116..1120].copy_from_slice(&mid(100));
        t[1120..1124].copy_from_slice(b"m mm");
        t
    }

    fn line(x0: i32, y0: i32, x1: i32, y1: i32, complex: bool) -> Vec<u8> {
        let mut body = disp(2, 1, 0);
        for v in [x0, y0, x1, y1] {
            body.extend(mid(v));
        }
        let mut r = record(3, 7, &body);
        if complex {
            r[0] |= 0x80;
        }
        r
    }

    #[test]
    fn complex_chain_groups_its_components() {
        let a = line(0, 0, 1000, 0, true);
        let b = line(1000, 0, 1000, 1000, true);
        let mut head = disp(0, 0, 0);
        head.extend(&0u16.to_le_bytes());
        head.extend(&2u16.to_le_bytes());
        head.extend([0u8; 8]);
        let mut chain = record(12, 7, &head);
        // The description length counts words after byte 38 of the header.
        let words = ((chain.len() - 38 + a.len() + b.len()) / 2) as u16;
        chain[36..38].copy_from_slice(&words.to_le_bytes());
        let mut file = tcb();
        file.extend(chain);
        file.extend(a);
        file.extend(b);
        file.extend(line(0, 0, 0, 500, false));
        file.extend([0xff, 0xff]);

        let doc = crate::read(&file, &ReadOptions::default()).unwrap();
        assert_eq!(doc.units.unit, cadkit_core::LengthUnit::Meter);
        let m = &doc.models[0];
        assert_eq!(m.entities.len(), 2, "{:?}", m.entities);
        let EntityKind::Group {
            group_kind,
            children,
            ..
        } = &m.entities[0].kind
        else {
            panic!()
        };
        assert_eq!(*group_kind, GroupKind::ComplexChain);
        assert_eq!(children.len(), 2);
        let EntityKind::Line { end, .. } = children[1].kind else {
            panic!()
        };
        assert_eq!((end.x, end.y), (1.0, 1.0));
        assert_eq!(m.entities[0].layer.as_deref(), Some("Level 7"));
    }
}
