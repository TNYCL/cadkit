//! Prints structural raster statistics only; never paths, strings or coordinates.

use cadkit_core::ReadOptions;
use cadkit_dgn::native::{decode_v8, linkage::LinkageData, v8};
use std::collections::{BTreeMap, HashMap};

fn scan(
    dir: &std::path::Path,
    counts: &mut BTreeMap<String, u64>,
    depth: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    if depth > 32 {
        return Err("directory depth limit".into());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            scan(&path, counts, depth + 1)?;
            continue;
        }
        if !path
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("dgn"))
        {
            continue;
        }
        let options = ReadOptions {
            fallback_codepage: Some("windows-1254".into()),
            ..Default::default()
        };
        if path.metadata()?.len() > options.limits.max_input_bytes {
            continue;
        }
        let raw = std::fs::read(&path)?;
        let file = match v8::read(&raw, &options) {
            Ok(f) => f,
            Err(_) => continue,
        };
        if std::env::var_os("CADKIT_RASTER_VERIFY").is_some() {
            verify(&raw, &file, &options, counts)?;
            continue;
        }
        for model in &file.models {
            let records: Vec<_> = model
                .graphic_pages
                .iter()
                .chain(&model.control_pages)
                .flat_map(|p| &p.elements)
                .collect();
            let kinds: HashMap<_, _> = records
                .iter()
                .filter_map(|r| r.id().map(|id| (id, r.type_code())))
                .collect();
            for r in &records {
                if !(90..=94).contains(&r.type_code()) {
                    continue;
                }
                let e = decode_v8::decode(r, encoding_rs::WINDOWS_1254, &options.limits);
                let mut structure = format!(
                    "type={} primary={} flags={:x} links=",
                    r.type_code(),
                    r.primary().len(),
                    r.type_word >> 16
                );
                for l in &e.linkages {
                    structure.push_str(&format!("{:x}/{}", l.id, l.bytes.len()));
                    match &l.data {
                        LinkageData::Dependency {
                            app_id,
                            app_value,
                            root_type,
                            roots,
                            ..
                        } => {
                            structure.push_str(&format!("(app={app_id:x},value={app_value},root={root_type},target_types={:?})", roots.iter().map(|id| kinds.get(id)).collect::<Vec<_>>()));
                        }
                        LinkageData::String { string_id, .. } => {
                            structure.push_str(&format!("(string={string_id:x})"));
                        }
                        _ => {}
                    }
                    structure.push(',');
                }
                *counts.entry(structure).or_default() += 1;
                for (off, b) in r.primary().chunks_exact(8).enumerate().skip(4) {
                    if let Ok(bytes) = <[u8; 8]>::try_from(b) {
                        if let Some(kind) = kinds.get(&u64::from_le_bytes(bytes)) {
                            *counts
                                .entry(format!(
                                    "type={} possible_id_at={:x} target_type={kind}",
                                    r.type_code(),
                                    off * 8
                                ))
                                .or_default() += 1;
                        }
                    }
                }
            }
            for a in model
                .graphic_aux
                .iter()
                .chain(&model.control_aux)
                .flat_map(|p| &p.records)
            {
                if let Some(kind) = kinds.get(&a.element_id).filter(|k| (90..=94).contains(*k)) {
                    *counts
                        .entry(format!(
                            "type={kind} aux_kind={:x} length={}",
                            a.kind,
                            a.payload.len()
                        ))
                        .or_default() += 1;
                }
            }
        }
    }
    Ok(())
}

fn verify(
    raw: &[u8],
    before: &v8::V8File,
    read: &ReadOptions,
    counts: &mut BTreeMap<String, u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    let doc = cadkit_dgn::read(raw, read)?;
    let options = cadkit_dgn::WriteOptions {
        codepage: read.fallback_codepage.clone(),
        clear_seed_model: true,
        ..Default::default()
    };
    let output = match cadkit_dgn::write_v8(&doc, raw, &options) {
        Ok(bytes) => bytes,
        Err(_) => {
            *counts.entry("rejected".into()).or_default() += 1;
            return Ok(());
        }
    };
    let after = v8::read(&output, read)?;
    let _container = cfb::CompoundFile::open_strict(std::io::Cursor::new(&output))?;
    *counts.entry("strict_cfb_valid".into()).or_default() += 1;
    *counts.entry("written".into()).or_default() += 1;
    let frame_ids: std::collections::HashSet<_> = before
        .models
        .iter()
        .flat_map(|m| &m.graphic_pages)
        .flat_map(|p| &p.elements)
        .filter(|r| r.type_code() == 94)
        .filter_map(|r| r.id())
        .collect();
    if frame_ids.is_empty() {
        return Ok(());
    }
    let frames = |f: &v8::V8File| {
        f.models
            .iter()
            .flat_map(|m| &m.graphic_pages)
            .flat_map(|p| &p.elements)
            .filter(|r| r.type_code() == 94)
            .map(|r| r.bytes.clone())
            .collect::<Vec<_>>()
    };
    let controls = |f: &v8::V8File| {
        f.models
            .iter()
            .flat_map(|m| &m.control_pages)
            .flat_map(|p| &p.elements)
            .filter(|r| (90..=93).contains(&r.type_code()))
            .map(|r| r.bytes.clone())
            .collect::<Vec<_>>()
    };
    let aux = |f: &v8::V8File| {
        f.models
            .iter()
            .flat_map(|m| &m.graphic_aux)
            .flat_map(|p| &p.records)
            .filter(|a| frame_ids.contains(&a.element_id))
            .map(|a| (a.kind, a.reserved, a.element_id, a.flags, a.payload.clone()))
            .collect::<Vec<_>>()
    };
    *counts.entry("raster_files".into()).or_default() += 1;
    *counts.entry("raster_frames".into()).or_default() += frame_ids.len() as u64;
    *counts.entry("frame_bytes_equal".into()).or_default() +=
        u64::from(frames(before) == frames(&after));
    *counts.entry("control_bytes_equal".into()).or_default() +=
        u64::from(controls(before) == controls(&after));
    *counts.entry("auxiliary_payloads_equal".into()).or_default() +=
        u64::from(aux(before) == aux(&after));
    Ok(())
}

#[allow(clippy::print_stdout)]
fn main() {
    let Some(path) = std::env::var_os("CADKIT_RASTER_CORPUS") else {
        return;
    };
    let mut counts = BTreeMap::new();
    if scan(std::path::Path::new(&path), &mut counts, 0).is_err() {
        eprintln!("structural scan failed");
        std::process::exit(1);
    }
    for (structure, count) in counts {
        println!("{count} {structure}");
    }
}
