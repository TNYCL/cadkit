//! Python bindings for cadkit (`cadkit._cadkit`).
//!
//! The document model stays in Rust; Python receives plain dicts/lists produced by
//! serializing the model to JSON (with the GIL released) and decoding it with the
//! standard library's C `json` module. That keeps the binding independent of the
//! model's shape: new entity kinds and fields appear automatically.

use cadkit::LimitKind;
use std::sync::Arc;

use cadkit::{DxfVersion, Error, LengthUnit, ReadOptions, SvgOptions};
use pyo3::exceptions::{PyIndexError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes};
use pyo3::{create_exception, intern};

create_exception!(
    _cadkit,
    CadkitError,
    pyo3::exceptions::PyException,
    "Base class of all cadkit errors."
);
create_exception!(
    _cadkit,
    UnknownFormatError,
    CadkitError,
    "The input is not a recognized drawing format."
);
create_exception!(
    _cadkit,
    UnsupportedError,
    CadkitError,
    "Valid input that uses a feature cadkit does not implement."
);
create_exception!(
    _cadkit,
    LimitExceededError,
    CadkitError,
    "The input would exceed a configured resource limit."
);
create_exception!(
    _cadkit,
    InvalidDataError,
    CadkitError,
    "The input is truncated or violates the format."
);

fn map_err(err: Error) -> PyErr {
    match err {
        Error::UnknownFormat => UnknownFormatError::new_err(err.to_string()),
        Error::Unsupported(_) => UnsupportedError::new_err(err.to_string()),
        Error::LimitExceeded { .. } => LimitExceededError::new_err(err.to_string()),
        Error::Truncated { .. } | Error::Invalid { .. } => {
            InvalidDataError::new_err(err.to_string())
        }
        // `PyErr::from(io::Error)` picks FileNotFoundError, PermissionError, ... by kind.
        Error::Io(e) => PyErr::from(e),
        other => CadkitError::new_err(other.to_string()),
    }
}

/// Decodes a JSON string into Python objects with `json.loads`.
fn from_json<'py>(py: Python<'py>, json: &str) -> PyResult<Bound<'py, PyAny>> {
    py.import(intern!(py, "json"))?
        .call_method1(intern!(py, "loads"), (json,))
}

fn to_py<'py, T: serde::Serialize + Sync>(
    py: Python<'py>,
    value: &T,
) -> PyResult<Bound<'py, PyAny>> {
    let json = py
        .detach(|| serde_json::to_string(value))
        .map_err(|e| CadkitError::new_err(format!("serialization failed: {e}")))?;
    from_json(py, &json)
}

fn model_range_error(index: usize, count: usize) -> PyErr {
    PyIndexError::new_err(format!(
        "model index {index} out of range (document has {count} models)"
    ))
}

fn parse_dxf_version(version: &str) -> PyResult<DxfVersion> {
    match version.to_ascii_lowercase().as_str() {
        "r12" => Ok(DxfVersion::R12),
        "r2000" => Ok(DxfVersion::R2000),
        "r2004" => Ok(DxfVersion::R2004),
        "r2007" => Ok(DxfVersion::R2007),
        "r2010" => Ok(DxfVersion::R2010),
        "r2013" => Ok(DxfVersion::R2013),
        "r2018" => Ok(DxfVersion::R2018),
        other => Err(PyValueError::new_err(format!(
            "unknown DXF version {other:?}; expected r12, r2000, r2004, r2007, r2010, r2013 or r2018"
        ))),
    }
}

fn build_read_options(
    keep_raw: bool,
    codepage: Option<String>,
    max_input_bytes: Option<u64>,
    max_decompressed_bytes: Option<u64>,
    max_objects: Option<u64>,
    max_depth: Option<u32>,
) -> ReadOptions {
    let mut options = {
        let mut options = ReadOptions::default().with_keep_raw(keep_raw);
        options.fallback_codepage = codepage;
        options
    };
    let limits = &mut options.limits;
    limits.max_input_bytes = max_input_bytes.unwrap_or(limits.max_input_bytes);
    limits.max_decompressed_bytes = max_decompressed_bytes.unwrap_or(limits.max_decompressed_bytes);
    limits.max_objects = max_objects.unwrap_or(limits.max_objects);
    limits.max_depth = max_depth.unwrap_or(limits.max_depth);
    options
}

fn extract_buffer_bytes(py: Python<'_>, obj: &Bound<'_, PyAny>) -> Option<PyResult<Vec<u8>>> {
    if let Ok(b) = obj.cast::<PyBytes>() {
        return Some(Ok(b.as_bytes().to_vec()));
    }
    let builtins = match py.import(intern!(py, "builtins")) {
        Ok(b) => b,
        Err(e) => return Some(Err(e)),
    };
    let is_memview = builtins
        .getattr(intern!(py, "memoryview"))
        .and_then(|mv| obj.is_instance(&mv))
        .unwrap_or(false);
    if obj.is_instance_of::<PyByteArray>() || is_memview {
        let res = builtins
            .getattr(intern!(py, "bytes"))
            .and_then(|bytes_fn| bytes_fn.call1((obj,)))
            .and_then(|b| b.extract::<Vec<u8>>());
        return Some(res);
    }
    None
}

/// Pre-parsed single-model DGN V8 seed file for repeated `Document.to_dgn` calls and document preparation.
#[pyclass(frozen, module = "cadkit")]
struct V8Seed {
    inner: Arc<cadkit::dgn::V8Seed>,
}

#[pymethods]
impl V8Seed {
    #[new]
    #[pyo3(signature = (source, *, codepage=None, max_input_bytes=None, max_decompressed_bytes=None, max_objects=None, max_depth=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        source: &Bound<'_, PyAny>,
        codepage: Option<String>,
        max_input_bytes: Option<u64>,
        max_decompressed_bytes: Option<u64>,
        max_objects: Option<u64>,
        max_depth: Option<u32>,
    ) -> PyResult<Self> {
        if let Ok(existing) = source.cast::<V8Seed>() {
            return Ok(Self {
                inner: Arc::clone(&existing.get().inner),
            });
        }
        let input = source_input(py, source)?;
        let options = build_read_options(
            true,
            codepage,
            max_input_bytes,
            max_decompressed_bytes,
            max_objects,
            max_depth,
        );
        let seed = py
            .detach(|| match input {
                Ok(bytes) => cadkit::dgn::V8Seed::parse(&bytes, &options),
                Err(path) => {
                    let file = std::fs::File::open(path)?;
                    let len = file.metadata()?.len();
                    if len > options.limits.max_input_bytes {
                        return Err(Error::limit(
                            LimitKind::InputBytes,
                            format!(
                                "input is {len} bytes, limit is {}",
                                options.limits.max_input_bytes
                            ),
                        ));
                    }
                    let mut bytes = Vec::new();
                    use std::io::Read as _;
                    file.take(options.limits.max_input_bytes.saturating_add(1))
                        .read_to_end(&mut bytes)?;
                    if bytes.len() as u64 > options.limits.max_input_bytes {
                        return Err(Error::limit(
                            LimitKind::InputBytes,
                            format!(
                                "input is {} bytes, limit is {}",
                                bytes.len(),
                                options.limits.max_input_bytes
                            ),
                        ));
                    }
                    cadkit::dgn::V8Seed::parse(&bytes, &options)
                }
            })
            .map_err(map_err)?;
        Ok(Self {
            inner: Arc::new(seed),
        })
    }

    /// Whether the seed's single design model is 3D (`False` = 2D).
    #[getter]
    fn is_3d(&self) -> bool {
        self.inner.is_3d()
    }

    /// Master units of the seed's design model (`{"unit": ..., "meters_per_unit": ...}`).
    #[getter]
    fn units<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.inner.units())
    }

    /// Whether the seed model already contains graphic or control records.
    #[getter]
    fn is_nonempty(&self) -> bool {
        self.inner.is_nonempty()
    }

    /// Whether the seed holds a complete raster attachment template.
    #[getter]
    fn has_raster_template(&self) -> bool {
        self.inner.has_raster_template()
    }

    /// Raw bytes of the underlying DGN V8 seed file.
    fn to_bytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, self.inner.as_bytes())
    }

    /// Returns a prepared copy of `doc` normalized to this seed's units, 2D/3D dimension, and DGN V8 symbology.
    #[pyo3(signature = (doc, options_json=None))]
    fn prepare_document(
        &self,
        py: Python<'_>,
        doc: &Document,
        options_json: Option<&str>,
    ) -> PyResult<Document> {
        let options: cadkit::dgn::SeedPrepareOptions = options_json
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| PyValueError::new_err(e.to_string()))?
            .unwrap_or_default();
        let seed = Arc::clone(&self.inner);
        let src = Arc::clone(&doc.inner);
        let prepared = py
            .detach(|| seed.prepare_document(&src, &options))
            .map_err(map_err)?;
        Ok(Document {
            inner: Arc::new(prepared),
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "<cadkit.V8Seed dim={} unit={} nonempty={}>",
            if self.inner.is_3d() { "3D" } else { "2D" },
            self.inner.units().unit.as_str(),
            self.inner.is_nonempty()
        )
    }
}

/// A drawing in the format-neutral model.
#[pyclass(frozen, module = "cadkit")]
struct Document {
    inner: Arc<cadkit::Document>,
}

#[pymethods]
impl Document {
    /// Constructs a format-neutral `Document` from its JSON contract.
    #[staticmethod]
    fn from_json(text: &str) -> PyResult<Self> {
        let doc = cadkit::export::document_from_json(text, &cadkit::Limits::default())
            .map_err(map_err)?;
        Ok(Self {
            inner: Arc::new(doc),
        })
    }

    /// Returns a new `Document` with `Insert` block references expanded into world-space entities.
    #[pyo3(signature = (max_depth=32))]
    fn explode_inserts(&self, py: Python<'_>, max_depth: u32) -> PyResult<Self> {
        let src = Arc::clone(&self.inner);
        let next = py
            .detach(|| {
                let mut copy = (*src).clone();
                if copy.explode_inserts(max_depth).truncated {
                    return Err(Error::limit(
                        LimitKind::Depth,
                        "block expansion work limit exceeded".into(),
                    ));
                }
                Ok(copy)
            })
            .map_err(map_err)?;
        Ok(Self {
            inner: Arc::new(next),
        })
    }

    /// Returns a new `Document` converted to `unit` (e.g. `"mm"`, `"meter"`, `"ft"`).
    fn convert_units(&self, py: Python<'_>, unit: &str) -> PyResult<Self> {
        let target = LengthUnit::from_name(unit)
            .ok_or_else(|| PyValueError::new_err(format!("unknown length unit {unit:?}")))?;
        let src = Arc::clone(&self.inner);
        let next = py
            .detach(|| {
                let mut copy = (*src).clone();
                copy.convert_units(target)?;
                Ok::<_, Error>(copy)
            })
            .map_err(map_err)?;
        Ok(Self {
            inner: Arc::new(next),
        })
    }

    /// Writes DGN V8 bytes using `seed` (`V8Seed` or raw bytes); `options_json` follows the Rust
    /// `dgn::WriteOptions` JSON contract.
    #[pyo3(signature=(seed, options_json=None))]
    fn to_dgn<'py>(
        &self,
        py: Python<'py>,
        seed: &Bound<'py, PyAny>,
        options_json: Option<&str>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let options: cadkit::dgn::WriteOptions = options_json
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| PyValueError::new_err(e.to_string()))?
            .unwrap_or_default();
        if let Ok(parsed_seed) = seed.cast::<V8Seed>() {
            let seed_arc = Arc::clone(&parsed_seed.get().inner);
            let doc_arc = Arc::clone(&self.inner);
            let bytes = py
                .detach(|| cadkit::to_dgn_with_seed(&doc_arc, &seed_arc, &options))
                .map_err(map_err)?;
            return Ok(PyBytes::new(py, &bytes));
        }
        let Some(seed_bytes_res) = extract_buffer_bytes(py, seed) else {
            return Err(PyTypeError::new_err(
                "seed must be a V8Seed, bytes, bytearray, or memoryview",
            ));
        };
        let seed_bytes = seed_bytes_res?;
        let bytes = py
            .detach(|| cadkit::to_dgn(&self.inner, &seed_bytes, &options))
            .map_err(map_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Exports generic CityGML 2.0 geometry with an explicit CRS and LoD without transforming coordinates.
    #[pyo3(signature=(srs_name, lod=1))]
    fn to_citygml<'py>(
        &self,
        py: Python<'py>,
        srs_name: String,
        lod: u8,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = py
            .detach(|| {
                cadkit::to_citygml(
                    &self.inner,
                    &cadkit::gml::ExportOptions::new(srs_name, lod)
                        .with_limits(cadkit::Limits::default()),
                )
            })
            .map_err(map_err)?;
        Ok(PyBytes::new(py, &bytes))
    }
    /// Returns XML bytes and diagnostics; metadata-only fallback is explicit.
    #[pyo3(signature=(srs_name, lod=1, metadata_only=false))]
    fn to_citygml_with_report<'py>(
        &self,
        py: Python<'py>,
        srs_name: String,
        lod: u8,
        metadata_only: bool,
    ) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyAny>)> {
        let (bytes, report) = py
            .detach(|| {
                cadkit::gml::write_document_with_report(
                    &self.inner,
                    &cadkit::gml::ExportOptions::new(srs_name, lod)
                        .with_limits(cadkit::Limits::default()),
                    if metadata_only {
                        cadkit::gml::UnsupportedGeometry::MetadataOnly
                    } else {
                        cadkit::gml::UnsupportedGeometry::Reject
                    },
                )
            })
            .map_err(map_err)?;
        Ok((PyBytes::new(py, &bytes), to_py(py, &report)?))
    }

    /// File format family: `"dwg"`, `"dxf"`, `"dgn_v7"`, `"dgn_v8"`, `"city_gml"`, or `"unknown"`.
    #[getter]
    fn format(&self, py: Python<'_>) -> PyResult<String> {
        to_py(py, &self.inner.source.format)?.extract()
    }

    /// Format version as written in the file (for example `"AC1032"`).
    #[getter]
    fn version(&self) -> &str {
        &self.inner.source.version
    }

    /// Producing application, if recorded.
    #[getter]
    fn application(&self) -> Option<&str> {
        self.inner.source.application.as_deref()
    }

    /// Code page used for 8-bit strings, if known.
    #[getter]
    fn codepage(&self) -> Option<&str> {
        self.inner.source.codepage.as_deref()
    }

    /// Drawing units: `{"unit": ..., "meters_per_unit": ...}`.
    #[getter]
    fn units<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.inner.units)
    }

    /// Layers as a list of dicts.
    #[getter]
    fn layers<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.inner.layers)
    }

    /// Block definitions as a list of dicts.
    #[getter]
    fn blocks<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.inner.blocks)
    }

    /// Models (model space, layouts, sheets) as a list of dicts.
    #[getter]
    fn models<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.inner.models)
    }

    /// Recoverable problems met while reading, as a list of dicts.
    #[getter]
    fn warnings<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &self.inner.warnings)
    }

    /// The whole document as nested dicts and lists.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        to_py(py, &*self.inner)
    }

    /// The whole document as a JSON string.
    #[pyo3(signature = (pretty=false))]
    fn to_json(&self, py: Python<'_>, pretty: bool) -> PyResult<String> {
        let doc = &self.inner;
        py.detach(|| cadkit::to_json(doc, pretty)).map_err(map_err)
    }

    /// Renders one model as an SVG string.
    #[allow(clippy::too_many_arguments, clippy::needless_update)]
    #[pyo3(signature = (model=0, width=1600.0, stroke=1.0, background="#ffffff", text=true, expand_blocks=true, images=true, include_hidden_layers=false))]
    fn to_svg(
        &self,
        py: Python<'_>,
        model: usize,
        width: f64,
        stroke: f64,
        background: Option<&str>,
        text: bool,
        expand_blocks: bool,
        images: bool,
        include_hidden_layers: bool,
    ) -> PyResult<String> {
        if model >= self.inner.models.len() {
            return Err(model_range_error(model, self.inner.models.len()));
        }
        if !(width.is_finite() && width > 0.0) || !(stroke.is_finite() && stroke > 0.0) {
            return Err(PyValueError::new_err(
                "width and stroke must be finite and greater than 0",
            ));
        }
        let options = {
            let mut options = SvgOptions::default()
                .with_model(model)
                .with_width_px(width)
                .with_stroke_px(stroke)
                .with_text(text)
                .with_expand_blocks(expand_blocks)
                .with_images(images);
            options.background = background
                .filter(|b| !b.is_empty() && *b != "none")
                .map(str::to_owned);
            options
        };
        let doc = &self.inner;
        Ok(py.detach(|| {
            if include_hidden_layers {
                cadkit::to_svg_all_layers(doc, &options)
            } else {
                cadkit::to_svg(doc, &options)
            }
        }))
    }

    /// Writes the document as ASCII DXF text.
    #[pyo3(signature = (version="r2018"))]
    fn to_dxf(&self, py: Python<'_>, version: &str) -> PyResult<String> {
        let v = parse_dxf_version(version)?;
        let doc = &self.inner;
        py.detach(|| cadkit::to_dxf(doc, v)).map_err(map_err)
    }

    /// Writes the document as DXF bytes with an optional legacy 8-bit `codepage` (e.g. `"DOS857"`).
    #[pyo3(signature = (version="r2018", codepage=None))]
    fn to_dxf_bytes<'py>(
        &self,
        py: Python<'py>,
        version: &str,
        codepage: Option<&str>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let v = parse_dxf_version(version)?;
        let opts = cadkit::dxf::WriteOptions::new(v).with_codepage(codepage.map(str::to_owned));
        let doc = &self.inner;
        let bytes = py
            .detach(|| cadkit::to_dxf_bytes(doc, &opts))
            .map_err(map_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Iterates over the entities of one model as dicts.
    #[pyo3(signature = (model=0))]
    fn entities<'py>(&self, py: Python<'py>, model: usize) -> PyResult<Bound<'py, PyAny>> {
        let m = self
            .inner
            .models
            .get(model)
            .ok_or_else(|| model_range_error(model, self.inner.models.len()))?;
        let list = to_py(py, &m.entities)?;
        Ok(list.try_iter()?.into_any())
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let count: usize = self.inner.models.iter().map(|m| m.entities.len()).sum();
        Ok(format!(
            "<cadkit.Document format={} version={:?} models={} entities={}>",
            self.format(py)?,
            self.inner.source.version,
            self.inner.models.len(),
            count
        ))
    }
}

/// Native CityGML 2.0 document preserving the object graph, IDs, references, and XML extensions.
#[pyclass(frozen, module = "cadkit")]
struct CityGmlDocument {
    inner: Arc<cadkit::gml::CityGmlDocument>,
}

#[pymethods]
impl CityGmlDocument {
    /// Local TKGM checks for city-model-tender or digital-building-registration.
    /// Requires an explicit profile; never establishes official acceptance.
    fn preflight_tkgm<'py>(&self, py: Python<'py>, profile: &str) -> PyResult<Bound<'py, PyAny>> {
        let profile = profile
            .parse::<cadkit::gml::tkgm::Profile>()
            .map_err(map_err)?;
        let report = py
            .detach(|| {
                cadkit::gml::tkgm::preflight(&self.inner, profile, &cadkit::Limits::default())
            })
            .map_err(map_err)?;
        to_py(py, &report)
    }
    /// Constructs and structurally validates a native `CityGmlDocument` from its JSON contract.
    #[staticmethod]
    fn from_json(text: &str) -> PyResult<Self> {
        let doc = cadkit::gml::read_native_json(text, &cadkit::ReadOptions::default())
            .map_err(map_err)?;
        Ok(Self {
            inner: Arc::new(doc),
        })
    }
    /// Serializes the native CityGML model to an editable JSON string.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&*self.inner).map_err(|e| PyValueError::new_err(e.to_string()))
    }
    /// Runs structural and geometric validation checks and returns summary counters (`ValidationOptions` JSON).
    #[pyo3(signature=(options_json=None))]
    fn validate<'py>(
        &self,
        py: Python<'py>,
        options_json: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let options = options_json
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| PyValueError::new_err(e.to_string()))?
            .unwrap_or_default();
        let report = py
            .detach(|| cadkit::gml::validate(&self.inner, &options))
            .map_err(map_err)?;
        to_py(py, &report)
    }
    /// Serializes UTF-8 CityGML 2.0 XML bytes; `options_json` follows the Rust `gml::ValidationOptions` JSON contract.
    #[pyo3(signature=(options_json=None))]
    fn to_gml<'py>(
        &self,
        py: Python<'py>,
        options_json: Option<&str>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let options = options_json
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| PyValueError::new_err(e.to_string()))?
            .unwrap_or_default();
        let bytes = py
            .detach(|| cadkit::gml::write(&self.inner, &options))
            .map_err(map_err)?;
        Ok(PyBytes::new(py, &bytes))
    }
    /// Projects native CityGML geometry (optionally filtered by `lod`) into a format-neutral `Document`.
    #[pyo3(signature=(lod=None))]
    fn to_document(&self, py: Python<'_>, lod: Option<u8>) -> PyResult<Document> {
        let doc = py
            .detach(|| {
                cadkit::gml::to_document(
                    &self.inner,
                    &cadkit::gml::ImportOptions::default().with_lod(lod),
                    &ReadOptions::default(),
                )
            })
            .map_err(map_err)?;
        Ok(Document {
            inner: Arc::new(doc),
        })
    }
}

/// Reads CityGML 2.0 bytes into a native `CityGmlDocument` without reducing to the neutral model (`ReadOptions` JSON).
#[pyfunction]
#[pyo3(signature=(data, options_json=None))]
fn read_citygml(
    py: Python<'_>,
    data: &[u8],
    options_json: Option<&str>,
) -> PyResult<CityGmlDocument> {
    let options = options_json
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| PyValueError::new_err(e.to_string()))?
        .unwrap_or_default();
    let doc = py
        .detach(|| cadkit::gml::read_native(data, &options))
        .map_err(map_err)?;
    Ok(CityGmlDocument {
        inner: Arc::new(doc),
    })
}

/// Resolves a `read()`/`inspect()` argument: `Ok(bytes)` for buffers, `Err(path)` for path-likes.
fn source_input(py: Python<'_>, source: &Bound<'_, PyAny>) -> PyResult<Result<Vec<u8>, String>> {
    if let Some(res) = extract_buffer_bytes(py, source) {
        return Ok(Ok(res?));
    }
    let os = py.import(intern!(py, "os"))?;
    let fspath = os.call_method1(intern!(py, "fspath"), (source,))?;
    let path: String = os
        .call_method1(intern!(py, "fsdecode"), (fspath,))?
        .extract()?;
    Ok(Err(path))
}

/// Inspects a drawing header from a path or bytes without decoding full entity streams.
#[pyfunction]
#[pyo3(signature = (source, *, codepage=None, max_input_bytes=None, max_decompressed_bytes=None, max_objects=None, max_depth=None))]
#[allow(clippy::too_many_arguments)]
fn inspect<'py>(
    py: Python<'py>,
    source: &Bound<'py, PyAny>,
    codepage: Option<String>,
    max_input_bytes: Option<u64>,
    max_decompressed_bytes: Option<u64>,
    max_objects: Option<u64>,
    max_depth: Option<u32>,
) -> PyResult<Bound<'py, PyAny>> {
    let input = source_input(py, source)?;
    let options = build_read_options(
        false,
        codepage,
        max_input_bytes,
        max_decompressed_bytes,
        max_objects,
        max_depth,
    );
    let summary = py
        .detach(|| match input {
            Ok(bytes) => cadkit::inspect_with(&bytes, &options),
            Err(path) => cadkit::inspect_file_with(path, &options),
        })
        .map_err(map_err)?;
    to_py(py, &summary)
}

/// Reads a drawing from a path or from bytes.
#[pyfunction]
#[pyo3(signature = (source, *, keep_raw=false, codepage=None, max_input_bytes=None, max_decompressed_bytes=None, max_objects=None, max_depth=None))]
#[allow(clippy::too_many_arguments)]
fn read(
    py: Python<'_>,
    source: &Bound<'_, PyAny>,
    keep_raw: bool,
    codepage: Option<String>,
    max_input_bytes: Option<u64>,
    max_decompressed_bytes: Option<u64>,
    max_objects: Option<u64>,
    max_depth: Option<u32>,
) -> PyResult<Document> {
    let input = source_input(py, source)?;
    let options = build_read_options(
        keep_raw,
        codepage,
        max_input_bytes,
        max_decompressed_bytes,
        max_objects,
        max_depth,
    );
    let result = py.detach(|| match input {
        Ok(bytes) => cadkit::read_with(&bytes, &options),
        Err(path) => cadkit::open_with(path, &options),
    });
    result
        .map(|d| Document { inner: Arc::new(d) })
        .map_err(map_err)
}

/// Detects the format of `data` from its header (`"dwg"`, `"dxf"`, `"dgn_v7"`, `"dgn_v8"`, or `"city_gml"`), or `None` when unrecognized.
#[pyfunction]
fn detect(py: Python<'_>, data: &[u8]) -> PyResult<Option<String>> {
    match py.detach(|| cadkit::detect(data)) {
        Some(f) => Ok(Some(to_py(py, &f)?.extract()?)),
        None => Ok(None),
    }
}

#[pymodule]
fn _cadkit(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Document>()?;
    m.add_class::<V8Seed>()?;
    m.add_function(wrap_pyfunction!(inspect, m)?)?;
    m.add_function(wrap_pyfunction!(read, m)?)?;
    m.add_function(wrap_pyfunction!(detect, m)?)?;
    m.add_function(wrap_pyfunction!(read_citygml, m)?)?;
    m.add_class::<CityGmlDocument>()?;
    m.add("CadkitError", py.get_type::<CadkitError>())?;
    m.add("UnknownFormatError", py.get_type::<UnknownFormatError>())?;
    m.add("UnsupportedError", py.get_type::<UnsupportedError>())?;
    m.add("LimitExceededError", py.get_type::<LimitExceededError>())?;
    m.add("InvalidDataError", py.get_type::<InvalidDataError>())?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
