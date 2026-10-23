//! Bounded CityGML 2.0 reader, validator, and writer.

mod curves;
mod export;
mod geometry;
mod mapping;
pub mod native;
pub mod tkgm;
pub mod validate;
mod xml;

use cadkit_core::{Document, Error, Format, ReadOptions, Result, Summary};
pub use export::{
    ExportIssue, ExportOptions, ExportReport, UnsupportedGeometry, from_document,
    from_document_with_report,
};
pub use geometry::{GeometryIssue, geometry_issues};
pub use mapping::{ImportOptions, to_document};
pub use native::CityGmlDocument;
pub use validate::{ValidationOptions, ValidationReport, validate};

/// Checks the root element's namespace and local name without relying on file extensions.
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

/// Inspects the CityGML root element and `boundedBy` envelope without parsing thematic objects.
pub fn inspect(bytes: &[u8], options: &ReadOptions) -> Result<Summary> {
    use quick_xml::{NsReader, events::Event, name::ResolveResult};
    if bytes.len() as u64 > options.limits.max_input_bytes {
        return Err(Error::LimitExceeded("CityGML input bytes".into()));
    }
    let head = bytes.get(..bytes.len().min(65536)).unwrap_or(&[]);
    let mut reader = NsReader::from_reader(head);
    let mut saw_root = false;
    let mut in_bounded_by = false;
    let mut srs_name: Option<String> = None;
    let mut srs_dim: Option<String> = None;

    let capture_srs = |e: &quick_xml::events::BytesStart<'_>,
                       srs_name: &mut Option<String>,
                       srs_dim: &mut Option<String>| {
        for attr in e.attributes().flatten() {
            let local = attr.key.local_name();
            let slot = if local.as_ref() == b"srsName" && srs_name.is_none() {
                Some(&mut *srs_name)
            } else if local.as_ref() == b"srsDimension" && srs_dim.is_none() {
                Some(&mut *srs_dim)
            } else {
                None
            };
            if let Some(target) = slot
                && let Ok(raw) = std::str::from_utf8(attr.value.as_ref())
                && let Ok(val) = quick_xml::escape::unescape(raw)
            {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    *target = Some(trimmed.to_owned());
                }
            }
        }
    };

    for _ in 0..256 {
        match reader.read_resolved_event() {
            Ok((res, Event::Start(e))) => {
                if !saw_root {
                    let ResolveResult::Bound(ns) = res else {
                        return Err(Error::UnknownFormat);
                    };
                    if ns.as_ref() == native::CORE.as_bytes()
                        && e.local_name().as_ref() == b"CityModel"
                    {
                        saw_root = true;
                        capture_srs(&e, &mut srs_name, &mut srs_dim);
                    } else {
                        return Err(Error::UnknownFormat);
                    }
                } else {
                    if let ResolveResult::Bound(ns) = res
                        && ns.as_ref() == native::GML.as_bytes()
                        && e.local_name().as_ref() == b"boundedBy"
                    {
                        in_bounded_by = true;
                    }
                    capture_srs(&e, &mut srs_name, &mut srs_dim);
                    if srs_name.is_some() {
                        break;
                    }
                }
            }
            Ok((res, Event::Empty(e))) => {
                if !saw_root {
                    let ResolveResult::Bound(ns) = res else {
                        return Err(Error::UnknownFormat);
                    };
                    if ns.as_ref() == native::CORE.as_bytes()
                        && e.local_name().as_ref() == b"CityModel"
                    {
                        saw_root = true;
                        capture_srs(&e, &mut srs_name, &mut srs_dim);
                        break;
                    }
                    return Err(Error::UnknownFormat);
                }
                capture_srs(&e, &mut srs_name, &mut srs_dim);
                if srs_name.is_some() {
                    break;
                }
            }
            Ok((ResolveResult::Bound(ns), Event::End(e))) => {
                if in_bounded_by
                    && ns.as_ref() == native::GML.as_bytes()
                    && e.local_name().as_ref() == b"boundedBy"
                    && srs_name.is_some()
                {
                    break;
                }
            }
            Ok((_, Event::Decl(_) | Event::Comment(_) | Event::Text(_))) => {}
            Ok((_, Event::Eof)) => break,
            Ok(_) if saw_root => {}
            _ => return Err(Error::UnknownFormat),
        }
    }

    if !saw_root {
        return Err(Error::UnknownFormat);
    }

    let is_3d = match srs_dim.as_deref() {
        Some("2") => Some(false),
        _ => Some(true),
    };
    let units = srs_name.as_deref().and_then(mapping::infer_srs_units);
    Ok(Summary::new(Format::CityGml, "2.0")
        .with_is_3d(is_3d)
        .with_units(units)
        .with_codepage(Some("UTF-8".into())))
}

/// Parses CityGML into its native XML tree while validating IDs, references, and numeric data.
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

/// Parses a serialized [`CityGmlDocument`] JSON string after checking the input byte limit and validates its structure.
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

/// Reads CityGML into the format-neutral [`Document`] model; use [`read_native`] to preserve the full object graph.
pub fn read(bytes: &[u8], options: &ReadOptions) -> Result<Document> {
    to_document(
        &read_native(bytes, options)?,
        &ImportOptions::default(),
        options,
    )
}

/// Writes a [`CityGmlDocument`] as UTF-8 XML while preserving unknown XML elements and extensions.
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

/// Writes neutral [`Document`] geometry to CityGML 2.0 with an explicit CRS and LoD.
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
