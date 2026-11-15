//! A seed prepared from a drawing: [`strip_seed_v8`] copies a DGN V8 seed without the
//! drawing content of its model and without the document properties that name people or
//! organisations, keeping the tables a seed is for byte for byte.
//!
//! The model is emptied by writing an empty document into the seed through the writer's own
//! path ([`super::write_v8_with_seed`] with `clear_seed_model`), so extents, the element id
//! high-water mark and the page counters follow the rules of every written file. The
//! container is then built anew from the streams that remain ([`container::build`]), so no
//! byte of a removed stream or record survives in a free sector or a free directory slot.
//! See `docs/dgn/FORMAT_NOTES.md`, "Seed stripping".

use std::collections::{BTreeMap, BTreeSet};

use cadkit_core::{Document, Entity, EntityKind, Error, Model, ReadOptions, Result, Warning};

use super::encode::{f64_at, put, string_link, u32_at, u64_at};
use super::{V8Seed, WriteOptions, compressed, container, named, oleps, tables::NamedRecord};
use crate::le;
use crate::native::decode_v8;
use crate::native::element::ElementData;
use crate::native::linkage::{self, LinkageData};
use crate::native::v8::{Page, RawElement, V8Model};

/// Model index magic (`Dgn^Ix/Dgn~Mix`, FORMAT_NOTES "Model index").
const MODEL_INDEX_MAGIC: u32 = 0xaa00_ba11;
/// Byte length of a model index entry header; the name follows.
const MODEL_INDEX_ENTRY: usize = 32;
/// Longest model name in UTF-16 units: a string linkage holds at most 512 bytes (12 bytes of
/// framing, the `ff fd` marker and the units).
const MAX_MODEL_NAME_UNITS: usize = 249;
/// The model header's model time (`f64` at `0x88`, FORMAT_NOTES "Model header").
const MODEL_TIME: usize = 0x88;
/// Element time (`f64` at `0x18`) of a record, the model header element included.
const ELEMENT_TIME: usize = 0x18;
/// The file header's element id high-water mark (`u64` at `0x128` of the inflated `Dgn~H`).
const HIGH_WATER_ID: usize = 0x128;
/// The file header's save time (`f64` at `0x130` of the inflated `Dgn~H`).
const FILE_TIME: usize = 0x130;
/// Latest accepted [`StripSeedOptions::timestamp_ms`] (about year 33658), well inside the
/// FILETIME range of the compound file entries.
const MAX_TIMESTAMP_MS: f64 = 1e15;
/// Type of a shared cell definition.
const SHARED_CELL_DEFINITION: u16 = 34;

/// Options for [`strip_seed_v8`].
///
/// The defaults remove every control record of the model, the raster attachments and no
/// shared cell definition, keep the model name and the seed's file header time, and name
/// `"cadkit"` as the application.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct StripSeedOptions {
    /// Keep the model's control records that reference no graphic record (type 66 level 20
    /// model settings, type 100 groups with their 99 components and type 39 tag set
    /// definitions in the observed files) byte for byte, as
    /// [`WriteOptions::preserve_seed_controls`] does. Raster control records follow
    /// [`keep_raster_template`](Self::keep_raster_template) instead. Default `false`.
    pub keep_controls: bool,
    /// Keep the seed's raster attachments (the frame graphic, its control group and
    /// XAttribute, unchanged) so that [`super::write_v8`] clones new attachments from the
    /// seed's group instead of its built-in one. The writer keeps a seed's attachments only
    /// all together, so every attachment stays, each with its file name and path. Default
    /// `false`.
    pub keep_raster_template: bool,
    /// Keep the dictionary's shared cell definitions (type 34 with their components).
    /// Default `true`.
    pub keep_cell_definitions: bool,
    /// New name of the model (model header name linkage and model index entry); `None`
    /// keeps the seed's. At most 249 UTF-16 units, not empty, no NUL.
    pub model_name: Option<String>,
    /// Time in milliseconds since the Unix epoch for the file header save time, the model
    /// header times, every compound file entry and the property sets, at most `1e15`. `0`
    /// uses the seed's
    /// file header time. A value below the newest time of a kept record is raised to it, as
    /// the file header time is never older than a record in any observed file.
    pub timestamp_ms: f64,
    /// Application name stored in `\u{5}SummaryInformation` (PIDSI_APPNAME); empty leaves it
    /// out. At most 255 UTF-16 units. Default `"cadkit"`.
    pub application: String,
}

impl Default for StripSeedOptions {
    fn default() -> Self {
        Self {
            keep_controls: false,
            keep_raster_template: false,
            keep_cell_definitions: true,
            model_name: None,
            timestamp_ms: 0.0,
            application: "cadkit".into(),
        }
    }
}

impl StripSeedOptions {
    /// The defaults: controls and raster attachments removed, cell definitions kept, seed
    /// time, application `"cadkit"`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets [`keep_controls`](Self::keep_controls).
    #[must_use]
    pub fn with_keep_controls(mut self, keep: bool) -> Self {
        self.keep_controls = keep;
        self
    }

    /// Sets [`keep_raster_template`](Self::keep_raster_template).
    #[must_use]
    pub fn with_keep_raster_template(mut self, keep: bool) -> Self {
        self.keep_raster_template = keep;
        self
    }

    /// Sets [`keep_cell_definitions`](Self::keep_cell_definitions).
    #[must_use]
    pub fn with_keep_cell_definitions(mut self, keep: bool) -> Self {
        self.keep_cell_definitions = keep;
        self
    }

    /// Sets [`model_name`](Self::model_name).
    #[must_use]
    pub fn with_model_name(mut self, name: Option<impl Into<String>>) -> Self {
        self.model_name = name.map(Into::into);
        self
    }

    /// Sets [`timestamp_ms`](Self::timestamp_ms).
    #[must_use]
    pub fn with_timestamp_ms(mut self, timestamp_ms: f64) -> Self {
        self.timestamp_ms = timestamp_ms;
        self
    }

    /// Sets [`application`](Self::application).
    #[must_use]
    pub fn with_application(mut self, application: impl Into<String>) -> Self {
        self.application = application.into();
        self
    }

    fn validate(&self) -> Result<()> {
        if !self.timestamp_ms.is_finite()
            || self.timestamp_ms < 0.0
            || self.timestamp_ms > MAX_TIMESTAMP_MS
        {
            return Err(Error::invalid(
                0,
                "DGN seed timestamp must lie between 0 and 1e15 ms",
            ));
        }
        if let Some(name) = &self.model_name {
            let units = name.encode_utf16().count();
            if units == 0 || units > MAX_MODEL_NAME_UNITS || name.contains('\0') {
                return Err(Error::invalid(
                    0,
                    "DGN model name must hold 1 to 249 UTF-16 units and no NUL",
                ));
            }
        }
        if self.application.encode_utf16().count() > oleps::MAX_APPLICATION_UNITS
            || self.application.contains('\0')
        {
            return Err(Error::invalid(
                0,
                "DGN application name must hold at most 255 UTF-16 units and no NUL",
            ));
        }
        Ok(())
    }
}

/// What [`strip_seed_v8`] removed and what remains.
///
/// The `removed_*` counts compare the seed with the result. The other fields count what the
/// result still holds that may identify the drawing it came from: names in the kept tables
/// (levels, fonts, tag sets, cell definitions), the model name and description, kept control
/// records, raster file references and streams cadkit does not decode. cadkit does not
/// review those contents; the caller must, before handing the seed on. The result is not
/// anonymous.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct StripSeedReport {
    /// Graphic records of the model removed.
    pub removed_graphic_records: u64,
    /// Control records of the model removed.
    pub removed_control_records: u64,
    /// Removed control records by element type.
    pub removed_control_types: BTreeMap<u16, u64>,
    /// XAttributes (auxiliary records) of removed model records.
    pub removed_auxiliary_records: u64,
    /// Shared cell definitions (type 34 with their components) removed from the dictionary.
    pub removed_cell_definitions: u64,
    /// Property set streams (`\u{5}...`) removed; `\u{5}SummaryInformation` and
    /// `\u{5}DocumentSummaryInformation` are then written anew.
    pub removed_property_sets: u64,
    /// Property ids of the seed's `\u{5}SummaryInformation` that the new stream does not
    /// hold (author `0x04`, last author `0x08`, thumbnail `0x11`, ...), sorted.
    pub removed_summary_properties: Vec<u32>,
    /// Property ids of the seed's `\u{5}DocumentSummaryInformation` that the new stream does
    /// not hold (company `0x0F`, manager `0x0E`, ...), sorted.
    pub removed_document_summary_properties: Vec<u32>,
    /// Control records the result keeps, kept byte for byte.
    pub kept_control_records: u64,
    /// Kept control records by element type.
    pub kept_control_types: BTreeMap<u16, u64>,
    /// Raster attachments (type 94 frames) the result keeps.
    pub kept_raster_attachments: u64,
    /// Raster file references (type 90 records, each with a file name and a path) kept.
    pub raster_file_references: u64,
    /// Dictionary (`Dgn^Nm`) records of the result.
    pub dictionary_records: u64,
    /// Dictionary and dictionary XAttribute pages whose stream differs from the seed's: 0
    /// unless cell definitions were removed. The dictionary is otherwise the seed's byte for
    /// byte, including table defects [`super::write_v8`] repairs when it writes into a seed.
    pub dictionary_pages_rewritten: u64,
    /// Level table entries; each may carry a name and a description.
    pub levels: u64,
    /// Of [`levels`](Self::levels), the entries with a non-empty name.
    pub named_levels: u64,
    /// Font table entries.
    pub fonts: u64,
    /// Tag set definitions in the dictionary, each with a name and tag names.
    pub tag_sets: u64,
    /// Shared cell definitions in the dictionary, each with a name.
    pub cell_definitions: u64,
    /// Whether the model keeps the seed's non-empty name.
    pub model_name_from_seed: bool,
    /// Whether the model header or model index keeps a non-empty description.
    pub model_description: bool,
    /// Streams cadkit does not decode, kept byte for byte (`Dgn~S` and `Dgn~Mf` in every
    /// observed file; also `Oda~SH`, `DgnTk` and others when present).
    pub unknown_streams: u64,
    /// The time written ([`StripSeedOptions::timestamp_ms`] after its rules), in
    /// milliseconds since the Unix epoch; 0 when neither the options nor the seed give one.
    pub timestamp_ms: f64,
    /// Recoverable problems: `dgn.strip_model_index` when the model index could not be
    /// edited (the model header is still renamed and its time still normalised).
    pub warnings: Vec<Warning>,
}

/// A seed returned by [`strip_seed_v8`] with its report.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct StrippedSeed {
    /// The DGN V8 seed file.
    pub bytes: Vec<u8>,
    /// What was removed and what remains.
    pub report: StripSeedReport,
}

/// Returns a copy of the single-model DGN V8 `seed` without the drawing content of its model
/// and without the identifying document properties, for use as a seed of
/// [`super::write_v8`].
///
/// Removed: every graphic record of the model with its XAttributes; by default every control
/// record (see [`StripSeedOptions::keep_controls`] and
/// [`keep_raster_template`](StripSeedOptions::keep_raster_template)); optionally the shared
/// cell definitions; every property set stream, replaced by a `\u{5}SummaryInformation` with
/// code page, application and times only and a `\u{5}DocumentSummaryInformation` with the code
/// page only (no author, last author, company, manager, title, subject, keywords, comments,
/// template or thumbnail). Reset or normalised: the model extents and page counter (as the
/// writer sets them for an empty model), the file header save time, the model header times,
/// the compound file entry times. Kept byte for byte: the level, font and colour tables, tag
/// set definitions, text styles, views and settings records, the other dictionary tables,
/// units and every stream cadkit does not decode; the element id high-water mark is the
/// seed's (raised to its highest record id where the seed's mark is lower).
///
/// Names stored in the kept tables (levels, fonts, tag sets and their tags, cell
/// definitions, text styles, views), the model name unless
/// [`model_name`](StripSeedOptions::model_name) replaces it, the model description, the
/// times of kept records and the contents of undecoded streams remain. [`StripSeedReport`]
/// counts them; review them before handing the seed on. cadkit does not claim the result is
/// anonymous, and no application other than cadkit was used to check it.
///
/// Equal seeds and options give equal bytes, and stripping a result again returns the same
/// bytes.
///
/// # Errors
///
/// The errors of [`V8Seed::parse`] for seeds the writer rejects (several models,
/// undecodable pages, an unrecognised file header), [`Error::Invalid`] for invalid options,
/// [`Error::LimitExceeded`] when a limit of `read` is exceeded and the writer's errors when
/// a kept raster attachment cannot be written back unchanged.
pub fn strip_seed_v8(
    seed: &[u8],
    options: &StripSeedOptions,
    read: &ReadOptions,
) -> Result<StrippedSeed> {
    options.validate()?;
    let mut read = read.clone();
    read.keep_raw = true;
    let input = V8Seed::parse(seed, &read)?;
    let input_streams = container::read_streams(seed, &read)?;
    let mut report = StripSeedReport::default();

    let without_cells = if options.keep_cell_definitions {
        None
    } else {
        let (bytes, removed) = remove_cell_definitions(&input, &read)?;
        report.removed_cell_definitions = removed;
        bytes
    };
    let reparsed;
    let source = match &without_cells {
        Some(bytes) => {
            reparsed = V8Seed::parse(bytes, &read)?;
            &reparsed
        }
        None => &input,
    };

    let mut doc = Document::default();
    doc.units = source.units();
    let mut model = Model::default();
    model.is_3d = source.is_3d();
    if options.keep_raster_template && source.has_raster_template() {
        model.entities = seed_images(source.as_bytes(), &read)?;
    }
    doc.models.push(model);
    let write = WriteOptions::new()
        .with_limits(read.limits)
        .with_codepage(read.fallback_codepage.clone())
        .with_timestamp_ms(options.timestamp_ms)
        .with_clear_seed_model(true)
        .with_preserve_seed_rasters(true)
        .with_preserve_seed_controls(options.keep_controls);
    let written = super::write_v8_with_seed(&doc, source, &write)?;

    let emptied = V8Seed::parse(&written, &read)?;
    let model = emptied.model()?;
    let header = emptied
        .model_header()
        .ok_or_else(|| Error::invalid(0, "missing seed model header"))?;
    // The dictionary written below is the source's; the model pages are the writer's.
    let newest_record = source
        .native
        .named_pages
        .iter()
        .chain(&model.graphic_pages)
        .chain(&model.control_pages)
        .flat_map(|p| &p.elements)
        .filter_map(|e| le::f64_at(&e.bytes, ELEMENT_TIME))
        .filter(|t| t.is_finite() && *t > 0.0)
        .fold(0.0f64, f64::max);
    let time = if options.timestamp_ms > 0.0 {
        options.timestamp_ms.max(newest_record)
    } else {
        emptied.seed_timestamp_ms
    };
    let filetime = oleps::filetime(time);

    let mut streams = container::read_streams(&written, &read)?;
    // The writer repairs the tables of some seeds as it writes into them (a second level table,
    // repeated level codes; 320 dictionary pages of the private corpus). A seed keeps its own
    // dictionary instead, so the repairs happen when the result is used, as for any seed.
    let source_streams;
    let dictionary_source = match &without_cells {
        Some(bytes) => {
            source_streams = container::read_streams(bytes, &read)?;
            &source_streams
        }
        None => &input_streams,
    };
    streams.retain(|k, _| !is_dictionary_page(k));
    streams.extend(
        dictionary_source
            .iter()
            .filter(|(k, _)| is_dictionary_page(k))
            .map(|(k, v)| (k.clone(), v.clone())),
    );
    let mut file_header = emptied.header_data.clone();
    // The id high-water mark of what is kept: records the writer added to the dictionary (a
    // level for a kept raster frame, say) are not kept, so its mark would grow on every strip.
    let max_id = model
        .graphic_pages
        .iter()
        .chain(&model.control_pages)
        .flat_map(|p| &p.elements)
        .filter_map(RawElement::id)
        .fold(source.max_id, u64::max);
    u64_at(&mut file_header, HIGH_WATER_ID, max_id)?;
    if time > 0.0 {
        f64_at(&mut file_header, FILE_TIME, time)?;
    }
    let mut h = emptied.header_prefix.to_vec();
    h.extend(compressed(&file_header)?);
    streams.insert("Dgn~H".into(), h);

    let mut element = header.raw.clone();
    let old_time = element
        .get(ELEMENT_TIME..ELEMENT_TIME + 8)
        .map(<[u8]>::to_vec);
    if time > 0.0 {
        f64_at(&mut element, MODEL_TIME, time)?;
        // Zero in all 567 private files, a time in the 3 public ones; only a time is replaced.
        if le::f64_at(&element, ELEMENT_TIME).is_some_and(|t| t.is_finite() && t > 0.0) {
            f64_at(&mut element, ELEMENT_TIME, time)?;
        }
    }
    let new_time = element
        .get(ELEMENT_TIME..ELEMENT_TIME + 8)
        .map(<[u8]>::to_vec);
    if let Some(name) = &options.model_name {
        element = rename_model_header(&element, name, &read)?;
    }
    let mut mh = emptied.mh_prefix.clone();
    mh.extend_from_slice(&element);
    streams.insert(emptied.mh_path.clone(), compressed(&mh)?);

    let time_change = match (&old_time, &new_time) {
        (Some(old), Some(new)) if old != new => Some((old.as_slice(), new.as_slice())),
        _ => None,
    };
    if options.model_name.is_some() || time_change.is_some() {
        let storage = model.storage.trim_start_matches('#').parse::<u16>().ok();
        let edited = streams.get("Dgn^Ix/Dgn~Mix").and_then(|raw| {
            let inflated = crate::zlib::inflate(
                raw,
                usize::try_from(read.limits.max_decompressed_bytes).unwrap_or(usize::MAX),
            );
            if inflated.problem.is_some() {
                return None;
            }
            edit_model_index(
                &inflated.data,
                storage?,
                options.model_name.as_deref(),
                time_change,
            )
        });
        match edited {
            Some(mix) => {
                streams.insert("Dgn^Ix/Dgn~Mix".into(), compressed(&mix)?);
            }
            None => report.warnings.push(Warning::new(
                "dgn.strip_model_index",
                "the model index could not be edited; it keeps the seed's entry",
            )),
        }
    }

    let property_sets: Vec<String> = streams
        .keys()
        .filter(|k| k.rsplit('/').next().is_some_and(|l| l.starts_with('\u{5}')))
        .cloned()
        .collect();
    for key in &property_sets {
        streams.remove(key);
    }
    report.removed_property_sets = property_sets.len() as u64;
    let summary = oleps::summary_information(&options.application, filetime)?;
    let document_summary = oleps::document_summary_information()?;
    report.removed_summary_properties = removed_ids(
        input_streams
            .get("\u{5}SummaryInformation")
            .map(Vec::as_slice),
        &summary,
    );
    report.removed_document_summary_properties = removed_ids(
        input_streams
            .get("\u{5}DocumentSummaryInformation")
            .map(Vec::as_slice),
        &document_summary,
    );
    streams.insert("\u{5}SummaryInformation".into(), summary);
    streams.insert("\u{5}DocumentSummaryInformation".into(), document_summary);

    // Storages left without streams (the emptied model's `Dgn^C`, `Dgn^GA`, `Dgn^CA`) go.
    let bytes = container::build(
        &streams,
        &BTreeSet::new(),
        container::BuildMeta {
            root_color: container::seed_root_color(&written),
            filetime: filetime.unwrap_or(0),
        },
        &read,
    )?;
    let output = V8Seed::parse(&bytes, &read)?;
    report.timestamp_ms = time;
    count_model(source.model()?, output.model()?, &mut report);
    count_dictionary(&output, &read, &mut report);
    report.dictionary_pages_rewritten = input_streams
        .iter()
        .filter(|(k, _)| is_dictionary_page(k))
        .filter(|(k, v)| streams.get(*k) != Some(*v))
        .count() as u64
        + streams
            .keys()
            .filter(|k| is_dictionary_page(k) && !input_streams.contains_key(*k))
            .count() as u64;
    let out_header = output.model_header();
    report.model_name_from_seed = options.model_name.is_none()
        && out_header
            .and_then(|h| h.name.as_deref())
            .is_some_and(|n| !n.is_empty());
    report.model_description = out_header
        .and_then(|h| h.description.as_deref())
        .is_some_and(|d| !d.is_empty())
        || output
            .native
            .model_index
            .iter()
            .any(|e| !e.description.is_empty());
    report.unknown_streams = streams.keys().filter(|k| !is_decoded_stream(k)).count() as u64;
    Ok(StrippedSeed { bytes, report })
}

/// The seed rewritten without its shared cell definitions (a type 34 record outside a complex
/// element and the components that follow it, on its page or continuing on the next), with
/// the number removed; `None` when it has none.
fn remove_cell_definitions(seed: &V8Seed, read: &ReadOptions) -> Result<(Option<Vec<u8>>, u64)> {
    let mut records = Vec::new();
    let mut removed = 0u64;
    let mut in_cell = false;
    for page in &seed.native.named_pages {
        for raw in &page.elements {
            if !raw.is_complex_component() {
                in_cell = raw.type_code() == SHARED_CELL_DEFINITION;
                removed += u64::from(in_cell);
            }
            if in_cell {
                continue;
            }
            records.push(NamedRecord {
                page: Some(page.number),
                prefix: raw.prefix,
                bytes: raw.bytes.clone(),
            });
        }
    }
    if removed == 0 {
        return Ok((None, 0));
    }
    let mut replacements = BTreeMap::new();
    named::write(
        &mut replacements,
        &seed.native.named_pages,
        &seed.native.named_aux,
        &records,
        read.limits.max_decompressed_bytes,
    )?;
    Ok((
        Some(container::rewrite(&seed.bytes, &replacements, read)?),
        removed,
    ))
}

/// The raster attachments of `seed` as the reader maps them, at the top level and without
/// attributes, for the writer to keep unchanged.
fn seed_images(seed: &[u8], read: &ReadOptions) -> Result<Vec<Entity>> {
    // The options the writer reads the seed with to compare the attachments it keeps.
    let mut writer_read = ReadOptions::default()
        .with_limits(read.limits)
        .with_keep_raw(true);
    writer_read.fallback_codepage = read.fallback_codepage.clone();
    let doc = crate::read(seed, &writer_read)?;
    let mut out = Vec::new();
    let mut todo: Vec<&Entity> = doc.models.iter().flat_map(|m| &m.entities).collect();
    while let Some(e) = todo.pop() {
        match &e.kind {
            EntityKind::Image { .. } if e.id.is_some() => {
                let mut image = e.clone();
                // Tags on a frame would be written as new graphic records.
                image.attributes.clear();
                out.push(image);
            }
            EntityKind::Group { children, .. } => todo.extend(children),
            _ => {}
        }
    }
    out.sort_by_key(|e| e.id);
    Ok(out)
}

/// The model header element with string linkage 1 (the model name) replaced by `name`, or
/// added after the other linkages; every other byte is kept.
fn rename_model_header(raw: &[u8], name: &str, read: &ReadOptions) -> Result<Vec<u8>> {
    let attr = usize::try_from(le::u32_at(raw, 8).unwrap_or(0))
        .unwrap_or(usize::MAX)
        .saturating_mul(2)
        .min(raw.len());
    let primary = raw.get(..attr).unwrap_or(&[]);
    let area = raw.get(attr..).unwrap_or(&[]);
    let enc = crate::text::encoding_for(read.fallback_codepage.as_deref());
    let links = linkage::parse(area, attr, true, enc);
    let name_link = string_link(1, name)?;
    let mut linkages = Vec::with_capacity(area.len() + name_link.len());
    let mut end = 0usize;
    let mut replaced = false;
    for l in &links {
        let start = l.offset.saturating_sub(attr);
        end = start.saturating_add(l.bytes.len());
        let is_name = matches!(l.data, LinkageData::String { string_id: 1, .. });
        if is_name && !replaced {
            linkages.extend_from_slice(&name_link);
            replaced = true;
        } else {
            linkages.extend_from_slice(&l.bytes);
        }
    }
    if !replaced {
        linkages.extend_from_slice(&name_link);
    }
    linkages.extend_from_slice(area.get(end..).unwrap_or(&[]));
    let mut out = primary.to_vec();
    out.extend(linkages);
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let words = u32::try_from(out.len() / 2)
        .map_err(|_| Error::invalid(0, "DGN model header exceeds its length field"))?;
    u32_at(&mut out, 4, words)?;
    Ok(out)
}

/// The inflated model index with the entry of model storage `storage` renamed to `name` and
/// its `+8` value replaced where it equalled the model header's old time (`time`: old and
/// new bytes); `None` when the index does not parse or holds no such entry.
fn edit_model_index(
    mix: &[u8],
    storage: u16,
    name: Option<&str>,
    time: Option<(&[u8], &[u8])>,
) -> Option<Vec<u8>> {
    if le::u32_at(mix, 0)? != MODEL_INDEX_MAGIC {
        return None;
    }
    let count = le::u32_at(mix, 8)? as usize;
    let mut out = mix.get(..16)?.to_vec();
    let mut off = 16usize;
    let mut found = false;
    for _ in 0..count.min(mix.len() / MODEL_INDEX_ENTRY) {
        let size = usize::from(le::u16_at(mix, off + 16)?);
        let entry = mix.get(off..off.checked_add(size)?)?;
        if size < MODEL_INDEX_ENTRY {
            return None;
        }
        if found || le::u32_at(entry, 0)? & 0xffff != u32::from(storage) {
            out.extend_from_slice(entry);
            off += size;
            continue;
        }
        found = true;
        let name_len = usize::from(le::u16_at(entry, 18)?);
        let desc_len = usize::try_from(le::u32_at(entry, 20)?).ok()?;
        let name_end = MODEL_INDEX_ENTRY.checked_add(name_len)?;
        let desc_end = name_end.checked_add(desc_len)?;
        let mut head = entry.get(..MODEL_INDEX_ENTRY)?.to_vec();
        let new_name = match name {
            Some(n) => n.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            None => entry.get(MODEL_INDEX_ENTRY..name_end)?.to_vec(),
        };
        if let Some((old, new)) = time
            && head.get(8..16) == Some(old)
        {
            put(&mut head, 8, new).ok()?;
        }
        if desc_end > size {
            return None;
        }
        let rest = entry.get(name_end..)?;
        let new_size = u16::try_from(size - name_len + new_name.len()).ok()?;
        put(&mut head, 16, &new_size.to_le_bytes()).ok()?;
        put(
            &mut head,
            18,
            &u16::try_from(new_name.len()).ok()?.to_le_bytes(),
        )
        .ok()?;
        out.extend(head);
        out.extend(new_name);
        out.extend_from_slice(rest);
        off += size;
    }
    out.extend_from_slice(mix.get(off..)?);
    found.then_some(out)
}

/// Property ids of the seed's property set `before` that `after` does not hold, sorted.
fn removed_ids(before: Option<&[u8]>, after: &[u8]) -> Vec<u32> {
    let kept: BTreeSet<u32> = oleps::property_ids(after).into_iter().collect();
    before
        .map(oleps::property_ids)
        .unwrap_or_default()
        .into_iter()
        .filter(|id| !kept.contains(id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn count_model(before: &V8Model, after: &V8Model, report: &mut StripSeedReport) {
    let records = |pages: &[Page]| pages.iter().map(|p| p.elements.len() as u64).sum::<u64>();
    let types = |pages: &[Page]| {
        let mut out: BTreeMap<u16, u64> = BTreeMap::new();
        for raw in pages.iter().flat_map(|p| &p.elements) {
            *out.entry(raw.type_code()).or_default() += 1;
        }
        out
    };
    let aux = |m: &V8Model| {
        m.graphic_aux
            .iter()
            .chain(&m.control_aux)
            .map(|p| p.records.len() as u64)
            .sum::<u64>()
    };
    report.removed_graphic_records =
        records(&before.graphic_pages).saturating_sub(records(&after.graphic_pages));
    report.kept_control_records = records(&after.control_pages);
    report.removed_control_records =
        records(&before.control_pages).saturating_sub(report.kept_control_records);
    report.kept_control_types = types(&after.control_pages);
    let mut removed = types(&before.control_pages);
    for (t, n) in &report.kept_control_types {
        if let Some(count) = removed.get_mut(t) {
            *count = count.saturating_sub(*n);
        }
    }
    removed.retain(|_, n| *n > 0);
    report.removed_control_types = removed;
    report.removed_auxiliary_records = aux(before).saturating_sub(aux(after));
    let kept = |pages: &[Page], t: u16| {
        pages
            .iter()
            .flat_map(|p| &p.elements)
            .filter(|r| r.type_code() == t && !r.is_complex_component())
            .count() as u64
    };
    report.kept_raster_attachments = kept(&after.graphic_pages, 94);
    report.raster_file_references = kept(&after.control_pages, 90);
}

fn count_dictionary(seed: &V8Seed, read: &ReadOptions, report: &mut StripSeedReport) {
    let enc = crate::text::encoding_for(read.fallback_codepage.as_deref());
    for raw in seed.native.named_pages.iter().flat_map(|p| &p.elements) {
        report.dictionary_records += 1;
        if raw.type_code() == SHARED_CELL_DEFINITION && !raw.is_complex_component() {
            report.cell_definitions += 1;
            continue;
        }
        if !matches!(raw.type_code(), 39 | 95 | 96) {
            continue;
        }
        match decode_v8::decode_with(raw, enc, &read.limits).data {
            ElementData::Level(level) => {
                report.levels += 1;
                report.named_levels += u64::from(level.name.is_some_and(|n| !n.is_empty()));
            }
            ElementData::Font { .. } => report.fonts += 1,
            ElementData::TagSet(_) => report.tag_sets += 1,
            _ => {}
        }
    }
}

fn is_dictionary_page(key: &str) -> bool {
    key.starts_with("Dgn^Nm/") || key.starts_with("Dgn^NmA/")
}

/// Streams cadkit decodes or writes: the file header, the model index, model storages, the
/// dictionary and the two property sets written here.
fn is_decoded_stream(key: &str) -> bool {
    key == "Dgn~H"
        || key == "Dgn^Ix/Dgn~Mix"
        || key == "\u{5}SummaryInformation"
        || key == "\u{5}DocumentSummaryInformation"
        || is_dictionary_page(key)
        || key.strip_prefix("Dgn-Md/").is_some_and(|rest| {
            rest.split_once('/').is_some_and(|(_, leaf)| {
                leaf == "Dgn~Mh"
                    || ["Dgn^G/", "Dgn^C/", "Dgn^GA/", "Dgn^CA/"]
                        .iter()
                        .any(|p| leaf.starts_with(p))
            })
        })
}
