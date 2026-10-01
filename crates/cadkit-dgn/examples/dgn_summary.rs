//! Prints a structural summary of a DGN file: models, entity kind histogram, layers,
//! blocks and warnings. `--entities` also dumps every top-level entity (Debug format).
//!
//! ```text
//! cargo run -p cadkit-dgn --example dgn_summary -- drawing.dgn [--entities]
//! ```
#![allow(clippy::print_stdout)] // a command-line example: printing is its purpose

use std::collections::BTreeMap;

use cadkit_core::{Entity, EntityKind, ReadOptions, Value};

fn count(e: &Entity, kinds: &mut BTreeMap<String, usize>, attrs: &mut usize, depth: usize) {
    let t = match e.props.get("dgn.type") {
        Some(Value::Int(t)) => format!("{} (type {t})", e.kind.type_name()),
        _ => e.kind.type_name().to_owned(),
    };
    *kinds
        .entry(format!("{}{}", "  ".repeat(depth), t))
        .or_default() += 1;
    *attrs += e.attributes.len();
    if let EntityKind::Group { children, .. } = &e.kind {
        for c in children {
            count(c, kinds, attrs, depth + 1);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("usage: dgn_summary <file.dgn> [--entities] [--codepage <label>]");
        return;
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{path}: {e}");
            return;
        }
    };
    let mut options = ReadOptions::default();
    if let Some(i) = args.iter().position(|a| a == "--codepage") {
        options.fallback_codepage = args.get(i + 1).cloned();
    }
    println!("sniff: {}", cadkit_dgn::sniff(&bytes));
    let doc = match cadkit_dgn::read(&bytes, &options) {
        Ok(d) => d,
        Err(e) => {
            println!("error: {e}");
            return;
        }
    };
    println!(
        "format: {:?} {} app={:?} codepage={:?}",
        doc.source.format, doc.source.version, doc.source.application, doc.source.codepage
    );
    println!("units: {:?}", doc.units);
    println!(
        "layers: {}  linetypes: {}  text styles: {}  blocks: {}",
        doc.layers.len(),
        doc.linetypes.len(),
        doc.text_styles.len(),
        doc.blocks.len()
    );
    for m in &doc.models {
        let mut kinds = BTreeMap::new();
        let mut attrs = 0;
        for e in &m.entities {
            count(e, &mut kinds, &mut attrs, 0);
        }
        println!(
            "model '{}' 3d={} entities={} attributes={}",
            m.name,
            m.is_3d,
            m.entities.len(),
            attrs
        );
        for (k, n) in kinds {
            println!("    {n:6}  {k}");
        }
        if args.iter().any(|a| a == "--entities") {
            for e in &m.entities {
                println!("{e:?}");
            }
        }
    }
    for w in &doc.warnings {
        println!("warning {}: {}", w.code, w.message);
    }
}
