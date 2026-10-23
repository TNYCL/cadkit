//! Preserves seed dictionary tables and creates new level and tag set definitions.

use super::encode::*;
use crate::native::{
    decode_v8,
    element::{ElementData, TagDef, TagSetData, TagValue},
    v8::{Page, RawElement},
};
use cadkit_core::{Attribute, Entity, Error, Result, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Type word bit of a complex header (a table header, a cell definition).
const COMPLEX_HEADER: u32 = 0x2000_0000;
/// Type word bit of a complex component (a table entry, a cell component).
const COMPLEX_COMPONENT: u32 = 0x4000_0000;
/// Level code (`u32`) of a level entry: the number users see, stable for a level name
/// across files and unique among the nonzero codes of a table, unlike the level id at
/// `0x20` that elements store (`docs/dgn/FORMAT_NOTES.md`, "Dictionary pages").
const LEVEL_CODE: usize = 0x5c;

/// A dictionary (`Dgn^Nm`) record with the seed page that holds it.
pub(super) struct NamedRecord {
    /// Number of the seed page holding the record; `None` for a record of a new table or
    /// tag set, which goes to a new page.
    pub page: Option<u32>,
    /// The 4-byte record prefix as stored in the seed (0 for new records).
    pub prefix: u32,
    /// The complete element.
    pub bytes: Vec<u8>,
}

impl NamedRecord {
    fn new(page: Option<u32>, bytes: Vec<u8>) -> Self {
        Self {
            page,
            prefix: 0,
            bytes,
        }
    }
}

fn table_record(b: &[u8], kind: u32) -> bool {
    crate::le::u32_at(b, 0).map(|v| v & 0xffff) == Some(kind)
        && crate::le::u32_at(b, 12) == Some(decode_v8::TABLE_LEVELS)
}

/// The header (type 96) of the level table.
fn is_level_header(b: &[u8]) -> bool {
    table_record(b, 96)
}

/// A level table entry (type 95).
fn is_level_entry(b: &[u8]) -> bool {
    table_record(b, 95)
}

fn is_component(b: &[u8]) -> bool {
    crate::le::u32_at(b, 0).is_some_and(|v| v & COMPLEX_COMPONENT != 0)
}

pub(super) struct Tables {
    /// Dictionary records in stored order, seed records first.
    pub records: Vec<NamedRecord>,
    pub fonts: BTreeMap<String, u32>,
    pub palette: [[u8; 3]; 256],
    pub levels: BTreeMap<String, u32>,
    pub sets: BTreeMap<Option<String>, (u64, TagSetData)>,
    pub last_id: u64,
    referenced_fonts: BTreeSet<u32>,
    max_level: u32,
    level_template: Option<Vec<u8>>,
    /// High 16 bits of the type word of new level entries: those of the seed's entries.
    level_flags: u32,
    /// Highest level code of the seed's and the new level entries.
    max_level_code: u32,
    /// New level entries in creation order; [`Tables::finish`] appends them to the table.
    new_levels: Vec<Vec<u8>>,
    tag_set_template: Option<Vec<u8>>,
    changed_sets: BTreeMap<Option<String>, ()>,
}
impl Tables {
    /// Reads the seed's dictionary `pages` in order. Every record keeps its page, except
    /// that complex components at the start of a page rejoin the page of the element before
    /// them and level entries after the level table header join that table, so no table or
    /// complex element spans pages.
    pub fn new(pages: &[Page], last_id: u64, read: &cadkit_core::ReadOptions) -> Result<Self> {
        let mut out = Self {
            records: vec![],
            fonts: BTreeMap::new(),
            palette: crate::palette::DEFAULT_PALETTE,
            levels: BTreeMap::new(),
            sets: BTreeMap::new(),
            last_id,
            referenced_fonts: BTreeSet::new(),
            max_level: 0,
            level_template: None,
            level_flags: 0,
            max_level_code: 0,
            new_levels: vec![],
            tag_set_template: None,
            changed_sets: BTreeMap::new(),
        };
        let mut level_names = HashSet::new();
        let read_record = |raw: &RawElement| {
            decode_v8::decode_with(
                raw,
                crate::text::encoding_for(read.fallback_codepage.as_deref()),
                &read.limits,
            )
        };
        // MicroStation keeps a complex element (a table with its entries, a cell definition
        // with its components) on one page and does not join one across a page boundary.
        // Components at the start of a page continue the element before them in a seed whose
        // writer split it (cadkit before 0.3.5 among them), so they move back to its page.
        let mut records: Vec<(u32, &RawElement)> = vec![];
        for page in pages {
            let mut leading = true;
            for raw in &page.elements {
                leading &= raw.is_complex_component();
                let number = match records.last() {
                    Some((previous, _)) if leading => *previous,
                    _ => page.number,
                };
                records.push((number, raw));
            }
        }
        // Some seeds (files a 3D post-processor touched) carry a second level table
        // whose codes repeat codes of the first one. Elements store the code only, so
        // a repeated code makes readers show new elements on the other table's level.
        // Later definitions of a code move to free codes after the highest one, and the
        // extra table headers are dropped: the output has one table with unique codes.
        let mut free_level = records
            .iter()
            .filter_map(|(_, raw)| match read_record(raw).data {
                ElementData::Level(level) => Some(level.id),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let mut level_codes = HashSet::new();
        // Indexes in `out.records` of the level table header and of the last record of its
        // run of entries; entries stored elsewhere after it, collected to join the run.
        let mut level_table: Option<(usize, usize)> = None;
        let mut joining = vec![];
        let mut header_word = None;
        for (number, raw) in records {
            let mut bytes = raw.bytes.clone();
            if is_level_header(&bytes) {
                if level_table.is_some() {
                    continue;
                }
                level_table = Some((out.records.len(), out.records.len()));
                header_word = Some(raw.type_word);
            }
            let mut e = read_record(raw);
            // No `let` chains: the MSRV (1.85) does not have them.
            if let ElementData::Level(level) = &mut e.data {
                if !level_codes.insert(level.id)
                    && crate::le::u32_at(&bytes, 0x20) == Some(level.id)
                {
                    free_level = free_level
                        .checked_add(1)
                        .ok_or_else(|| Error::LimitExceeded("DGN levels".into()))?;
                    u32_at(&mut bytes, 0x20, free_level)?;
                    level.id = free_level;
                    level_codes.insert(free_level);
                }
            }
            let primary = bytes.get(..raw.attribute_offset()).unwrap_or(&[]);
            let entry = matches!(e.data, ElementData::Level(_));
            if entry {
                if let Some(code) = crate::le::u32_at(primary, LEVEL_CODE) {
                    out.max_level_code = out.max_level_code.max(code);
                }
            }
            match e.data {
                ElementData::Font { number, name } => {
                    out.fonts.insert(name, number);
                }
                ElementData::ColorTable { colors } => {
                    for (dst, src) in out.palette.iter_mut().zip(colors) {
                        *dst = src;
                    }
                }
                ElementData::Level(level) => {
                    out.max_level = out.max_level.max(level.id);
                    // Use the same names as the reader: a name-only map of the raw
                    // table would overwrite duplicate names and move their entities.
                    let name = crate::level_names::assign(
                        level.name.as_deref(),
                        level.id,
                        &mut level_names,
                    );
                    out.levels.insert(name, level.id);
                    out.level_template.get_or_insert(primary.to_vec());
                }
                ElementData::TagSet(set) => {
                    out.sets
                        .insert(set.name.clone(), (raw.id().unwrap_or(0), set));
                    out.tag_set_template
                        .get_or_insert(raw.primary().get(..0x3c).unwrap_or(&[]).to_vec());
                }
                _ => {}
            }
            let record = NamedRecord {
                page: Some(number),
                prefix: raw.prefix,
                bytes,
            };
            match level_table {
                // Entries of the dropped tables, and entries a split seed left on a later
                // page, join the level table after its last entry, on its page.
                Some((header, last)) if entry => {
                    let page = out.records.get(header).and_then(|r| r.page);
                    let record = NamedRecord { page, ..record };
                    if last.checked_add(1) == Some(out.records.len()) {
                        out.records.push(record);
                        level_table = Some((header, out.records.len() - 1));
                    } else {
                        joining.push(record);
                    }
                }
                _ => out.records.push(record),
            }
        }
        if let Some((_, last)) = level_table {
            let at = last.saturating_add(1).min(out.records.len());
            out.records.splice(at..at, joining);
        }
        // A new entry looks like its siblings: the first entry's flags, or the component
        // flags that go with the header's (`0x2440_0060` header, `0x4440_005F` entries).
        out.level_flags = match (&out.level_template, header_word) {
            (Some(template), _) => crate::le::u32_at(template, 0).unwrap_or(0) & 0xffff_0000,
            (None, Some(word)) if word & COMPLEX_HEADER != 0 => {
                (word & 0xffff_0000 & !COMPLEX_HEADER) | COMPLEX_COMPONENT
            }
            _ => 0,
        };
        Ok(out)
    }
    pub fn observe_model_fonts<'a>(
        &mut self,
        records: impl Iterator<Item = &'a RawElement>,
        read: &cadkit_core::ReadOptions,
    ) {
        for raw in records {
            let font = match decode_v8::decode_with(
                raw,
                crate::text::encoding_for(read.fallback_codepage.as_deref()),
                &read.limits,
            )
            .data
            {
                ElementData::Text(text) => Some(text.font),
                ElementData::TextNode(text) => Some(text.font),
                _ => None,
            };
            if let Some(font) = font {
                self.referenced_fonts.insert(font);
            }
        }
    }
    pub fn id(&mut self) -> Result<u64> {
        self.last_id = self
            .last_id
            .checked_add(1)
            .ok_or_else(|| Error::LimitExceeded("DGN element ids".into()))?;
        Ok(self.last_id)
    }
    pub fn level(&mut self, name: Option<&str>, time: f64) -> Result<u32> {
        let name = name.unwrap_or("Default");
        if let Some(id) = self.levels.get(name) {
            return Ok(*id);
        }
        self.max_level = self
            .max_level
            .checked_add(1)
            .ok_or_else(|| Error::LimitExceeded("DGN levels".into()))?;
        let level = self.max_level;
        let id = self.id()?;
        let mut b = if let Some(t) = &self.level_template {
            t.clone()
        } else {
            prefix(95, 0x30, id, 1, time)?
        };
        u32_at(&mut b, 0, self.level_flags | 95)?;
        u32_at(&mut b, 12, 1)?;
        u64_at(&mut b, 16, id)?;
        f64_at(&mut b, 24, time)?;
        u32_at(&mut b, 0x20, level)?;
        u32_at(&mut b, 0x24, u32::MAX)?;
        // A template entry carries its own level's code; a new level takes the next one.
        if b.len() >= LEVEL_CODE + 4 {
            self.max_level_code = self
                .max_level_code
                .checked_add(1)
                .ok_or_else(|| Error::LimitExceeded("DGN level codes".into()))?;
            u32_at(&mut b, LEVEL_CODE, self.max_level_code)?;
        }
        let b = finish(b, &string_link(1, name)?)?;
        self.new_levels.push(b);
        self.levels.insert(name.into(), level);
        Ok(level)
    }

    pub fn font(&self, entity: &Entity, style: Option<&str>) -> Result<u32> {
        let explicit = match entity.props.get("dgn.font_number") {
            Some(Value::Int(n)) => {
                Some(u32::try_from(*n).map_err(|_| Error::invalid(0, "invalid DGN font number"))?)
            }
            Some(_) => return Err(Error::invalid(0, "DGN font number must be an integer")),
            None => None,
        };
        let known = |number| {
            number == 0
                || self.fonts.values().any(|n| *n == number)
                || self.referenced_fonts.contains(&number)
        };
        let named = match style {
            Some(name) => Some(
                self.fonts
                    .get(name)
                    .copied()
                    .or_else(|| {
                        name.strip_prefix("DGN_FONT_")?
                            .parse()
                            .ok()
                            .filter(|n| known(*n))
                    })
                    .ok_or_else(|| {
                        Error::Unsupported("text font style is absent from seed".into())
                    })?,
            ),
            None => None,
        };
        if let Some(number) = explicit {
            // Font zero is the existing implicit fallback. A seed can also reference
            // an external RSC font in its graphics without a named table definition.
            // Do not accept unreferenced foreign indexes merely because they are small.
            if !known(number) {
                return Err(Error::Unsupported(
                    "text font number is absent from seed".into(),
                ));
            }
            if named.is_some_and(|n| n != number) {
                return Err(Error::invalid(0, "DGN font number and style name disagree"));
            }
            return Ok(number);
        }
        // Without a style or number the format default (font 0) applies; the first table
        // entry by name is an arbitrary seed font (often a CJK TrueType face).
        Ok(named.unwrap_or(0))
    }

    /// Font number of a named style for a displayed tag; `None` style uses font 0.
    pub fn tag_font(&self, props: &cadkit_core::Props, style: Option<&str>) -> Result<u32> {
        let mut probe = Entity::new(cadkit_core::EntityKind::Point {
            position: cadkit_core::Point3::default(),
        });
        if let Some(number) = props.get("dgn.font_number") {
            probe.props.insert("dgn.font_number".into(), number.clone());
        }
        self.font(&probe, style)
    }
    pub fn tag(&mut self, attribute: &Attribute) -> Result<(u64, u16, u16)> {
        let name = &attribute.set;
        let value_type = match attribute.value {
            Value::Text(_) => 1,
            Value::Int(_) => 3,
            Value::Float(v) if v.is_finite() => 4,
            _ => return Err(Error::Unsupported("DGN tag value type".into())),
        };
        if !self.sets.contains_key(name) {
            let id = self.id()?;
            self.sets.insert(
                name.clone(),
                (
                    id,
                    TagSetData {
                        name: name.clone(),
                        number: None,
                        tags: vec![],
                    },
                ),
            );
            self.changed_sets.insert(name.clone(), ());
        }
        let (id, set) = self
            .sets
            .get_mut(name)
            .ok_or_else(|| Error::invalid(0, "missing DGN tag set"))?;
        if let Some(def) = set.tags.iter().find(|d| d.name == attribute.tag) {
            if def.value_type != value_type {
                return Err(Error::invalid(0, "DGN tag definition type mismatch"));
            }
            return Ok((*id, def.id, value_type));
        }
        let next = set
            .tags
            .iter()
            .map(|d| d.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| Error::LimitExceeded("DGN tag definitions".into()))?;
        if set.tags.len() >= 4096 {
            return Err(Error::LimitExceeded("DGN tag definitions".into()));
        }
        let default = match value_type {
            1 => TagValue::Text(String::new()),
            3 => TagValue::Int(0),
            _ => TagValue::Float(0.0),
        };
        let flags = set
            .tags
            .iter()
            .find(|d| d.value_type == value_type)
            .map(|d| d.flags)
            .unwrap_or([0, 0, value_type as u8, 0, 0]);
        set.tags.push(TagDef {
            id: next,
            name: attribute.tag.clone(),
            prompt: attribute.tag.clone(),
            value_type,
            default,
            flags,
        });
        self.changed_sets.insert(name.clone(), ());
        Ok((*id, next, value_type))
    }
    pub fn finish(mut self, time: f64) -> Result<Vec<NamedRecord>> {
        for name in self.changed_sets.keys() {
            let (id, set) = self
                .sets
                .get(name)
                .ok_or_else(|| Error::invalid(0, "missing tag set"))?;
            let mut definitions = vec![];
            for def in &set.tags {
                definitions.extend(utf16(&def.name)?);
                definitions.extend(def.id.to_le_bytes());
                definitions.extend(utf16(&def.prompt)?);
                definitions.extend(def.value_type.to_le_bytes());
                definitions.extend(def.flags);
                match &def.default {
                    TagValue::Text(s) => definitions.extend(utf16(s)?),
                    TagValue::Int(n) => definitions.extend(
                        i32::try_from(*n)
                            .map_err(|_| Error::invalid(0, "tag default exceeds i32"))?
                            .to_le_bytes(),
                    ),
                    TagValue::Float(n) => definitions.extend(n.to_le_bytes()),
                    TagValue::Binary(b) => definitions.extend(b),
                }
            }
            let mut b = self
                .tag_set_template
                .clone()
                .filter(|b| b.len() == 0x3c)
                .unwrap_or(prefix(39, 0x3c, *id, 0, time)?);
            u32_at(&mut b, 0, 39)?;
            u64_at(&mut b, 16, *id)?;
            f64_at(&mut b, 24, time)?;
            put(&mut b, 0x28, b"teSt")?;
            u32_at(&mut b, 0x2c, 0xf81)?;
            let len = u32::try_from(definitions.len())
                .map_err(|_| Error::LimitExceeded("DGN tag definitions".into()))?;
            u32_at(&mut b, 0x34, len)?;
            u32_at(&mut b, 0x38, len)?;
            b.extend(definitions);
            let links = name
                .as_deref()
                .map(|name| string_link(1, name))
                .transpose()?
                .unwrap_or_default();
            let b = finish(b, &links)?;
            // A changed seed tag set stays where it is; a new one goes to a new page.
            if let Some(old) = self
                .records
                .iter_mut()
                .find(|r| crate::le::u64_at(&r.bytes, 16) == Some(*id))
            {
                old.bytes = b;
            } else {
                self.records.push(NamedRecord::new(None, b));
            }
        }
        let level_count = u32::try_from(self.levels.len())
            .map_err(|_| Error::LimitExceeded("DGN level count".into()))?;
        let mut header = None;
        for (i, r) in self.records.iter_mut().enumerate() {
            if is_level_header(&r.bytes) {
                u32_at(&mut r.bytes, 0x20, level_count)?;
                header.get_or_insert(i);
            }
        }
        if header.is_none() && level_count > 0 {
            let id = self
                .last_id
                .checked_add(1)
                .ok_or_else(|| Error::LimitExceeded("DGN table header id".into()))?;
            let mut b = prefix(96, 0x28, id, 1, time)?;
            u32_at(&mut b, 0x20, level_count)?;
            // The header goes before the seed's level entries, on their page; without any,
            // the new table goes before the new tag sets, on the new page.
            let (at, page) = match self.records.iter().position(|r| is_level_entry(&r.bytes)) {
                Some(first) => (first, self.records.get(first).and_then(|r| r.page)),
                None => (
                    self.records
                        .iter()
                        .position(|r| r.page.is_none())
                        .unwrap_or(self.records.len()),
                    None,
                ),
            };
            self.records
                .insert(at, NamedRecord::new(page, finish(b, &[])?));
            header = Some(at);
        }
        if let Some(header) = header {
            // New entries follow the table's last entry, on the table's page.
            let page = self.records.get(header).and_then(|r| r.page);
            let mut at = header.saturating_add(1);
            while self
                .records
                .get(at)
                .is_some_and(|r| is_component(&r.bytes) || is_level_entry(&r.bytes))
            {
                at += 1;
            }
            let levels = std::mem::take(&mut self.new_levels);
            self.records.splice(
                at..at,
                levels.into_iter().map(|b| NamedRecord::new(page, b)),
            );
        }
        Ok(self.records)
    }
}
