//! The committed JSON Schema of the document model matches the Rust types.
//!
//! Run with `cargo test -p cadkit-core --features schemars --test schema`. After an intended
//! model change, regenerate the file with `CADKIT_UPDATE_SCHEMA=1` and review the diff.
#![cfg(feature = "schemars")]

use cadkit_core::{Document, Entity, Point3, SchemaVersion, export::document_from_json};

const PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/schema/document-v1.schema.json"
);

#[test]
fn committed_schema_matches_the_model() {
    let schema = schemars::schema_for!(Document);
    let text = serde_json::to_string_pretty(&schema).unwrap() + "\n";
    if std::env::var_os("CADKIT_UPDATE_SCHEMA").is_some() {
        std::fs::write(PATH, &text).unwrap();
    }
    let committed = std::fs::read_to_string(PATH).unwrap();
    assert!(
        committed == text,
        "docs/schema/document-v1.schema.json is stale; rerun with CADKIT_UPDATE_SCHEMA=1"
    );
}

#[test]
fn schema_names_the_version_field() {
    let schema = schemars::schema_for!(Document);
    assert!(schema.as_value()["properties"]["schema_version"].is_object());
    let mut doc = Document::new();
    doc.model_space_mut().entities.push(Entity::line(
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
    ));
    let json = serde_json::to_value(&doc).unwrap();
    assert_eq!(json["schema_version"], 1);
    let read = document_from_json(&json.to_string(), &Default::default()).unwrap();
    assert_eq!(read.schema_version, SchemaVersion::CURRENT);
}
