//! Structural report of a DGN V8 file: the compound file directory and the pages, as far as
//! a host checks them before handing a file on (see `docs/dgn/FORMAT_NOTES.md`,
//! "Structural report").

use std::collections::BTreeSet;

use cadkit_core::{ReadOptions, Result, Warning};

use crate::cfb::CompoundFile;
use crate::cfb_tree::{
    self, CHILD, ENTRY_LEN, NOSTREAM, OBJECT_TYPE, TreeCheck, directory_entry_offsets,
};
use crate::le;
use crate::native::v8::{self, Page, V8File};

/// Structural facts of a DGN V8 file's container and pages.
///
/// The report states what the file holds; it does not claim that any application accepts
/// the file. Counts that differ between observed writers (the page header's number and
/// population, the directory slot order) are information, not warnings. Recoverable
/// problems are listed in [`warnings`](Self::warnings) with stable codes:
///
/// | Code | Meaning |
/// |---|---|
/// | `dgn.cfb_tree_unordered` | a sibling tree is not ordered by CFB name ([MS-CFB] 2.6.4), reaches a slot twice or points outside the directory: lookups by name can miss entries |
/// | `dgn.cfb_tree_unbalanced` | a sibling tree is ordered, but its colours are not a red-black tree (MS-CFB permits this, an all-black tree for instance) |
/// | `dgn.cfb_tree_shared` | a storage's sibling tree reaches entries an earlier storage's tree holds (an entry with two parents, which [MS-CFB] does not allow); the tree is counted in [`storages`](Self::storages) but not analysed |
/// | `dgn.cfb_free_entry` | a slot the root does not reach is not in the free form of [MS-CFB] 2.6.3 |
/// | `dgn.record_time` | a graphic or control record has no positive finite time at `+0x18` |
/// | `dgn.dictionary_page_split` | a `Dgn^Nm` page starts with a complex component |
/// | `dgn.model_page_counter` | the first dword of a model header stream lies outside [pages holding records, pages] of its graphic and control pages |
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct StructureReport {
    /// Compound file major version (3: 512-byte sectors, 4: 4096-byte sectors).
    pub cfb_version: u16,
    /// Storages, the root included, whose child (sibling) tree is not empty.
    pub storages: u32,
    /// Of [`storages`](Self::storages), the trees that are ordered red-black trees: colours
    /// 0 or 1, a black tree root, no red node with a red child and the same number of black
    /// nodes on every path.
    pub red_black_trees: u32,
    /// Of [`storages`](Self::storages), the trees whose shape equals top-down 2-3-4
    /// red-black insertion of the children in slot order, the shape the writer produces.
    pub top_down_trees: u32,
    /// Directory slots the root does not reach that are not in the MS-CFB free form.
    pub irregular_free_entries: u32,
    /// Whether the reachable entries fill slots 1, 2, ... without a gap in the order the
    /// writer uses. That order is one seed lineage's; observed files differ, and readers
    /// find entries through the sibling trees, not by slot.
    pub writer_slot_order: bool,
    /// Page streams (`$n`) of every model's graphic, control and auxiliary storages and of
    /// `Dgn^Nm` / `Dgn^NmA`.
    pub pages: u32,
    /// Of [`pages`](Self::pages), those whose header stores a page number or population
    /// (`+0x08` / `+0x0C`) other than 0. Public files written by other applications store
    /// both; this is not a defect.
    pub pages_with_number_or_population: u32,
    /// Graphic and control records whose time (`f64` at `+0x18`, milliseconds since 1970)
    /// is not positive and finite.
    pub model_records_without_time: u32,
    /// Numbers of the `Dgn^Nm` pages that start with a complex component.
    pub split_dictionary_pages: Vec<u32>,
    /// Recoverable problems, see the table above.
    pub warnings: Vec<Warning>,
}

impl StructureReport {
    /// Whether the report holds no warning.
    pub fn is_clean(&self) -> bool {
        self.warnings.is_empty()
    }
}

/// Reports the structure of a DGN V8 file without changing it.
///
/// Reads the compound file directory and every page with the limits of `options`.
/// Recoverable problems are [`StructureReport::warnings`].
///
/// # Errors
///
/// `Err` when `bytes` is not a readable DGN V8 compound file or a limit of `options` is
/// exceeded.
pub fn check_structure(bytes: &[u8], options: &ReadOptions) -> Result<StructureReport> {
    let native = v8::read(bytes, options)?;
    let mut report = StructureReport {
        cfb_version: native.cfb_version,
        ..StructureReport::default()
    };
    let cf = CompoundFile::parse(bytes)?;
    directory(bytes, &mut report)?;
    report.writer_slot_order = writer_slot_order(&cf);
    pages(&native, &mut report);
    model_counters(&cf, &native, options, &mut report);
    Ok(report)
}

/// A warning for `count` occurrences of one problem, located at the first one.
fn warn(code: &str, count: u32, what: &str, offset: Option<u64>, object: Option<u64>) -> Warning {
    let mut w = Warning::new(code, format!("{count} {what}"));
    w.offset = offset;
    w.object = object;
    w
}

fn directory(bytes: &[u8], report: &mut StructureReport) -> Result<()> {
    let offsets = directory_entry_offsets(bytes)?;
    let mut unordered = (0u32, None);
    let mut unbalanced = (0u32, None);
    let mut shared = (0u32, None);
    // The root and the storages it reaches, each child tree walked once: every slot is
    // claimed by the first tree that reaches it, and a tree reaching a claimed slot (a
    // subtree several storages share) is reported, not analysed again, so the work stays
    // proportional to the directory. A stale storage entry in an unused slot is an
    // irregular free entry instead.
    let mut claimed = BTreeSet::new();
    let mut todo = Vec::new();
    if !offsets.is_empty() {
        claimed.insert(0u32);
        todo.push(0u32);
    }
    while let Some(slot) = todo.pop() {
        let Some(&off) = offsets.get(slot as usize) else {
            continue;
        };
        let kind = bytes.get(off + OBJECT_TYPE).copied().unwrap_or(0);
        let child = cfb_tree::u32_le(bytes, off + CHILD).unwrap_or(NOSTREAM);
        if child == NOSTREAM {
            continue;
        }
        let (slots, overlaps) = cfb_tree::claim_tree_slots(bytes, &offsets, child, &mut claimed);
        todo.extend(slots.iter().copied());
        if !matches!(kind, 1 | 5) {
            continue;
        }
        report.storages += 1;
        let at = Some(off as u64);
        if overlaps {
            shared.0 += 1;
            shared.1 = shared.1.or(at);
            continue;
        }
        match cfb_tree::check_tree(bytes, &offsets, child) {
            TreeCheck::RedBlack => report.red_black_trees += 1,
            TreeCheck::Unbalanced => {
                unbalanced.0 += 1;
                unbalanced.1 = unbalanced.1.or(at);
            }
            TreeCheck::Unordered => {
                unordered.0 += 1;
                unordered.1 = unordered.1.or(at);
            }
        }
        if cfb_tree::top_down_shape(bytes, &offsets, &slots)
            == cfb_tree::stored_shape(bytes, &offsets, child)
        {
            report.top_down_trees += 1;
        }
    }
    let mut irregular = None;
    for (slot, &off) in offsets.iter().enumerate() {
        if u32::try_from(slot).is_ok_and(|s| claimed.contains(&s)) {
            continue;
        }
        let free = bytes
            .get(off..off.saturating_add(ENTRY_LEN))
            .is_some_and(cfb_tree::is_free_entry);
        if !free {
            report.irregular_free_entries += 1;
            irregular = irregular.or(Some(off as u64));
        }
    }
    for (code, (count, at), what) in [
        (
            "dgn.cfb_tree_unordered",
            unordered,
            "CFB sibling trees are not ordered by name",
        ),
        (
            "dgn.cfb_tree_unbalanced",
            unbalanced,
            "CFB sibling trees are ordered but not red-black",
        ),
        (
            "dgn.cfb_tree_shared",
            shared,
            "CFB storages share sibling tree entries with an earlier storage",
        ),
        (
            "dgn.cfb_free_entry",
            (report.irregular_free_entries, irregular),
            "unused CFB directory slots are not in the free entry form",
        ),
    ] {
        if count > 0 {
            report.warnings.push(warn(code, count, what, at, None));
        }
    }
    Ok(())
}

/// Whether the entries below the root occupy slots 1..=n in the writer's order.
fn writer_slot_order(cf: &CompoundFile<'_>) -> bool {
    let mut slots: Vec<(usize, &str)> = cf
        .paths()
        .iter()
        .map(|p| (p.index, p.path.as_str()))
        .collect();
    slots.sort_unstable();
    let contiguous = slots.iter().zip(1usize..).all(|((slot, _), n)| *slot == n);
    contiguous
        && slots.windows(2).all(|w| match w {
            [(_, a), (_, b)] => cfb_tree::writer_slot_rank(a) < cfb_tree::writer_slot_rank(b),
            _ => true,
        })
}

fn pages(native: &V8File, report: &mut StructureReport) {
    let mut all: Vec<&v8::PageHeader> = Vec::new();
    let mut untimed = (0u32, None);
    for model in &native.models {
        all.extend(model.graphic_pages.iter().map(|p| &p.header));
        all.extend(model.control_pages.iter().map(|p| &p.header));
        all.extend(model.graphic_aux.iter().map(|p| &p.header));
        all.extend(model.control_aux.iter().map(|p| &p.header));
        for raw in model
            .graphic_pages
            .iter()
            .chain(&model.control_pages)
            .flat_map(|p: &Page| &p.elements)
        {
            let timed = le::f64_at(&raw.bytes, 0x18).is_some_and(|t| t.is_finite() && t > 0.0);
            if !timed {
                untimed.0 += 1;
                untimed.1 = untimed.1.or(raw.id());
            }
        }
    }
    all.extend(native.named_pages.iter().map(|p| &p.header));
    all.extend(native.named_aux.iter().map(|p| &p.header));
    report.pages = u32::try_from(all.len()).unwrap_or(u32::MAX);
    report.pages_with_number_or_population = u32::try_from(
        all.iter()
            .filter(|h| h.page_number != 0 || h.population != 0)
            .count(),
    )
    .unwrap_or(u32::MAX);
    report.model_records_without_time = untimed.0;
    if untimed.0 > 0 {
        report.warnings.push(warn(
            "dgn.record_time",
            untimed.0,
            "graphic or control records have no positive finite time at +0x18",
            None,
            untimed.1,
        ));
    }
    report.split_dictionary_pages = native
        .named_pages
        .iter()
        .filter(|p| p.elements.first().is_some_and(|e| e.is_complex_component()))
        .map(|p| p.number)
        .collect();
    if let Ok(count @ 1..) = u32::try_from(report.split_dictionary_pages.len()) {
        report.warnings.push(warn(
            "dgn.dictionary_page_split",
            count,
            "dictionary pages start with a complex component",
            None,
            None,
        ));
    }
}

/// Checks the first dword of every model header stream against the model's pages.
fn model_counters(
    cf: &CompoundFile<'_>,
    native: &V8File,
    options: &ReadOptions,
    report: &mut StructureReport,
) {
    let max = usize::try_from(options.limits.max_decompressed_bytes).unwrap_or(usize::MAX);
    for model in &native.models {
        let Some(header) = &model.header else {
            continue;
        };
        let path = format!("Dgn-Md/{}/Dgn~Mh", model.storage);
        let Some(index) = cf.find(&path) else {
            continue;
        };
        let Ok(raw) = cf.read_stream(index) else {
            continue;
        };
        let inflated = crate::zlib::inflate(&raw, max);
        let prefix = inflated.data.len().saturating_sub(header.raw.len());
        let Some(counter) = le::u32_at(&inflated.data, 0).filter(|_| prefix >= 4) else {
            continue;
        };
        let pages = model.graphic_pages.iter().chain(&model.control_pages);
        let total = pages.clone().count();
        let filled = pages.filter(|p| p.header.record_count > 0).count();
        if !(filled..=total).contains(&(counter as usize)) {
            report.warnings.push(Warning::new(
                "dgn.model_page_counter",
                format!(
                    "{path}: first dword {counter} lies outside {filled}..={total} graphic and control pages"
                ),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;
    use crate::native::v8::tests::{element, model_header_stream, page};
    use cadkit_core::{Error, Limits};

    /// A file the test container builder lays out: sibling trees as red right-leaning chains
    /// in insertion order, zeroed slots after the last entry, page headers with number and
    /// population, records without a time, a dictionary page opening with a component and a
    /// model header whose first dword (0) undercounts the one graphic page.
    fn odd_file() -> Vec<u8> {
        let line = element(3, &[0u8; 0x68 - 0x0c + 48], &[]);
        let component = element(0x4000_005f, &[0u8; 0x20], &[]);
        crate::cfb::tests::build(&[
            ("Dgn~H", vec![0u8; 64]),
            (
                "Dgn-Md/#000000/Dgn~Mh",
                model_header_stream(true, 1000.0, "M"),
            ),
            ("Dgn-Md/#000000/Dgn^G/$1", page(std::slice::from_ref(&line))),
            ("Dgn^Nm/$1", page(&[component])),
        ])
    }

    #[test]
    fn reports_every_warning_code_on_a_synthetic_file() {
        let r = check_structure(&odd_file(), &ReadOptions::default()).unwrap();
        assert_eq!(r.cfb_version, 3);
        // Root, Dgn-Md, #000000, Dgn^G and Dgn^Nm hold child trees.
        assert_eq!(r.storages, 5);
        assert_eq!(r.red_black_trees, 0, "red tree roots");
        // Nine entries in three 4-slot sectors: three zeroed unused slots.
        assert_eq!(r.irregular_free_entries, 3);
        assert!(!r.writer_slot_order);
        assert_eq!((r.pages, r.pages_with_number_or_population), (2, 2));
        assert_eq!(r.model_records_without_time, 1);
        assert_eq!(r.split_dictionary_pages, [1]);
        let codes: Vec<&str> = r.warnings.iter().map(|w| w.code.as_str()).collect();
        for code in [
            "dgn.cfb_tree_unbalanced",
            "dgn.cfb_free_entry",
            "dgn.record_time",
            "dgn.dictionary_page_split",
            "dgn.model_page_counter",
        ] {
            assert!(codes.contains(&code), "{code} in {codes:?}");
        }
        assert!(!r.is_clean());
    }

    /// A version 3 compound file whose root holds `Dgn~H` and `m` storages that all point at
    /// one subtree of `n` streams (a right-leaning chain of black entries). cadkit's reader
    /// visits every slot once and accepts it; a checker that analyses the subtree per storage
    /// does `m` times the work.
    fn shared_subtree_file(m: u32, n: u32) -> Vec<u8> {
        const NOS: u32 = u32::MAX;
        const EOC: u32 = 0xffff_fffe;
        let entry = |name: &str, kind: u8, right: u32, child: u32, start: u32, size: u64| {
            let mut e = vec![0u8; 128];
            let units: Vec<u16> = name.encode_utf16().collect();
            for (i, u) in units.iter().enumerate() {
                e[2 * i..2 * i + 2].copy_from_slice(&u.to_le_bytes());
            }
            let len = (units.len() as u16 + 1) * 2;
            e[0x40..0x42].copy_from_slice(&len.to_le_bytes());
            e[0x42] = kind;
            e[0x43] = 1;
            e[0x44..0x48].copy_from_slice(&NOS.to_le_bytes());
            e[0x48..0x4c].copy_from_slice(&right.to_le_bytes());
            e[0x4c..0x50].copy_from_slice(&child.to_le_bytes());
            e[0x74..0x78].copy_from_slice(&start.to_le_bytes());
            e[0x78..0x80].copy_from_slice(&size.to_le_bytes());
            e
        };
        let subtree = 2 + m;
        let mut entries = vec![
            entry("Root Entry", 5, NOS, 1, EOC, 0),
            entry("Dgn~H", 2, if m > 0 { 2 } else { NOS }, NOS, 0, 4096),
        ];
        for i in 0..m {
            let right = if i + 1 < m { 3 + i } else { NOS };
            entries.push(entry(&format!("d{i}"), 1, right, subtree, EOC, 0));
        }
        for j in 0..n {
            let right = if j + 1 < n { subtree + 1 + j } else { NOS };
            entries.push(entry(&format!("s{j}"), 2, right, NOS, EOC, 0));
        }
        while entries.len() % 4 != 0 {
            // The MS-CFB 2.6.3 free form: zeros, sibling and child ids NOSTREAM.
            let mut free = vec![0u8; 128];
            free[0x44..0x50].fill(0xff);
            entries.push(free);
        }
        // Sectors: 8 for the 4096-byte `Dgn~H`, then the directory, then the FAT.
        let data_sectors = 8u32;
        let dir_sectors = (entries.len() / 4) as u32;
        let used = data_sectors + dir_sectors;
        let mut fat_sectors = 1u32;
        while fat_sectors * 128 < used + fat_sectors {
            fat_sectors += 1;
        }
        assert!(fat_sectors <= 109);
        let mut fat: Vec<u32> = Vec::new();
        for s in 0..data_sectors {
            fat.push(if s + 1 < data_sectors { s + 1 } else { EOC });
        }
        for k in 0..dir_sectors {
            let s = data_sectors + k;
            fat.push(if k + 1 < dir_sectors { s + 1 } else { EOC });
        }
        fat.extend(std::iter::repeat_n(0xffff_fffd, fat_sectors as usize));
        fat.resize(fat_sectors as usize * 128, NOS);
        let mut out = vec![0u8; 512];
        out[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
        for (at, v) in [
            (0x18, 0x3eu16),
            (0x1a, 3),
            (0x1c, 0xfffe),
            (0x1e, 9),
            (0x20, 6),
        ] {
            out[at..at + 2].copy_from_slice(&v.to_le_bytes());
        }
        for (at, v) in [
            (0x2c, fat_sectors),
            (0x30, data_sectors),
            (0x38, 4096),
            (0x3c, EOC),
            (0x44, EOC),
        ] {
            out[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        for i in 0..109u32 {
            let v = if i < fat_sectors { used + i } else { NOS };
            let at = 0x4c + 4 * i as usize;
            out[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        out.extend(vec![0u8; 4096]);
        for e in entries {
            out.extend(e);
        }
        for v in fat {
            out.extend(v.to_le_bytes());
        }
        out
    }

    #[test]
    fn storages_sharing_a_subtree_are_reported_once_each_without_reanalysis() {
        let (m, n) = (300u32, 300u32);
        let file = shared_subtree_file(m, n);
        let r = check_structure(&file, &ReadOptions::default()).unwrap();
        // The root and every storage hold a child tree; the shared subtree is analysed for
        // the first storage that reaches it only (one all-black chain: ordered, not
        // red-black), and the other m - 1 storages are reported instead of analysed.
        assert_eq!(r.storages, m + 1);
        let count = |code: &str| {
            r.warnings
                .iter()
                .find(|w| w.code == code)
                .map(|w| w.message.split(' ').next().unwrap().parse::<u32>().unwrap())
        };
        assert_eq!(count("dgn.cfb_tree_shared"), Some(m - 1));
        assert_eq!(count("dgn.cfb_tree_unbalanced"), Some(1));
        // The root's chain ("Dgn~H" before the shorter "d0") is not in name order.
        assert_eq!(count("dgn.cfb_tree_unordered"), Some(1));
        assert_eq!(r.red_black_trees + r.top_down_trees, 0);
        assert_eq!(r.irregular_free_entries, 0);
        // One storage only: nothing is shared, and the tree is analysed as before.
        let single = check_structure(&shared_subtree_file(1, n), &ReadOptions::default()).unwrap();
        assert!(
            single
                .warnings
                .iter()
                .all(|w| w.code != "dgn.cfb_tree_shared")
        );
        assert_eq!(single.storages, 2);
        // The reader visits every slot once: it accepts the file with the n + 1 streams.
        let doc = crate::read(&file, &ReadOptions::default()).unwrap();
        assert_eq!(
            doc.props.get("dgn.stream_count"),
            Some(&cadkit_core::Value::Int(i64::from(n) + 1))
        );
        assert!(doc.models.is_empty());
        assert_eq!(
            v8::read(&file, &ReadOptions::default())
                .unwrap()
                .cfb_version,
            3
        );
    }

    #[test]
    fn unordered_trees_are_reported_and_bad_input_fails() {
        // The builder chains siblings to the right in insertion order: "Zz" before "Aa"
        // breaks the name order.
        let file =
            crate::cfb::tests::build(&[("Dgn~H", vec![0u8; 64]), ("Zz", vec![1]), ("Aa", vec![2])]);
        let r = check_structure(&file, &ReadOptions::default()).unwrap();
        assert!(
            r.warnings
                .iter()
                .any(|w| w.code == "dgn.cfb_tree_unordered" && w.offset.is_some())
        );
        assert!(check_structure(b"not a compound file", &ReadOptions::default()).is_err());
        let not_dgn = crate::cfb::tests::build(&[("WordDocument", vec![0u8; 64])]);
        assert!(matches!(
            check_structure(&not_dgn, &ReadOptions::default()),
            Err(Error::UnknownFormat)
        ));
        let tight = ReadOptions::default().with_limits(Limits::default().with_max_input_bytes(64));
        assert!(matches!(
            check_structure(&odd_file(), &tight),
            Err(Error::LimitExceeded { .. })
        ));
        // Truncated and bit-flipped input returns without panicking.
        let file = odd_file();
        for cut in (0..file.len()).step_by(97) {
            let _ = check_structure(&file[..cut], &ReadOptions::default());
        }
        for at in (0..file.len()).step_by(61) {
            let mut flipped = file.clone();
            flipped[at] ^= 0xa5;
            let _ = check_structure(&flipped, &ReadOptions::default());
        }
    }
}
