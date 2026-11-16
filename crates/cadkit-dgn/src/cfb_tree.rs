//! Raw access to the compound file directory: entry offsets, CFB name order, red-black
//! tree shapes and the free entry form. Shared by the writer (which rebalances the
//! directory the `cfb` crate writes) and [`crate::check_structure`] (which reports the
//! directory of any file); [`crate::cfb`] is the reader and has no colours or slot order.

use cadkit_core::{Error, Result};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const NOSTREAM: u32 = 0xffff_ffff;
/// Byte length of one directory entry ([MS-CFB] 2.6.1).
pub(crate) const ENTRY_LEN: usize = 128;
/// Offsets inside a directory entry ([MS-CFB] 2.6.1).
pub(crate) const NAME_LEN: usize = 0x40;
pub(crate) const OBJECT_TYPE: usize = 0x42;
pub(crate) const COLOR: usize = 0x43;
pub(crate) const LEFT: usize = 0x44;
pub(crate) const RIGHT: usize = 0x48;
pub(crate) const CHILD: usize = 0x4c;

pub(crate) fn u16_le(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset.checked_add(2)?)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
}

pub(crate) fn u32_le(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset.checked_add(4)?)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
}

/// Byte offset of sector `sec`: the header fills sector -1, which is 512 bytes in a version 3
/// file and a whole 4096-byte sector in a version 4 file (MS-CFB 2.2), so sector `sec` starts at
/// `(sec + 1) * ss`, not at `512 + sec * ss`.
fn sector_offset(sec: u32, ss: usize, field: u64, what: &str) -> Result<usize> {
    (sec as usize)
        .checked_add(1)
        .and_then(|n| n.checked_mul(ss))
        .ok_or_else(|| Error::invalid(field, format!("CFB {what} sector offset overflow")))
}

/// Byte offsets of every 128-byte directory entry in `data`, following the CFB FAT chain.
///
/// The work is bounded by the input: the DIFAT walk stops at the header's FAT sector count
/// and at a repeated sector, and the directory chain at a repeated sector.
pub(crate) fn directory_entry_offsets(data: &[u8]) -> Result<Vec<usize>> {
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
    // A FAT sector holds `ss / 4` entries, so no file holds more FAT sectors than this.
    let n_fat = n_fat.min(data.len() / ss + 1);
    let mut fat_sectors = Vec::new();
    for i in 0..109usize {
        if let Some(sec) = u32_le(data, 0x4c + i * 4).filter(|&s| s < 0xffff_fffa) {
            fat_sectors.push(sec);
        }
    }
    let per_difat = (ss / 4).saturating_sub(1);
    let mut visited_difat = BTreeSet::new();
    for _ in 0..n_difat {
        if difat_sec >= 0xffff_fffa
            || fat_sectors.len() >= n_fat
            || !visited_difat.insert(difat_sec)
        {
            break;
        }
        let base = sector_offset(difat_sec, ss, 0x44, "DIFAT")?;
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
        let base = sector_offset(sec, ss, 0x2c, "FAT")?;
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
        let base = sector_offset(dir_sec, ss, 0x30, "directory")?;
        for slot in 0..(ss / ENTRY_LEN) {
            let off = base + slot * ENTRY_LEN;
            if off
                .checked_add(ENTRY_LEN)
                .is_none_or(|end| end > data.len())
            {
                return Err(Error::invalid(off as u64, "truncated CFB directory sector"));
            }
            offsets.push(off);
        }
        dir_sec = fat.get(dir_sec as usize).copied().unwrap_or(NOSTREAM);
    }
    Ok(offsets)
}

/// The UTF-16 name of the entry at `off`, without its terminator.
pub(crate) fn entry_utf16_name(data: &[u8], off: usize) -> Vec<u16> {
    let byte_len = usize::from(u16_le(data, off + NAME_LEN).unwrap_or(0))
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

/// The upper-case form of `c` that CFB name order compares: the simple case conversion of
/// [MS-CFB] 2.6.4, a character maps to one character or stays unchanged.
///
/// It equals the `cfb` crate's conversion, which checks sibling order when it opens a file:
/// Rust's uppercase mapping where that is one character; otherwise (`ß`, `ŉ`, `ǰ`,
/// ligatures, Greek with breathing or iota marks, ...) the character itself, except that
/// the Greek letters with ypogegrammeni map to their titlecase form (`ᾀ` → `ᾈ`, `ᾳ` →
/// `ᾼ`), as their simple uppercase mapping does. Letters that received an uppercase in
/// Unicode 16 (`ƛ`, `ɤ`, `ᲊ`, `ꟍ`, `ꟛ` and Garay small letters) stay unchanged, as in
/// the ICU data the `cfb` crate uses.
fn cfb_upper(c: char) -> u32 {
    let u = u32::from(c);
    match u {
        0x1F80..=0x1FA7 if u & 0x8 == 0 => u + 8,
        0x1FB3 | 0x1FC3 | 0x1FF3 => u + 9,
        0x019B | 0x0264 | 0x1C8A | 0xA7CD | 0xA7DB | 0x10D70..=0x10D85 => u,
        _ => {
            let mut up = c.to_uppercase();
            match (up.next(), up.next()) {
                (Some(one), None) => u32::from(one),
                _ => u,
            }
        }
    }
}

/// The characters of a UTF-16 name, upper-cased by [`cfb_upper`]; an unpaired surrogate
/// stands for itself.
fn cfb_key(name: &[u16]) -> impl Iterator<Item = u32> + '_ {
    char::decode_utf16(name.iter().copied()).map(|r| match r {
        Ok(c) => cfb_upper(c),
        Err(e) => u32::from(e.unpaired_surrogate()),
    })
}

/// CFB sibling order ([MS-CFB] 2.6.4): shorter names (in UTF-16 units) first, then the
/// upper-cased characters by code point, as the `cfb` crate orders them.
pub(crate) fn cmp_cfb_name(a: &[u16], b: &[u16]) -> std::cmp::Ordering {
    a.len()
        .cmp(&b.len())
        .then_with(|| cfb_key(a).cmp(cfb_key(b)))
}

/// Whether the 128 entry bytes are the free form of [MS-CFB] 2.6.3: all zero except the
/// left sibling, right sibling and child ids, which hold `NOSTREAM`.
pub(crate) fn is_free_entry(entry: &[u8]) -> bool {
    entry.len() == ENTRY_LEN
        && entry.iter().enumerate().all(|(i, b)| {
            if (LEFT..CHILD + 4).contains(&i) {
                *b == 0xff
            } else {
                *b == 0
            }
        })
}

/// The free entry form of [MS-CFB] 2.6.3.
#[cfg(feature = "write")]
pub(crate) fn free_entry() -> [u8; ENTRY_LEN] {
    let mut entry = [0u8; ENTRY_LEN];
    for b in entry.iter_mut().skip(LEFT).take(12) {
        *b = 0xff;
    }
    entry
}

/// Slots of the sibling tree under `first_child`, each once (a slot reached twice, or a
/// child id outside the directory, is not followed again).
pub(crate) fn tree_slots(data: &[u8], offsets: &[usize], first_child: u32) -> BTreeSet<u32> {
    let mut children = BTreeSet::new();
    let mut stack = vec![first_child];
    while let Some(idx) = stack.pop() {
        if idx == NOSTREAM || (idx as usize) >= offsets.len() || !children.insert(idx) {
            continue;
        }
        if let Some(&off) = offsets.get(idx as usize) {
            for field in [LEFT, RIGHT] {
                let next = u32_le(data, off + field).unwrap_or(NOSTREAM);
                if next != NOSTREAM {
                    stack.push(next);
                }
            }
        }
    }
    children
}

/// Slots of the sibling tree under `first_child` that no earlier call claimed, now claimed,
/// and whether the tree reaches a slot an earlier call claimed (an entry with two parents).
/// The walk does not enter claimed slots, so calls over a whole directory walk every slot
/// once, however many storages point at one shared subtree. Without an overlap the slots
/// equal [`tree_slots`].
pub(crate) fn claim_tree_slots(
    data: &[u8],
    offsets: &[usize],
    first_child: u32,
    claimed: &mut BTreeSet<u32>,
) -> (BTreeSet<u32>, bool) {
    let mut mine = BTreeSet::new();
    let mut overlaps = false;
    let mut stack = vec![first_child];
    while let Some(idx) = stack.pop() {
        if idx == NOSTREAM || (idx as usize) >= offsets.len() || mine.contains(&idx) {
            continue;
        }
        if !claimed.insert(idx) {
            overlaps = true;
            continue;
        }
        mine.insert(idx);
        if let Some(&off) = offsets.get(idx as usize) {
            for field in [LEFT, RIGHT] {
                let next = u32_le(data, off + field).unwrap_or(NOSTREAM);
                if next != NOSTREAM {
                    stack.push(next);
                }
            }
        }
    }
    (mine, overlaps)
}

/// Every slot reachable from the root entry (slot 0) through child trees. Each slot's
/// siblings are followed once, so a subtree shared by several storages costs one walk.
#[cfg(feature = "write")]
pub(crate) fn reachable_slots(data: &[u8], offsets: &[usize]) -> BTreeSet<u32> {
    let mut reached = BTreeSet::new();
    if offsets.is_empty() {
        return reached;
    }
    let mut walked = BTreeSet::new();
    let mut storages = vec![0u32];
    reached.insert(0);
    while let Some(storage) = storages.pop() {
        let Some(&off) = offsets.get(storage as usize) else {
            continue;
        };
        let mut stack = vec![u32_le(data, off + CHILD).unwrap_or(NOSTREAM)];
        while let Some(idx) = stack.pop() {
            if idx == NOSTREAM || (idx as usize) >= offsets.len() || !walked.insert(idx) {
                continue;
            }
            if reached.insert(idx) {
                storages.push(idx);
            }
            if let Some(&o) = offsets.get(idx as usize) {
                for field in [LEFT, RIGHT] {
                    let next = u32_le(data, o + field).unwrap_or(NOSTREAM);
                    if next != NOSTREAM {
                        stack.push(next);
                    }
                }
            }
        }
    }
    reached
}

/// Shape of one sibling tree: its root and, per slot, the left and right child and colour.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct TreeShape {
    pub root: Option<u32>,
    /// Slot -> (left, right, red).
    pub nodes: BTreeMap<u32, (Option<u32>, Option<u32>, bool)>,
}

#[derive(Clone)]
struct RbNode {
    left: Option<u32>,
    right: Option<u32>,
    parent: Option<u32>,
    red: bool,
    key: Vec<u16>,
}

type Nodes = BTreeMap<u32, RbNode>;

fn rb_left(nodes: &Nodes, x: u32) -> Option<u32> {
    nodes.get(&x).and_then(|n| n.left)
}

fn rb_right(nodes: &Nodes, x: u32) -> Option<u32> {
    nodes.get(&x).and_then(|n| n.right)
}

fn rb_parent(nodes: &Nodes, x: u32) -> Option<u32> {
    nodes.get(&x).and_then(|n| n.parent)
}

fn rb_red(nodes: &Nodes, x: Option<u32>) -> bool {
    x.and_then(|id| nodes.get(&id)).is_some_and(|n| n.red)
}

fn set_red(nodes: &mut Nodes, x: Option<u32>, red: bool) {
    if let Some(node) = x.and_then(|id| nodes.get_mut(&id)) {
        node.red = red;
    }
}

fn rotate_left(nodes: &mut Nodes, root: &mut Option<u32>, x: u32) {
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

fn rotate_right(nodes: &mut Nodes, root: &mut Option<u32>, x: u32) {
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

fn rb_fixup(nodes: &mut Nodes, root: &mut Option<u32>, mut z: u32) {
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

/// The tree Sedgewick's top-down 2-3-4 red-black insertion builds from `slots` inserted in
/// slot order. Windows Structured Storage (`ole32`) trees have this shape, and so do most
/// observed DGN files (FORMAT_NOTES, "Compound file").
pub(crate) fn top_down_shape(data: &[u8], offsets: &[usize], slots: &BTreeSet<u32>) -> TreeShape {
    let mut nodes: Nodes = BTreeMap::new();
    let mut root: Option<u32> = None;
    for &z in slots {
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
    TreeShape {
        root,
        nodes: nodes
            .into_iter()
            .map(|(id, n)| (id, (n.left, n.right, n.red)))
            .collect(),
    }
}

/// The tree stored under `first_child`, in the form of [`top_down_shape`].
pub(crate) fn stored_shape(data: &[u8], offsets: &[usize], first_child: u32) -> TreeShape {
    let link = |v: u32| (v != NOSTREAM).then_some(v);
    TreeShape {
        root: link(first_child),
        nodes: tree_slots(data, offsets, first_child)
            .into_iter()
            .filter_map(|slot| {
                let off = *offsets.get(slot as usize)?;
                let left = link(u32_le(data, off + LEFT).unwrap_or(NOSTREAM));
                let right = link(u32_le(data, off + RIGHT).unwrap_or(NOSTREAM));
                Some((slot, (left, right, data.get(off + COLOR) == Some(&0))))
            })
            .collect(),
    }
}

/// How a stored sibling tree measures against [MS-CFB] 2.6.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TreeCheck {
    /// Ordered, and a red-black tree: colours 0/1, a black root, no red node with a red
    /// child and the same number of black nodes on every path.
    RedBlack,
    /// Ordered, so lookups by name work, but the colours break a red-black rule (MS-CFB
    /// allows an all-black tree, which is ordered but not balanced).
    Unbalanced,
    /// A lookup by name can fail: the in-order names do not strictly ascend, a slot is
    /// reached twice or a child id lies outside the directory.
    Unordered,
}

/// Checks the sibling tree under `first_child` without recursion.
pub(crate) fn check_tree(data: &[u8], offsets: &[usize], first_child: u32) -> TreeCheck {
    let colour = |slot: u32| {
        offsets
            .get(slot as usize)
            .and_then(|&off| data.get(off + COLOR))
            .copied()
    };
    let link = |slot: u32, field: usize| {
        offsets
            .get(slot as usize)
            .and_then(|&off| u32_le(data, off + field))
            .unwrap_or(NOSTREAM)
    };
    let mut ordered = true;
    let mut red_black = colour(first_child) == Some(1);
    let mut seen = BTreeSet::new();
    let mut previous: Option<Vec<u16>> = None;
    // In-order walk; a frame is (slot, black nodes above it, left subtree done).
    let mut stack: Vec<(u32, u32, bool)> = vec![(first_child, 0, false)];
    let mut leaf_height: Option<u32> = None;
    while let Some((slot, blacks, left_done)) = stack.pop() {
        if slot == NOSTREAM {
            if leaf_height.is_some_and(|h| h != blacks) {
                red_black = false;
            }
            leaf_height.get_or_insert(blacks);
            continue;
        }
        let Some(&off) = offsets.get(slot as usize) else {
            ordered = false;
            continue;
        };
        let c = colour(slot).unwrap_or(2);
        let blacks = blacks + u32::from(c == 1);
        if !left_done {
            if !seen.insert(slot) {
                ordered = false;
                continue;
            }
            if c > 1 {
                red_black = false;
            }
            for field in [LEFT, RIGHT] {
                let next = link(slot, field);
                if c == 0 && next != NOSTREAM && colour(next) == Some(0) {
                    red_black = false;
                }
            }
            stack.push((slot, blacks - u32::from(c == 1), true));
            stack.push((link(slot, LEFT), blacks, false));
            continue;
        }
        let key = entry_utf16_name(data, off);
        if previous
            .as_ref()
            .is_some_and(|p| !cmp_cfb_name(p, &key).is_lt())
        {
            ordered = false;
        }
        previous = Some(key);
        stack.push((link(slot, RIGHT), blacks, false));
    }
    match (ordered, red_black) {
        (false, _) => TreeCheck::Unordered,
        (true, true) => TreeCheck::RedBlack,
        (true, false) => TreeCheck::Unbalanced,
    }
}

/// Directory slot rank of the writer's output: `Dgn~H` at slot 1, `Dgn~S` at slot 2,
/// `Dgn-Md` at slot 3, every storage before its children and `^AH` before `$1` in
/// `Dgn^GA`/`Dgn^CA`. It is the order of one seed lineage of the private corpus (277 of
/// 567 files start with these three slots, the others and the public files differ); no
/// reader is known to depend on it, since lookups go through the sibling trees.
pub(crate) fn writer_slot_rank(path: &str) -> (usize, &str, usize, &str) {
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
