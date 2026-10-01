//! Prints the neutral-model summary of a DWG file: units, tables, blocks, models.
//!
//! `cargo run -p cadkit-dwg --example docsummary -- file.dwg`

#![allow(clippy::print_stdout)]

use std::collections::BTreeMap;

use cadkit_core::ReadOptions;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        println!("usage: docsummary FILE");
        return;
    };
    let doc = match std::fs::read(&path)
        .map_err(|e| e.to_string())
        .and_then(|b| cadkit_dwg::read(&b, &ReadOptions::default()).map_err(|e| e.to_string()))
    {
        Ok(d) => d,
        Err(e) => {
            println!("error: {e}");
            return;
        }
    };
    println!(
        "{:?} {} units {:?} codepage {:?}",
        doc.source.format, doc.source.version, doc.units, doc.source.codepage
    );
    println!(
        "layers {} linetypes {} styles {} blocks {}",
        doc.layers.len(),
        doc.linetypes.len(),
        doc.text_styles.len(),
        doc.blocks.len()
    );
    for lt in &doc.linetypes {
        println!("  linetype {} {:?}", lt.name, lt.pattern);
    }
    for st in &doc.text_styles {
        println!("  style {} font {:?} h {}", st.name, st.font, st.height);
    }
    for b in &doc.blocks {
        println!("  block {} ({} entities)", b.name, b.entities.len());
    }
    for m in &doc.models {
        let mut hist: BTreeMap<String, usize> = BTreeMap::new();
        for e in &m.entities {
            let name = match &e.kind {
                cadkit_core::EntityKind::Unknown { type_name } => type_name.clone(),
                k => format!("{k:?}")
                    .split([' ', '{'])
                    .next()
                    .unwrap_or("?")
                    .to_owned(),
            };
            *hist.entry(name).or_default() += 1;
        }
        println!(
            "model {:?} {:?} {:?}: {hist:?}",
            m.name,
            m.kind,
            m.props.get("dwg.block")
        );
    }
    println!("warnings {}", doc.warnings.len());
    for w in &doc.warnings {
        println!("  [{}] {}", w.code, w.message);
    }
}
