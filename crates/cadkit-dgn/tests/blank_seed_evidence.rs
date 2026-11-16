//! Cross-writer comparison behind `docs/dgn/FORMAT_NOTES.md`, "Built-in blank seed": which
//! bytes of the streams every seed needs are equal in the public V8 samples of both writer
//! families (test_dgnv8 written with ODA Teigha, TreeText and Water saved by MicroStation), and
//! which undecoded fields differ between them, so that no built-in seed can be made of decoded
//! fields and format constants alone. The private corpus (`corpus/private/**/*.dgn`) is used for
//! corroborating counts only (AGENTS.md section 3). Skips silently when the files are absent.
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

mod common;

use std::io::Read;

use cadkit_dgn::cfb::CompoundFile;
use common::{corpus, private_dgn_paths};

const PUBLIC: [&str; 3] = [
    "public/gdal/test_dgnv8.dgn",
    "public/safe/TreeTextNodeLabelsWithTags.dgn",
    "public/safe/Water_distribution_mains.dgn",
];

/// Inflated `Dgn~H` dwords cadkit decodes and a writer sets per file: the element id
/// high-water mark (`u64` at `0x128`) and the save time (`f64` at `0x130`).
const HEADER_DECODED: [usize; 4] = [0x128, 0x12c, 0x130, 0x134];
/// Undecoded `Dgn~H` dwords whose values differ between the public samples.
const HEADER_DIFFERING: [usize; 9] = [
    0x110, 0x118, 0x138, 0x13c, 0x140, 0x308, 0x310, 0x314, 0x388,
];
/// Model header stream prefix dwords whose values differ between the public samples (dword 0,
/// the page counter, is set by the writer).
const PREFIX_DIFFERING: [usize; 2] = [4, 8];
/// Model header element dwords a writer sets from decoded meaning: type word (2D flag), length,
/// element id, element time, sub unit numerator, model time, extents, UOR per master.
const ELEMENT_DECODED: [usize; 24] = [
    0x00, 0x04, 0x10, 0x14, 0x18, 0x1c, 0x60, 0x64, 0x88, 0x8c, 0x90, 0x94, 0x98, 0x9c, 0xa0, 0xa4,
    0xa8, 0xac, 0xb0, 0xb4, 0xb8, 0xbc, 0xe0, 0xe4,
];
/// Undecoded model header element dwords whose values differ between the public samples: the
/// `u32` at `0x20` and `0x44` and the high halves of the `f64` at `0xF8`, `0x120`, `0x140`,
/// `0x160`, `0x1C8` and `0x1D8`.
const ELEMENT_DIFFERING: [usize; 8] = [0x20, 0x44, 0xfc, 0x124, 0x144, 0x164, 0x1cc, 0x1dc];
/// Length of the model header element's fixed part (attribute offset `0xF8` words) in every
/// public sample.
const ELEMENT_FIXED: usize = 0x1f0;
/// Bytes of the model header stream before its element.
const PREFIX_LEN: usize = 0x1004;

struct Streams {
    header_prefix: Vec<u8>,
    header: Vec<u8>,
    s: Vec<u8>,
    mf: Vec<u8>,
    mh_prefix: Vec<u8>,
    element: Vec<u8>,
    mix: Vec<u8>,
}

fn inflate(raw: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(raw)
        .read_to_end(&mut out)
        .ok()?;
    Some(out)
}

fn dword(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

/// The streams of a single-model V8 file, or `None` for anything else.
fn streams(bytes: &[u8]) -> Option<Streams> {
    let cf = CompoundFile::parse(bytes).ok()?;
    let read = |path: &str| cf.find(path).and_then(|i| cf.read_stream(i).ok());
    let h = read("Dgn~H")?;
    let models: Vec<String> = cf
        .paths()
        .iter()
        .filter(|p| p.path.starts_with("Dgn-Md/") && p.path.ends_with("/Dgn~Mh"))
        .map(|p| p.path.clone())
        .collect();
    let [mh_path] = models.as_slice() else {
        return None;
    };
    let mh = inflate(&read(mh_path)?)?;
    let header = inflate(h.get(0x14..)?)?;
    if header.len() != 1576 || mh.len() < PREFIX_LEN + ELEMENT_FIXED {
        return None;
    }
    let element = mh[PREFIX_LEN..].to_vec();
    if dword(&element, 0) & 0xffff != 66 {
        return None;
    }
    Some(Streams {
        header_prefix: h[..0x14].to_vec(),
        header,
        s: read("Dgn~S")?,
        mf: inflate(&read("Dgn~Mf")?)?,
        mh_prefix: mh[..PREFIX_LEN].to_vec(),
        element,
        mix: inflate(&read("Dgn^Ix/Dgn~Mix")?)?,
    })
}

/// Dword offsets of `0..len` (without `skip`) whose values differ between `blocks`.
fn differing(blocks: &[&[u8]], len: usize, skip: &[usize]) -> Vec<usize> {
    (0..len)
        .step_by(4)
        .filter(|o| !skip.contains(o))
        .filter(|&o| blocks.iter().any(|b| dword(b, o) != dword(blocks[0], o)))
        .collect()
}

fn public() -> Option<Vec<Streams>> {
    PUBLIC
        .iter()
        .map(|rel| corpus(rel).map(|b| streams(&b).expect(rel)))
        .collect()
}

#[test]
fn public_writers_agree_on_the_constant_blocks() {
    let Some(p) = public() else { return };
    // Dgn~H: the 0x14-byte prefix and 381 of the 394 payload dwords are equal; the equal
    // undecoded dwords are 0 except 0x114 (8) and 0x11C (2).
    assert!(p.iter().all(|s| s.header_prefix == p[0].header_prefix));
    assert_eq!(&p[0].header_prefix[..4], &[1, 0, 4, 0]);
    assert!(p[0].header_prefix[4..].iter().all(|&b| b == 0));
    let headers: Vec<&[u8]> = p.iter().map(|s| s.header.as_slice()).collect();
    assert_eq!(differing(&headers, 1576, &HEADER_DECODED), HEADER_DIFFERING);
    for o in (0..1576).step_by(4) {
        if HEADER_DECODED.contains(&o) || HEADER_DIFFERING.contains(&o) {
            continue;
        }
        let expected = match o {
            0x114 => 8,
            0x11c => 2,
            _ => 0,
        };
        assert_eq!(dword(&p[0].header, o), expected, "Dgn~H {o:#x}");
    }
    // Dgn~S: 208 bytes 0xFF. Dgn~Mf: 276 inflated bytes, 0x10 0x01 then zeros.
    for s in &p {
        assert_eq!(s.s, vec![0xff; 208]);
        assert_eq!(s.mf.len(), 276);
        assert_eq!(s.mf, p[0].mf);
    }
    assert_eq!(&p[0].mf[..2], &[0x10, 0x01]);
    assert!(p[0].mf[2..].iter().all(|&b| b == 0));
    // Model header prefix: zero apart from the page counter and the two differing dwords.
    let prefixes: Vec<&[u8]> = p.iter().map(|s| s.mh_prefix.as_slice()).collect();
    assert_eq!(differing(&prefixes, PREFIX_LEN, &[0]), PREFIX_DIFFERING);
    for o in (12..PREFIX_LEN).step_by(4) {
        assert_eq!(dword(&p[0].mh_prefix, o), 0);
    }
    // Model header element: the same fixed part, eight undecoded dwords differ.
    for s in &p {
        assert_eq!(2 * dword(&s.element, 8) as usize, ELEMENT_FIXED);
    }
    let elements: Vec<&[u8]> = p.iter().map(|s| s.element.as_slice()).collect();
    assert_eq!(
        differing(&elements, ELEMENT_FIXED, &ELEMENT_DECODED),
        ELEMENT_DIFFERING
    );
}

#[test]
fn public_writers_differ_in_the_blocking_fields() {
    let Some(p) = public() else { return };
    // Values in the order test_dgnv8, TreeText, Water (FORMAT_NOTES table).
    let at = |f: &dyn Fn(&Streams) -> u32| -> [u32; 3] { [f(&p[0]), f(&p[1]), f(&p[2])] };
    assert_eq!(at(&|s| dword(&s.header, 0x118)), [5, 0x67, 5]);
    assert_eq!(at(&|s| dword(&s.header, 0x13c)), [3, 0, 0]);
    assert_eq!(at(&|s| dword(&s.header, 0x140)), [0, 6, 4]);
    assert_eq!(at(&|s| dword(&s.element, 0x20)), [4, 6, 4]);
    assert_eq!(at(&|s| dword(&s.element, 0x44)), [1, 0, 1]);
    let f64_at = |b: &[u8], o: usize| f64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let f = |o: usize| -> [f64; 3] {
        [
            f64_at(&p[0].element, o),
            f64_at(&p[1].element, o),
            f64_at(&p[2].element, o),
        ]
    };
    assert_eq!(f(0xf8), [1e7, 0.0, 0.0]);
    assert_eq!(f(0x120), [0.0, 1.0, 1.0]);
    assert_eq!(f(0x1d8), [0.0, -1000.0, 0.0]);
    // The 3D bit 0x00400000 of the type word: clear in all three (two are 2D models).
    assert!(p.iter().all(|s| dword(&s.element, 0) & 0x0040_0000 == 0));
    // Model index entry (after the 16-byte index header): the first dword and the dword at
    // entry +0x1C differ.
    let entry = |s: &Streams, o: usize| dword(&s.mix, 16 + o);
    assert_eq!(
        [entry(&p[0], 0), entry(&p[1], 0), entry(&p[2], 0)],
        [0x1_0000, 0, 0x1_0000]
    );
    assert_ne!(entry(&p[1], 0x1c), entry(&p[0], 0x1c));
    assert_eq!(entry(&p[0], 0x1c), entry(&p[2], 0x1c));
}

/// Private corpus, counts only: every V8 file agrees with the public samples on every constant
/// of `Dgn~H`, `Dgn~Mf` and the model header prefix, and on the constant dwords of the model
/// header element except `0x38`; `Dgn~S` is all 0xFF in some files only.
#[test]
fn private_corpus_corroborates_the_constants() {
    let Some(p) = public() else { return };
    let paths = private_dgn_paths();
    let mut files = 0usize;
    let mut s_all_ff = 0usize;
    let mut element_0x38 = 0usize;
    for path in &paths {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Some(s) = streams(&bytes) else { continue };
        files += 1;
        assert_eq!(s.header_prefix, p[0].header_prefix);
        for o in (0..1576).step_by(4) {
            if !HEADER_DECODED.contains(&o) && !HEADER_DIFFERING.contains(&o) {
                assert_eq!(dword(&s.header, o), dword(&p[0].header, o));
            }
        }
        assert_eq!(s.mf, p[0].mf);
        for o in (12..PREFIX_LEN).step_by(4) {
            assert_eq!(dword(&s.mh_prefix, o), 0);
        }
        for o in (0..ELEMENT_FIXED).step_by(4) {
            if ELEMENT_DECODED.contains(&o) || ELEMENT_DIFFERING.contains(&o) {
                continue;
            }
            if o == 0x38 {
                element_0x38 += usize::from(dword(&s.element, o) == dword(&p[0].element, o));
                continue;
            }
            assert_eq!(dword(&s.element, o), dword(&p[0].element, o));
        }
        s_all_ff += usize::from(s.s.iter().all(|&b| b == 0xff));
    }
    if files == 0 {
        return;
    }
    eprintln!(
        "blank seed evidence: {files} private V8 files, Dgn~S all 0xFF in {s_all_ff}, model header 0x38 as public in {element_0x38}"
    );
    assert!(s_all_ff > 0 && s_all_ff < files);
    assert!(element_0x38 < files);
}
