//! Truncated and bit-flipped corpus files must never panic: `read` returns Ok or Err.
//! Uses every public sample and (when present) the private corpus.
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

mod common;

use cadkit_core::{Limits, ReadOptions};
use common::{corpus, private_files};

const SAMPLES: [(&str, usize); 7] = [
    // (path, number of mutated variants of each kind)
    ("public/gdal/test_dgnv8.dgn", 300),
    ("public/gdal/smalltest.dgn", 300),
    ("public/gdal/knot_oob.dgn", 200),
    ("public/gdal/seed_2d.dgn", 100),
    ("public/gdal/seed_3d.dgn", 100),
    ("public/safe/TreeTextNodeLabelsWithTags.dgn", 6),
    ("public/safe/Water_distribution_mains.dgn", 3),
];

/// Deterministic xorshift generator (no dependency needed).
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

fn options() -> ReadOptions {
    // Small but sufficient limits keep hostile variants cheap.
    ReadOptions {
        limits: Limits {
            max_decompressed_bytes: 64 << 20,
            max_objects: 2_000_000,
            ..Limits::default()
        },
        keep_raw: true,
        fallback_codepage: Some("windows-1254".into()),
    }
}

#[test]
fn truncated_and_mutated_samples_do_not_panic() {
    let opts = options();
    let public = SAMPLES
        .iter()
        .filter_map(|(path, variants)| corpus(path).map(|b| (b, *variants)));
    let private = private_files("dgn").into_iter().map(|b| (b, 120));
    for (bytes, variants) in public.chain(private) {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ bytes.len() as u64);
        // Truncations: fixed edge cases plus evenly spread cut points.
        let mut cuts = vec![0, 1, 4, 8, 511, 512, 513, 1535, 1536, 1537];
        cuts.extend((0..variants).map(|i| bytes.len() * i / variants.max(1)));
        for cut in cuts.into_iter().filter(|&c| c < bytes.len()) {
            let _ = cadkit_dgn::sniff(&bytes[..cut]);
            let _ = cadkit_dgn::read(&bytes[..cut], &opts);
        }
        // Bit flips: 1..=16 random bytes, sometimes concentrated in the header region.
        for i in 0..variants {
            let mut m = bytes.clone();
            let flips = 1 + rng.below(16);
            for _ in 0..flips {
                let range = if i % 3 == 0 {
                    m.len().min(4096)
                } else {
                    m.len()
                };
                let at = rng.below(range);
                m[at] ^= 1 << rng.below(8);
            }
            let _ = cadkit_dgn::sniff(&m);
            let _ = cadkit_dgn::read(&m, &opts);
        }
    }
}

#[test]
fn garbage_and_other_formats_are_rejected() {
    let opts = ReadOptions::default();
    for junk in [
        &b""[..],
        b"\x00",
        b"hello world",
        &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1][..],
    ] {
        assert!(!cadkit_dgn::sniff(junk));
        assert!(cadkit_dgn::read(junk, &opts).is_err());
    }
    // A compound file without the DGN header stream (e.g. a .doc) is not a DGN.
    if let Some(dwg) = corpus("public/acadsharp/sample_AC1032.dwg") {
        assert!(!cadkit_dgn::sniff(&dwg));
    }
}

#[test]
fn limits_are_honoured() {
    let Some(bytes) = corpus("public/gdal/test_dgnv8.dgn") else {
        return;
    };
    let tight = ReadOptions {
        limits: Limits {
            max_decompressed_bytes: 1000,
            ..Limits::default()
        },
        ..Default::default()
    };
    assert!(matches!(
        cadkit_dgn::read(&bytes, &tight),
        Err(cadkit_core::Error::LimitExceeded(_))
    ));
    let few = ReadOptions {
        limits: Limits {
            max_objects: 10,
            ..Limits::default()
        },
        ..Default::default()
    };
    assert!(matches!(
        cadkit_dgn::read(&bytes, &few),
        Err(cadkit_core::Error::LimitExceeded(_))
    ));
    let small = ReadOptions {
        limits: Limits {
            max_input_bytes: 100,
            ..Limits::default()
        },
        ..Default::default()
    };
    assert!(cadkit_dgn::read(&bytes, &small).is_err());
}
