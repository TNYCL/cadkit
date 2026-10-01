//! Developer helper: prints the document read from a DXF file as JSON, or one model-space
//! entity in debug form when an index is given.
//!
//! `cargo run -p cadkit-dxf --example dump -- FILE.dxf [ENTITY_INDEX]`
#![allow(clippy::print_stdout)]

use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: dump FILE.dxf [ENTITY_INDEX]")?;
    let which: Option<usize> = args.next().and_then(|s| s.parse().ok());
    let bytes = std::fs::read(path)?;
    let doc = cadkit_dxf::read(&bytes, &cadkit_core::ReadOptions::default())?;
    match which {
        Some(i) => {
            let e = doc
                .models
                .first()
                .and_then(|m| m.entities.get(i))
                .ok_or("no such entity")?;
            println!("{e:#?}");
        }
        None => println!("{}", serde_json::to_string_pretty(&doc)?),
    }
    Ok(())
}
