//! Writes the DGN acceptance kit: small synthetic DGN V8 files written into a public seed,
//! each exercising one thing an external DGN application must confirm, with `MANIFEST.json`
//! and `README.md`. Every file is verified with cadkit first; the protocol for the person
//! opening them is `docs/dgn/ACCEPTANCE.md`.
//!
//! ```sh
//! python3 scripts/buildlock.py cargo run -p cadkit --example interchange -- \
//!   corpus/public/gdal/test_dgnv8.dgn [OUTPUT_DIRECTORY]
//! ```
//!
//! The output directory defaults to `target/interchange-validation`.

#[path = "support/acceptance_kit.rs"]
mod acceptance_kit;

use std::{
    io::{self, Write},
    path::PathBuf,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let seed_path = PathBuf::from(
        args.next()
            .ok_or_else(|| io::Error::other("usage: interchange SEED [OUTPUT_DIRECTORY]"))?,
    );
    let directory = args.next().map_or_else(
        || PathBuf::from("target/interchange-validation"),
        PathBuf::from,
    );
    let seed = std::fs::read(&seed_path)?;
    let seed_name = seed_path
        .file_name()
        .map_or_else(|| "seed.dgn".into(), |n| n.to_string_lossy().into_owned());
    let summary = acceptance_kit::generate(&seed, &seed_name, &directory)?;
    let mut out = io::stdout().lock();
    for f in &summary.files {
        writeln!(out, "{:>9}  {}", f.bytes, directory.join(&f.name).display())?;
    }
    writeln!(
        out,
        "{} files written and verified with cadkit; no external acceptance is claimed \
         (docs/dgn/ACCEPTANCE.md).",
        summary.files.len()
    )?;
    Ok(())
}
