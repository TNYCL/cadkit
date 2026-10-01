//! Truncated and bit-flipped public samples must make `read` return (Ok or Err)
//! without panicking, hanging or exceeding the limits. Skips when the corpus is missing.

use std::path::PathBuf;
use std::time::Instant;

use cadkit_core::{Limits, ReadOptions};

const SAMPLES: [&str; 7] = [
    "AC1014", "AC1015", "AC1018", "AC1021", "AC1024", "AC1027", "AC1032",
];

fn corpus_file(name: &str) -> Option<Vec<u8>> {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "corpus",
        "public",
        "acadsharp",
        name,
    ]
    .iter()
    .collect();
    std::fs::read(path).ok()
}

/// xorshift64*: deterministic, so failures reproduce.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn options() -> ReadOptions {
    ReadOptions {
        limits: Limits {
            max_decompressed_bytes: 256 << 20,
            ..Limits::default()
        },
        ..ReadOptions::default()
    }
}

fn read_both(bytes: &[u8], options: &ReadOptions) {
    let _ = cadkit_dwg::read(bytes, options);
    let _ = cadkit_dwg::native::read_native(bytes, options);
}

#[test]
fn truncated_samples_never_panic() {
    let options = options();
    for version in SAMPLES {
        let Some(bytes) = corpus_file(&format!("sample_{version}.dwg")) else {
            continue;
        };
        let len = bytes.len();
        let mut cuts = vec![
            0,
            1,
            5,
            6,
            0x0D,
            0x15,
            0x20,
            0x80,
            0xEC,
            0x100,
            0x400,
            0x480,
            0x500,
            len.saturating_sub(1),
        ];
        cuts.extend((1..24).map(|k| len * k / 24));
        for cut in cuts {
            let slice = bytes.get(..cut.min(len)).unwrap_or(&[]);
            read_both(slice, &options);
        }
    }
}

#[test]
fn bit_flipped_samples_never_panic() {
    let options = options();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for version in SAMPLES {
        let Some(bytes) = corpus_file(&format!("sample_{version}.dwg")) else {
            continue;
        };
        let started = Instant::now();
        for round in 0..60 {
            let mut mutated = bytes.clone();
            let flips = 1 + rng.below(16);
            for _ in 0..flips {
                // Half of the flips target the first 0x1000 bytes (file header, maps).
                let at = if round % 2 == 0 {
                    rng.below(0x1000.min(mutated.len()))
                } else {
                    rng.below(mutated.len())
                };
                if let Some(b) = mutated.get_mut(at) {
                    *b ^= 1 << rng.below(8);
                }
            }
            read_both(&mutated, &options);
        }
        // Each sample's 60 mutated reads must stay fast (no pathological loops).
        assert!(
            started.elapsed().as_secs() < 120,
            "{version}: mutated reads took {:?}",
            started.elapsed()
        );
    }
}

#[test]
fn random_overwrites_in_object_data_never_panic() {
    // Byte-level garbage inside the (uncompressed) R14/R2000 object data exercises the
    // object decoders directly; compressed versions mostly fail in the decompressor.
    let options = options();
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    for version in ["AC1014", "AC1015"] {
        let Some(bytes) = corpus_file(&format!("sample_{version}.dwg")) else {
            continue;
        };
        for _ in 0..40 {
            let mut mutated = bytes.clone();
            let start = rng.below(mutated.len());
            let run = 1 + rng.below(64);
            for i in start..(start + run).min(mutated.len()) {
                if let Some(b) = mutated.get_mut(i) {
                    *b = rng.next() as u8;
                }
            }
            read_both(&mutated, &options);
        }
    }
}

#[test]
fn tiny_limits_are_respected() {
    let Some(bytes) = corpus_file("sample_AC1018.dwg") else {
        return;
    };
    let options = ReadOptions {
        limits: Limits {
            max_decompressed_bytes: 1024,
            ..Limits::default()
        },
        ..ReadOptions::default()
    };
    assert!(matches!(
        cadkit_dwg::read(&bytes, &options),
        Err(cadkit_core::Error::LimitExceeded(_))
    ));
    let options = ReadOptions {
        limits: Limits {
            max_input_bytes: 100,
            ..Limits::default()
        },
        ..ReadOptions::default()
    };
    assert!(matches!(
        cadkit_dwg::read(&bytes, &options),
        Err(cadkit_core::Error::LimitExceeded(_))
    ));
    let options = ReadOptions {
        limits: Limits {
            max_objects: 10,
            ..Limits::default()
        },
        ..ReadOptions::default()
    };
    assert!(matches!(
        cadkit_dwg::read(&bytes, &options),
        Err(cadkit_core::Error::LimitExceeded(_))
    ));
}

/// Long mutation run for local use: `DWG_SOAK_ROUNDS=2000 cargo test -p cadkit-dwg
/// --test robustness soak -- --ignored`.
#[test]
#[ignore = "long-running soak test"]
fn soak_mutations() {
    let rounds: usize = std::env::var("DWG_SOAK_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(500);
    let seed: u64 = std::env::var("DWG_SOAK_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x5EED_1234_ABCD_0001);
    let options = options();
    let mut rng = Rng(seed);
    let mut inputs: Vec<Vec<u8>> = SAMPLES
        .iter()
        .filter_map(|v| corpus_file(&format!("sample_{v}.dwg")))
        .collect();
    // Extra drawings (e.g. the Apache Tika DWG test documents) from CADKIT_DWG_EXTRA_SAMPLES.
    if let Some(dir) = std::env::var_os("CADKIT_DWG_EXTRA_SAMPLES") {
        for entry in std::fs::read_dir(PathBuf::from(dir))
            .into_iter()
            .flatten()
            .flatten()
        {
            if entry
                .path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("dwg"))
            {
                inputs.extend(std::fs::read(entry.path()).ok());
            }
        }
    }
    for bytes in inputs {
        for _ in 0..rounds {
            let mut mutated = bytes.clone();
            match rng.below(3) {
                0 => {
                    for _ in 0..1 + rng.below(32) {
                        let at = rng.below(mutated.len());
                        if let Some(b) = mutated.get_mut(at) {
                            *b ^= 1 << rng.below(8);
                        }
                    }
                }
                1 => {
                    let start = rng.below(mutated.len());
                    for i in start..(start + 1 + rng.below(256)).min(mutated.len()) {
                        if let Some(b) = mutated.get_mut(i) {
                            *b = rng.next() as u8;
                        }
                    }
                }
                _ => {
                    let cut = rng.below(mutated.len());
                    mutated.truncate(cut);
                }
            }
            read_both(&mutated, &options);
        }
    }
}

/// Structural check over private drawings (`corpus/private/**/*.dwg`), skipped when
/// none exist. Asserts only that reading returns without panicking and that a readable
/// file has a model space; no names or contents are reported.
#[test]
fn private_corpus_reads_structurally() {
    let root: PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", "..", "corpus", "private"]
        .iter()
        .collect();
    let mut stack = vec![root];
    let mut files = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("dwg"))
            {
                files.push(path);
            }
        }
    }
    let mut readable = 0usize;
    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if let Ok(doc) = cadkit_dwg::read(&bytes, &ReadOptions::default()) {
            readable += 1;
            assert!(
                doc.models
                    .iter()
                    .any(|m| m.kind == cadkit_core::ModelKind::Model),
                "private file #{readable} has no model space"
            );
        }
    }
    assert!(readable <= files.len());
}
