//! Dictionary pages of the output: `Dgn^Nm/$n` (tables, tag sets, cell definitions) and
//! their XAttribute pages `Dgn^NmA/$n`.
//!
//! The seed's page layout is kept: every seed page keeps its number, its records stay in
//! order, and a page whose records did not change keeps its stream byte for byte (empty
//! pages included). A changed page is rewritten in place; new tables and tag sets share one
//! new page after the seed's last page. MicroStation keeps a complex element (a table with
//! its entries, a cell definition with its components) on one page and does not join one
//! across a page boundary: a level table split over two pages is not loaded as one table.
//! Its pages hold as many records as that takes (595,424 inflated bytes observed), so only
//! the caller's decompression limit bounds a page; nothing is split at a fixed size
//! (`docs/dgn/FORMAT_NOTES.md`, "Dictionary pages").

use super::{compressed, encode::u32_at, raster, tables::NamedRecord};
use crate::native::v8::{AuxPage, AuxRecord, Page, PageHeader, RawElement};
use cadkit_core::LimitKind;
use cadkit_core::{Error, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Puts the dictionary pages that differ from the seed's, and the XAttribute pages whose
/// records moved with their elements, into `replacements`. `records` are in stored order
/// (as [`super::tables::Tables::finish`] returns them). The inflated records together must
/// not exceed `max` bytes.
pub(super) fn write(
    replacements: &mut BTreeMap<String, Option<Vec<u8>>>,
    seed_pages: &[Page],
    seed_aux: &[AuxPage],
    records: &[NamedRecord],
    max: u64,
) -> Result<()> {
    let total = records
        .iter()
        .try_fold(0u64, |sum, r| sum.checked_add(r.bytes.len() as u64 + 4))
        .ok_or_else(|| Error::limit(LimitKind::Output, "DGN dictionary records".into()))?;
    if total > max {
        return Err(Error::limit(
            LimitKind::Output,
            "DGN dictionary records".into(),
        ));
    }
    let new_page = seed_pages
        .iter()
        .map(|p| p.number)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| Error::limit(LimitKind::Capacity, "DGN page count".into()))?;
    let mut pages: BTreeMap<u32, Vec<&NamedRecord>> = BTreeMap::new();
    for r in records {
        pages.entry(r.page.unwrap_or(new_page)).or_default().push(r);
    }
    for page in seed_pages {
        let kept = pages.get(&page.number).map(Vec::as_slice).unwrap_or(&[]);
        let same = kept.len() == page.elements.len()
            && kept
                .iter()
                .zip(&page.elements)
                .all(|(r, e)| r.prefix == e.prefix && r.bytes == e.bytes);
        if !same {
            replacements.insert(
                page.path.clone(),
                Some(encode(kept, page.header.format_version)?),
            );
        }
    }
    // As before 0.3.5, new pages take the version of the seed's first page.
    let version = seed_pages
        .first()
        .map(|p| p.header.format_version)
        .unwrap_or(3);
    if let Some(added) = pages.get(&new_page) {
        replacements.insert(
            format!(
                "{}/${new_page}",
                storage(seed_pages.first().map(|p| &p.path), "Dgn^Nm")
            ),
            Some(encode(added, version)?),
        );
    }
    move_xattributes(replacements, seed_pages, seed_aux, &pages)
}

/// The storage of a seed page `path` (`Dgn^Nm` of `Dgn^Nm/$1`), or `default`.
fn storage<'a>(path: Option<&'a String>, default: &'a str) -> &'a str {
    path.and_then(|p| p.rsplit_once('/'))
        .map(|(storage, _)| storage)
        .unwrap_or(default)
}

/// A dictionary page: `{record count, format version, 0, 0}`, then the zlib-compressed
/// records, each after its 4-byte prefix. An empty page is the header alone. The page
/// number and population are 0 as on every dictionary page MicroStation wrote (private
/// corpus: 3,932 of 3,932 pages); cadkit before 0.3.5 wrote the page number and the
/// record count there.
fn encode(records: &[&NamedRecord], version: u32) -> Result<Vec<u8>> {
    let mut out = vec![0; 16];
    u32_at(
        &mut out,
        0,
        u32::try_from(records.len())
            .map_err(|_| Error::limit(LimitKind::Capacity, "DGN page records".into()))?,
    )?;
    u32_at(&mut out, 4, version)?;
    if !records.is_empty() {
        let mut data = vec![];
        for r in records {
            data.extend(r.prefix.to_le_bytes());
            data.extend_from_slice(&r.bytes);
        }
        out.extend(compressed(&data)?);
    }
    Ok(out)
}

/// Keeps every XAttribute on the auxiliary page numbered like the page of its element, as
/// on every observed file (private corpus: 4,215 of 4,215 `Dgn^NmA` records): records of
/// moved elements move with them, records of dropped seed elements are dropped, and records
/// of ids that are not dictionary elements stay where they are. Only pages that change are
/// rewritten; `Dgn^NmA/^AH` is kept (it never exceeds the highest dictionary page number
/// in the corpus, and no page is removed).
fn move_xattributes(
    replacements: &mut BTreeMap<String, Option<Vec<u8>>>,
    seed_pages: &[Page],
    seed_aux: &[AuxPage],
    pages: &BTreeMap<u32, Vec<&NamedRecord>>,
) -> Result<()> {
    if seed_aux.is_empty() {
        return Ok(());
    }
    let mut location: HashMap<u64, u32> = HashMap::new();
    for (number, records) in pages {
        for r in records {
            if let Some(id) = crate::le::u64_at(&r.bytes, 16) {
                location.entry(id).or_insert(*number);
            }
        }
    }
    let seed_ids: HashSet<u64> = seed_pages
        .iter()
        .flat_map(|p| &p.elements)
        .filter_map(RawElement::id)
        .collect();
    let mut placed: BTreeMap<u32, Vec<AuxRecord>> = BTreeMap::new();
    let mut changed = BTreeSet::new();
    for page in seed_aux {
        placed.entry(page.number).or_default();
        for record in &page.records {
            let target = match location.get(&record.element_id) {
                Some(number) => *number,
                None if seed_ids.contains(&record.element_id) => {
                    changed.insert(page.number);
                    continue;
                }
                None => page.number,
            };
            if target != page.number {
                changed.insert(page.number);
                changed.insert(target);
            }
            placed.entry(target).or_default().push(record.clone());
        }
    }
    for number in changed {
        let seed = seed_aux.iter().find(|p| p.number == number);
        if let Some(page) = seed.filter(|p| !p.complete) {
            // Its undecoded tail would be lost.
            return Err(Error::invalid(
                0,
                format!("seed page {} could not be decoded", page.path),
            ));
        }
        let path = match seed {
            Some(page) => page.path.clone(),
            None => format!(
                "{}/${number}",
                storage(seed_aux.first().map(|p| &p.path), "Dgn^NmA")
            ),
        };
        // A new page takes the version of the seed's first one.
        let version = seed
            .or(seed_aux.first())
            .map(|p| p.header.format_version)
            .unwrap_or(2);
        let records = placed.get(&number).map(Vec::as_slice).unwrap_or(&[]);
        replacements.insert(path, Some(aux_page(records, version)?));
    }
    Ok(())
}

/// An XAttribute page with page number and population 0 (private corpus: 1,188 of 1,188
/// `Dgn^NmA` pages); an empty page is the header alone.
fn aux_page(records: &[AuxRecord], version: u32) -> Result<Vec<u8>> {
    let header = PageHeader {
        record_count: 0,
        format_version: version,
        page_number: 0,
        population: 0,
    };
    if records.is_empty() {
        let mut out = vec![0; 16];
        u32_at(&mut out, 4, version)?;
        return Ok(out);
    }
    let mut page = raster::aux_page(records, header)?;
    u32_at(&mut page, 12, 0)?;
    Ok(page)
}
