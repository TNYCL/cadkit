//! Object type registry: type code (fixed codes < 500) or class DXF name (codes
//! ≥ 500) → type name, entity/object kind and type-specific decoder.
//!
//! **Extension point.** To support a new object type, write a decoder
//! `fn(&mut Streams) -> Result<ObjectData>` that reads the type-specific fields from
//! `s.main` (data), `s.tv()` (strings) and `s.h()` (handles) — common data has
//! already been consumed — add an [`ObjectData`] variant for its result, and register
//! it in [`fixed_decoder`] or [`class_decoder`]. The mapping to the neutral model
//! lives in `crate::mapping::map_entity`.

use cadkit_core::Result;

use super::{Streams, annotation, curves, entities, hatch, misc, tables};
use crate::native::{ClassTable, ObjectData};

/// A type-specific decoder. It runs after the common object/entity data.
pub type Decoder = fn(&mut Streams<'_>) -> Result<ObjectData>;

/// What the registry knows about a type code.
#[derive(Debug, Clone)]
pub struct TypeInfo {
    /// Type name (spec name or class DXF name).
    pub name: String,
    /// Objects of this type carry common entity data.
    pub entity: bool,
    /// Type-specific decoder, if implemented.
    pub decoder: Option<Decoder>,
}

/// Name and entity flag of the fixed type codes (spec §20.3).
pub fn fixed_type(code: u16) -> Option<(&'static str, bool)> {
    Some(match code {
        0x01 => ("TEXT", true),
        0x02 => ("ATTRIB", true),
        0x03 => ("ATTDEF", true),
        0x04 => ("BLOCK", true),
        0x05 => ("ENDBLK", true),
        0x06 => ("SEQEND", true),
        0x07 => ("INSERT", true),
        0x08 => ("MINSERT", true),
        0x0A => ("VERTEX_2D", true),
        0x0B => ("VERTEX_3D", true),
        0x0C => ("VERTEX_MESH", true),
        0x0D => ("VERTEX_PFACE", true),
        0x0E => ("VERTEX_PFACE_FACE", true),
        0x0F => ("POLYLINE_2D", true),
        0x10 => ("POLYLINE_3D", true),
        0x11 => ("ARC", true),
        0x12 => ("CIRCLE", true),
        0x13 => ("LINE", true),
        0x14 => ("DIMENSION_ORDINATE", true),
        0x15 => ("DIMENSION_LINEAR", true),
        0x16 => ("DIMENSION_ALIGNED", true),
        0x17 => ("DIMENSION_ANG_3PT", true),
        0x18 => ("DIMENSION_ANG_2LN", true),
        0x19 => ("DIMENSION_RADIUS", true),
        0x1A => ("DIMENSION_DIAMETER", true),
        0x1B => ("POINT", true),
        0x1C => ("3DFACE", true),
        0x1D => ("POLYLINE_PFACE", true),
        0x1E => ("POLYLINE_MESH", true),
        0x1F => ("SOLID", true),
        0x20 => ("TRACE", true),
        0x21 => ("SHAPE", true),
        0x22 => ("VIEWPORT", true),
        0x23 => ("ELLIPSE", true),
        0x24 => ("SPLINE", true),
        0x25 => ("REGION", true),
        0x26 => ("3DSOLID", true),
        0x27 => ("BODY", true),
        0x28 => ("RAY", true),
        0x29 => ("XLINE", true),
        0x2A => ("DICTIONARY", false),
        0x2B => ("OLEFRAME", true),
        0x2C => ("MTEXT", true),
        0x2D => ("LEADER", true),
        0x2E => ("TOLERANCE", true),
        0x2F => ("MLINE", true),
        0x30 => ("BLOCK_CONTROL", false),
        0x31 => ("BLOCK_HEADER", false),
        0x32 => ("LAYER_CONTROL", false),
        0x33 => ("LAYER", false),
        0x34 => ("STYLE_CONTROL", false),
        0x35 => ("STYLE", false),
        0x38 => ("LTYPE_CONTROL", false),
        0x39 => ("LTYPE", false),
        0x3C => ("VIEW_CONTROL", false),
        0x3D => ("VIEW", false),
        0x3E => ("UCS_CONTROL", false),
        0x3F => ("UCS", false),
        0x40 => ("VPORT_CONTROL", false),
        0x41 => ("VPORT", false),
        0x42 => ("APPID_CONTROL", false),
        0x43 => ("APPID", false),
        0x44 => ("DIMSTYLE_CONTROL", false),
        0x45 => ("DIMSTYLE", false),
        0x46 => ("VP_ENT_HDR_CONTROL", false),
        0x47 => ("VP_ENT_HDR", false),
        0x48 => ("GROUP", false),
        0x49 => ("MLINESTYLE", false),
        0x4A => ("OLE2FRAME", true),
        0x4B => ("DUMMY", false),
        0x4C => ("LONG_TRANSACTION", false),
        0x4D => ("LWPOLYLINE", true),
        0x4E => ("HATCH", true),
        0x4F => ("XRECORD", false),
        0x50 => ("ACDBPLACEHOLDER", false),
        0x51 => ("VBA_PROJECT", false),
        0x52 => ("LAYOUT", false),
        0x1F2 => ("ACAD_PROXY_ENTITY", true),
        0x1F3 => ("ACAD_PROXY_OBJECT", false),
        _ => return None,
    })
}

/// Decoder for a fixed type code.
pub fn fixed_decoder(code: u16) -> Option<Decoder> {
    Some(match code {
        0x01 => entities::text,
        0x02 => entities::attrib,
        0x03 => entities::attdef,
        0x04 => entities::block,
        0x05 => |_| Ok(ObjectData::EndBlock),
        0x06 => |_| Ok(ObjectData::Seqend),
        0x07 => entities::insert,
        0x08 => entities::minsert,
        0x0A => curves::vertex_2d,
        0x0B..=0x0D => curves::vertex_3d,
        0x0E => curves::pface_face,
        0x0F => curves::polyline_2d,
        0x10 => curves::polyline_3d,
        0x11 => entities::arc,
        0x12 => entities::circle,
        0x13 => entities::line,
        0x14 => annotation::dim_ordinate,
        0x15 => annotation::dim_linear,
        0x16 => annotation::dim_aligned,
        0x17 => annotation::dim_angular_3pt,
        0x18 => annotation::dim_angular_2line,
        0x19 => annotation::dim_radius,
        0x1A => annotation::dim_diameter,
        0x1B => entities::point,
        0x1C => curves::face_3d,
        0x1D => curves::polyline_pface,
        0x1E => curves::polyline_mesh,
        0x1F => curves::solid,
        0x20 => curves::trace,
        0x21 => misc::shape,
        0x22 => misc::viewport,
        0x23 => curves::ellipse,
        0x24 => curves::spline,
        0x25..=0x27 => misc::acis,
        0x28 | 0x29 => misc::ray_line,
        0x2C => annotation::mtext,
        0x2D => annotation::leader,
        0x2E => annotation::tolerance,
        0x2F => misc::mline,
        0x4A => misc::ole2frame,
        0x4E => hatch::hatch,
        0x2A => tables::dictionary,
        0x30 => tables::block_control,
        0x38 => tables::ltype_control,
        0x44 => tables::dimstyle_control,
        0x32 | 0x34 | 0x3C | 0x3E | 0x40 | 0x42 | 0x46 => tables::control,
        0x31 => tables::block_header,
        0x33 => tables::layer,
        0x35 => tables::style,
        0x39 => tables::ltype,
        0x43 => tables::appid,
        0x3D | 0x3F | 0x41 | 0x45 | 0x47 => tables::table_entry_only,
        0x4D => entities::lwpolyline,
        0x52 => tables::layout,
        _ => return None,
    })
}

/// Decoder for a class (type code ≥ 500) by DXF name.
pub fn class_decoder(dxf_name: &str) -> Option<Decoder> {
    Some(match dxf_name {
        "LWPOLYLINE" => entities::lwpolyline,
        "LAYOUT" => tables::layout,
        "DICTIONARYWDFLT" | "ACDBDICTIONARYWDFLT" => tables::dictionary_with_default,
        "DBCOLOR" => tables::db_color,
        "HATCH" => hatch::hatch,
        "IMAGE" | "WIPEOUT" => misc::image,
        "IMAGEDEF" => misc::imagedef,
        _ => return None,
    })
}

/// Looks up a type code (classes by number through the index, not a scan per object).
pub fn lookup(type_code: u16, classes: &ClassTable<'_>) -> TypeInfo {
    if let Some((name, entity)) = fixed_type(type_code) {
        return TypeInfo {
            name: name.to_owned(),
            entity,
            decoder: fixed_decoder(type_code),
        };
    }
    if let Some(class) = classes.get(type_code) {
        return TypeInfo {
            name: class.dxf_name.clone(),
            entity: class.is_entity(),
            decoder: class_decoder(&class.dxf_name),
        };
    }
    TypeInfo {
        name: format!("TYPE_{type_code}"),
        entity: false,
        decoder: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::DwgClass;

    #[test]
    fn fixed_and_class_lookup() {
        let list = [
            DwgClass {
                number: 500,
                dxf_name: "LWPOLYLINE".into(),
                item_class_id: 0x1F2,
                ..DwgClass::default()
            },
            // A repeated number does not override the first class.
            DwgClass {
                number: 500,
                dxf_name: "OTHER".into(),
                ..DwgClass::default()
            },
        ];
        let classes = ClassTable::new(&list);
        let line = lookup(0x13, &classes);
        assert_eq!(
            (line.name.as_str(), line.entity, line.decoder.is_some()),
            ("LINE", true, true)
        );
        let lw = lookup(500, &classes);
        assert_eq!(
            (lw.name.as_str(), lw.entity, lw.decoder.is_some()),
            ("LWPOLYLINE", true, true)
        );
        let unknown = lookup(777, &classes);
        assert_eq!(unknown.name, "TYPE_777");
        assert!(unknown.decoder.is_none());
        assert!(!lookup(0x33, &classes).entity);
    }
}
