//! Prints a structural summary of a DWG file: container facts, header variables,
//! classes, an object type histogram and decode errors.
//!
//! `cargo run -p cadkit-dwg --example dwgdump -- file.dwg [--vars] [--objects] [--handle HEX]`

#![allow(clippy::print_stdout)]

use std::collections::BTreeMap;

use cadkit_core::ReadOptions;
use cadkit_dwg::native::read_native;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(path) = args.get(1) else {
        println!("usage: dwgdump FILE [--vars] [--objects] [--handle HEX]");
        return;
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            println!("cannot read {path}: {e}");
            return;
        }
    };
    let file = match read_native(&bytes, &ReadOptions::default()) {
        Ok(f) => f,
        Err(e) => {
            println!("error: {e}");
            return;
        }
    };
    println!(
        "version {:?} maint {} codepage {} ({})",
        file.version, file.maintenance_version, file.codepage, file.encoding
    );
    println!("sections {:?}", file.section_names);
    println!(
        "header: {} vars, complete {}, crc {:?}",
        file.header.vars.len(),
        file.header.complete,
        file.header.crc_ok
    );
    for name in [
        "INSUNITS",
        "EXTMIN",
        "EXTMAX",
        "HANDSEED",
        "LUNITS",
        "FINGERPRINTGUID",
        "BLOCK_RECORD_MODEL_SPACE",
    ] {
        println!("  {name} = {:?}", file.header.get(name));
    }
    if args.iter().any(|a| a == "--vars") {
        for (n, v) in &file.header.vars {
            println!("  {n} = {v:?}");
        }
    }
    println!("measurement {:?}", file.measurement);
    println!("classes: {}", file.classes.len());
    for c in &file.classes {
        println!(
            "  {} {} {} {:#x}",
            c.number, c.dxf_name, c.cpp_class_name, c.item_class_id
        );
    }
    println!(
        "objects: {} of {} in map",
        file.objects.len(),
        file.handle_map.len()
    );
    let mut hist: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
    for o in file.objects.values() {
        let e = hist.entry(o.type_name.as_str()).or_default();
        e.0 += 1;
        if o.error.is_some() {
            e.1 += 1;
        }
        if o.crc_ok == Some(false) {
            e.2 += 1;
        }
    }
    for (name, (n, err, crc)) in hist {
        println!("  {name}: {n} (decode errors {err}, crc failures {crc})");
    }
    let mut left: BTreeMap<(&str, u64), usize> = BTreeMap::new();
    for o in file.objects.values() {
        if let Some(bits) = o.data_bits_left {
            *left.entry((o.type_name.as_str(), bits)).or_default() += 1;
        }
    }
    println!("decoded objects by (type, main-stream bits left): {left:?}");
    let mut hleft: BTreeMap<(&str, u64), usize> = BTreeMap::new();
    for o in file.objects.values() {
        if let Some(bits) = o.handle_bits_left {
            *hleft.entry((o.type_name.as_str(), bits)).or_default() += 1;
        }
    }
    println!("decoded objects by (type, handle-stream bits left): {hleft:?}");
    println!("warnings: {}", file.warnings.len());
    for w in file.warnings.iter().take(40) {
        println!(
            "  [{}] {} (obj {:?})",
            w.code,
            w.message,
            w.object.map(|h| format!("{h:X}"))
        );
    }
    if args.iter().any(|a| a == "--objects") {
        for o in file.objects.values() {
            println!(
                "{:X} {} {:?} err={:?}",
                o.handle, o.type_name, o.data, o.error
            );
        }
    }
    if args.iter().any(|a| a == "--eed") {
        for o in file.objects.values().filter(|o| !o.common.eed.is_empty()) {
            for b in &o.common.eed {
                println!(
                    "{:X} {} app {:X} {} bytes: {:?}",
                    o.handle,
                    o.type_name,
                    b.app,
                    b.data.len(),
                    b.items
                );
                if b.items.is_empty() {
                    println!("    raw {:02X?}", b.data);
                }
            }
        }
    }
    if let Some(pos) = args.iter().position(|a| a == "--handle") {
        if let Some(h) = args
            .get(pos + 1)
            .and_then(|s| u64::from_str_radix(s, 16).ok())
        {
            println!("{:#?}", file.objects.get(&h));
        }
    }
}
