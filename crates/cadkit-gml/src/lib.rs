//! CityGML 2.0 için kaynak sınırları uygulayan okuma ve yazma katmanı.

mod geometry;
mod mapping;
pub mod native;
pub mod validate;
mod xml;

use cadkit_core::{Document, ReadOptions, Result};
pub use geometry::{GeometryIssue, geometry_issues};
pub use mapping::{ExportOptions, ImportOptions, from_document, to_document};
pub use native::CityGmlDocument;
pub use validate::{ValidationOptions, ValidationReport, validate};

/// Kökün ad alanını ve yerel adını denetler; dosya uzantısına güvenmez.
pub fn sniff(bytes: &[u8]) -> bool {
    use quick_xml::{NsReader, events::Event, name::ResolveResult};
    let mut reader = NsReader::from_reader(bytes.get(..bytes.len().min(65536)).unwrap_or(&[]));
    for _ in 0..64 {
        match reader.read_resolved_event() {
            Ok((ResolveResult::Bound(ns), Event::Start(e) | Event::Empty(e))) => {
                return ns.as_ref() == native::CORE.as_bytes()
                    && e.local_name().as_ref() == b"CityModel";
            }
            Ok((_, Event::Decl(_) | Event::Comment(_) | Event::Text(_))) => {}
            _ => return false,
        }
    }
    false
}

/// Kaynak XML yapısını korur; kimlikleri ve sayısal veriyi doğrular.
pub fn read_native(bytes: &[u8], options: &ReadOptions) -> Result<CityGmlDocument> {
    let doc = xml::parse(bytes, options)?;
    validate(
        &doc,
        &ValidationOptions {
            limits: options.limits,
            geometry: false,
            ..Default::default()
        },
    )?;
    Ok(doc)
}

/// Özgün model JSON'unu girdi sınırından sonra ayrıştırır ve yapısını denetler.
pub fn read_native_json(text: &str, options: &ReadOptions) -> Result<CityGmlDocument> {
    if text.len() as u64 > options.limits.max_input_bytes {
        return Err(cadkit_core::Error::LimitExceeded(
            "CityGML JSON input bytes".into(),
        ));
    }
    let doc =
        serde_json::from_str(text).map_err(|e| cadkit_core::Error::invalid(0, e.to_string()))?;
    validate(
        &doc,
        &ValidationOptions {
            limits: options.limits,
            geometry: false,
            ..Default::default()
        },
    )?;
    Ok(doc)
}

/// CityGML'i nötr modele okur; özgün ilişki ağı için `read_native` kullanılır.
pub fn read(bytes: &[u8], options: &ReadOptions) -> Result<Document> {
    to_document(
        &read_native(bytes, options)?,
        &ImportOptions::default(),
        options,
    )
}

/// CityGML modelini UTF-8 XML olarak yazar; bilinmeyen XML öğeleri korunur.
pub fn write(doc: &CityGmlDocument, options: &ValidationOptions) -> Result<Vec<u8>> {
    validate(doc, options)?;
    xml::serialize(
        doc,
        &ReadOptions {
            limits: options.limits,
            ..Default::default()
        },
    )
}

/// Nötr model geometrisini açık CRS seçimiyle CityGML'e yazar.
pub fn write_document(doc: &Document, options: &ExportOptions) -> Result<Vec<u8>> {
    write(
        &from_document(doc, options)?,
        &ValidationOptions {
            limits: options.limits,
            ..Default::default()
        },
    )
}
