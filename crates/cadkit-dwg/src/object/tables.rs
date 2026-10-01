//! Table, control, dictionary and layout decoders (spec §20.4.44–§20.4.84).
//! Each runs after the common non-entity data (owner, reactors, xdictionary).

use cadkit_core::{Error, Result};

use super::{Streams, read_handles as handles};
use crate::native::{
    BlockHeader, Control, Dash, Dictionary, LayerRecord, Layout, LinetypeRecord, ObjectData,
    TableEntry, TextStyleRecord,
};

fn control_entries(s: &mut Streams<'_>) -> Result<Control> {
    let count = s.main.bl()?;
    Ok(Control {
        entries: handles(s, count, "table entry")?,
        extra: Vec::new(),
    })
}

/// Generic table control object: `BL` entry count, entry handles.
pub fn control(s: &mut Streams<'_>) -> Result<ObjectData> {
    Ok(ObjectData::Control(control_entries(s)?))
}

/// BLOCK_CONTROL (§20.4.51): entries, then *MODEL_SPACE and *PAPER_SPACE.
pub fn block_control(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut c = control_entries(s)?;
    c.extra = vec![s.h()?, s.h()?];
    Ok(ObjectData::Control(c))
}

/// DIMSTYLE_CONTROL (§20.4.67): entries, then an undocumented `RC` count followed (in
/// the handle stream) by that many more handles. Observed in R2000+ samples; the
/// count/handle pairing consumes both streams exactly.
pub fn dimstyle_control(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut c = control_entries(s)?;
    if s.version().r2000_plus() {
        let count = s.main.rc()?;
        c.extra = handles(s, i32::from(count), "dimstyle control extra")?;
    }
    Ok(ObjectData::Control(c))
}

/// APPID (§20.4.66): name, xref fields, an undocumented `RC` (DXF 71), the xref block handle.
pub fn appid(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut entry = table_entry(s)?;
    let _unknown = s.main.rc()?;
    entry.xref_block = s.h_opt()?;
    Ok(ObjectData::TableRecord(entry))
}

/// LTYPE_CONTROL (§20.4.57): entries, then BYLAYER and BYBLOCK.
pub fn ltype_control(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut c = control_entries(s)?;
    c.extra = vec![s.h()?, s.h()?];
    Ok(ObjectData::Control(c))
}

/// Name and xref fields of a table record. Before R2007: `B` 64-flag, `BS`
/// xrefindex+1, `B` xdep. From R2007 only the `BS`, with xdep in bit 0x100
/// (spec erratum; matches the files and ACadSharp).
fn table_entry(s: &mut Streams<'_>) -> Result<TableEntry> {
    let name = s.tv()?;
    if s.version().r2007_plus() {
        let xref = s.main.bs()?;
        Ok(TableEntry {
            name,
            flag64: false,
            xref_index_plus1: xref,
            xref_dependent: xref & 0x100 != 0,
            xref_block: None,
        })
    } else {
        let flag64 = s.main.b()?;
        let xref_index_plus1 = s.main.bs()?;
        let xref_dependent = s.main.b()?;
        Ok(TableEntry {
            name,
            flag64,
            xref_index_plus1,
            xref_dependent,
            xref_block: None,
        })
    }
}

/// Table records decoded only up to the name (APPID, VIEW, UCS, VPORT, DIMSTYLE, …).
pub fn table_entry_only(s: &mut Streams<'_>) -> Result<ObjectData> {
    Ok(ObjectData::TableRecord(table_entry(s)?))
}

/// LAYER (§20.4.54).
pub fn layer(s: &mut Streams<'_>) -> Result<ObjectData> {
    let v = s.version();
    let mut l = LayerRecord {
        entry: table_entry(s)?,
        on: true,
        plot: true,
        ..LayerRecord::default()
    };
    if v.r13_14() {
        l.frozen = s.main.b()?;
        // The spec says "1 if on"; the R14 sample stores 1 for layers that are off.
        l.on = !s.main.b()?;
        l.frozen_new_viewports = s.main.b()?;
        l.locked = s.main.b()?;
    } else {
        let values = s.main.bs()? as u16;
        l.frozen = values & 0x01 != 0;
        l.on = values & 0x02 == 0;
        l.frozen_new_viewports = values & 0x04 != 0;
        l.locked = values & 0x08 != 0;
        l.plot = values & 0x10 != 0;
        l.lineweight = Some(((values & 0x03E0) >> 5) as u8);
    }
    l.color = s.cmc()?;
    l.entry.xref_block = s.h_opt()?;
    if v.r2000_plus() {
        l.plotstyle = s.h_opt()?;
    }
    if v.r2007_plus() {
        l.material = s.h_opt()?;
    }
    l.linetype = s.h_opt()?;
    // The spec lists one more (always null) handle for all versions; only R2013+ files
    // have it (the handle stream is fully consumed with this rule in every sample).
    if v.r2013_plus() {
        let _unknown = s.h_opt()?;
    }
    Ok(ObjectData::Layer(l))
}

/// STYLE (§20.4.56). The two flag bits are shape-file then vertical (the spec lists
/// them the other way round; DXF flag 1 = shape, 4 = vertical).
pub fn style(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut st = TextStyleRecord {
        entry: table_entry(s)?,
        ..TextStyleRecord::default()
    };
    st.is_shape = s.main.b()?;
    st.vertical = s.main.b()?;
    st.fixed_height = s.main.bd()?;
    st.width_factor = s.main.bd()?;
    st.oblique = s.main.bd()?;
    st.generation = s.main.rc()?;
    st.last_height = s.main.bd()?;
    st.font = s.tv()?;
    st.big_font = s.tv()?;
    st.entry.xref_block = s.h_opt()?;
    Ok(ObjectData::TextStyle(st))
}

/// LTYPE (§20.4.58).
pub fn ltype(s: &mut Streams<'_>) -> Result<ObjectData> {
    let v = s.version();
    let mut lt = LinetypeRecord {
        entry: table_entry(s)?,
        ..LinetypeRecord::default()
    };
    lt.description = s.tv()?;
    lt.pattern_length = s.main.bd()?;
    lt.alignment = s.main.rc()?;
    let count = s.main.rc()?;
    let mut has_text = false;
    for _ in 0..count {
        let dash = Dash {
            length: s.main.bd()?,
            shape_code: s.main.bs()?,
            x_offset: s.main.rd()?,
            y_offset: s.main.rd()?,
            scale: s.main.bd()?,
            rotation: s.main.bd()?,
            shape_flags: s.main.bs()?,
            style: None,
        };
        has_text |= dash.shape_flags & 0x02 != 0;
        lt.dashes.push(dash);
    }
    if !v.r2007_plus() {
        lt.text_area = s.main.bytes(256)?;
    } else if has_text {
        lt.text_area = s.main.bytes(512)?;
    }
    lt.entry.xref_block = s.h_opt()?;
    for dash in &mut lt.dashes {
        dash.style = s.h_opt()?;
    }
    Ok(ObjectData::Linetype(lt))
}

/// BLOCK_HEADER (§20.4.52).
pub fn block_header(s: &mut Streams<'_>) -> Result<ObjectData> {
    let v = s.version();
    let mut b = BlockHeader {
        entry: table_entry(s)?,
        ..BlockHeader::default()
    };
    b.anonymous = s.main.b()?;
    b.has_attdefs = s.main.b()?;
    b.is_xref = s.main.b()?;
    b.is_overlaid = s.main.b()?;
    if v.r2000_plus() {
        b.unloaded = s.main.b()?;
    }
    let xref = b.is_xref || b.is_overlaid;
    // The owned-object count is absent for xrefs (ACadSharp; spec lists it always).
    let owned = if v.r2004_plus() && !xref {
        s.main.bl()?
    } else {
        0
    };
    b.base_point = s.main.bd3()?;
    b.xref_path = s.tv()?;
    let mut insert_count: usize = 0;
    if v.r2000_plus() {
        while s.main.rc()? != 0 {
            insert_count += 1;
        }
        b.description = s.tv()?;
        let preview = s.main.bl()?;
        let preview =
            usize::try_from(preview).map_err(|_| s.main.invalid("negative preview size"))?;
        b.preview = s.main.bytes(preview)?;
    }
    if v.r2007_plus() {
        b.insert_units = Some(s.main.bs()?);
        b.explodable = Some(s.main.b()?);
        b.scaling = Some(s.main.rc()?);
    }

    b.entry.xref_block = s.h_opt()?;
    b.block_entity = s.h_opt()?;
    if !v.r2004_plus() && !xref {
        b.first_entity = s.h_opt()?;
        b.last_entity = s.h_opt()?;
    }
    if v.r2004_plus() {
        b.entities = handles(s, owned, "owned entity")?;
    }
    b.end_block = s.h_opt()?;
    if v.r2000_plus() {
        let count =
            i32::try_from(insert_count).map_err(|_| Error::LimitExceeded("insert count".into()))?;
        b.inserts = handles(s, count, "insert")?;
        b.layout = s.h_opt()?;
    }
    Ok(ObjectData::BlockHeader(b))
}

fn dictionary_data(s: &mut Streams<'_>) -> Result<Dictionary> {
    let v = s.version();
    let count = s.main.bl()?;
    let mut d = Dictionary::default();
    if v == crate::version::DwgVersion::R14 {
        let _unknown = s.main.rc()?;
    }
    if v.r2000_plus() {
        d.cloning = Some(s.main.bs()?);
        d.hard_owner = Some(s.main.rc()?);
    }
    let n = usize::try_from(count).map_err(|_| s.main.invalid("negative dictionary size"))?;
    let remaining = s.handles.as_ref().map_or(0, |h| h.remaining_bits());
    if (n as u64).saturating_mul(8) > remaining {
        return Err(s
            .main
            .invalid(format!("{n} dictionary entries exceed the handle stream")));
    }
    for _ in 0..n {
        let name = s.tv()?;
        let handle = s.h()?;
        d.entries.push((name, handle));
    }
    Ok(d)
}

/// DICTIONARY (§20.4.44).
pub fn dictionary(s: &mut Streams<'_>) -> Result<ObjectData> {
    Ok(ObjectData::Dictionary(dictionary_data(s)?))
}

/// DICTIONARYWDFLT (§20.4.45): a dictionary plus a default entry handle.
pub fn dictionary_with_default(s: &mut Streams<'_>) -> Result<ObjectData> {
    let mut d = dictionary_data(s)?;
    d.default_entry = s.h_opt()?;
    Ok(ObjectData::Dictionary(d))
}

/// LAYOUT (§20.4.84): plot settings, then layout fields.
pub fn layout(s: &mut Streams<'_>) -> Result<ObjectData> {
    let v = s.version();
    let mut l = Layout {
        page_setup: s.tv()?,
        ..Layout::default()
    };
    let _printer = s.tv()?;
    let _plot_flags = s.main.bs()?;
    for _ in 0..4 {
        let _margin = s.main.bd()?;
    }
    l.paper_width = s.main.bd()?;
    l.paper_height = s.main.bd()?;
    let _paper_size = s.tv()?;
    let _plot_origin = s.main.bd2()?;
    let _paper_units = s.main.bs()?;
    let _rotation = s.main.bs()?;
    let _plot_type = s.main.bs()?;
    let _window_min = s.main.bd2()?;
    let _window_max = s.main.bd2()?;
    if !v.r2004_plus() {
        let _plot_view_name = s.tv()?;
    }
    let _real_world_units = s.main.bd()?;
    let _drawing_units = s.main.bd()?;
    let _style_sheet = s.tv()?;
    let _scale_type = s.main.bs()?;
    let _scale_factor = s.main.bd()?;
    let _paper_image_origin = s.main.bd2()?;
    if v.r2004_plus() {
        let _shade_plot_mode = s.main.bs()?;
        let _shade_plot_res = s.main.bs()?;
        let _shade_plot_dpi = s.main.bs()?;
    }
    l.name = s.tv()?;
    l.tab_order = s.main.bl()?;
    l.flags = s.main.bs()?;
    let _ucs_origin = s.main.bd3()?;
    l.limits_min = s.main.rd2()?;
    l.limits_max = s.main.rd2()?;
    let _insertion_base = s.main.bd3()?;
    let _ucs_x = s.main.bd3()?;
    let _ucs_y = s.main.bd3()?;
    let _elevation = s.main.bd()?;
    let _ortho_type = s.main.bs()?;
    l.extents_min = s.main.bd3()?;
    l.extents_max = s.main.bd3()?;
    let viewports = if v.r2004_plus() { s.main.bl()? } else { 0 };

    if v.r2004_plus() {
        let _plot_view = s.h_opt()?;
    }
    if v.r2007_plus() {
        let _visual_style = s.h_opt()?;
    }
    l.block_header = s.h_opt()?;
    l.active_viewport = s.h_opt()?;
    let _base_ucs = s.h_opt()?;
    let _named_ucs = s.h_opt()?;
    if v.r2004_plus() {
        l.viewports = handles(s, viewports, "viewport")?;
    }
    Ok(ObjectData::Layout(l))
}

/// DBCOLOR (class): a `CMC` color (with color and book names).
pub fn db_color(s: &mut Streams<'_>) -> Result<ObjectData> {
    Ok(ObjectData::DbColor(s.cmc()?))
}
