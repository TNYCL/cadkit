//! CFB container writer that preserves streams, directory metadata, and resource limits.

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
        Error::LimitExceeded("CFB output limit exceeded".into())
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
        return Err(Error::LimitExceeded("DGN seed bytes".into()));
    }
    let root_color = seed_root_color(seed);
    let mut source = ::cfb::CompoundFile::open(Cursor::new(seed))?;
    let mut entries = vec![];
    let mut budget = 0u64;
    for entry in source.walk() {
        if entries.len() as u64 >= options.limits.max_objects {
            return Err(Error::LimitExceeded("CFB entries".into()));
        }
        budget = budget
            .checked_add(entry.len())
            .ok_or_else(|| Error::LimitExceeded("CFB streams".into()))?;
        if budget > options.limits.max_decompressed_bytes {
            return Err(Error::LimitExceeded("CFB streams".into()));
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
    items.sort_by(|a, b| microstation_rank(a.key()).cmp(&microstation_rank(b.key())));
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
                            if let Some(parent) = entry.path().parent() {
                                if parent != Path::new("/") && !parent.as_os_str().is_empty() {
                                    output.create_storage_all(parent)?;
                                }
                            }
                            output.create_stream(entry.path())?.write_all(bytes)?;
                        }
                        None => {
                            if let Some(parent) = entry.path().parent() {
                                if parent != Path::new("/") && !parent.as_os_str().is_empty() {
                                    output.create_storage_all(parent)?;
                                }
                            }
                            let mut src = source.open_stream(entry.path())?;
                            let mut dst = output.create_stream(entry.path())?;
                            std::io::copy(&mut src.by_ref().take(entry.len()), &mut dst)?;
                        }
                    }
                }
                Item::New(name, data) => {
                    let path = format!("/{name}");
                    if let Some((parent, _)) = path.rsplit_once('/') {
                        if !parent.is_empty() {
                            output.create_storage_all(parent)?;
                        }
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

const NOSTREAM: u32 = 0xffff_ffff;

fn u16_le(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset.checked_add(2)?)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
}

fn u32_le(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset.checked_add(4)?)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
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

/// Byte offsets of every 128-byte directory entry in `data`, following the CFB FAT chain.
fn directory_entry_offsets(data: &[u8]) -> Result<Vec<usize>> {
    let shift = u16_le(data, 0x1e).ok_or_else(|| Error::invalid(0, "short CFB header"))?;
    if !matches!(shift, 9 | 12) {
        return Err(Error::invalid(0x1e, "unsupported CFB sector shift"));
    }
    let ss = 1usize << shift;
    let n_fat =
        u32_le(data, 0x2c).ok_or_else(|| Error::invalid(0x2c, "short CFB header"))? as usize;
    let mut dir_sec = u32_le(data, 0x30).ok_or_else(|| Error::invalid(0x30, "short CFB header"))?;
    let mut difat_sec =
        u32_le(data, 0x44).ok_or_else(|| Error::invalid(0x44, "short CFB header"))?;
    let n_difat =
        u32_le(data, 0x48).ok_or_else(|| Error::invalid(0x48, "short CFB header"))? as usize;
    let mut fat_sectors = Vec::new();
    for i in 0..109usize {
        if let Some(sec) = u32_le(data, 0x4c + i * 4).filter(|&s| s < 0xffff_fffa) {
            fat_sectors.push(sec);
        }
    }
    let per_difat = (ss / 4).saturating_sub(1);
    for _ in 0..n_difat {
        if difat_sec >= 0xffff_fffa {
            break;
        }
        let base = 512usize
            .checked_add(
                (difat_sec as usize)
                    .checked_mul(ss)
                    .ok_or_else(|| Error::invalid(0x44, "CFB DIFAT sector offset overflow"))?,
            )
            .ok_or_else(|| Error::invalid(0x44, "CFB DIFAT sector offset overflow"))?;
        for i in 0..per_difat {
            if let Some(sec) = u32_le(data, base + i * 4).filter(|&s| s < 0xffff_fffa) {
                fat_sectors.push(sec);
            }
        }
        difat_sec = u32_le(data, base + per_difat * 4).unwrap_or(NOSTREAM);
    }
    fat_sectors.truncate(n_fat);
    let mut fat = Vec::new();
    for sec in fat_sectors {
        let base = 512usize
            .checked_add(
                (sec as usize)
                    .checked_mul(ss)
                    .ok_or_else(|| Error::invalid(0x2c, "CFB FAT sector offset overflow"))?,
            )
            .ok_or_else(|| Error::invalid(0x2c, "CFB FAT sector offset overflow"))?;
        for i in 0..(ss / 4) {
            fat.push(
                u32_le(data, base + i * 4)
                    .ok_or_else(|| Error::invalid(base as u64, "truncated CFB FAT sector"))?,
            );
        }
    }
    let mut offsets = Vec::new();
    let mut visited = BTreeSet::new();
    while dir_sec < 0xffff_fffa && visited.insert(dir_sec) {
        let base = 512usize
            .checked_add(
                (dir_sec as usize)
                    .checked_mul(ss)
                    .ok_or_else(|| Error::invalid(0x30, "CFB directory sector offset overflow"))?,
            )
            .ok_or_else(|| Error::invalid(0x30, "CFB directory sector offset overflow"))?;
        for slot in 0..(ss / 128) {
            let off = base + slot * 128;
            if off.checked_add(128).is_none_or(|end| end > data.len()) {
                return Err(Error::invalid(off as u64, "truncated CFB directory sector"));
            }
            offsets.push(off);
        }
        dir_sec = fat.get(dir_sec as usize).copied().unwrap_or(NOSTREAM);
    }
    Ok(offsets)
}

fn entry_utf16_name(data: &[u8], off: usize) -> Vec<u16> {
    let byte_len = usize::from(u16_le(data, off + 64).unwrap_or(0))
        .saturating_sub(2)
        .min(62);
    let mut out = Vec::with_capacity(byte_len / 2);
    for i in (0..byte_len).step_by(2) {
        if let Some(unit) = u16_le(data, off + i) {
            out.push(unit);
        }
    }
    out
}

/// Returns the root entry's `color_flag` from `seed` (0 in MicroStation V8 files).
fn seed_root_color(seed: &[u8]) -> u8 {
    directory_entry_offsets(seed)
        .ok()
        .and_then(|offsets| offsets.first().copied())
        .and_then(|off| seed.get(off + 67).copied())
        .unwrap_or(0)
}

/// Directory slot rank observed across MicroStation V8 files (`Dgn~H` at slot 1, `Dgn~S` at
/// slot 2, `Dgn-Md` at slot 3, every storage before its children, and `^AH` before `$1` in
/// `Dgn^GA`/`Dgn^CA`).
fn microstation_rank(path: &str) -> (usize, &str, usize, &str) {
    let fixed = match path {
        "" => 0,
        "Dgn~H" => 1,
        "Dgn~S" => 2,
        "Dgn-Md" => 3,
        "Dgn-Md/#000000" => 4,
        "Dgn-Md/#000000/Dgn^C" => 5,
        "Dgn-Md/#000000/Dgn^C/$1" => 6,
        "Dgn-Md/#000000/Dgn^G" => 7,
        "Dgn-Md/#000000/Dgn^CA" => 8,
        "Dgn-Md/#000000/Dgn^CA/^AH" => 9,
        "Dgn-Md/#000000/Dgn~Mh" => 10,
        "Dgn^Ix" => 11,
        "Dgn^Ix/Dgn~Mix" => 12,
        "Dgn^Nm" => 13,
        "Dgn^Nm/$1" => 14,
        "Dgn^Nm/$2" => 15,
        "Dgn^Nm/$3" => 16,
        "Dgn~Mf" => 17,
        "Dgn^NmA" => 18,
        "Dgn^NmA/$1" => 19,
        "Dgn^NmA/$2" => 20,
        "Dgn^NmA/$3" => 21,
        "Dgn^NmA/^AH" => 22,
        "\u{5}SummaryInformation" => 23,
        "\u{5}DocumentSummaryInformation" => 24,
        "Dgn-Md/#000000/Dgn^GA" => 25,
        "Dgn-Md/#000000/Dgn^G/$1" => 26,
        "Dgn^Nm/$4" => 27,
        "Dgn-Md/#000000/Dgn^GA/^AH" => 28,
        "Dgn-Md/#000000/Dgn^GA/$1" => 29,
        _ => usize::MAX,
    };
    let parent = path.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
    let page_num = path
        .rsplit_once("/$")
        .and_then(|(_, n)| n.parse::<usize>().ok())
        .unwrap_or(usize::MAX);
    (fixed, parent, page_num, path)
}

#[derive(Clone)]
struct RbNode {
    left: Option<u32>,
    right: Option<u32>,
    parent: Option<u32>,
    red: bool,
    key: Vec<u16>,
}

fn u16_upper(unit: u16) -> u16 {
    char::from_u32(u32::from(unit))
        .and_then(|c| c.to_uppercase().next())
        .and_then(|up| u16::try_from(u32::from(up)).ok())
        .unwrap_or(unit)
}

fn cmp_cfb_name(a: &[u16], b: &[u16]) -> std::cmp::Ordering {
    a.len().cmp(&b.len()).then_with(|| {
        a.iter()
            .copied()
            .map(u16_upper)
            .cmp(b.iter().copied().map(u16_upper))
    })
}

fn rb_left(nodes: &BTreeMap<u32, RbNode>, x: u32) -> Option<u32> {
    nodes.get(&x).and_then(|n| n.left)
}

fn rb_right(nodes: &BTreeMap<u32, RbNode>, x: u32) -> Option<u32> {
    nodes.get(&x).and_then(|n| n.right)
}

fn rb_parent(nodes: &BTreeMap<u32, RbNode>, x: u32) -> Option<u32> {
    nodes.get(&x).and_then(|n| n.parent)
}

fn rb_red(nodes: &BTreeMap<u32, RbNode>, x: Option<u32>) -> bool {
    x.and_then(|id| nodes.get(&id)).is_some_and(|n| n.red)
}

fn set_red(nodes: &mut BTreeMap<u32, RbNode>, x: Option<u32>, red: bool) {
    if let Some(node) = x.and_then(|id| nodes.get_mut(&id)) {
        node.red = red;
    }
}

fn rotate_left(nodes: &mut BTreeMap<u32, RbNode>, root: &mut Option<u32>, x: u32) {
    let Some(y) = rb_right(nodes, x) else {
        return;
    };
    let y_left = rb_left(nodes, y);
    let x_parent = rb_parent(nodes, x);
    if let Some(nx) = nodes.get_mut(&x) {
        nx.right = y_left;
        nx.parent = Some(y);
    }
    if let Some(yl) = y_left.and_then(|id| nodes.get_mut(&id)) {
        yl.parent = Some(x);
    }
    if let Some(ny) = nodes.get_mut(&y) {
        ny.left = Some(x);
        ny.parent = x_parent;
    }
    match x_parent {
        None => *root = Some(y),
        Some(p) => {
            if let Some(np) = nodes.get_mut(&p) {
                if np.left == Some(x) {
                    np.left = Some(y);
                } else {
                    np.right = Some(y);
                }
            }
        }
    }
}

fn rotate_right(nodes: &mut BTreeMap<u32, RbNode>, root: &mut Option<u32>, x: u32) {
    let Some(y) = rb_left(nodes, x) else {
        return;
    };
    let y_right = rb_right(nodes, y);
    let x_parent = rb_parent(nodes, x);
    if let Some(nx) = nodes.get_mut(&x) {
        nx.left = y_right;
        nx.parent = Some(y);
    }
    if let Some(yr) = y_right.and_then(|id| nodes.get_mut(&id)) {
        yr.parent = Some(x);
    }
    if let Some(ny) = nodes.get_mut(&y) {
        ny.right = Some(x);
        ny.parent = x_parent;
    }
    match x_parent {
        None => *root = Some(y),
        Some(p) => {
            if let Some(np) = nodes.get_mut(&p) {
                if np.right == Some(x) {
                    np.right = Some(y);
                } else {
                    np.left = Some(y);
                }
            }
        }
    }
}

fn rb_fixup(nodes: &mut BTreeMap<u32, RbNode>, root: &mut Option<u32>, mut z: u32) {
    while rb_red(nodes, rb_parent(nodes, z)) {
        let Some(mut p) = rb_parent(nodes, z) else {
            break;
        };
        let Some(mut gp) = rb_parent(nodes, p) else {
            break;
        };
        if Some(p) == rb_left(nodes, gp) {
            if Some(z) == rb_right(nodes, p) {
                z = p;
                rotate_left(nodes, root, z);
                let Some(next_p) = rb_parent(nodes, z) else {
                    break;
                };
                let Some(next_gp) = rb_parent(nodes, next_p) else {
                    break;
                };
                p = next_p;
                gp = next_gp;
            }
            set_red(nodes, Some(p), false);
            set_red(nodes, Some(gp), true);
            rotate_right(nodes, root, gp);
        } else {
            if Some(z) == rb_left(nodes, p) {
                z = p;
                rotate_right(nodes, root, z);
                let Some(next_p) = rb_parent(nodes, z) else {
                    break;
                };
                let Some(next_gp) = rb_parent(nodes, next_p) else {
                    break;
                };
                p = next_p;
                gp = next_gp;
            }
            set_red(nodes, Some(p), false);
            set_red(nodes, Some(gp), true);
            rotate_left(nodes, root, gp);
        }
    }
    set_red(nodes, *root, false);
}

/// Rebalances every storage's child sibling tree in `data` using Sedgewick's top-down 2-3-4
/// red-black insertion in directory slot order, matching Windows Structured Storage (`ole32`).
fn balance_directory(data: &mut [u8], root_color: u8) -> Result<()> {
    let offsets = directory_entry_offsets(data)?;
    if let Some(&root_off) = offsets.first() {
        if let Some(color_byte) = data.get_mut(root_off + 67) {
            *color_byte = root_color;
        }
    }
    for &storage_off in &offsets {
        let kind = data.get(storage_off + 66).copied().unwrap_or(0);
        if !matches!(kind, 1 | 5) {
            continue;
        }
        let first_child = u32_le(data, storage_off + 76).unwrap_or(NOSTREAM);
        if first_child == NOSTREAM {
            continue;
        }
        let mut children = BTreeSet::new();
        let mut stack = vec![first_child];
        while let Some(idx) = stack.pop() {
            if idx == NOSTREAM || (idx as usize) >= offsets.len() || !children.insert(idx) {
                continue;
            }
            if let Some(&off) = offsets.get(idx as usize) {
                let left = u32_le(data, off + 68).unwrap_or(NOSTREAM);
                let right = u32_le(data, off + 72).unwrap_or(NOSTREAM);
                if left != NOSTREAM {
                    stack.push(left);
                }
                if right != NOSTREAM {
                    stack.push(right);
                }
            }
        }
        let mut nodes: BTreeMap<u32, RbNode> = BTreeMap::new();
        let mut root: Option<u32> = None;
        for z in children {
            let Some(&z_off) = offsets.get(z as usize) else {
                continue;
            };
            let z_key = entry_utf16_name(data, z_off);
            let mut y: Option<u32> = None;
            let mut x = root;
            while let Some(curr) = x {
                let l = rb_left(&nodes, curr);
                let r = rb_right(&nodes, curr);
                if rb_red(&nodes, l) && rb_red(&nodes, r) {
                    set_red(&mut nodes, l, false);
                    set_red(&mut nodes, r, false);
                    set_red(&mut nodes, Some(curr), true);
                    rb_fixup(&mut nodes, &mut root, curr);
                }
                y = Some(curr);
                let ord = nodes
                    .get(&curr)
                    .map(|n| cmp_cfb_name(&z_key, &n.key))
                    .unwrap_or(std::cmp::Ordering::Greater);
                x = if ord.is_lt() {
                    rb_left(&nodes, curr)
                } else {
                    rb_right(&nodes, curr)
                };
            }
            nodes.insert(
                z,
                RbNode {
                    left: None,
                    right: None,
                    parent: y,
                    red: true,
                    key: z_key.clone(),
                },
            );
            match y {
                None => root = Some(z),
                Some(p) => {
                    if let Some(np) = nodes.get_mut(&p) {
                        if cmp_cfb_name(&z_key, &np.key).is_lt() {
                            np.left = Some(z);
                        } else {
                            np.right = Some(z);
                        }
                    }
                }
            }
            rb_fixup(&mut nodes, &mut root, z);
            set_red(&mut nodes, root, false);
        }
        put_u32_le(data, storage_off + 76, root.unwrap_or(NOSTREAM))?;
        for (id, node) in nodes {
            if let Some(&off) = offsets.get(id as usize) {
                if let Some(color_byte) = data.get_mut(off + 67) {
                    *color_byte = u8::from(!node.red);
                }
                put_u32_le(data, off + 68, node.left.unwrap_or(NOSTREAM))?;
                put_u32_le(data, off + 72, node.right.unwrap_or(NOSTREAM))?;
            }
        }
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
        return Err(Error::LimitExceeded("DGN seed stream".into()));
    }
    let mut data = vec![];
    cf.open_stream(name)?.take(n).read_to_end(&mut data)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn rewrite_balances_cfb_directory_as_red_black_trees() {
        let mut cf =
            ::cfb::CompoundFile::create_with_version(::cfb::Version::V3, Cursor::new(Vec::new()))
                .unwrap();
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
        let offsets = directory_entry_offsets(&out).unwrap();
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

        // Replacing a stream with data larger than `max_input_bytes` yields LimitExceeded.
        let mut tight = ReadOptions::default();
        tight.limits.max_input_bytes = seed.len() as u64;
        let mut replacements = BTreeMap::new();
        replacements.insert("Dgn~H".into(), Some(vec![0u8; 64 * 1024]));
        assert!(matches!(
            rewrite(&seed, &replacements, &tight),
            Err(Error::LimitExceeded(_))
        ));
    }
}
