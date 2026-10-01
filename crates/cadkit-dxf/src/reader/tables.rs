//! TABLES section entries: LAYER, LTYPE, STYLE.

use cadkit_core::{Color, Layer, Linetype, Lineweight, Props, TextStyle, Value as MValue};

use super::common::{handle, lineweight, normalize_linetype, xdata_props};
use super::records::{Group, Record};

pub(crate) fn layer(rec: &Record) -> Layer {
    let g = Group(rec.body());
    let flags = g.i64_or(70, 0);
    let aci = g.i64_or(62, 7);
    let color = match g.i64(420) {
        Some(tc) => {
            let v = tc as u32;
            Color::Rgb {
                r: (v >> 16) as u8,
                g: (v >> 8) as u8,
                b: v as u8,
            }
        }
        None => match aci.unsigned_abs() {
            i @ 1..=255 => Color::Aci { index: i as u8 },
            _ => Color::Aci { index: 7 },
        },
    };
    let mut props = Props::new();
    props.insert("dxf.flags".into(), MValue::Int(flags));
    xdata_props(rec, &mut props);
    Layer {
        name: g.string(2).unwrap_or_default(),
        id: handle(g),
        color,
        linetype: normalize_linetype(g.string(6)),
        lineweight: if g.value(370).is_some() {
            lineweight(g)
        } else {
            Lineweight::Default
        },
        // A negative color means "layer off".
        visible: aci >= 0,
        frozen: flags & 1 != 0,
        locked: flags & 4 != 0,
        plottable: g.i64_or(290, 1) != 0,
        description: None,
        props,
    }
}

pub(crate) fn linetype(rec: &Record) -> Option<Linetype> {
    let g = Group(rec.body());
    let name = g.string(2)?;
    let mut props = Props::new();
    if let Some(len) = g.f64(40) {
        props.insert("dxf.pattern_length".into(), MValue::Float(len));
    }
    xdata_props(rec, &mut props);
    Some(Linetype {
        name,
        description: g.string(3).filter(|s| !s.is_empty()),
        pattern: g.all(49).filter_map(|v| v.as_f64()).collect(),
        props,
    })
}

pub(crate) fn text_style(rec: &Record) -> Option<TextStyle> {
    let g = Group(rec.body());
    let name = g.string(2).filter(|s| !s.is_empty())?;
    let mut props = Props::new();
    if let Some(v) = g.string(4).filter(|s| !s.is_empty()) {
        props.insert("dxf.bigfont".into(), MValue::Text(v));
    }
    if let Some(v) = g.i64(70) {
        props.insert("dxf.flags".into(), MValue::Int(v));
    }
    if let Some(v) = g.i64(71) {
        props.insert("dxf.text_generation_flags".into(), MValue::Int(v));
    }
    if let Some(v) = g.f64(42) {
        props.insert("dxf.last_height".into(), MValue::Float(v));
    }
    xdata_props(rec, &mut props);
    Some(TextStyle {
        name,
        font: g.string(3).filter(|s| !s.is_empty()),
        height: g.f64_or(40, 0.0),
        width_factor: g.f64_or(41, 1.0),
        oblique: g.f64_or(50, 0.0).to_radians(),
        props,
    })
}
