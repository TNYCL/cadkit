//! Pieces shared by table and entity decoding: colors, line weights, handles, xdata.

use cadkit_core::{Color, Lineweight, Point3, Props, Value as MValue};

use super::records::{Group, Record, parse_handle};
use crate::native::{Pair, Value};

pub(crate) fn model_value(v: &Value) -> MValue {
    match v {
        Value::Str(s) => MValue::Text(s.clone()),
        Value::Int(i) => MValue::Int(*i),
        Value::Float(f) => MValue::Float(*f),
        Value::Bool(b) => MValue::Bool(*b),
        Value::Bytes(b) => MValue::Bytes(b.clone()),
    }
}

pub(crate) fn point_value(p: Point3) -> MValue {
    MValue::List(vec![
        MValue::Float(p.x),
        MValue::Float(p.y),
        MValue::Float(p.z),
    ])
}

/// Entity color from group 62 (ACI, 0 = ByBlock, 256 = ByLayer) and 420 (true color).
pub(crate) fn color(g: Group) -> Color {
    if let Some(tc) = g.i64(420) {
        let v = tc as u32;
        return Color::Rgb {
            r: (v >> 16) as u8,
            g: (v >> 8) as u8,
            b: v as u8,
        };
    }
    match g.i64(62) {
        Some(0) => Color::ByBlock,
        Some(i @ 1..=255) => Color::Aci { index: i as u8 },
        Some(i @ -255..=-1) => Color::Aci { index: (-i) as u8 },
        _ => Color::ByLayer,
    }
}

/// Line weight from group 370 (hundredths of a millimetre; -1 ByLayer, -2 ByBlock, -3 default).
pub(crate) fn lineweight(g: Group) -> Lineweight {
    match g.i64(370) {
        Some(-2) => Lineweight::ByBlock,
        Some(-3) => Lineweight::Default,
        Some(v) if v >= 0 => Lineweight::Millimeters(v as f64 / 100.0),
        _ => Lineweight::ByLayer,
    }
}

pub(crate) fn handle(g: Group) -> Option<u64> {
    g.text(5)
        .and_then(parse_handle)
        .or_else(|| g.text(105).and_then(parse_handle))
}

/// Extended data (`1001` groups) to props `dxf.xdata.<APPID>`: a list of `[code, value]` lists.
pub(crate) fn xdata_props(rec: &Record, props: &mut Props) {
    xdata_pairs_to_props(rec.xdata(), props);
}

pub(crate) fn xdata_pairs_to_props(pairs: &[Pair], props: &mut Props) {
    let mut app: Option<String> = None;
    let mut items: Vec<MValue> = Vec::new();
    let flush = |app: &mut Option<String>, items: &mut Vec<MValue>, props: &mut Props| {
        if let Some(name) = app.take() {
            props.insert(
                format!("dxf.xdata.{name}"),
                MValue::List(std::mem::take(items)),
            );
        }
        items.clear();
    };
    for p in pairs {
        if p.code == 1001 {
            flush(&mut app, &mut items, props);
            app = Some(p.value.as_str().unwrap_or("").to_owned());
        } else if app.is_some() {
            items.push(MValue::List(vec![
                MValue::Int(i64::from(p.code)),
                model_value(&p.value),
            ]));
        }
    }
    flush(&mut app, &mut items, props);
}

/// Entity linetype from group 6: `BYLAYER` (any case) becomes `None`, `BYBLOCK` is spelled
/// `ByBlock`, other names are kept as written.
pub(crate) fn normalize_linetype(name: Option<String>) -> Option<String> {
    let n = name?;
    if n.eq_ignore_ascii_case("bylayer") || n.is_empty() {
        None
    } else if n.eq_ignore_ascii_case("byblock") {
        Some("ByBlock".to_owned())
    } else {
        Some(n)
    }
}
