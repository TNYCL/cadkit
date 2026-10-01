//! Minimal read-only reader for the Compound File Binary format (OLE2 / "CFB").
//!
//! DGN V8 files are compound files. This reader follows the public `[MS-CFB]` layout:
//! a 512-byte header, a FAT (located through the header DIFAT and chained DIFAT sectors),
//! a directory of 128-byte entries arranged as red-black trees, and a mini stream (with
//! its own mini FAT) for streams below the cutoff size. Everything read from the file is
//! bounds-checked; sector chains are cycle-checked against the number of sectors the
//! file can hold, so a corrupt file cannot make the reader loop or over-allocate.

use cadkit_core::{Error, Result};

use crate::le;

/// CFB file signature.
pub const SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

const HEADER_LEN: usize = 512;
const DIR_ENTRY_LEN: usize = 128;
const MAX_REGSECT: u32 = 0xFFFF_FFFA;
const ENDOFCHAIN: u32 = 0xFFFF_FFFE;
const NOSTREAM: u32 = 0xFFFF_FFFF;
/// Deepest storage nesting followed when building paths.
const MAX_TREE_DEPTH: usize = 64;

/// Kind of a directory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Unused slot.
    Empty,
    /// A storage (directory).
    Storage,
    /// A stream (file).
    Stream,
    /// The root storage; owns the mini stream.
    Root,
    /// Unknown object type byte.
    Other(u8),
}

/// One raw directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Entry name (UTF-16 in the file).
    pub name: String,
    /// Entry kind.
    pub kind: EntryKind,
    /// Left sibling id.
    pub left: u32,
    /// Right sibling id.
    pub right: u32,
    /// First child id (storages).
    pub child: u32,
    /// First sector of the stream data.
    pub start: u32,
    /// Stream size in bytes.
    pub size: u64,
}

/// A stream or storage with its full path (`/` separated, no leading slash).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathEntry {
    /// Full path such as `Dgn-Md/#000000/Dgn^G/$1`.
    pub path: String,
    /// Index into [`CompoundFile::entries`].
    pub index: usize,
    /// Entry kind.
    pub kind: EntryKind,
    /// Stream size in bytes (0 for storages).
    pub size: u64,
}

/// A parsed compound file borrowing the input bytes.
#[derive(Debug, Clone)]
pub struct CompoundFile<'a> {
    data: &'a [u8],
    /// Major version (3 = 512-byte sectors, 4 = 4096-byte sectors).
    pub major_version: u16,
    sector_size: usize,
    mini_sector_size: usize,
    mini_cutoff: u64,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    mini_stream: Vec<u8>,
    entries: Vec<DirEntry>,
    /// Storage/stream paths in tree order, computed once at parse time.
    paths: Vec<PathEntry>,
}

impl<'a> CompoundFile<'a> {
    /// Parses the header, allocation tables, directory and mini stream.
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        Self::parse_impl(data, true)
    }

    fn parse_impl(data: &'a [u8], load_mini: bool) -> Result<Self> {
        let header = data.get(..HEADER_LEN).ok_or(Error::Truncated {
            offset: 0,
            needed: (HEADER_LEN.saturating_sub(data.len())) as u64,
        })?;
        if header.get(..8) != Some(&SIGNATURE[..]) {
            return Err(Error::UnknownFormat);
        }
        let field = |off: usize| le::u32_at(header, off).unwrap_or(0);
        let major_version = le::u16_at(header, 0x1A).unwrap_or(0);
        let sector_shift = le::u16_at(header, 0x1E).unwrap_or(0);
        let mini_shift = le::u16_at(header, 0x20).unwrap_or(0);
        let sector_size = match sector_shift {
            9 => 512usize,
            12 => 4096,
            s => {
                return Err(Error::invalid(
                    0x1E,
                    format!("unsupported CFB sector shift {s}"),
                ));
            }
        };
        if mini_shift != 6 {
            return Err(Error::invalid(
                0x20,
                format!("unsupported CFB mini sector shift {mini_shift}"),
            ));
        }
        let mini_sector_size = 64usize;
        let num_fat = field(0x2C);
        let first_dir = field(0x30);
        let mini_cutoff = u64::from(field(0x38));
        let first_mini_fat = field(0x3C);
        let first_difat = field(0x44);
        let num_difat = field(0x48);

        // Upper bound for every sector id or chain length: the sectors the file can hold.
        let max_sectors = data.len() / sector_size;

        // DIFAT: 109 entries in the header, then chained DIFAT sectors.
        let mut difat: Vec<u32> = (0..109)
            .filter_map(|i| le::u32_at(header, 0x4C + 4 * i))
            .collect();
        let per_difat = sector_size / 4 - 1;
        let mut next = first_difat;
        let mut seen = 0usize;
        while next <= MAX_REGSECT && seen < (num_difat as usize).min(max_sectors) {
            let sec = sector_slice(data, sector_size, next).ok_or_else(|| {
                Error::invalid(
                    sector_offset(sector_size, next),
                    "DIFAT sector outside the file",
                )
            })?;
            difat.extend((0..per_difat).filter_map(|i| le::u32_at(sec, 4 * i)));
            next = le::u32_at(sec, 4 * per_difat).unwrap_or(ENDOFCHAIN);
            seen += 1;
        }
        let num_fat = (num_fat as usize).min(max_sectors).min(difat.len());

        let mut fat: Vec<u32> = Vec::with_capacity(num_fat.saturating_mul(sector_size / 4));
        for &sid in difat.iter().take(num_fat) {
            if sid > MAX_REGSECT {
                continue;
            }
            match sector_slice(data, sector_size, sid) {
                Some(sec) => {
                    fat.extend((0..sector_size / 4).filter_map(|i| le::u32_at(sec, 4 * i)))
                }
                None => {
                    return Err(Error::invalid(
                        sector_offset(sector_size, sid),
                        "FAT sector outside the file",
                    ));
                }
            }
        }

        let mut cf = Self {
            data,
            major_version,
            sector_size,
            mini_sector_size,
            mini_cutoff,
            fat,
            mini_fat: Vec::new(),
            mini_stream: Vec::new(),
            entries: Vec::new(),
            paths: Vec::new(),
        };

        let dir = cf.read_chain(first_dir, None)?;
        let entries: Vec<DirEntry> = dir
            .chunks_exact(DIR_ENTRY_LEN)
            .map(parse_dir_entry)
            .collect();
        if entries.first().map(|e| e.kind) != Some(EntryKind::Root) {
            return Err(Error::invalid(
                sector_offset(sector_size, first_dir),
                "CFB directory has no root entry",
            ));
        }
        cf.entries = entries;
        cf.paths = cf.collect_paths();

        if !load_mini {
            return Ok(cf);
        }
        if first_mini_fat <= MAX_REGSECT {
            let mf = cf.read_chain(first_mini_fat, None)?;
            cf.mini_fat = mf
                .chunks_exact(4)
                .filter_map(|c| le::u32_at(c, 0))
                .collect();
        }
        if let Some(root) = cf.entries.first().cloned() {
            if root.start <= MAX_REGSECT && root.size > 0 {
                cf.mini_stream = cf.read_chain(root.start, Some(root.size))?;
            }
        }
        Ok(cf)
    }

    /// All raw directory entries (index 0 is the root).
    pub fn entries(&self) -> &[DirEntry] {
        &self.entries
    }

    /// Every storage and stream below the root, with full paths, in tree order.
    /// Computed once when the file is parsed.
    pub fn paths(&self) -> &[PathEntry] {
        &self.paths
    }

    fn collect_paths(&self) -> Vec<PathEntry> {
        let mut out = Vec::new();
        if let Some(root) = self.entries.first() {
            let mut visited = vec![false; self.entries.len()];
            if let Some(v) = visited.first_mut() {
                *v = true;
            }
            self.collect(root.child, "", 0, &mut visited, &mut out);
        }
        out
    }

    fn collect(
        &self,
        start: u32,
        prefix: &str,
        depth: usize,
        visited: &mut [bool],
        out: &mut Vec<PathEntry>,
    ) {
        if depth > MAX_TREE_DEPTH {
            return;
        }
        // Siblings form a binary tree; walk it with an explicit stack (in-order) so a
        // corrupt, deeply unbalanced tree cannot overflow the call stack.
        let mut stack: Vec<u32> = Vec::new();
        let mut cur = start;
        loop {
            while let Some(e) = self.entry_once(cur, visited) {
                stack.push(cur);
                cur = e.left;
            }
            let Some(id) = stack.pop() else { break };
            let Some(e) = self.entries.get(id as usize) else {
                break;
            };
            let path = if prefix.is_empty() {
                e.name.clone()
            } else {
                format!("{prefix}/{}", e.name)
            };
            out.push(PathEntry {
                path: path.clone(),
                index: id as usize,
                kind: e.kind,
                size: e.size,
            });
            if e.kind == EntryKind::Storage {
                self.collect(e.child, &path, depth + 1, visited, out);
            }
            cur = e.right;
        }
    }

    /// The entry `id` if it exists and has not been visited yet (marks it visited).
    fn entry_once(&self, id: u32, visited: &mut [bool]) -> Option<&DirEntry> {
        if id == NOSTREAM {
            return None;
        }
        let slot = visited.get_mut(id as usize)?;
        if *slot {
            return None;
        }
        *slot = true;
        self.entries
            .get(id as usize)
            .filter(|e| e.kind != EntryKind::Empty)
    }

    /// Index of the entry at `path` (`/` separated, case-insensitive as in CFB).
    pub fn find(&self, path: &str) -> Option<usize> {
        let want = path.trim_start_matches('/');
        self.paths
            .iter()
            .find(|p| p.path.eq_ignore_ascii_case(want))
            .map(|p| p.index)
    }

    /// Reads the stream with directory index `index`. Fails on missing or truncated data.
    pub fn read_stream(&self, index: usize) -> Result<Vec<u8>> {
        let e = self
            .entries
            .get(index)
            .ok_or_else(|| Error::invalid(0, format!("no CFB directory entry {index}")))?;
        if e.kind != EntryKind::Stream {
            return Err(Error::invalid(
                0,
                format!("CFB entry {} is not a stream", e.name),
            ));
        }
        if e.size == 0 {
            return Ok(Vec::new());
        }
        if e.size > self.data.len() as u64 {
            return Err(Error::invalid(
                0,
                format!("CFB stream {} is larger than the file", e.name),
            ));
        }
        if e.size < self.mini_cutoff {
            self.read_mini_chain(e.start, e.size)
        } else {
            self.read_chain(e.start, Some(e.size))
        }
    }

    /// Follows a regular sector chain; `size` truncates (and must be covered).
    fn read_chain(&self, start: u32, size: Option<u64>) -> Result<Vec<u8>> {
        let max_sectors = self.data.len() / self.sector_size;
        let mut out = Vec::new();
        let mut sid = start;
        let mut count = 0usize;
        while sid <= MAX_REGSECT {
            if count >= max_sectors {
                return Err(Error::invalid(
                    sector_offset(self.sector_size, sid),
                    "CFB sector chain loops",
                ));
            }
            let sec = sector_slice(self.data, self.sector_size, sid).ok_or_else(|| {
                Error::invalid(
                    sector_offset(self.sector_size, sid),
                    "CFB sector outside the file",
                )
            })?;
            out.extend_from_slice(sec);
            count += 1;
            if let Some(s) = size {
                if out.len() as u64 >= s {
                    break;
                }
            }
            sid = self.fat.get(sid as usize).copied().unwrap_or(ENDOFCHAIN);
        }
        finish(out, size)
    }

    fn read_mini_chain(&self, start: u32, size: u64) -> Result<Vec<u8>> {
        let max = self.mini_stream.len() / self.mini_sector_size + 1;
        let mut out = Vec::new();
        let mut sid = start;
        let mut count = 0usize;
        while sid <= MAX_REGSECT {
            if count >= max {
                return Err(Error::invalid(0, "CFB mini sector chain loops"));
            }
            let off = (sid as usize).saturating_mul(self.mini_sector_size);
            let sec = le::bytes(&self.mini_stream, off, self.mini_sector_size)
                .ok_or_else(|| Error::invalid(0, "CFB mini sector outside the mini stream"))?;
            out.extend_from_slice(sec);
            count += 1;
            if out.len() as u64 >= size {
                break;
            }
            sid = self
                .mini_fat
                .get(sid as usize)
                .copied()
                .unwrap_or(ENDOFCHAIN);
        }
        finish(out, Some(size))
    }
}

fn finish(mut out: Vec<u8>, size: Option<u64>) -> Result<Vec<u8>> {
    if let Some(s) = size {
        let s = usize::try_from(s).unwrap_or(usize::MAX);
        if out.len() < s {
            return Err(Error::Truncated {
                offset: 0,
                needed: (s - out.len()) as u64,
            });
        }
        out.truncate(s);
    }
    Ok(out)
}

fn sector_offset(sector_size: usize, sid: u32) -> u64 {
    (u64::from(sid) + 1).saturating_mul(sector_size as u64)
}

fn sector_slice(data: &[u8], sector_size: usize, sid: u32) -> Option<&[u8]> {
    let off = usize::try_from(sector_offset(sector_size, sid)).ok()?;
    le::bytes(data, off, sector_size)
}

fn parse_dir_entry(e: &[u8]) -> DirEntry {
    let name_len = usize::from(le::u16_at(e, 64).unwrap_or(0)).min(64);
    let units: Vec<u16> = e
        .get(..name_len.saturating_sub(2))
        .unwrap_or(&[])
        .chunks_exact(2)
        .filter_map(|c| le::u16_at(c, 0))
        .collect();
    let kind = match le::u8_at(e, 66).unwrap_or(0) {
        0 => EntryKind::Empty,
        1 => EntryKind::Storage,
        2 => EntryKind::Stream,
        5 => EntryKind::Root,
        k => EntryKind::Other(k),
    };
    DirEntry {
        name: String::from_utf16_lossy(&units),
        kind,
        left: le::u32_at(e, 68).unwrap_or(NOSTREAM),
        right: le::u32_at(e, 72).unwrap_or(NOSTREAM),
        child: le::u32_at(e, 76).unwrap_or(NOSTREAM),
        start: le::u32_at(e, 116).unwrap_or(ENDOFCHAIN),
        // Version 3 files may leave garbage in the high size dword.
        size: u64::from(le::u32_at(e, 120).unwrap_or(0)),
    }
}

/// Cheap check that `data` is a compound file whose root storage directly contains a
/// stream or storage named `name` (case-insensitive). Reads the header, the FAT and the
/// directory chain only; no stream data is touched.
pub fn root_contains(data: &[u8], name: &str) -> bool {
    let Ok(cf) = CompoundFile::parse_impl(data, false) else {
        return false;
    };
    let Some(root) = cf.entries.first() else {
        return false;
    };
    let mut visited = vec![false; cf.entries.len()];
    let mut stack = vec![root.child];
    while let Some(id) = stack.pop() {
        let Some(e) = cf.entry_once(id, &mut visited) else {
            continue;
        };
        if e.name.eq_ignore_ascii_case(name) {
            return true;
        }
        stack.push(e.left);
        stack.push(e.right);
    }
    false
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds a version-3 compound file. `streams` are (path, data); storages are created
    /// from the path prefixes. Streams below 4096 bytes go to the mini stream.
    pub(crate) fn build(streams: &[(&str, Vec<u8>)]) -> Vec<u8> {
        build_with(streams, 4096)
    }

    pub(crate) fn build_with(streams: &[(&str, Vec<u8>)], cutoff: u32) -> Vec<u8> {
        const SS: usize = 512;
        // Directory model: (name, kind, children, data)
        struct Node {
            name: String,
            kind: u8,
            children: Vec<usize>,
            data: Vec<u8>,
        }
        let mut nodes = vec![Node {
            name: "Root Entry".into(),
            kind: 5,
            children: vec![],
            data: vec![],
        }];
        for (path, data) in streams {
            let mut parent = 0usize;
            let parts: Vec<&str> = path.split('/').collect();
            for (i, part) in parts.iter().enumerate() {
                let last = i + 1 == parts.len();
                let found = nodes[parent]
                    .children
                    .iter()
                    .copied()
                    .find(|&c| nodes[c].name == *part);
                parent = match found {
                    Some(c) => c,
                    None => {
                        nodes.push(Node {
                            name: (*part).into(),
                            kind: if last { 2 } else { 1 },
                            children: vec![],
                            data: if last { data.clone() } else { vec![] },
                        });
                        let id = nodes.len() - 1;
                        nodes[parent].children.push(id);
                        id
                    }
                };
            }
        }
        // Sectors: [FAT][DIR...][MINIFAT...][ministream...][big streams...]
        let mut sectors: Vec<Vec<u8>> = Vec::new();
        let mut fat: Vec<u32> = Vec::new();
        let alloc = |data: &[u8], sectors: &mut Vec<Vec<u8>>, fat: &mut Vec<u32>| -> u32 {
            if data.is_empty() {
                return ENDOFCHAIN;
            }
            let start = sectors.len() as u32;
            let n = data.len().div_ceil(SS);
            for i in 0..n {
                let mut s = data[i * SS..((i + 1) * SS).min(data.len())].to_vec();
                s.resize(SS, 0);
                sectors.push(s);
                fat.push(if i + 1 == n {
                    ENDOFCHAIN
                } else {
                    start + i as u32 + 1
                });
            }
            start
        };
        // Reserve sector 0 for the FAT.
        sectors.push(vec![0; SS]);
        fat.push(0xFFFF_FFFD);
        // Mini stream.
        let mut mini = Vec::new();
        let mut minifat: Vec<u32> = Vec::new();
        let mut starts = vec![(ENDOFCHAIN, 0u64); nodes.len()];
        for (i, n) in nodes.iter().enumerate() {
            if n.kind == 2 && !n.data.is_empty() && (n.data.len() as u32) < cutoff {
                let start = (mini.len() / 64) as u32;
                let k = n.data.len().div_ceil(64);
                for j in 0..k {
                    minifat.push(if j + 1 == k {
                        ENDOFCHAIN
                    } else {
                        start + j as u32 + 1
                    });
                }
                mini.extend_from_slice(&n.data);
                mini.resize(mini.len().div_ceil(64) * 64, 0);
                starts[i] = (start, n.data.len() as u64);
            }
        }
        let mut mf_bytes = Vec::new();
        for v in &minifat {
            mf_bytes.extend_from_slice(&v.to_le_bytes());
        }
        let minifat_start = alloc(&mf_bytes, &mut sectors, &mut fat);
        let mini_start = alloc(&mini, &mut sectors, &mut fat);
        starts[0] = (mini_start, mini.len() as u64);
        for (i, n) in nodes.iter().enumerate() {
            if n.kind == 2 && n.data.len() as u32 >= cutoff {
                let s = alloc(&n.data, &mut sectors, &mut fat);
                starts[i] = (s, n.data.len() as u64);
            }
        }
        // Directory: siblings as a right-leaning chain.
        let mut dir = Vec::new();
        for (i, n) in nodes.iter().enumerate() {
            let mut e = vec![0u8; 128];
            let units: Vec<u16> = n.name.encode_utf16().collect();
            for (j, u) in units.iter().enumerate() {
                e[j * 2..j * 2 + 2].copy_from_slice(&u.to_le_bytes());
            }
            e[64..66].copy_from_slice(&((units.len() as u16 + 1) * 2).to_le_bytes());
            e[66] = n.kind;
            let parent = nodes.iter().position(|p| p.children.contains(&i));
            let right = parent
                .and_then(|p| {
                    let sib = &nodes[p].children;
                    let pos = sib.iter().position(|&c| c == i)?;
                    sib.get(pos + 1).copied()
                })
                .map(|r| r as u32)
                .unwrap_or(NOSTREAM);
            e[68..72].copy_from_slice(&NOSTREAM.to_le_bytes());
            e[72..76].copy_from_slice(&right.to_le_bytes());
            let child = n.children.first().map(|&c| c as u32).unwrap_or(NOSTREAM);
            e[76..80].copy_from_slice(&child.to_le_bytes());
            e[116..120].copy_from_slice(&starts[i].0.to_le_bytes());
            e[120..124].copy_from_slice(&(starts[i].1 as u32).to_le_bytes());
            dir.extend_from_slice(&e);
        }
        let dir_start = alloc(&dir, &mut sectors, &mut fat);
        // Sector 0 is the first FAT sector; files with more than 128 sectors get extra FAT
        // sectors appended at the end (each also listed in the FAT and the header DIFAT).
        const PER_FAT: usize = SS / 4;
        let mut fat_sectors = vec![0u32];
        while fat_sectors.len() * PER_FAT < fat.len() {
            fat_sectors.push(sectors.len() as u32);
            sectors.push(vec![0; SS]);
            fat.push(0xFFFF_FFFD);
        }
        assert!(
            fat_sectors.len() <= 109,
            "test builder supports header DIFAT only"
        );
        fat.resize(fat_sectors.len() * PER_FAT, NOSTREAM);
        for (k, &sec) in fat_sectors.iter().enumerate() {
            let mut fat_sec = vec![0u8; SS];
            for (i, v) in fat[k * PER_FAT..(k + 1) * PER_FAT].iter().enumerate() {
                fat_sec[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            sectors[sec as usize] = fat_sec;
        }
        let mut h = vec![0u8; 512];
        h[..8].copy_from_slice(&SIGNATURE);
        h[0x18..0x1A].copy_from_slice(&0x3Eu16.to_le_bytes());
        h[0x1A..0x1C].copy_from_slice(&3u16.to_le_bytes());
        h[0x1C..0x1E].copy_from_slice(&0xFFFEu16.to_le_bytes());
        h[0x1E..0x20].copy_from_slice(&9u16.to_le_bytes());
        h[0x20..0x22].copy_from_slice(&6u16.to_le_bytes());
        h[0x2C..0x30].copy_from_slice(&(fat_sectors.len() as u32).to_le_bytes());
        h[0x30..0x34].copy_from_slice(&dir_start.to_le_bytes());
        h[0x38..0x3C].copy_from_slice(&cutoff.to_le_bytes());
        h[0x3C..0x40].copy_from_slice(&minifat_start.to_le_bytes());
        h[0x40..0x44].copy_from_slice(&((mf_bytes.len().div_ceil(SS)) as u32).to_le_bytes());
        h[0x44..0x48].copy_from_slice(&ENDOFCHAIN.to_le_bytes());
        for i in 0..109 {
            let v: u32 = fat_sectors.get(i).copied().unwrap_or(NOSTREAM);
            h[0x4C + i * 4..0x50 + i * 4].copy_from_slice(&v.to_le_bytes());
        }
        let mut out = h;
        for s in sectors {
            out.extend_from_slice(&s);
        }
        out
    }

    #[test]
    fn reads_mini_and_regular_streams() {
        let big: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        let file = build(&[
            ("A/small", b"hello".to_vec()),
            ("A/B/big", big.clone()),
            ("top", vec![1, 2, 3]),
        ]);
        let cf = CompoundFile::parse(&file).unwrap();
        let paths: Vec<String> = cf.paths().iter().map(|p| p.path.clone()).collect();
        assert!(paths.contains(&"A/small".to_string()));
        assert!(paths.contains(&"A/B/big".to_string()));
        let small = cf.find("/A/small").unwrap();
        assert_eq!(cf.read_stream(small).unwrap(), b"hello");
        assert_eq!(cf.read_stream(cf.find("a/b/BIG").unwrap()).unwrap(), big);
        assert!(root_contains(&file, "top"));
        assert!(root_contains(&file, "A"));
        assert!(!root_contains(&file, "small"));
    }

    #[test]
    fn rejects_non_cfb_and_survives_corruption() {
        assert!(CompoundFile::parse(b"not a compound file").is_err());
        assert!(!root_contains(&[0u8; 600], "x"));
        let file = build(&[("s", vec![9u8; 5000])]);
        for cut in [0, 100, 512, 700, 1024, file.len() - 1] {
            let _ = CompoundFile::parse(&file[..cut]).map(|cf| {
                for p in cf.paths() {
                    let _ = cf.read_stream(p.index);
                }
            });
        }
        // A FAT that points every sector at itself must not loop forever.
        let mut looped = file.clone();
        let fat_off = 512;
        for i in 1..20 {
            looped[fat_off + i * 4..fat_off + i * 4 + 4].copy_from_slice(&(i as u32).to_le_bytes());
        }
        if let Ok(cf) = CompoundFile::parse(&looped) {
            for p in cf.paths() {
                let _ = cf.read_stream(p.index);
            }
        }
    }
}
