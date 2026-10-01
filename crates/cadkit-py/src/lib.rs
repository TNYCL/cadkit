//! Python bindings for cadkit (`cadkit._cadkit`).
//!
//! The document model stays in Rust; Python receives plain dicts/lists produced by
//! serializing the model to JSON (with the GIL released) and decoding it with the
//! standard library's C `json` module. That keeps the binding independent of the
//! model's shape: new entity kinds and fields appear automatically.

use std::sync::Arc;

use cadkit::{DxfVersion, Error, ReadOptions, SvgOptions};
use pyo3::exceptions::{PyIndexError, PyValueError};
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
        Error::LimitExceeded(_) => LimitExceededError::new_err(err.to_string()),
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

/// A drawing in the format-neutral model.
#[pyclass(frozen, module = "cadkit")]
struct Document {
    inner: Arc<cadkit::Document>,
}

#[pymethods]
impl Document {
    /// Yeni nötr modeli JSON sözleşmesinden oluşturur.
    #[staticmethod]
    fn from_json(text: &str) -> PyResult<Self> {
        let doc = cadkit::export::document_from_json(text, &cadkit::Limits::default())
            .map_err(map_err)?;
        Ok(Self {
            inner: Arc::new(doc),
        })
    }

    /// Seed baytlarıyla V8 çıktısı üretir; seçenekler Rust WriteOptions JSON sözleşmesidir.
    #[pyo3(signature=(seed, options_json=None))]
    fn to_dgn<'py>(
        &self,
        py: Python<'py>,
        seed: &[u8],
        options_json: Option<&str>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let options: cadkit::dgn::WriteOptions = options_json
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| PyValueError::new_err(e.to_string()))?
            .unwrap_or_default();
        let bytes = py
            .detach(|| cadkit::to_dgn(&self.inner, seed, &options))
            .map_err(map_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Koordinatları değiştirmeden genel CityGML geometrisi üretir.
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
                    &cadkit::gml::ExportOptions {
                        srs_name,
                        lod,
                        limits: cadkit::Limits::default(),
                    },
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
                    &cadkit::gml::ExportOptions {
                        srs_name,
                        lod,
                        limits: cadkit::Limits::default(),
                    },
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

    /// File format family: `"dwg"`, `"dxf"`, `"dgn_v7"`, `"dgn_v8"` or `"unknown"`.
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
    #[pyo3(signature = (model=0, width=1600.0, stroke=1.0, background="#ffffff", text=true, expand_blocks=true, images=true))]
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
    ) -> PyResult<String> {
        if model >= self.inner.models.len() {
            return Err(model_range_error(model, self.inner.models.len()));
        }
        if !(width.is_finite() && width > 0.0) || !(stroke.is_finite() && stroke > 0.0) {
            return Err(PyValueError::new_err(
                "width and stroke must be finite and greater than 0",
            ));
        }
        let options = SvgOptions {
            model,
            width_px: width,
            stroke_px: stroke,
            background: background
                .filter(|b| !b.is_empty() && *b != "none")
                .map(str::to_owned),
            text,
            expand_blocks,
            images,
            ..SvgOptions::default()
        };
        let doc = &self.inner;
        Ok(py.detach(|| cadkit::to_svg(doc, &options)))
    }

    /// Writes the document as ASCII DXF text.
    #[pyo3(signature = (version="r2018"))]
    fn to_dxf(&self, py: Python<'_>, version: &str) -> PyResult<String> {
        let v = match version.to_ascii_lowercase().as_str() {
            "r12" => DxfVersion::R12,
            "r2000" => DxfVersion::R2000,
            "r2004" => DxfVersion::R2004,
            "r2007" => DxfVersion::R2007,
            "r2010" => DxfVersion::R2010,
            "r2013" => DxfVersion::R2013,
            "r2018" => DxfVersion::R2018,
            other => {
                return Err(PyValueError::new_err(format!(
                    "unknown DXF version {other:?}; expected r12, r2000, r2004, r2007, r2010, r2013 or r2018"
                )));
            }
        };
        let doc = &self.inner;
        py.detach(|| cadkit::to_dxf(doc, v)).map_err(map_err)
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

/// Özgün CityGML nesne ağını ve XML uzantılarını koruyan belge.
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
    /// Yeni CityGML modelini JSON sözleşmesinden oluşturur ve yapısını denetler.
    #[staticmethod]
    fn from_json(text: &str) -> PyResult<Self> {
        let doc = cadkit::gml::read_native_json(text, &cadkit::ReadOptions::default())
            .map_err(map_err)?;
        Ok(Self {
            inner: Arc::new(doc),
        })
    }
    /// Özgün modeli düzenlenebilir JSON metnine dönüştürür.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&*self.inner).map_err(|e| PyValueError::new_err(e.to_string()))
    }
    /// Yapısal/geometrik kontrolleri çalıştırıp sayaçları döndürür.
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
    /// UTF-8 CityGML baytlarını üretir; seçenekler ValidationOptions JSON sözleşmesidir.
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
    /// Seçilen LoD geometrisini nötr modele aktarır.
    #[pyo3(signature=(lod=None))]
    fn to_document(&self, py: Python<'_>, lod: Option<u8>) -> PyResult<Document> {
        let doc = py
            .detach(|| {
                cadkit::gml::to_document(
                    &self.inner,
                    &cadkit::gml::ImportOptions { lod },
                    &ReadOptions::default(),
                )
            })
            .map_err(map_err)?;
        Ok(Document {
            inner: Arc::new(doc),
        })
    }
}

/// CityGML baytlarını nötr modele indirgemeden okur; seçenekler ReadOptions JSON sözleşmesidir.
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

/// Resolves a `read()` argument: `Ok(bytes)` for buffers, `Err(path)` for path-likes.
fn source_input(py: Python<'_>, source: &Bound<'_, PyAny>) -> PyResult<Result<Vec<u8>, String>> {
    if let Ok(b) = source.cast::<PyBytes>() {
        return Ok(Ok(b.as_bytes().to_vec()));
    }
    let builtins = py.import(intern!(py, "builtins"))?;
    if source.is_instance_of::<PyByteArray>()
        || source.is_instance(&builtins.getattr(intern!(py, "memoryview"))?)?
    {
        let b: Vec<u8> = builtins
            .getattr(intern!(py, "bytes"))?
            .call1((source,))?
            .extract()?;
        return Ok(Ok(b));
    }
    let os = py.import(intern!(py, "os"))?;
    let fspath = os.call_method1(intern!(py, "fspath"), (source,))?;
    let path: String = os
        .call_method1(intern!(py, "fsdecode"), (fspath,))?
        .extract()?;
    Ok(Err(path))
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
    let mut options = ReadOptions {
        keep_raw,
        fallback_codepage: codepage,
        ..ReadOptions::default()
    };
    let limits = &mut options.limits;
    limits.max_input_bytes = max_input_bytes.unwrap_or(limits.max_input_bytes);
    limits.max_decompressed_bytes = max_decompressed_bytes.unwrap_or(limits.max_decompressed_bytes);
    limits.max_objects = max_objects.unwrap_or(limits.max_objects);
    limits.max_depth = max_depth.unwrap_or(limits.max_depth);
    let result = py.detach(|| match input {
        Ok(bytes) => cadkit::read_with(&bytes, &options),
        Err(path) => cadkit::open_with(path, &options),
    });
    result
        .map(|d| Document { inner: Arc::new(d) })
        .map_err(map_err)
}

/// Detects the format of `data` from its header; returns `None` when unrecognized.
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
