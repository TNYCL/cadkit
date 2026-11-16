//! Runs the acceptance kit generator (`examples/support/acceptance_kit.rs`) against the
//! public GDAL seed `corpus/public/gdal/test_dgnv8.dgn` into a temporary directory; skips
//! silently when the public corpus is absent.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

#[path = "../examples/support/acceptance_kit.rs"]
mod acceptance_kit;

use std::path::Path;

const FILES: [&str; 18] = [
    "01-seed-repacked.dgn",
    "02-seed-stripped.dgn",
    "03-stripped-with-geometry.dgn",
    "04-geometry.dgn",
    "05-text-utf16.dgn",
    "06-text-cp1254.dgn",
    "07-tags.dgn",
    "08-hidden.dgn",
    "09-plan-in-3d.dgn",
    "10-large-coordinates.dgn",
    "11-raster.dgn",
    "kit-raster.tif",
    "12-chain-step1.dgn",
    "13-chain-step2.dgn",
    "14-bylayer.dgn",
    "synthetic.gml",
    "MANIFEST.json",
    "README.md",
];

#[test]
fn kit_from_the_public_seed_verifies_and_is_reproducible() {
    let seed_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/public/gdal/test_dgnv8.dgn");
    let Ok(seed) = std::fs::read(&seed_path) else {
        return;
    };
    let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join("acceptance-kit");
    let _ = std::fs::remove_dir_all(&base);
    let (a, b) = (base.join("a"), base.join("b"));
    let first = acceptance_kit::generate(&seed, "test_dgnv8.dgn", &a).unwrap();
    let second = acceptance_kit::generate(&seed, "test_dgnv8.dgn", &b).unwrap();
    let names: Vec<&str> = first.files.iter().map(|f| f.name.as_str()).collect();
    // test_dgnv8 is a 3D seed, so the 2D-into-3D item is written, not skipped.
    assert_eq!(names, FILES);
    for (f, g) in first.files.iter().zip(&second.files) {
        assert!(f.bytes > 0, "{}", f.name);
        let (x, y) = (
            std::fs::read(a.join(&f.name)),
            std::fs::read(b.join(&g.name)),
        );
        assert_eq!(x.unwrap(), y.unwrap(), "{} is not reproducible", f.name);
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(a.join("MANIFEST.json")).unwrap()).unwrap();
    let items = manifest["items"].as_array().unwrap();
    let dgn: Vec<&serde_json::Value> = items
        .iter()
        .filter(|i| i["file"].as_str().is_some_and(|f| f.ends_with(".dgn")))
        .collect();
    assert_eq!(dgn.len(), 14);
    for item in dgn {
        assert!(item.get("skipped").is_none());
        assert_eq!(item["structure"]["warnings"], serde_json::json!([]));
        assert_eq!(item["verified"]["cfb_open_strict"], true);
        assert!(!item["checks"].as_array().unwrap().is_empty());
    }
    let _ = std::fs::remove_dir_all(&base);
}
