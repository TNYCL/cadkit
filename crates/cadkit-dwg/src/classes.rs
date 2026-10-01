//! Classes section (ODA spec chapter 10 and §5.8).

use cadkit_core::{Error, Result};

use crate::bits::BitReader;
use crate::container::{Section, sentinels};
use crate::native::DwgClass;
use crate::object::{DecodeContext, Streams, locate_string_stream};

/// Smallest possible class record in bits (two `BS` zeros, three empty strings,
/// a bit and a `BS`); stops the loop from spinning on padding.
const MIN_CLASS_BITS: u64 = 13;

/// Decodes the class list. Records that fail to decode end the list with an error
/// message; the classes read so far are kept.
pub fn parse(
    section: &Section<'_>,
    maintenance: u8,
    ctx: DecodeContext,
    max_classes: u64,
) -> Result<(Vec<DwgClass>, Option<String>, Option<bool>)> {
    let v = ctx.version;
    let data: &[u8] = &section.data;
    let mut r = BitReader::with_base(data, section.base);
    r.sentinel(&sentinels::CLASSES_START)?;
    let size = u64::from(r.rl()?);
    if (v.r2010_plus() && maintenance > 3) || v.r2018_plus() {
        let _high = r.rl()?;
    }
    let start = r.bit_pos();
    let end = size
        .checked_mul(8)
        .and_then(|b| start.checked_add(b))
        .filter(|&e| e <= r.end_bit())
        .ok_or_else(|| {
            Error::invalid(
                section.base.saturating_add(16),
                format!("class data size {size} exceeds the section"),
            )
        })?;
    let mut main = r.range(start, end);
    let mut strings = None;
    let mut data_end = end;
    if v.r2007_plus() {
        let bits = u64::from(main.rl()?);
        let flag = start
            .checked_add(bits)
            .and_then(|b| b.checked_sub(1))
            .filter(|&f| f < end && f > start)
            .ok_or_else(|| main.invalid(format!("class bit size {bits}")))?;
        let located = locate_string_stream(&main, start, flag)?;
        strings = located.map(|(a, b)| main.range(a, b));
        data_end = located.map_or(flag, |(a, _)| a);
        main.set_end_bit(data_end);
    }
    let mut s = Streams::new(main, strings, None, ctx);
    if v.r2004_plus() {
        // BS maximum class number, RC 0, RC 0, B true. (ACadSharp reads BL + B for
        // R2007+, which is the same bit layout for class numbers ≥ 256.)
        let _max_class = s.main.bs()?;
        let _zero = s.main.rc()?;
        let _zero = s.main.rc()?;
        let _flag = s.main.b()?;
    }

    let mut classes = Vec::new();
    let mut error = None;
    while s.main.bit_pos() + MIN_CLASS_BITS <= data_end {
        if classes.len() as u64 >= max_classes {
            error = Some("class count limit reached".to_owned());
            break;
        }
        match read_class(&mut s) {
            Ok(c) => classes.push(c),
            Err(e) => {
                error = Some(e.to_string());
                break;
            }
        }
    }
    let trailer = crate::header::trailer_ok(data, end, &sentinels::CLASSES_END);
    Ok((classes, error, trailer))
}

fn read_class(s: &mut Streams<'_>) -> Result<DwgClass> {
    let mut c = DwgClass {
        number: s.main.bs()? as u16,
        proxy_flags: s.main.bs()? as u16,
        app_name: s.tv()?,
        cpp_class_name: s.tv()?,
        dxf_name: s.tv()?,
        was_zombie: s.main.b()?,
        item_class_id: s.main.bs()? as u16,
        ..DwgClass::default()
    };
    if s.ctx.version.r2004_plus() {
        c.instance_count = Some(s.main.bl()?);
        c.dwg_version = Some(s.main.bl()?);
        c.maintenance_version = Some(s.main.bl()?);
        let _unknown1 = s.main.bl()?;
        let _unknown2 = s.main.bl()?;
    }
    Ok(c)
}
