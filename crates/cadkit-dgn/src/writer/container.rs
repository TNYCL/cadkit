//! CFB container writer that preserves streams, directory metadata, and resource limits.

use crate::cfb_tree::{
    CHILD, COLOR, ENTRY_LEN, LEFT, NOSTREAM, OBJECT_TYPE, RIGHT, directory_entry_offsets,
    free_entry, reachable_slots, top_down_shape, tree_slots, u32_le, writer_slot_rank,
};
use cadkit_core::LimitKind;
use cadkit_core::{Error, ReadOptions, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Seek, SeekFrom, Write},
    path::{Component, Path},
};

pub(super) struct BoundedCursor {
    inner: Cursor<Vec<u8>>,
    limit: u64,
    exceeded: bool,
}
impl BoundedCursor {
    fn new(limit: u64) -> Self {
        Self {
            inner: Cursor::new(vec![]),
            limit,
            exceeded: false,
        }
    }
    pub(super) fn bytes(self) -> Vec<u8> {
        self.inner.into_inner()
    }
}
impl Read for BoundedCursor {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(b)
    }
}
impl Seek for BoundedCursor {
    fn seek(&mut self, p: SeekFrom) -> std::io::Result<u64> {
        let old = self.inner.position();
        let result = self.inner.seek(p)?;
        if result > self.limit {
            self.inner.set_position(old);
            self.exceeded = true;
            return Err(std::io::Error::other("CFB output limit exceeded"));
        }
        Ok(result)
    }
}
impl Write for BoundedCursor {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if self
            .inner
            .position()
            .checked_add(b.len() as u64)
            .is_none_or(|n| n > self.limit)
        {
            self.exceeded = true;
            return Err(std::io::Error::other("CFB output limit exceeded"));
        }
        self.inner.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn map_cfb_io(e: std::io::Error) -> Error {
    if e.to_string().contains("CFB output limit exceeded") {
        Error::limit(LimitKind::Output, "CFB output limit exceeded".into())
    } else {
        Error::Io(e)
    }
}

pub(super) fn rewrite(
    seed: &[u8],
    replacements: &BTreeMap<String, Option<Vec<u8>>>,
    options: &ReadOptions,
) -> Result<Vec<u8>> {
    if seed.len() as u64 > options.limits.max_input_bytes {
        return Err(Error::limit(LimitKind::InputBytes, "DGN seed bytes".into()));
    }
    let root_color = seed_root_color(seed);
    let mut source = ::cfb::CompoundFile::open(Cursor::new(seed))?;
    let mut entries = vec![];
    let mut budget = 0u64;
    for entry in source.walk() {
        if entries.len() as u64 >= options.limits.max_objects {
            return Err(Error::limit(LimitKind::Objects, "CFB entries".into()));
        }
        budget = budget
            .checked_add(entry.len())
            .ok_or_else(|| Error::limit(LimitKind::Objects, "CFB streams".into()))?;
        if budget > options.limits.max_decompressed_bytes {
            return Err(Error::limit(
                LimitKind::DecompressedBytes,
                "CFB streams".into(),
            ));
        }
        entries.push(entry);
    }
    let existing: BTreeSet<String> = entries
        .iter()
        .filter(|e| !e.is_root())
        .map(|e| stream_key(e.path()))
        .collect();
    enum Item<'a> {
        Seed(String, &'a ::cfb::Entry),
        New(&'a str, &'a [u8]),
    }
    impl<'a> Item<'a> {
        fn key(&self) -> &str {
            match self {
                Self::Seed(k, _) => k.as_str(),
                Self::New(k, _) => k,
            }
        }
    }
    let mut items: Vec<Item<'_>> = entries
        .iter()
        .filter(|e| !e.is_root())
        .map(|e| Item::Seed(stream_key(e.path()), e))
        .chain(replacements.iter().filter_map(|(name, data)| {
            data.as_deref()
                .filter(|_| !existing.contains(name))
                .map(|bytes| Item::New(name.as_str(), bytes))
        }))
        .collect();
    // Slot order is one seed lineage's (`writer_slot_rank`); lookups use the sibling trees.
    items.sort_by(|a, b| writer_slot_rank(a.key()).cmp(&writer_slot_rank(b.key())));
    let mut bytes = (|| -> std::io::Result<Vec<u8>> {
        let mut output = ::cfb::CompoundFile::create_with_version(
            source.version(),
            BoundedCursor::new(options.limits.max_input_bytes),
        )?;
        for item in items {
            match item {
                Item::Seed(key, entry) => {
                    if entry.is_storage() {
                        output.create_storage_all(entry.path())?;
                        continue;
                    }
                    if !entry.is_stream() {
                        continue;
                    }
                    match replacements.get(&key) {
                        Some(None) => {}
                        Some(Some(bytes)) => {
                            if let Some(parent) = entry.path().parent()
                                && parent != Path::new("/")
                                && !parent.as_os_str().is_empty()
                            {
                                output.create_storage_all(parent)?;
                            }
                            output.create_stream(entry.path())?.write_all(bytes)?;
                        }
                        None => {
                            if let Some(parent) = entry.path().parent()
                                && parent != Path::new("/")
                                && !parent.as_os_str().is_empty()
                            {
                                output.create_storage_all(parent)?;
                            }
                            let mut src = source.open_stream(entry.path())?;
                            let mut dst = output.create_stream(entry.path())?;
                            std::io::copy(&mut src.by_ref().take(entry.len()), &mut dst)?;
                        }
                    }
                }
                Item::New(name, data) => {
                    let path = format!("/{name}");
                    if let Some((parent, _)) = path.rsplit_once('/')
                        && !parent.is_empty()
                    {
                        output.create_storage_all(parent)?;
                    }
                    output.create_stream(&path)?.write_all(data)?;
                }
            }
        }
        let created: Vec<_> = output
            .walk()
            .filter(|e| !source.exists(e.path()))
            .map(|e| e.path().to_owned())
            .collect();
        let seed_time = source.entry("/")?.created();
        for path in created {
            output.set_created_time(&path, seed_time)?;
            output.set_modified_time(&path, seed_time)?;
        }
        for entry in &entries {
            if !output.exists(entry.path()) {
                continue;
            }
            if entry.is_storage() {
                output.set_storage_clsid(entry.path(), *entry.clsid())?;
            }
            output.set_state_bits(entry.path(), entry.state_bits())?;
            output.set_created_time(entry.path(), entry.created())?;
            output.set_modified_time(entry.path(), entry.modified())?;
        }
        output.flush()?;
        Ok(output.into_inner().bytes())
    })()
    .map_err(map_cfb_io)?;
    balance_directory(&mut bytes, root_color)?;
    Ok(bytes)
}

fn put_u32_le(data: &mut [u8], offset: usize, value: u32) -> Result<()> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| Error::invalid(offset as u64, "CFB directory offset overflow"))?;
    data.get_mut(offset..end)
        .ok_or_else(|| Error::invalid(offset as u64, "CFB directory offset out of bounds"))?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

/// Returns the root entry's `color_flag` from `seed`; it is 0 in every observed V8 file
/// (567 private, 3 public), whose writers leave the root entry red.
pub(super) fn seed_root_color(seed: &[u8]) -> u8 {
    directory_entry_offsets(seed)
        .ok()
        .and_then(|offsets| offsets.first().copied())
        .and_then(|off| seed.get(off + COLOR).copied())
        .unwrap_or(0)
}

/// Rebalances every storage's child sibling tree in `data` using Sedgewick's top-down 2-3-4
/// red-black insertion in directory slot order, matching Windows Structured Storage (`ole32`),
/// and writes every slot the root does not reach in the free entry form of [MS-CFB] 2.6.3.
fn balance_directory(data: &mut [u8], root_color: u8) -> Result<()> {
    let offsets = directory_entry_offsets(data)?;
    if let Some(&root_off) = offsets.first()
        && let Some(color_byte) = data.get_mut(root_off + COLOR)
    {
        *color_byte = root_color;
    }
    for &storage_off in &offsets {
        let kind = data.get(storage_off + OBJECT_TYPE).copied().unwrap_or(0);
        if !matches!(kind, 1 | 5) {
            continue;
        }
        let first_child = u32_le(data, storage_off + CHILD).unwrap_or(NOSTREAM);
        if first_child == NOSTREAM {
            continue;
        }
        let children = tree_slots(data, &offsets, first_child);
        let shape = top_down_shape(data, &offsets, &children);
        put_u32_le(data, storage_off + CHILD, shape.root.unwrap_or(NOSTREAM))?;
        for (id, (left, right, red)) in shape.nodes {
            if let Some(&off) = offsets.get(id as usize) {
                if let Some(color_byte) = data.get_mut(off + COLOR) {
                    *color_byte = u8::from(!red);
                }
                put_u32_le(data, off + LEFT, left.unwrap_or(NOSTREAM))?;
                put_u32_le(data, off + RIGHT, right.unwrap_or(NOSTREAM))?;
            }
        }
    }
    // The `cfb` crate writes unused slots with a name length of 2; MS-CFB 2.6.3 requires all
    // zeroes except the three ids, the form 823 of the 829 unused slots of the private
    // corpus and the 3 of the public TreeText sample (saved by MicroStation 8.11) have.
    let reachable = reachable_slots(data, &offsets);
    for (slot, &off) in offsets.iter().enumerate() {
        if u32::try_from(slot).is_ok_and(|s| reachable.contains(&s)) {
            continue;
        }
        data.get_mut(off..off.saturating_add(ENTRY_LEN))
            .ok_or_else(|| Error::invalid(off as u64, "CFB directory entry out of bounds"))?
            .copy_from_slice(&free_entry());
    }
    Ok(())
}

/// Replacement key of a CFB entry: its names joined by `/`, without a leading slash.
/// The `cfb` crate builds entry paths with `PathBuf::push`, which separates them with `\` on
/// Windows, so the platform string form of the path cannot be used as the key.
fn stream_key(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub(super) fn stream(seed: &[u8], name: &str, max: u64) -> Result<Vec<u8>> {
    let mut cf = ::cfb::CompoundFile::open(Cursor::new(seed))?;
    let n = cf.entry(name)?.len();
    if n > max {
        return Err(Error::limit(
            LimitKind::InputBytes,
            "DGN seed stream".into(),
        ));
    }
    let mut data = vec![];
    cf.open_stream(name)?.take(n).read_to_end(&mut data)?;
    Ok(data)
}

/// Directory metadata of a container built without a seed by [`build`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct BuildMeta {
    /// Colour flag of the root entry: 0 (red) in every observed V8 file (567 private, 3
    /// public), as [`seed_root_color`] reads it.
    pub root_color: u8,
    /// Modification time (FILETIME, 100 ns since 1601) of the root and of every storage, and
    /// creation time of every storage; 0 writes the zero form. The root's creation time is
    /// zero and streams keep zero times, as [MS-CFB] 2.6.1 requires: the `cfb` crate reads
    /// a stream's times as zero (and its strict mode rejects others), so the seed-based
    /// [`rewrite`] writes them zero too, although the observed MicroStation files store a
    /// time on every entry.
    pub filetime: u64,
}

/// Offsets of the creation and modification times in a directory entry ([MS-CFB] 2.6.1).
const CREATED: usize = 0x64;
const MODIFIED: usize = 0x6c;

/// Builds a version 3 compound file from `streams` without a seed. Keys are entry names
/// joined by `/` without a leading slash (as [`stream_key`] gives them); every parent path
/// becomes a storage, and so does every key of `storages` (a storage without streams, which
/// the seed-based [`rewrite`] keeps from its seed). Entries are created in [`writer_slot_rank`] order, every sibling tree
/// is rebuilt by top-down red-black insertion and unused slots take the [MS-CFB] 2.6.3 free
/// form ([`balance_directory`]), as [`rewrite`] does; CLSIDs are null and state bits 0 (every
/// observed V8 file has null CLSIDs). Only the given bytes are written, so no byte of a stream
/// left out reaches the output. The output is limited to `max_input_bytes` of `options`.
pub(super) fn build(
    streams: &BTreeMap<String, Vec<u8>>,
    extra_storages: &BTreeSet<String>,
    meta: BuildMeta,
    options: &ReadOptions,
) -> Result<Vec<u8>> {
    if (streams.len() as u64).saturating_add(extra_storages.len() as u64)
        >= options.limits.max_objects
    {
        return Err(Error::limit(LimitKind::Objects, "CFB entries".into()));
    }
    let valid = |key: &str| !key.is_empty() && !key.split('/').any(str::is_empty);
    let mut total = 0u64;
    let mut storages: BTreeSet<&str> = BTreeSet::new();
    for key in streams.keys().chain(extra_storages) {
        if !valid(key) {
            return Err(Error::invalid(0, "empty CFB entry name"));
        }
        for (i, c) in key.char_indices() {
            if c == '/'
                && let Some(parent) = key.get(..i)
            {
                storages.insert(parent);
            }
        }
    }
    storages.extend(extra_storages.iter().map(String::as_str));
    for data in streams.values() {
        total = total
            .checked_add(data.len() as u64)
            .ok_or_else(|| Error::limit(LimitKind::Output, "CFB streams".into()))?;
    }
    if total > options.limits.max_input_bytes {
        return Err(Error::limit(LimitKind::Output, "CFB streams".into()));
    }
    if storages.iter().any(|s| streams.contains_key(*s)) {
        return Err(Error::invalid(0, "CFB path is both a storage and a stream"));
    }
    enum Item<'a> {
        Storage(&'a str),
        Stream(&'a str, &'a [u8]),
    }
    impl Item<'_> {
        fn key(&self) -> &str {
            match self {
                Self::Storage(k) | Self::Stream(k, _) => k,
            }
        }
    }
    let mut items: Vec<Item<'_>> = storages
        .iter()
        .map(|s| Item::Storage(s))
        .chain(streams.iter().map(|(k, d)| Item::Stream(k, d)))
        .collect();
    items.sort_by(|a, b| writer_slot_rank(a.key()).cmp(&writer_slot_rank(b.key())));
    let mut bytes = (|| -> std::io::Result<Vec<u8>> {
        let mut output = ::cfb::CompoundFile::create_with_version(
            ::cfb::Version::V3,
            BoundedCursor::new(options.limits.max_input_bytes),
        )?;
        for item in &items {
            match item {
                Item::Storage(key) => output.create_storage_all(format!("/{key}"))?,
                Item::Stream(key, data) => {
                    if let Some((parent, _)) = key.rsplit_once('/') {
                        output.create_storage_all(format!("/{parent}"))?;
                    }
                    output.create_stream(format!("/{key}"))?.write_all(data)?;
                }
            }
        }
        output.flush()?;
        Ok(output.into_inner().bytes())
    })()
    .map_err(map_cfb_io)?;
    balance_directory(&mut bytes, meta.root_color)?;
    let offsets = directory_entry_offsets(&bytes)?;
    for slot in reachable_slots(&bytes, &offsets) {
        let Some(&off) = offsets.get(slot as usize) else {
            continue;
        };
        match bytes.get(off + OBJECT_TYPE) {
            // [MS-CFB] 2.6.1: the root storage's creation time MUST be all zeroes.
            Some(5) => {
                put_u64_le(&mut bytes, off + CREATED, 0)?;
                put_u64_le(&mut bytes, off + MODIFIED, meta.filetime)?;
            }
            Some(1) => {
                put_u64_le(&mut bytes, off + CREATED, meta.filetime)?;
                put_u64_le(&mut bytes, off + MODIFIED, meta.filetime)?;
            }
            _ => {}
        }
    }
    Ok(bytes)
}

fn put_u64_le(data: &mut [u8], offset: usize, value: u64) -> Result<()> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| Error::invalid(offset as u64, "CFB directory offset overflow"))?;
    data.get_mut(offset..end)
        .ok_or_else(|| Error::invalid(offset as u64, "CFB directory offset out of bounds"))?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

/// Every stream of the compound file `bytes` by [`stream_key`], read within the limits of
/// `options` (input bytes, entry count, total stream bytes) like [`rewrite`].
pub(super) fn read_streams(
    bytes: &[u8],
    options: &ReadOptions,
) -> Result<BTreeMap<String, Vec<u8>>> {
    if bytes.len() as u64 > options.limits.max_input_bytes {
        return Err(Error::limit(LimitKind::InputBytes, "DGN seed bytes".into()));
    }
    let mut cf = ::cfb::CompoundFile::open(Cursor::new(bytes))?;
    let mut entries = vec![];
    let mut budget = 0u64;
    for entry in cf.walk() {
        if entries.len() as u64 >= options.limits.max_objects {
            return Err(Error::limit(LimitKind::Objects, "CFB entries".into()));
        }
        if !entry.is_stream() {
            continue;
        }
        budget = budget
            .checked_add(entry.len())
            .ok_or_else(|| Error::limit(LimitKind::Objects, "CFB streams".into()))?;
        if budget > options.limits.max_decompressed_bytes {
            return Err(Error::limit(
                LimitKind::DecompressedBytes,
                "CFB streams".into(),
            ));
        }
        entries.push((entry.path().to_owned(), entry.len()));
    }
    let mut out = BTreeMap::new();
    for (path, len) in entries {
        let mut data = vec![];
        cf.open_stream(&path)?.take(len).read_to_end(&mut data)?;
        out.insert(stream_key(&path), data);
    }
    Ok(out)
}

/// Every storage of the compound file `bytes` by [`stream_key`].
#[cfg(test)]
pub(super) fn read_storages(bytes: &[u8]) -> Result<BTreeSet<String>> {
    let cf = ::cfb::CompoundFile::open(Cursor::new(bytes))?;
    Ok(cf
        .walk()
        .filter(|e| e.is_storage() && !e.is_root())
        .map(|e| stream_key(e.path()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfb_tree::{cmp_cfb_name, entry_utf16_name, is_free_entry};
    use std::path::PathBuf;

    #[test]
    fn stream_key_uses_slashes_on_every_platform() {
        // Built the way the `cfb` crate builds entry paths, so the separator is the
        // platform's own (`\` on Windows).
        let mut path = PathBuf::from("/");
        for name in ["Dgn-Md", "#000000", "Dgn^G", "$1"] {
            path.push(name);
        }
        assert_eq!(stream_key(&path), "Dgn-Md/#000000/Dgn^G/$1");
        assert_eq!(stream_key(Path::new("/Dgn~H")), "Dgn~H");
    }

    /// Clears the times of every entry (and the CLSID and state bits, which the rewrite copies
    /// from the seed) so two containers compare by layout alone.
    fn without_entry_metadata(mut data: Vec<u8>) -> Vec<u8> {
        let offsets = directory_entry_offsets(&data).unwrap();
        for off in offsets {
            data[off + 0x50..off + 0x74].fill(0);
        }
        data
    }

    /// [`build`] given the streams of a rewritten file lays them out as [`rewrite`] did:
    /// the same sectors, directory slots, trees and free entries.
    fn assert_build_matches_rewrite(seed: &[u8]) {
        let read = ReadOptions::default();
        let rewritten = rewrite(seed, &BTreeMap::new(), &read).unwrap();
        let streams = read_streams(&rewritten, &read).unwrap();
        let meta = BuildMeta {
            root_color: seed_root_color(&rewritten),
            filetime: 0,
        };
        let storages = read_storages(&rewritten).unwrap();
        let built = build(&streams, &storages, meta, &read).unwrap();
        ::cfb::CompoundFile::open_strict(Cursor::new(built.clone())).expect("valid CFB");
        let (built, rewritten) = (
            without_entry_metadata(built),
            without_entry_metadata(rewritten),
        );
        let first_difference = built.iter().zip(&rewritten).position(|(a, b)| a != b);
        assert_eq!(
            (first_difference, built.len()),
            (None, rewritten.len()),
            "first differing byte"
        );
    }

    #[test]
    fn build_lays_out_streams_as_the_rewrite_does() {
        assert_build_matches_rewrite(&balances_directory(::cfb::Version::V3));
        for rel in [
            "public/gdal/test_dgnv8.dgn",
            "public/safe/TreeTextNodeLabelsWithTags.dgn",
            "public/safe/Water_distribution_mains.dgn",
        ] {
            if let Ok(seed) = std::fs::read(crate::fuzz_tests::corpus_root().join(rel)) {
                assert_build_matches_rewrite(&seed);
            }
        }
    }

    #[test]
    fn build_writes_the_given_times_and_rejects_bad_paths() {
        let read = ReadOptions::default();
        let mut streams = BTreeMap::new();
        streams.insert("Dgn~H".to_string(), vec![1u8; 100]);
        streams.insert("Dgn-Md/#000000/Dgn^G/$1".to_string(), vec![2u8; 5000]);
        streams.insert("Dgn^Nm/$1".to_string(), vec![3u8; 16]);
        let meta = BuildMeta {
            root_color: 0,
            filetime: 0x01d9_0000_0000_0000,
        };
        let built = build(&streams, &BTreeSet::new(), meta, &read).unwrap();
        ::cfb::CompoundFile::open_strict(Cursor::new(built.clone())).expect("valid CFB");
        assert_eq!(read_streams(&built, &read).unwrap(), streams);
        let offsets = directory_entry_offsets(&built).unwrap();
        let reachable = reachable_slots(&built, &offsets);
        // Root, 3 streams and the storages Dgn-Md, #000000, Dgn^G and Dgn^Nm.
        assert_eq!(reachable.len(), 8);
        for (slot, &off) in offsets.iter().enumerate() {
            if reachable.contains(&(slot as u32)) {
                // MS-CFB 2.6.1: storages carry the time, the root its modification time
                // only (its creation time MUST be zero), streams none.
                let (created, modified) = match built[off + OBJECT_TYPE] {
                    5 => (0, meta.filetime),
                    1 => (meta.filetime, meta.filetime),
                    _ => (0, 0),
                };
                assert_eq!(crate::le::u64_at(&built, off + CREATED), Some(created));
                assert_eq!(crate::le::u64_at(&built, off + MODIFIED), Some(modified));
            } else {
                assert!(is_free_entry(&built[off..off + ENTRY_LEN]));
            }
        }
        assert_eq!(
            String::from_utf16_lossy(&entry_utf16_name(&built, offsets[1])),
            "Dgn~H"
        );
        assert_eq!(
            build(&streams, &BTreeSet::new(), meta, &read).unwrap(),
            built,
            "deterministic"
        );

        let mut bad = streams.clone();
        bad.insert("Dgn^Nm".to_string(), vec![]);
        assert!(
            build(&bad, &BTreeSet::new(), meta, &read).is_err(),
            "storage and stream"
        );
        let mut bad = streams.clone();
        bad.insert("Dgn^Nm//$2".to_string(), vec![]);
        assert!(
            build(&bad, &BTreeSet::new(), meta, &read).is_err(),
            "empty name"
        );
        let mut tight = ReadOptions::default();
        tight.limits.max_input_bytes = 4000;
        assert!(matches!(
            build(&streams, &BTreeSet::new(), meta, &tight),
            Err(Error::LimitExceeded { .. })
        ));
        // A storage without streams is kept when asked for.
        let empty = BTreeSet::from(["Dgn^GA".to_string()]);
        let with_empty = build(&streams, &empty, meta, &read).unwrap();
        assert!(read_storages(&with_empty).unwrap().contains("Dgn^GA"));
        assert!(matches!(
            read_streams(&built, &tight),
            Err(Error::LimitExceeded { .. })
        ));
    }

    #[test]
    fn rewrite_balances_cfb_directory_as_red_black_trees() {
        let seed = balances_directory(::cfb::Version::V3);

        // Replacing a stream with data larger than `max_input_bytes` yields LimitExceeded.
        let mut tight = ReadOptions::default();
        tight.limits.max_input_bytes = seed.len() as u64;
        let mut replacements = BTreeMap::new();
        replacements.insert("Dgn~H".into(), Some(vec![0u8; 64 * 1024]));
        assert!(matches!(
            rewrite(&seed, &replacements, &tight),
            Err(Error::LimitExceeded { .. })
        ));
    }

    /// Version 4 files have 4096-byte sectors, and the header fills a whole one: the rewrite
    /// once took sector `n` to start at `512 + n * 4096`, so it rebalanced FAT bytes as if they
    /// were directory entries and wrote colour flags into the FAT (an output `cfb` refused to
    /// open as a seed: "Malformed FAT").
    #[test]
    fn rewrite_balances_cfb_v4_directory_and_leaves_the_fat_alone() {
        balances_directory(::cfb::Version::V4);
    }

    /// Rewrites a small seed of `version` and checks every sibling tree; returns the seed.
    fn balances_directory(version: ::cfb::Version) -> Vec<u8> {
        let mut cf =
            ::cfb::CompoundFile::create_with_version(version, Cursor::new(Vec::new())).unwrap();
        for name in ["Dgn~H", "Dgn~S", "Dgn~Mf"] {
            cf.create_stream(name).unwrap().write_all(b"hdr").unwrap();
        }
        cf.create_storage_all("Dgn-Md/#000000/Dgn^G").unwrap();
        cf.create_storage_all("Dgn^Nm").unwrap();
        for n in 1..=13 {
            cf.create_stream(format!("Dgn^Nm/${n}"))
                .unwrap()
                .write_all(&[n as u8; 16])
                .unwrap();
        }
        cf.flush().unwrap();
        let seed = cf.into_inner().into_inner();
        let out = rewrite(&seed, &BTreeMap::new(), &ReadOptions::default()).unwrap();
        ::cfb::CompoundFile::open_strict(Cursor::new(out.clone())).expect("output is a valid CFB");
        let offsets = directory_entry_offsets(&out).unwrap();
        // Unused slots: the `cfb` crate writes them with a name length of 2, the output holds
        // the MS-CFB 2.6.3 free form, and both `cfb` and cadkit's reader accept it, again
        // as a seed too.
        let unused = |data: &[u8]| -> Vec<bool> {
            let offsets = directory_entry_offsets(data).unwrap();
            let reachable = reachable_slots(data, &offsets);
            (0..offsets.len() as u32)
                .filter(|s| !reachable.contains(s))
                .map(|s| is_free_entry(&data[offsets[s as usize]..][..ENTRY_LEN]))
                .collect()
        };
        let before = unused(&seed);
        assert!(!before.is_empty() && before.iter().any(|free| !free));
        let after = unused(&out);
        assert_eq!(after.len(), before.len());
        assert!(after.iter().all(|free| *free));
        let paths: Vec<String> = crate::cfb::CompoundFile::parse(&out)
            .unwrap()
            .paths()
            .iter()
            .map(|p| p.path.clone())
            .collect();
        assert!(paths.iter().any(|p| p == "Dgn^Nm/$13"));
        let again = rewrite(&out, &BTreeMap::new(), &ReadOptions::default()).unwrap();
        ::cfb::CompoundFile::open_strict(Cursor::new(again.clone())).expect("reseeded output");
        assert_eq!(again, out, "rewriting an output changes nothing");
        let name_at =
            |idx: u32| String::from_utf16_lossy(&entry_utf16_name(&out, offsets[idx as usize]));
        assert_eq!(name_at(1), "Dgn~H");
        assert_eq!(name_at(2), "Dgn~S");

        fn check_rb(data: &[u8], offsets: &[usize], idx: u32) -> usize {
            if idx == NOSTREAM {
                return 1;
            }
            let off = offsets[idx as usize];
            let color = data[off + 67];
            assert!(matches!(color, 0 | 1));
            let left = u32_le(data, off + 68).unwrap();
            let right = u32_le(data, off + 72).unwrap();
            let key = entry_utf16_name(data, off);
            if left != NOSTREAM {
                let l_off = offsets[left as usize];
                assert!(cmp_cfb_name(&entry_utf16_name(data, l_off), &key).is_lt());
                assert!(
                    !(color == 0 && data[l_off + 67] == 0),
                    "red parent with red left"
                );
            }
            if right != NOSTREAM {
                let r_off = offsets[right as usize];
                assert!(cmp_cfb_name(&key, &entry_utf16_name(data, r_off)).is_lt());
                assert!(
                    !(color == 0 && data[r_off + 67] == 0),
                    "red parent with red right"
                );
            }
            let bh_left = check_rb(data, offsets, left);
            let bh_right = check_rb(data, offsets, right);
            assert_eq!(bh_left, bh_right, "equal black height");
            bh_left + usize::from(color == 1)
        }

        for &off in &offsets {
            if matches!(out[off + 66], 1 | 5) {
                let child = u32_le(&out, off + 76).unwrap();
                if child != NOSTREAM {
                    assert_eq!(out[offsets[child as usize] + 67], 1, "tree root is black");
                    check_rb(&out, &offsets, child);
                }
            }
        }
        seed
    }
}
