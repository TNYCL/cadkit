//! ASCII and binary versions of the same drawing must produce identical documents.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::print_stdout
)]
mod common;

use common::*;

#[test]
fn ascii_and_binary_documents_are_identical() {
    let mut compared = 0;
    for (ver, _) in VERSIONS {
        let (Some(a), Some(b)) = (
            corpus_file(&format!("sample_{ver}_ascii.dxf")),
            corpus_file(&format!("sample_{ver}_binary.dxf")),
        ) else {
            continue;
        };
        assert!(
            cadkit_dxf::sniff(&a) && cadkit_dxf::sniff(&b),
            "{ver}: sniff"
        );
        let da = read(&a);
        let db = read(&b);
        assert_eq!(da.source.version, ver);
        let total: usize = da.models.iter().map(|m| m.entities.len()).sum();
        assert!(total > 50, "{ver}: only {total} entities");
        assert_eq!(da.models.len(), db.models.len(), "{ver}: model count");
        for (ma, mb) in da.models.iter().zip(&db.models) {
            assert_eq!(
                histogram(&ma.entities),
                histogram(&mb.entities),
                "{ver}: histogram of {}",
                ma.name
            );
        }
        let ja = comparable_json(&da);
        let jb = comparable_json(&db);
        let max = json_max_diff(&ja, &jb).unwrap_or_else(|| {
            panic!(
                "{ver}: ASCII and binary differ structurally: {:?}",
                json_diff(&ja, &jb, 1e-3, "$")
            )
        });
        // The R12 ASCII sample was exported with fewer digits than its binary twin (many
        // coordinates have 1-4 decimals); all later versions agree to double precision.
        let limit = if ver == "AC1009" { 1e-5 } else { 1e-9 };
        assert!(max <= limit, "{ver}: max deviation {max:e} > {limit:e}");
        println!("{ver}: max numeric deviation {max:e}");
        compared += 1;
        println!(
            "{ver}: {} models, {} layers, {} blocks, {total} model entities identical",
            da.models.len(),
            da.layers.len(),
            da.blocks.len()
        );
    }
    if corpus_file("sample_AC1009_ascii.dxf").is_some() {
        assert_eq!(compared, 7);
    }
}
