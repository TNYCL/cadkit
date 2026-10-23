//! Read DWG, DGN (V7/V8), DXF, and CityGML 2.0 files into one format-neutral model,
//! and write DXF, seed-based DGN V8, CityGML 2.0, SVG, and JSON.
//!
//! ```no_run
//! let doc = cadkit::open("plan.dgn")?;
//! for model in &doc.models {
//!     println!("{}: {} entities", model.name, model.entities.len());
//! }
//! # Ok::<(), cadkit::Error>(())
//! ```
//!
//! Native, lossless APIs of each format are available under `dgn`, `dwg`, `dxf`, and `gml`
//! when the corresponding Cargo features are enabled.

pub mod prelude;

use std::io::Read;
#[cfg(any(
    feature = "json",
    feature = "svg",
    feature = "dxf",
    feature = "dgn",
    feature = "gml"
))]
use std::io::Write;
use std::path::Path;

#[cfg(feature = "svg")]
pub use cadkit_core::svg::SvgOptions;
pub use cadkit_core::*;
#[cfg(feature = "dgn-read")]
pub use cadkit_dgn as dgn;
#[cfg(feature = "dwg")]
pub use cadkit_dwg as dwg;
#[cfg(feature = "dxf")]
pub use cadkit_dxf as dxf;
#[cfg(feature = "dxf")]
pub use cadkit_dxf::DxfVersion;
#[cfg(feature = "gml")]
pub use cadkit_gml as gml;

fn sniff_dwg(bytes: &[u8]) -> bool {
    #[cfg(feature = "dwg")]
    {
        cadkit_dwg::sniff(bytes)
    }
    #[cfg(not(feature = "dwg"))]
    {
        matches!(
            bytes.get(..6),
            Some(
                b"AC1012"
                    | b"AC1014"
                    | b"AC1015"
                    | b"AC1018"
                    | b"AC1021"
                    | b"AC1024"
                    | b"AC1027"
                    | b"AC1032"
            )
        )
    }
}

fn sniff_dgn(bytes: &[u8]) -> Option<Format> {
    #[cfg(feature = "dgn-read")]
    {
        if cadkit_dgn::sniff(bytes) {
            return Some(if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
                Format::DgnV8
            } else {
                Format::DgnV7
            });
        }
        None
    }
    #[cfg(not(feature = "dgn-read"))]
    {
        match bytes.get(..4) {
            Some([0x08, 0x05, 0x17, 0x00]) => Some(Format::DgnV7),
            Some([b0, 0x09, 0xFE, 0x02]) if *b0 == 0x08 || *b0 == 0xC8 => Some(Format::DgnV7),
            _ => None,
        }
    }
}

fn sniff_dxf(bytes: &[u8]) -> bool {
    #[cfg(feature = "dxf")]
    {
        cadkit_dxf::sniff(bytes)
    }
    #[cfg(not(feature = "dxf"))]
    {
        bytes.starts_with(b"AutoCAD Binary DXF\r\n\x1a\0")
    }
}

fn sniff_gml(bytes: &[u8]) -> bool {
    #[cfg(feature = "gml")]
    {
        cadkit_gml::sniff(bytes)
    }
    #[cfg(not(feature = "gml"))]
    {
        let head = std::str::from_utf8(bytes.get(..bytes.len().min(4096)).unwrap_or(&[]))
            .unwrap_or_default();
        head.contains("CityModel") && head.contains("opengis.net/citygml")
    }
}

/// Detects the format of `bytes` from its header.
pub fn detect(bytes: &[u8]) -> Option<Format> {
    if sniff_dwg(bytes) {
        Some(Format::Dwg)
    } else if let Some(dgn_fmt) = sniff_dgn(bytes) {
        Some(dgn_fmt)
    } else if sniff_dxf(bytes) {
        Some(Format::Dxf)
    } else if sniff_gml(bytes) {
        Some(Format::CityGml)
    } else {
        None
    }
}

fn check_input_len(len: u64, limit: u64) -> Result<()> {
    if len > limit {
        return Err(Error::LimitExceeded(format!(
            "input is {len} bytes, limit is {limit}"
        )));
    }
    Ok(())
}

fn read_bounded_reader(reader: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    check_input_len(bytes.len() as u64, limit)?;
    Ok(bytes)
}

fn read_bounded_file(path: impl AsRef<Path>, limit: u64) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    // Check metadata before reading so an oversized file is rejected before allocating.
    check_input_len(file.metadata()?.len(), limit)?;
    read_bounded_reader(file, limit)
}

/// Inspects a drawing header with default options without decoding full entity streams.
pub fn inspect(bytes: &[u8]) -> Result<Summary> {
    inspect_with(bytes, &ReadOptions::default())
}

/// Inspects a drawing header without decoding full entity streams.
pub fn inspect_with(bytes: &[u8], options: &ReadOptions) -> Result<Summary> {
    check_input_len(bytes.len() as u64, options.limits.max_input_bytes)?;
    match detect(bytes) {
        Some(Format::Dwg) => {
            #[cfg(feature = "dwg")]
            {
                cadkit_dwg::inspect(bytes, options)
            }
            #[cfg(not(feature = "dwg"))]
            {
                Err(Error::Unsupported(
                    "DWG support requires the `dwg` Cargo feature".into(),
                ))
            }
        }
        Some(Format::DgnV7 | Format::DgnV8) => {
            #[cfg(feature = "dgn-read")]
            {
                cadkit_dgn::inspect(bytes, options)
            }
            #[cfg(not(feature = "dgn-read"))]
            {
                Err(Error::Unsupported(
                    "DGN support requires the `dgn-read` or `dgn` Cargo feature".into(),
                ))
            }
        }
        Some(Format::Dxf) => {
            #[cfg(feature = "dxf")]
            {
                cadkit_dxf::inspect(bytes, options)
            }
            #[cfg(not(feature = "dxf"))]
            {
                Err(Error::Unsupported(
                    "DXF support requires the `dxf` Cargo feature".into(),
                ))
            }
        }
        Some(Format::CityGml) => {
            #[cfg(feature = "gml")]
            {
                cadkit_gml::inspect(bytes, options)
            }
            #[cfg(not(feature = "gml"))]
            {
                Err(Error::Unsupported(
                    "CityGML support requires the `gml` Cargo feature".into(),
                ))
            }
        }
        _ => Err(Error::UnknownFormat),
    }
}

/// Inspects a drawing file header from disk with default options.
pub fn inspect_file(path: impl AsRef<Path>) -> Result<Summary> {
    inspect_file_with(path, &ReadOptions::default())
}

/// Inspects a drawing file header from disk.
pub fn inspect_file_with(path: impl AsRef<Path>, options: &ReadOptions) -> Result<Summary> {
    let bytes = read_bounded_file(path, options.limits.max_input_bytes)?;
    inspect_with(&bytes, options)
}

/// Reads a drawing of any supported format with default options.
pub fn read(bytes: &[u8]) -> Result<Document> {
    read_with(bytes, &ReadOptions::default())
}

/// Reads a drawing of any supported format.
pub fn read_with(bytes: &[u8], options: &ReadOptions) -> Result<Document> {
    check_input_len(bytes.len() as u64, options.limits.max_input_bytes)?;
    match detect(bytes) {
        Some(Format::Dwg) => {
            #[cfg(feature = "dwg")]
            {
                cadkit_dwg::read(bytes, options)
            }
            #[cfg(not(feature = "dwg"))]
            {
                Err(Error::Unsupported(
                    "DWG support requires the `dwg` Cargo feature".into(),
                ))
            }
        }
        Some(Format::DgnV7 | Format::DgnV8) => {
            #[cfg(feature = "dgn-read")]
            {
                cadkit_dgn::read(bytes, options)
            }
            #[cfg(not(feature = "dgn-read"))]
            {
                Err(Error::Unsupported(
                    "DGN support requires the `dgn-read` or `dgn` Cargo feature".into(),
                ))
            }
        }
        Some(Format::Dxf) => {
            #[cfg(feature = "dxf")]
            {
                cadkit_dxf::read(bytes, options)
            }
            #[cfg(not(feature = "dxf"))]
            {
                Err(Error::Unsupported(
                    "DXF support requires the `dxf` Cargo feature".into(),
                ))
            }
        }
        Some(Format::CityGml) => {
            #[cfg(feature = "gml")]
            {
                cadkit_gml::read(bytes, options)
            }
            #[cfg(not(feature = "gml"))]
            {
                Err(Error::Unsupported(
                    "CityGML support requires the `gml` Cargo feature".into(),
                ))
            }
        }
        _ => Err(Error::UnknownFormat),
    }
}

/// Reads a drawing from any [`std::io::Read`] stream, bounded by `options.limits.max_input_bytes`.
pub fn read_from(reader: impl Read, options: &ReadOptions) -> Result<Document> {
    let bytes = read_bounded_reader(reader, options.limits.max_input_bytes)?;
    read_with(&bytes, options)
}

/// Reads a drawing file from disk with default options.
pub fn open(path: impl AsRef<Path>) -> Result<Document> {
    open_with(path, &ReadOptions::default())
}

/// Reads a drawing file from disk.
pub fn open_with(path: impl AsRef<Path>, options: &ReadOptions) -> Result<Document> {
    let bytes = read_bounded_file(path, options.limits.max_input_bytes)?;
    read_with(&bytes, options)
}

/// Serializes a document to JSON.
#[cfg(feature = "json")]
pub fn to_json(doc: &Document, pretty: bool) -> Result<String> {
    let out = if pretty {
        serde_json::to_string_pretty(doc)
    } else {
        serde_json::to_string(doc)
    };
    out.map_err(|e| Error::Unsupported(format!("JSON serialization failed: {e}")))
}

/// Streams a document as JSON directly into `writer` without buffering a full `String`.
#[cfg(feature = "json")]
pub fn write_json_to(doc: &Document, pretty: bool, writer: impl Write) -> Result<()> {
    let res = if pretty {
        serde_json::to_writer_pretty(writer, doc)
    } else {
        serde_json::to_writer(writer, doc)
    };
    res.map_err(|e| Error::Unsupported(format!("JSON serialization failed: {e}")))
}

/// Renders a document model as SVG.
#[cfg(feature = "svg")]
pub fn to_svg(doc: &Document, options: &SvgOptions) -> String {
    cadkit_core::svg::render(doc, options)
}

/// Renders a document model as SVG, including entities on hidden or frozen layers.
#[cfg(feature = "svg")]
pub fn to_svg_all_layers(doc: &Document, options: &SvgOptions) -> String {
    cadkit_core::svg::render_all_layers(doc, options)
}

/// Writes an SVG rendering of `doc` into `writer`.
#[cfg(feature = "svg")]
pub fn write_svg_to(doc: &Document, options: &SvgOptions, mut writer: impl Write) -> Result<()> {
    let svg = to_svg(doc, options);
    writer.write_all(svg.as_bytes())?;
    Ok(())
}

/// Writes a document as ASCII/UTF-8 DXF text.
#[cfg(feature = "dxf")]
pub fn to_dxf(doc: &Document, version: DxfVersion) -> Result<String> {
    cadkit_dxf::write(doc, version)
}

/// Writes a document as DXF bytes using `options` (including optional legacy 8-bit code pages).
#[cfg(feature = "dxf")]
pub fn to_dxf_bytes(doc: &Document, options: &dxf::WriteOptions) -> Result<Vec<u8>> {
    cadkit_dxf::write_bytes(doc, options)
}

/// Writes a document as ASCII/UTF-8 DXF into `writer`.
#[cfg(feature = "dxf")]
pub fn write_dxf_to(doc: &Document, version: DxfVersion, mut writer: impl Write) -> Result<()> {
    let text = to_dxf(doc, version)?;
    writer.write_all(text.as_bytes())?;
    Ok(())
}

/// Writes a document as DXF bytes into `writer` using `options`.
#[cfg(feature = "dxf")]
pub fn write_dxf_bytes_to(
    doc: &Document,
    options: &dxf::WriteOptions,
    mut writer: impl Write,
) -> Result<()> {
    let bytes = to_dxf_bytes(doc, options)?;
    writer.write_all(&bytes)?;
    Ok(())
}

/// Writes a single-model neutral document to DGN V8 using raw seed bytes.
#[cfg(feature = "dgn")]
pub fn to_dgn(doc: &Document, seed: &[u8], options: &dgn::WriteOptions) -> Result<Vec<u8>> {
    dgn::write_v8(doc, seed, options)
}

/// Writes a single-model neutral document to DGN V8 using a pre-parsed [`dgn::V8Seed`].
#[cfg(feature = "dgn")]
pub fn to_dgn_with_seed(
    doc: &Document,
    seed: &dgn::V8Seed,
    options: &dgn::WriteOptions,
) -> Result<Vec<u8>> {
    dgn::write_v8_with_seed(doc, seed, options)
}

/// Writes a single-model neutral document to DGN V8 into `writer` using raw seed bytes.
#[cfg(feature = "dgn")]
pub fn write_dgn_to(
    doc: &Document,
    seed: &[u8],
    options: &dgn::WriteOptions,
    mut writer: impl Write,
) -> Result<()> {
    let bytes = to_dgn(doc, seed, options)?;
    writer.write_all(&bytes)?;
    Ok(())
}

/// Writes a single-model neutral document to DGN V8 into `writer` using a pre-parsed [`dgn::V8Seed`].
#[cfg(feature = "dgn")]
pub fn write_dgn_with_seed_to(
    doc: &Document,
    seed: &dgn::V8Seed,
    options: &dgn::WriteOptions,
    mut writer: impl Write,
) -> Result<()> {
    let bytes = to_dgn_with_seed(doc, seed, options)?;
    writer.write_all(&bytes)?;
    Ok(())
}

/// Writes neutral geometry to generic CityGML 2.0 objects with an explicit CRS and LoD, preserving source coordinates and units without reprojection.
#[cfg(feature = "gml")]
pub fn to_citygml(doc: &Document, options: &gml::ExportOptions) -> Result<Vec<u8>> {
    gml::write_document(doc, options)
}

/// Writes neutral geometry to CityGML 2.0 XML into `writer`.
#[cfg(feature = "gml")]
pub fn write_citygml_to(
    doc: &Document,
    options: &gml::ExportOptions,
    mut writer: impl Write,
) -> Result<()> {
    let bytes = to_citygml(doc, options)?;
    writer.write_all(&bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_from_enforces_max_input_bytes_without_reading_unbounded_stream() {
        let opts = ReadOptions::default().with_limits(Limits::default().with_max_input_bytes(16));
        let err = read_from(std::io::repeat(b'A'), &opts).expect_err("must reject stream > 16B");
        assert!(matches!(err, Error::LimitExceeded(_)));
    }

    #[cfg(feature = "dxf")]
    #[test]
    fn inspect_and_streaming_dxf_helpers_roundtrip_metadata_and_legacy_codepages() {
        use std::io::Cursor;

        use crate::prelude::*;

        let mut doc = Document::new();
        doc.units = Units::from_insunits(4); // Millimeters
        doc.model_space_mut().entities.push(
            Entity::text(Point3::new(5.0, 10.0, 3.5), 2.5, "Şehir şube — Ø20").on_layer("TEXT"),
        );

        let opts = DxfWriteOptions::new(DxfVersion::R2000).with_codepage(Some("DOS857".into()));
        let mut buf = Vec::new();
        write_dxf_bytes_to(&doc, &opts, &mut buf).expect("write legacy DXF");

        // `Ş` (0x9E in CP857) is emitted raw while `—` (not in CP857) stays `\U+2014`.
        assert!(buf.contains(&0x9E));
        assert!(std::str::from_utf8(&buf).is_err());

        let summary = inspect(&buf).expect("inspect DXF header");
        assert_eq!(summary.format, Format::Dxf);
        assert_eq!(summary.version, "AC1015");
        assert_eq!(summary.codepage.as_deref(), Some("DOS857"));
        assert_eq!(summary.units.map(|u| u.unit), Some(LengthUnit::Millimeter));
        assert_eq!(summary.is_3d, Some(true));

        let decoded = read_from(Cursor::new(&buf), &ReadOptions::default()).expect("read_from");
        let EntityKind::Text { value, .. } = &decoded.model_space().unwrap().entities[0].kind
        else {
            panic!("expected Text");
        };
        assert_eq!(value, "Şehir şube — Ø20");
    }

    #[cfg(feature = "gml")]
    #[test]
    fn inspect_and_streaming_citygml_helpers_detect_srs_units() {
        let mut doc = Document::new();
        doc.model_space_mut().entities.push(Entity::line(
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 2.0),
        ));
        let mut xml = Vec::new();
        write_citygml_to(&doc, &gml::ExportOptions::new("EPSG:5254", 2), &mut xml)
            .expect("write_citygml_to");

        let summary = inspect(&xml).expect("inspect CityGML");
        assert_eq!(summary.format, Format::CityGml);
        assert_eq!(summary.version, "2.0");
        assert_eq!(summary.is_3d, Some(true));
        assert_eq!(summary.units.map(|u| u.unit), Some(LengthUnit::Meter));
        assert_eq!(summary.codepage.as_deref(), Some("UTF-8"));
    }

    #[cfg(all(feature = "svg", feature = "json"))]
    #[test]
    fn streaming_svg_and_json_helpers_produce_identical_bytes() {
        let mut doc = Document::new();
        doc.model_space_mut()
            .entities
            .push(Entity::circle(Point3::ZERO, 5.0));

        let mut svg_buf = Vec::new();
        write_svg_to(&doc, &SvgOptions::default(), &mut svg_buf).expect("write_svg_to");
        assert_eq!(svg_buf, to_svg(&doc, &SvgOptions::default()).into_bytes());

        let mut json_buf = Vec::new();
        write_json_to(&doc, false, &mut json_buf).expect("write_json_to");
        assert_eq!(
            json_buf,
            to_json(&doc, false).expect("to_json").into_bytes()
        );
    }

    #[cfg(feature = "dgn-read")]
    #[test]
    fn inspect_dgn_v7_header_without_graphic_records() {
        let mut v7 = vec![0xC8, 0x09, 0xFE, 0x02];
        v7.resize(4 + 2 * 0x02FE, 0);
        v7.extend_from_slice(&[0xFF, 0xFF]);
        let summary = inspect(&v7).expect("inspect DGN V7");
        assert_eq!(summary.format, Format::DgnV7);
        assert_eq!(summary.version, "V7");
        assert_eq!(summary.is_3d, Some(true));
    }

    #[cfg(all(feature = "dwg", feature = "dgn"))]
    #[test]
    fn inspect_matches_full_read_on_public_corpus_when_present() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/public");
        for rel in ["acadsharp/2018/sample_AC1032.dwg", "gdal/dgnv8_test.dgn"] {
            let p = root.join(rel);
            let Ok(bytes) = std::fs::read(&p) else {
                continue;
            };
            let summary = inspect(&bytes).expect("inspect corpus sample");
            let doc = read(&bytes).expect("read corpus sample");
            assert_eq!(summary.format, doc.source.format);
            assert_eq!(summary.version, doc.source.version);
            assert_eq!(summary.units, Some(doc.units));
        }
    }
}
