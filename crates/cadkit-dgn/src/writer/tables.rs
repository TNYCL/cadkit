//! Seed tablolarının korunması ve yeni level/etiket tanımlarının oluşturulması.

use super::encode::*;
use crate::native::{
    decode_v8,
    element::{ElementData, TagDef, TagSetData, TagValue},
    v8::RawElement,
};
use cadkit_core::{Attribute, Entity, Error, Result, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub(super) struct Tables {
    pub records: Vec<Vec<u8>>,
    pub fonts: BTreeMap<String, u32>,
    pub palette: [[u8; 3]; 256],
    pub levels: BTreeMap<String, u32>,
    pub sets: BTreeMap<Option<String>, (u64, TagSetData)>,
    pub last_id: u64,
    referenced_fonts: BTreeSet<u32>,
    max_level: u32,
    level_template: Option<Vec<u8>>,
    tag_set_template: Option<Vec<u8>>,
    changed_sets: BTreeMap<Option<String>, ()>,
}
impl Tables {
    pub fn new(
        records: impl Iterator<Item = RawElement>,
        last_id: u64,
        read: &cadkit_core::ReadOptions,
    ) -> Result<Self> {
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
            tag_set_template: None,
            changed_sets: BTreeMap::new(),
        };
        let mut level_names = HashSet::new();
        for raw in records {
            let e = decode_v8::decode(
                &raw,
                crate::text::encoding_for(read.fallback_codepage.as_deref()),
                &read.limits,
            );
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
                    out.level_template.get_or_insert(raw.primary().to_vec());
                }
                ElementData::TagSet(set) => {
                    out.sets
                        .insert(set.name.clone(), (raw.id().unwrap_or(0), set));
                    out.tag_set_template
                        .get_or_insert(raw.primary().get(..0x3c).unwrap_or(&[]).to_vec());
                }
                _ => {}
            }
            out.records.push(raw.bytes);
        }
        Ok(out)
    }
    pub fn observe_model_fonts<'a>(
        &mut self,
        records: impl Iterator<Item = &'a RawElement>,
        read: &cadkit_core::ReadOptions,
    ) {
        for raw in records {
            let font = match decode_v8::decode(
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
        u32_at(&mut b, 0, 95)?;
        u32_at(&mut b, 12, 1)?;
        u64_at(&mut b, 16, id)?;
        f64_at(&mut b, 24, time)?;
        u32_at(&mut b, 0x20, level)?;
        u32_at(&mut b, 0x24, u32::MAX)?;
        let b = finish(b, &string_link(1, name)?)?;
        self.records.push(b);
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
        Ok(named.unwrap_or_else(|| self.fonts.values().next().copied().unwrap_or(0)))
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
    pub fn finish(mut self, time: f64) -> Result<Vec<Vec<u8>>> {
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
            if let Some(old) = self
                .records
                .iter_mut()
                .find(|b| crate::le::u64_at(b, 16) == Some(*id))
            {
                *old = b;
            } else {
                self.records.push(b);
            }
        }
        let level_count = u32::try_from(self.levels.len())
            .map_err(|_| Error::LimitExceeded("DGN level count".into()))?;
        let mut header = false;
        for r in &mut self.records {
            if crate::le::u32_at(r, 0).map(|v| v & 0xffff) == Some(96)
                && crate::le::u32_at(r, 12) == Some(1)
            {
                u32_at(r, 0x20, level_count)?;
                header = true;
            }
        }
        if !header && level_count > 0 {
            let id = self
                .last_id
                .checked_add(1)
                .ok_or_else(|| Error::LimitExceeded("DGN table header id".into()))?;
            let mut b = prefix(96, 0x28, id, 1, time)?;
            u32_at(&mut b, 0x20, level_count)?;
            self.records.insert(0, finish(b, &[])?);
        }
        Ok(self.records)
    }
}
