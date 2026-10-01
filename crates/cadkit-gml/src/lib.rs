//! CityGML 2.0 için kaynak sınırları uygulayan okuma ve yazma katmanı.

mod curves;
mod export;
mod geometry;
mod mapping;
pub mod native;
pub mod tkgm;
pub mod validate;
mod xml;

use cadkit_core::{Document, ReadOptions, Result};
pub use export::{
    ExportIssue, ExportOptions, ExportReport, UnsupportedGeometry, from_document,
    from_document_with_report,
};
pub use geometry::{GeometryIssue, geometry_issues};
pub use mapping::{ImportOptions, to_document};
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

/// Writes a generic CAD projection with explicit handling of unsupported geometry.
/// The report identifies entities by model index and child-index path, never by client values.
/// Metadata-only entities are not rendered CityGML geometry and are not a delivery profile.
pub fn write_document_with_report(
    doc: &Document,
    options: &ExportOptions,
    unsupported: UnsupportedGeometry,
) -> Result<(Vec<u8>, ExportReport)> {
    let (native, report) = from_document_with_report(doc, options, unsupported)?;
    let bytes = write(
        &native,
        &ValidationOptions {
            limits: options.limits,
            ..Default::default()
        },
    )?;
    Ok((bytes, report))
}
