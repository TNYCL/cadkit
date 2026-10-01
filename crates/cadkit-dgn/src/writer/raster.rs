//! Lossless retention of unchanged seed raster attachments.
//!
//! Raster control records contain fields which are not yet decoded. Retaining the
//! whole attachment graph with its original IDs avoids fabricating those fields or
//! leaving known dependencies pointing at regenerated graphic IDs.

use super::{WriteOptions, compressed, encode::*};
use crate::native::v8::{AuxPage, V8Model};
use cadkit_core::{Document, Entity, EntityKind, Error, ReadOptions, Result};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Preserved {
    frames: BTreeMap<u64, Vec<u8>>,
    auxiliary: Vec<AuxPage>,
    controls: Vec<Vec<u8>>,
    control_prefix: String,
    control_version: u32,
    max_bytes: u64,
    pub bytes: u64,
    pub objects: u64,
}

fn images(doc: &Document) -> Result<BTreeMap<u64, &Entity>> {
    let mut out = BTreeMap::new();
    let mut todo: Vec<_> = doc.models.iter().flat_map(|m| &m.entities).collect();
    while let Some(e) = todo.pop() {
        match &e.kind {
            EntityKind::Image { .. } => {
                let id = e.id.ok_or_else(|| {
                    Error::Unsupported("new DGN raster attachments need a native encoder".into())
                })?;
                if out.insert(id, e).is_some() {
                    return Err(Error::invalid(0, "duplicate raster entity ID"));
                }
            }
            EntityKind::Group { children, .. } => todo.extend(children),
            _ => {}
        }
    }
    Ok(out)
}

impl Preserved {
    pub fn prepare(
        doc: &Document,
        seed: &[u8],
        model: &V8Model,
        read: &ReadOptions,
        options: &WriteOptions,
    ) -> Result<Self> {
        let requested = images(doc)?;
        if requested.is_empty() {
            return Ok(Self::default());
        }
        if !options.preserve_seed_rasters {
            return Err(Error::Unsupported(
                "seed raster preservation is disabled".into(),
            ));
        }
        let source = crate::read(seed, read)?;
        let original = images(&source)?;
        if requested.len() != original.len() {
            return Err(Error::Unsupported(
                "adding/removing seed rasters requires rewriting their native control graph".into(),
            ));
        }
        for (id, e) in &requested {
            let before = original
                .get(id)
                .ok_or_else(|| Error::Unsupported("raster is absent from the seed".into()))?;
            if e.kind != before.kind
                || e.layer != before.layer
                || e.color != before.color
                || e.linetype != before.linetype
                || e.lineweight != before.lineweight
                || e.visible != before.visible
                || e.props != before.props
            {
                return Err(Error::Unsupported(
                    "edited seed raster requires a native attachment encoder".into(),
                ));
            }
        }
        let mut out = Self {
            control_prefix: format!("Dgn-Md/{}/Dgn^C", model.storage),
            control_version: model
                .control_pages
                .first()
                .map(|p| p.header.format_version)
                .unwrap_or(3),
            max_bytes: options.limits.max_decompressed_bytes,
            ..Self::default()
        };
        let mut control_ids = BTreeSet::new();
        for page in &model.control_pages {
            for raw in &page.elements {
                if !(90..=93).contains(&raw.type_code()) {
                    continue;
                }
                if let Some(id) = raw.id() {
                    control_ids.insert(id);
                }
                out.bytes = out.bytes.saturating_add(raw.bytes.len() as u64 + 4);
                out.objects += 1;
                if raw.prefix != 0 {
                    return Err(Error::Unsupported(
                        "nonzero raster control record prefix".into(),
                    ));
                }
                out.controls.push(raw.bytes.clone());
            }
        }
        for page in &model.control_aux {
            if !page.complete {
                return Err(Error::Unsupported(
                    "incomplete raster control auxiliary records".into(),
                ));
            }
            let mut retained = page.clone();
            retained
                .records
                .retain(|a| control_ids.contains(&a.element_id));
            if retained.records.is_empty() {
                continue;
            }
            out.bytes = out.bytes.saturating_add(
                retained
                    .records
                    .iter()
                    .map(|a| a.payload.len() as u64 + 28)
                    .sum::<u64>(),
            );
            out.objects += retained.records.len() as u64;
            out.auxiliary.push(retained);
        }
        for raw in model.graphic_pages.iter().flat_map(|p| &p.elements) {
            if raw.type_code() != 94 {
                continue;
            }
            let id = raw
                .id()
                .ok_or_else(|| Error::invalid(0, "raster record without ID"))?;
            if !requested.contains_key(&id) || out.frames.insert(id, raw.bytes.clone()).is_some() {
                return Err(Error::Unsupported(
                    "unmapped or duplicate native raster frames".into(),
                ));
            }
        }
        if out.frames.len() != requested.len() {
            return Err(Error::Unsupported(
                "raster frame is missing from seed graphics".into(),
            ));
        }
        for page in &model.graphic_aux {
            if !page.complete {
                return Err(Error::invalid(0, "incomplete raster auxiliary page"));
            }
            let mut retained = page.clone();
            retained
                .records
                .retain(|a| out.frames.contains_key(&a.element_id));
            if retained.records.is_empty() {
                continue;
            }
            out.bytes = out.bytes.saturating_add(
                retained
                    .records
                    .iter()
                    .map(|a| a.payload.len() as u64 + 28)
                    .sum::<u64>(),
            );
            out.objects += retained.records.len() as u64;
            out.auxiliary.push(retained);
        }
        if out.bytes > options.limits.max_decompressed_bytes
            || out.objects >= options.limits.max_objects
        {
            return Err(Error::LimitExceeded(
                "preserved raster control/auxiliary budget".into(),
            ));
        }
        Ok(out)
    }

    pub fn frame(&self, e: &Entity, component: bool) -> Result<(u64, Vec<u8>)> {
        let id =
            e.id.ok_or_else(|| Error::Unsupported("new raster attachment".into()))?;
        let bytes = self
            .frames
            .get(&id)
            .ok_or_else(|| Error::Unsupported("raster attachment is not preserved".into()))?;
        let flags = crate::le::u32_at(bytes, 0).unwrap_or(0);
        if (flags & 0x4000_0000 != 0) != component {
            return Err(Error::Unsupported(
                "moving rasters into/out of complex groups requires native re-encoding".into(),
            ));
        }
        Ok((id, bytes.clone()))
    }

    pub fn write_auxiliary(
        &self,
        replacements: &mut BTreeMap<String, Option<Vec<u8>>>,
    ) -> Result<()> {
        if !self.controls.is_empty() {
            super::put_pages(
                replacements,
                &self.control_prefix,
                &self.controls,
                self.control_version,
                self.max_bytes,
            )?;
        }
        for page in &self.auxiliary {
            let mut payload = Vec::new();
            for a in &page.records {
                let mut record = vec![0; 28];
                u32_at(&mut record, 0, 0xa11b)?;
                u32_at(
                    &mut record,
                    4,
                    u32::try_from(a.payload.len())
                        .map_err(|_| Error::LimitExceeded("raster auxiliary bytes".into()))?,
                )?;
                u32_at(&mut record, 8, a.kind)?;
                u32_at(&mut record, 12, a.reserved)?;
                u64_at(&mut record, 16, a.element_id)?;
                u32_at(&mut record, 24, a.flags)?;
                payload.extend(record);
                payload.extend_from_slice(&a.payload);
            }
            let mut encoded = vec![0; 16];
            let count = u32::try_from(page.records.len())
                .map_err(|_| Error::LimitExceeded("raster auxiliary count".into()))?;
            u32_at(&mut encoded, 0, count)?;
            u32_at(&mut encoded, 4, page.header.format_version)?;
            u32_at(&mut encoded, 8, page.header.page_number)?;
            u32_at(&mut encoded, 12, count)?;
            encoded.extend(compressed(&payload)?);
            replacements.insert(page.path.clone(), Some(encoded));
        }
        Ok(())
    }
}
