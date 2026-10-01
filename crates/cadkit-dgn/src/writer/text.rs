//! Inverse of the reader's DGN text anchor mapping.

use cadkit_core::{Entity, Error, HAlign, Result, VAlign, Value};

pub(super) fn justification(e: &Entity, h: HAlign, v: VAlign) -> Result<(u16, f64, f64)> {
    let horizontal = match h {
        HAlign::Left => 0,
        HAlign::Center => 6,
        HAlign::Right => 12,
        _ => {
            return Err(Error::Unsupported(
                "DGN fitted/middle text alignment".into(),
            ));
        }
    };
    let vertical = match v {
        VAlign::Top => 0,
        VAlign::Middle => 1,
        VAlign::Baseline => 2,
        VAlign::Bottom => {
            return Err(Error::Unsupported(
                "DGN descender-relative bottom text alignment".into(),
            ));
        }
    };
    // Margin variants decode to the same neutral alignment. Retain the source
    // variant only while it still agrees with the edited neutral alignment.
    let code = match e.props.get("dgn.justification") {
        Some(Value::Int(raw)) => u16::try_from(*raw).ok().filter(|&code| {
            let (source_h, source_v, _, _) = crate::map::justification(code);
            code <= 14 && source_h == h && source_v == v
        }),
        _ => None,
    }
    .unwrap_or(horizontal + vertical);
    let (_, _, fx, fy) = crate::map::justification(code);
    Ok((code, fx, fy))
}

pub(super) fn length(e: &Entity, value: &str, char_width: f64, fx: f64) -> Result<f64> {
    let length = match e.props.get("dgn.text_length") {
        Some(Value::Float(v)) => *v,
        Some(Value::Int(v)) => *v as f64,
        Some(_) => return Err(Error::invalid(0, "DGN text length must be numeric")),
        None if fx != 0.0 => {
            return Err(Error::Unsupported(
                "center/right DGN text needs measured dgn.text_length in drawing units".into(),
            ));
        }
        None => value.chars().count() as f64 * char_width,
    };
    if !length.is_finite() || length < 0.0 || (length == 0.0 && !value.is_empty()) {
        return Err(Error::invalid(0, "invalid DGN text length"));
    }
    Ok(length)
}
