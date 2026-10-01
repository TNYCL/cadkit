//! Truncated and bit-flipped inputs must never panic, and limits must be enforced.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::print_stdout
)]
mod common;

use cadkit_core::{Error, Limits, ReadOptions};
use common::*;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*: deterministic, no dependency.
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % (n.max(1) as u64)) as usize
    }
}

fn read_must_not_panic(label: &str, bytes: &[u8], opts: &ReadOptions) -> bool {
    let r = std::panic::catch_unwind(|| cadkit_dxf::read(bytes, opts));
    match r {
        Ok(res) => res.is_ok(),
        Err(_) => panic!("read panicked on {label}"),
    }
}

#[test]
fn truncation_and_bit_flips_never_panic() {
    let opts = ReadOptions::default();
    let mut total = 0usize;
    let mut ok = 0usize;
    for name in [
        "sample_AC1009_ascii.dxf",
        "sample_AC1009_binary.dxf",
        "sample_AC1015_ascii.dxf",
        "sample_AC1032_binary.dxf",
    ] {
        let Some(bytes) = corpus_file(name) else {
            continue;
        };
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ bytes.len() as u64);
        // Truncations: the first bytes (header sniffing) and random cut points.
        let mut cuts: Vec<usize> = (0..40).collect();
        cuts.extend((0..24).map(|_| rng.below(bytes.len())));
        for cut in cuts {
            total += 1;
            ok += usize::from(read_must_not_panic(
                &format!("{name} truncated at {cut}"),
                &bytes[..cut.min(bytes.len())],
                &opts,
            ));
        }
        // Bit flips and byte overwrites.
        for round in 0..24 {
            let mut m = bytes.clone();
            let flips = 1 + rng.below(32);
            for _ in 0..flips {
                let at = rng.below(m.len());
                if round % 3 == 0 {
                    m[at] = rng.next() as u8;
                } else {
                    m[at] ^= 1 << rng.below(8);
                }
            }
            total += 1;
            ok += usize::from(read_must_not_panic(
                &format!("{name} mutated (round {round})"),
                &m,
                &opts,
            ));
        }
    }
    println!("{total} damaged inputs, {ok} still produced a document, no panics");
}

#[test]
fn damaged_binary_headers_and_noise() {
    let opts = ReadOptions::default();
    let mut rng = Rng(42);
    let sentinel = cadkit_dxf::native::BINARY_SENTINEL;
    for len in [0usize, 1, 7, 22, 23, 24, 25, 100, 4096] {
        let mut bytes = sentinel.to_vec();
        bytes.truncate(len.min(sentinel.len()));
        while bytes.len() < len {
            bytes.push(rng.next() as u8);
        }
        read_must_not_panic("sentinel + noise", &bytes, &opts);
        let noise: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        read_must_not_panic("pure noise", &noise, &opts);
        let text = format!("0\nSECTION\n2\nENTITIES\n{}", "0\nLINE\n10\n".repeat(len));
        read_must_not_panic("repeated partial records", text.as_bytes(), &opts);
    }
}

fn tiny(limits: Limits) -> ReadOptions {
    ReadOptions {
        limits,
        ..ReadOptions::default()
    }
}

#[test]
fn limits_are_enforced() {
    // String length.
    let text =
        "0\nSECTION\n2\nENTITIES\n0\nTEXT\n1\nabcdefghijklmnopqrstuvwxyz\n0\nENDSEC\n0\nEOF\n";
    let r = cadkit_dxf::read(
        text.as_bytes(),
        &tiny(Limits {
            max_string_bytes: 8,
            ..Limits::default()
        }),
    );
    assert!(
        matches!(r, Err(Error::LimitExceeded(_))),
        "string limit: {r:?}"
    );
    assert!(cadkit_dxf::read(text.as_bytes(), &ReadOptions::default()).is_ok());

    // Input size.
    let r = cadkit_dxf::read(
        text.as_bytes(),
        &tiny(Limits {
            max_input_bytes: 10,
            ..Limits::default()
        }),
    );
    assert!(
        matches!(r, Err(Error::LimitExceeded(_))),
        "input limit: {r:?}"
    );

    // Entity count.
    let many = format!(
        "0\nSECTION\n2\nENTITIES\n{}0\nENDSEC\n0\nEOF\n",
        "0\nPOINT\n10\n1\n20\n2\n".repeat(50)
    );
    let r = cadkit_dxf::read(
        many.as_bytes(),
        &tiny(Limits {
            max_objects: 10,
            ..Limits::default()
        }),
    );
    assert!(
        matches!(r, Err(Error::LimitExceeded(_))),
        "object limit: {r:?}"
    );

    // Vertex counts: LWPOLYLINE, SPLINE, POLYLINE and HATCH.
    let lw = format!(
        "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n{}0\nENDSEC\n0\nEOF\n",
        "10\n1\n20\n2\n".repeat(50)
    );
    let r = cadkit_dxf::read(
        lw.as_bytes(),
        &tiny(Limits {
            max_vertices: 10,
            ..Limits::default()
        }),
    );
    assert!(
        matches!(r, Err(Error::LimitExceeded(_))),
        "lwpolyline vertex limit: {r:?}"
    );
    let sp = format!(
        "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n{}0\nENDSEC\n0\nEOF\n",
        "40\n1\n".repeat(50)
    );
    let r = cadkit_dxf::read(
        sp.as_bytes(),
        &tiny(Limits {
            max_vertices: 10,
            ..Limits::default()
        }),
    );
    assert!(
        matches!(r, Err(Error::LimitExceeded(_))),
        "spline knot limit: {r:?}"
    );
    let pl = format!(
        "0\nSECTION\n2\nENTITIES\n0\nPOLYLINE\n66\n1\n{}0\nSEQEND\n0\nENDSEC\n0\nEOF\n",
        "0\nVERTEX\n10\n1\n20\n2\n".repeat(50)
    );
    let r = cadkit_dxf::read(
        pl.as_bytes(),
        &tiny(Limits {
            max_vertices: 10,
            ..Limits::default()
        }),
    );
    assert!(
        matches!(r, Err(Error::LimitExceeded(_))),
        "polyline vertex limit: {r:?}"
    );
    let hatch = "0\nSECTION\n2\nENTITIES\n0\nHATCH\n91\n1\n92\n2\n72\n0\n73\n1\n93\n2000000000\n0\nENDSEC\n0\nEOF\n";
    let r = cadkit_dxf::read(
        hatch.as_bytes(),
        &tiny(Limits {
            max_vertices: 100,
            ..Limits::default()
        }),
    );
    assert!(
        matches!(r, Err(Error::LimitExceeded(_))),
        "hatch count limit: {r:?}"
    );

    // A record with an absurd number of groups.
    let rec = format!(
        "0\nSECTION\n2\nENTITIES\n0\nLINE\n{}0\nENDSEC\n0\nEOF\n",
        "1\nx\n".repeat(5000)
    );
    let r = cadkit_dxf::read(
        rec.as_bytes(),
        &tiny(Limits {
            max_vertices: 100,
            ..Limits::default()
        }),
    );
    assert!(
        matches!(r, Err(Error::LimitExceeded(_))),
        "record group limit: {r:?}"
    );
}

#[test]
fn truncated_file_degrades_to_a_partial_document() {
    let Some(bytes) = corpus_file("sample_AC1015_ascii.dxf") else {
        return;
    };
    // Cut inside the ENTITIES section: tables and blocks must survive with a warning.
    let at = bytes.windows(8).rposition(|w| w == b"ENTITIES").unwrap();
    let doc = cadkit_dxf::read(&bytes[..at + 2000], &ReadOptions::default()).unwrap();
    assert!(!doc.layers.is_empty() && !doc.blocks.is_empty());
    assert!(!doc.models[0].entities.is_empty());
}

#[test]
fn not_dxf_is_rejected() {
    assert!(matches!(
        cadkit_dxf::read(b"hello", &ReadOptions::default()),
        Err(Error::UnknownFormat)
    ));
    assert!(matches!(
        cadkit_dxf::read(b"", &ReadOptions::default()),
        Err(Error::UnknownFormat)
    ));
    assert!(!cadkit_dxf::sniff(b"AC1032\0\0\0\0"));
}
