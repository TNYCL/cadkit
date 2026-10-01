//! Structure-aware mutation tests.
//!
//! Plain bit flips in a V8 file mostly break zlib, so the element decoders never see the
//! damage. Here corpus files are unpacked, their *inflated* streams mutated (random flips,
//! extreme values stamped into element fields, truncation), recompressed and repacked into
//! a new compound file, then read. The reader must return Ok or Err, never panic.

use std::path::PathBuf;

use cadkit_core::{Limits, ReadOptions};

use crate::cfb::tests::build;
use crate::cfb::{CompoundFile, EntryKind};
use crate::{le, zlib};

fn corpus(rel: &str) -> Option<Vec<u8>> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus")
            .join(rel),
    )
    .ok()
}

/// Every V8 (compound file) DGN under `corpus/private/dgn`.
fn private_v8() -> Vec<Vec<u8>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/private/dgn");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| std::fs::read(p).ok())
        .filter(|b| crate::native::v8::sniff(b))
        .collect()
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Unpacks every stream, lets `mutate` change the inflated payload of zlib streams, and
/// builds a new compound file.
fn repack(bytes: &[u8], mut mutate: impl FnMut(&str, &mut Vec<u8>)) -> Vec<u8> {
    let cf = CompoundFile::parse(bytes).unwrap();
    let mut streams: Vec<(String, Vec<u8>)> = Vec::new();
    for p in cf.paths().iter().filter(|p| p.kind == EntryKind::Stream) {
        let data = cf.read_stream(p.index).unwrap();
        let leaf = p.path.rsplit('/').next().unwrap_or_default();
        let head = if leaf.starts_with('$') {
            Some(16)
        } else if leaf == "Dgn~Mh" || leaf == "Dgn~Mix" {
            Some(0)
        } else if leaf == "Dgn~H" {
            Some(0x14)
        } else {
            None
        };
        let data = match head {
            Some(h) if data.len() > h => {
                let mut inflated = zlib::inflate(&data[h..], 1 << 28).data;
                mutate(&p.path, &mut inflated);
                let mut out = data[..h].to_vec();
                out.extend_from_slice(&zlib::compress(&inflated));
                out
            }
            _ => data,
        };
        streams.push((p.path.clone(), data));
    }
    let refs: Vec<(&str, Vec<u8>)> = streams
        .iter()
        .map(|(p, d)| (p.as_str(), d.clone()))
        .collect();
    build(&refs)
}

/// Element start offsets in an inflated page (after the 4-byte record prefix).
fn element_offsets(page: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while let Some(words) = le::u32_at(page, off + 8) {
        let len = words as usize * 2;
        if len < 12 || off + 4 + len > page.len() {
            break;
        }
        out.push((off + 4, len));
        off += 4 + len;
    }
    out
}

fn options() -> ReadOptions {
    ReadOptions {
        limits: Limits {
            max_decompressed_bytes: 256 << 20,
            max_vertices: 200_000,
            ..Limits::default()
        },
        keep_raw: false,
        fallback_codepage: None,
    }
}

#[test]
fn repacking_without_changes_reads_the_same_document() {
    let Some(bytes) = corpus("public/gdal/test_dgnv8.dgn") else {
        return;
    };
    let original = crate::read(&bytes, &options()).unwrap();
    let repacked = crate::read(&repack(&bytes, |_, _| {}), &options()).unwrap();
    assert_eq!(original.models, repacked.models);
    assert_eq!(original.layers, repacked.layers);
    assert_eq!(original.blocks, repacked.blocks);
}

#[test]
fn mutated_inflated_streams_do_not_panic() {
    const EXTREMES: [u32; 6] = [0, 1, 0x7fff_ffff, 0x8000_0000, 0xffff_ffff, 0x0001_0000];
    // Public sample plus every private V8 file (discovered, never named).
    let mut samples: Vec<(Vec<u8>, usize)> = corpus("public/gdal/test_dgnv8.dgn")
        .map(|b| (b, 160))
        .into_iter()
        .collect();
    samples.extend(private_v8().into_iter().map(|b| (b, 60)));
    for (bytes, rounds) in samples {
        let mut rng = Rng(0xD1B5_4A32_D192_ED03 ^ bytes.len() as u64);
        for round in 0..rounds {
            let mode = round % 4;
            let mutated = repack(&bytes, |stream, data| {
                if data.is_empty() || rng.below(3) == 0 {
                    return;
                }
                match mode {
                    // Random byte flips anywhere.
                    0 => {
                        for _ in 0..1 + rng.below(8) {
                            let at = rng.below(data.len());
                            data[at] ^= 1 << rng.below(8);
                        }
                    }
                    // Extreme values stamped into fields of whole elements (records still walk).
                    1 | 2 if stream.contains('$') => {
                        let elements = element_offsets(data);
                        for _ in 0..1 + rng.below(6) {
                            let Some(&(start, len)) = elements.get(rng.below(elements.len()))
                            else {
                                break;
                            };
                            if len <= 16 {
                                continue;
                            }
                            let at = start + 12 + rng.below(len - 16) / 2 * 2;
                            let v = EXTREMES[rng.below(EXTREMES.len())];
                            if mode == 1 {
                                data[at..at + 4].copy_from_slice(&v.to_le_bytes());
                            } else {
                                let f = [f64::NAN, f64::INFINITY, 1e308, -1e308, 0.0][rng.below(5)];
                                let at = at.min(start + len - 8);
                                data[at..at + 8].copy_from_slice(&f.to_le_bytes());
                            }
                        }
                    }
                    // Truncation.
                    _ => {
                        let keep = rng.below(data.len());
                        data.truncate(keep);
                    }
                }
            });
            let _ = crate::read(&mutated, &options());
            let _ = crate::native::v8::read(&mutated, &options());
        }
    }
}

#[test]
fn mutated_v7_records_do_not_panic() {
    let Some(bytes) = corpus("public/gdal/smalltest.dgn") else {
        return;
    };
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    // Record starts, to stamp extreme values into bodies while keeping the framing.
    let mut starts = Vec::new();
    let mut off = 0usize;
    while let Some(w) = le::u16_at(&bytes, off + 2) {
        if bytes[off] == 0xff && bytes[off + 1] == 0xff {
            break;
        }
        starts.push((off, 4 + 2 * w as usize));
        off += 4 + 2 * w as usize;
    }
    for _ in 0..400 {
        let mut m = bytes.clone();
        for _ in 0..1 + rng.below(6) {
            let (start, len) = starts[rng.below(starts.len())];
            if len <= 8 {
                continue;
            }
            let at = start + 4 + rng.below(len - 8);
            m[at] = [0x00, 0xff, 0x80, 0x7f][rng.below(4)];
        }
        let _ = crate::read(&m, &options());
    }
}
