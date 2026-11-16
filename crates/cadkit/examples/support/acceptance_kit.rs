//! Acceptance kit for an external DGN application: small DGN V8 files written into a
//! caller-supplied public seed, each exercising one thing a person must confirm by hand, plus
//! a manifest (`MANIFEST.json`, `README.md`). Every file is verified with cadkit before it is
//! written: it reads back equal to what was written, `check_structure` reports no warning and
//! the `cfb` crate opens it in strict mode. Nothing here has been opened in another
//! application; the protocol is `docs/dgn/ACCEPTANCE.md`.
//!
//! Shared by `examples/interchange.rs` and `tests/acceptance_kit.rs`. All names, texts, tag
//! values and coordinates are invented.

use cadkit::dgn::{SeedPrepareOptions, StripSeedOptions, StructureReport, V8Seed, WriteOptions};
use cadkit::{
    Attribute, AttributeDisplay, Document, Entity, EntityKind, GroupKind, HAlign, Layer, Model,
    Point3, ReadOptions, VAlign, Value, Vec3, Vertex,
};
use serde_json::{Value as Json, json};
use std::{
    error::Error,
    f64::consts::{FRAC_PI_2, FRAC_PI_6, PI, TAU},
    fmt::Write as _,
    path::Path,
};

type KitResult<T> = Result<T, Box<dyn Error>>;

/// Time of every new record (2026-11-15T00:00:00Z), fixed so that the kit is reproducible.
const WRITE_TIME_MS: f64 = 1_794_700_800_000.0;
/// Name of the synthetic image the raster item references beside the DGN file.
const RASTER_FILE: &str = "kit-raster.tif";
/// Pixel size of that image.
const RASTER_SIZE: [u32; 2] = [64, 32];

/// One file of the kit.
pub struct KitFile {
    /// File name inside the output directory.
    pub name: String,
    /// Size in bytes.
    pub bytes: usize,
}

/// What [`generate`] wrote.
pub struct KitSummary {
    /// The files in kit order, the manifest files last.
    pub files: Vec<KitFile>,
}

/// An entity of a kit document with the sentence the manifest shows for it.
struct Spec {
    label: &'static str,
    entity: Entity,
    /// Colour index the record must store when the entity names none itself.
    written_index: Option<i64>,
}

fn spec(label: &'static str, entity: Entity) -> Spec {
    Spec {
        label,
        entity,
        written_index: None,
    }
}

/// Collects the manifest while the files are written.
struct Kit<'a> {
    dir: &'a Path,
    items: Vec<Json>,
    files: Vec<KitFile>,
    is_3d: bool,
    units: cadkit::Units,
}

/// Writes the kit for `seed` (a DGN V8 file; `seed_name` is the file name the manifest
/// shows) into `dir` and returns the files written.
///
/// # Errors
///
/// Fails when a write fails or a written file does not verify; the message names the file
/// and the check.
pub fn generate(seed: &[u8], seed_name: &str, dir: &Path) -> KitResult<KitSummary> {
    let read = ReadOptions::default();
    let parsed = V8Seed::parse(seed, &read).map_err(|e| format!("seed: not usable: {e}"))?;
    let seed_structure = cadkit::dgn::check_structure(seed, &read)?;
    let seed_doc = cadkit::read(seed)?;
    std::fs::create_dir_all(dir)?;
    let mut kit = Kit {
        dir,
        items: vec![],
        files: vec![],
        is_3d: parsed.is_3d(),
        units: parsed.units(),
    };

    repacked(&mut kit, seed, seed_name, &seed_doc)?;
    let stripped = stripped(&mut kit, seed, seed_name)?;
    let z = if kit.is_3d { 5.0 } else { 0.0 };
    kit.write(
        "03-stripped-with-geometry.dgn",
        "New geometry, a text and a tag written into the stripped seed (02), without clear_seed_model.",
        "02-seed-stripped.dgn",
        &stripped,
        stripped_geometry(),
        options(false, None),
        &read,
        &[
            "Opens without an error or repair prompt.",
            "Level 'Kit Stripped' holds a closed square (10 x 10 at the origin), its diagonal line and the text 'Stripped seed'.",
            "The square carries tag set 'Kit Stripped Tags' with tag 'Note' = 'from the stripped seed' (hidden: visible in the tag dialog only).",
            "Save in the application and send the saved copy back.",
        ],
    )?;
    kit.write(
        "04-geometry.dgn",
        "Every supported geometry kind on three new levels with distinct colour indexes.",
        seed_name,
        seed,
        geometry(z),
        options(true, None),
        &read,
        &[
            "Opens without an error or repair prompt.",
            "Levels 'Kit Lines', 'Kit Curves' and 'Kit Groups' exist; every element is on the level the manifest names and is drawn in the colour of its colour index (manifest: colour_index and the RGB cadkit resolves).",
            "Element types: lines and a line string (Kit Lines); a circle, an arc and an ellipse (Kit Curves); a complex chain, a closed shape, a cell 'KITCELL' and a shape with a hole (Kit Groups).",
            "The holed shape is an unnamed cell of two shapes, the inner one flagged as a hole: record whether the application shows a hole (with fill on, the inner area stays empty) or two plain shapes.",
            "In a 3D seed: the line 'rising' climbs from Z 0 to Z 5.",
            "Save in the application and send the saved copy back.",
        ],
    )?;
    kit.write(
        "05-text-utf16.dgn",
        "Left / centre / right text, Turkish characters and rotated text in UTF-16 (the writer's default).",
        seed_name,
        seed,
        texts(),
        options(true, None),
        &read,
        &[
            "Opens without an error or repair prompt.",
            "Every text reads exactly as in the manifest, Turkish letters included (Ç Ğ İ Ö Ş Ü ç ğ ı ö ş ü).",
            "Each text's justification point lies on the small red cross drawn on level 'Kit Text Anchors' (left-baseline, centre-middle, right-top, rotated left-baseline).",
            "The rotated text runs at 30 degrees counter-clockwise.",
            "Centre and right texts use an invented text length (dgn.text_length); a small offset from the cross along the baseline is expected when the application's font is wider or narrower. Record it.",
            "Save in the application and send the saved copy back.",
        ],
    )?;
    let cp1254 = ReadOptions::default().with_fallback_codepage("windows-1254");
    kit.write(
        "06-text-cp1254.dgn",
        "The texts of 05 and a displayed Turkish tag written with codepage windows-1254 (8-bit text with the code page linkage).",
        seed_name,
        seed,
        texts_cp1254(),
        options(true, Some("windows-1254")),
        &cp1254,
        &[
            "Opens without an error or repair prompt.",
            "Every text reads exactly as in 05, Turkish letters included.",
            "The square on 'Kit Text Tags' displays tag 'Açıklama' = 'Işık şiddeti ölçümü' (set 'Kit Türkçe'); record whether the letters are correct.",
            "cadkit reads the 8-bit tag value correctly only with --codepage windows-1254; note in the results whether the application needed a code page setting.",
            "Save in the application and send the saved copy back.",
        ],
    )?;
    kit.write(
        "07-tags.dgn",
        "One tag set with text, integer and double tags: displayed unrotated, displayed at 30 degrees and hidden, on their own level.",
        seed_name,
        seed,
        tags(),
        options(true, None),
        &read,
        &[
            "Opens without an error or repair prompt.",
            "Tag set 'Kit Tag Set' exists with tags 'Label' (text), 'Count' (integer) and 'Length' (double).",
            "Square A shows its three values unrotated above it; square B shows them rotated 30 degrees; square C shows none, but its values are listed in the application's tag dialog.",
            "Every tag element is on level 'Kit Tags'; the squares are on 'Kit Tag Owners'. Switching 'Kit Tags' off hides the displayed values only.",
            "Values: see the manifest (text, integer and double exactly; doubles to the shown digits).",
            "Save in the application and send the saved copy back.",
        ],
    )?;
    kit.write(
        "08-hidden.dgn",
        "A hidden graphic element (property bit 0x0080) next to a visible one.",
        seed_name,
        seed,
        hidden(),
        options(true, None),
        &read,
        &[
            "Opens without an error or repair prompt.",
            "The line under the label 'VISIBLE' is drawn; nothing is drawn under the label 'HIDDEN'.",
            "Record whether the hidden line can be found at all (select all, element information) and what the application reports for it.",
            "Save in the application and send the saved copy back; record whether the line is still hidden.",
        ],
    )?;
    if kit.is_3d {
        kit.write(
            "09-plan-in-3d.dgn",
            "A 2D document (every Z = 0) promoted to the 3D seed by V8Seed::prepare_document and written into it.",
            seed_name,
            seed,
            plan(),
            options(true, None).with_document_2d(),
            &read,
            &[
                "Opens without an error or repair prompt.",
                "A square, a circle and the text 'Plan in 3D' lie at Z = 0 on level 'Kit Plan'; a side view shows them flat.",
                "Save in the application and send the saved copy back.",
            ],
        )?;
    } else {
        kit.skip(
            "09-plan-in-3d.dgn",
            "The seed is 2D; a 2D document into a 3D model needs a 3D seed.",
        );
    }
    kit.write(
        "10-large-coordinates.dgn",
        "Geometry far from the origin (X 4,500,000, Y 4,400,000 master units).",
        seed_name,
        seed,
        large(z),
        options(true, None),
        &read,
        &[
            "Opens without an error or repair prompt.",
            "'Fit view' shows a 10 x 10 square, its circle and the text 'Far away' on level 'Kit Far'; the coordinate readout at the square's lower-left corner is X 4,500,000, Y 4,400,000 (master units of the seed).",
            "Record any warning about the design plane or coordinate range.",
            "Save in the application and send the saved copy back.",
        ],
    )?;
    write_tiff(dir)?;
    kit.write(
        "11-raster.dgn",
        "A new raster attachment naming 'kit-raster.tif' beside the DGN file (no folder stored), framed by an outline.",
        seed_name,
        seed,
        raster(),
        options(true, None),
        &read,
        &[
            "Keep kit-raster.tif in the same folder as 11-raster.dgn.",
            "Opens without an error or repair prompt; the raster manager lists one attachment 'kit-raster.tif'.",
            "Record whether the application found the image from the DGN's folder without asking (cadkit stores an empty full path for a bare file name).",
            "The image (64 x 32 pixels, 1 bit) fills the outline on level 'Kit Raster Frame' exactly: its black border on the outline, the solid black square in the top-left corner.",
            "Save in the application and send the saved copy back; record whether the attachment path changed.",
        ],
    )?;
    kit.add_file(RASTER_FILE)?;
    let step1 = kit.write(
        "12-chain-step1.dgn",
        "First write of a chain: a new level and a tag set written into the seed.",
        seed_name,
        seed,
        chain_step1(),
        options(true, None),
        &read,
        &[
            "Opens without an error or repair prompt.",
            "Level 'Kit Chain' holds a line with tag 'Step' = 1 of set 'Kit Chain Tags' (hidden).",
        ],
    )?;
    let step1_levels = V8Seed::parse(&step1, &read)?.levels().len();
    let step2 = kit.write(
        "13-chain-step2.dgn",
        "Second write of the chain: 12 used as the seed (clear_seed_model), its level and tag set reused, one new level.",
        "12-chain-step1.dgn",
        &step1,
        chain_step2(),
        options(true, None),
        &read,
        &[
            "Opens without an error or repair prompt.",
            "Level 'Kit Chain' (from step 1) holds a circle with tag 'Step' = 2; level 'Kit Chain Two' holds a line with 'Step' = 3. Step 1's line is gone.",
            "The level list holds 'Kit Chain' once and the tag set 'Kit Chain Tags' once.",
            "Save in the application and send the saved copy back.",
        ],
    )?;
    let step2_levels = V8Seed::parse(&step2, &read)?.levels().len();
    if step2_levels != step1_levels + 1 {
        return Err(format!(
            "13-chain-step2.dgn: {step2_levels} levels, expected the {step1_levels} of step 1 plus one"
        )
        .into());
    }
    let sets = tag_set_records(&step2)?;
    if sets != 1 {
        return Err(format!("13-chain-step2.dgn: {sets} tag set records, expected 1").into());
    }
    kit.write(
        "14-bylayer.dgn",
        "ByLayer colour: an entity without a colour index on a level whose document layer has one, and an element storing the by-level colour value 0xFFFFFFFF.",
        seed_name,
        seed,
        bylayer(),
        options(true, None).with_layers(vec![
            Layer::new("Kit ByLayer").with_prop("dgn.color_index", BYLAYER_INDEX),
        ]),
        &read,
        &[
            "Opens without an error or repair prompt.",
            "Record the colour the level manager shows for level 'Kit ByLayer' (cadkit writes no level colour: a new level copies the seed's first level entry; manifest: read_back_color).",
            "The line labelled 'materialized' is drawn in colour index 6 and its element information shows colour 6, not 'by level'.",
            "The line labelled 'by level' is drawn in the level's colour and its element information shows 'by level' (or the application's equivalent).",
            "Change the level's colour in the application: record which of the two lines follows.",
            "Save (after undoing the level colour change) and send the saved copy back.",
        ],
    )?;
    citygml(&mut kit)?;

    let manifest = json!({
        "kit": "cadkit DGN acceptance kit",
        "cadkit_version": env!("CARGO_PKG_VERSION"),
        "generator": "crates/cadkit/examples/interchange.rs",
        "protocol": "docs/dgn/ACCEPTANCE.md",
        "external_acceptance": "none claimed: no file of this kit has been opened in another DGN application by cadkit",
        "record_time_ms": WRITE_TIME_MS,
        "seed": {
            "file": seed_name,
            "bytes": seed.len(),
            "is_3d": kit.is_3d,
            "units": format!("{:?}", kit.units.unit),
            "meters_per_unit": kit.units.meters_per_unit,
            "levels": parsed.levels().len(),
            "structure": structure_summary(&seed_structure),
        },
        "items": std::mem::take(&mut kit.items),
    });
    std::fs::write(
        dir.join("MANIFEST.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    kit.add_file("MANIFEST.json")?;
    std::fs::write(dir.join("README.md"), readme(&manifest))?;
    kit.add_file("README.md")?;
    Ok(KitSummary { files: kit.files })
}

impl Kit<'_> {
    fn add_file(&mut self, name: &str) -> KitResult<()> {
        let bytes = std::fs::metadata(self.dir.join(name))?.len();
        self.files.push(KitFile {
            name: name.into(),
            bytes: usize::try_from(bytes)?,
        });
        Ok(())
    }

    fn skip(&mut self, file: &str, reason: &str) {
        self.items.push(json!({ "file": file, "skipped": reason }));
    }

    /// Writes `specs` into `seed`, verifies the output and records it.
    #[allow(clippy::too_many_arguments)]
    fn write(
        &mut self,
        file: &str,
        purpose: &str,
        seed_label: &str,
        seed: &[u8],
        specs: Vec<Spec>,
        options: KitOptions,
        read: &ReadOptions,
        checks: &[&str],
    ) -> KitResult<Vec<u8>> {
        let mut doc = Document::default();
        doc.units = self.units;
        doc.layers.clone_from(&options.layers);
        let is_3d = self.is_3d && !options.document_2d;
        let entities: Vec<Entity> = specs.iter().map(|s| s.entity.clone()).collect();
        doc.models
            .push(Model::default().with_is_3d(is_3d).with_entities(entities));
        if options.document_2d {
            // write_v8 requires equal dimensions; a host promotes a plan to a 3D seed with
            // prepare_document (DimensionPolicy::Promote2dTo3d, the default). Normalisation
            // is off so the entities are written as given.
            let prepare = SeedPrepareOptions::new().with_normalize_for_dgn(false);
            doc = V8Seed::parse(seed, read)?.prepare_document(&doc, &prepare)?;
        }
        let bytes = cadkit::to_dgn(&doc, seed, &options.write)
            .map_err(|e| format!("{file}: write failed: {e}"))?;
        let (structure, back) = verify_container(file, &bytes, read)?;
        if !back.warnings.is_empty() {
            return Err(format!("{file}: read back with warnings: {:?}", codes(&back)).into());
        }
        let got = back
            .models
            .first()
            .map(|m| m.entities.as_slice())
            .unwrap_or_default();
        let wanted = doc
            .models
            .first()
            .map(|m| m.entities.as_slice())
            .unwrap_or_default();
        compare_all(wanted, got).map_err(|e| format!("{file}: read back differs: {e}"))?;
        for (i, (s, back)) in specs.iter().zip(got).enumerate() {
            let stored = back.props.get("dgn.color_index").and_then(Value::as_i64);
            if s.written_index.is_some() && stored != s.written_index {
                return Err(format!(
                    "{file}: entity {i} stores colour index {stored:?}, expected {:?}",
                    s.written_index
                )
                .into());
            }
        }
        std::fs::write(self.dir.join(file), &bytes)?;
        self.add_file(file)?;
        let described: Vec<Json> = specs
            .iter()
            .zip(got)
            .map(|(s, back)| describe(s, back))
            .collect();
        self.items.push(json!({
            "file": file,
            "purpose": purpose,
            "seed": seed_label,
            "write_options": {
                "clear_seed_model": options.write.clear_seed_model,
                "codepage": options.write.codepage,
                "timestamp_ms": options.write.timestamp_ms,
                "document_is_3d": is_3d,
                "prepared_with": options.document_2d.then_some(
                    "V8Seed::prepare_document (DimensionPolicy::Promote2dTo3d, normalize_for_dgn = false)"
                ),
            },
            "bytes": bytes.len(),
            "entities": described,
            "levels": levels(&specs, &back),
            "structure": structure_summary(&structure),
            "verified": verified(),
            "checks": checks,
        }));
        Ok(bytes)
    }
}

/// Write options of one kit item.
struct KitOptions {
    write: WriteOptions,
    document_2d: bool,
    layers: Vec<Layer>,
}

impl KitOptions {
    fn with_document_2d(mut self) -> Self {
        self.document_2d = true;
        self
    }

    fn with_layers(mut self, layers: Vec<Layer>) -> Self {
        self.layers = layers;
        self
    }
}

fn options(clear: bool, codepage: Option<&str>) -> KitOptions {
    KitOptions {
        write: WriteOptions::new()
            .with_clear_seed_model(clear)
            .with_timestamp_ms(WRITE_TIME_MS)
            .with_codepage(codepage),
        document_2d: false,
        layers: vec![],
    }
}

fn verified() -> Json {
    json!({
        "read_back_equal": true,
        "read_back_warnings": 0,
        "check_structure_clean": true,
        "cfb_open_strict": true,
    })
}

/// The container checks every DGN file of the kit passes; returns the structure report and
/// the read-back document.
fn verify_container(
    file: &str,
    bytes: &[u8],
    read: &ReadOptions,
) -> KitResult<(StructureReport, Document)> {
    let structure = cadkit::dgn::check_structure(bytes, read)
        .map_err(|e| format!("{file}: check_structure failed: {e}"))?;
    if !structure.is_clean() {
        let codes: Vec<&str> = structure.warnings.iter().map(|w| w.code.as_str()).collect();
        return Err(format!("{file}: check_structure warnings {codes:?}").into());
    }
    cfb::CompoundFile::open_strict(std::io::Cursor::new(bytes))
        .map_err(|e| format!("{file}: cfb strict open failed: {e}"))?;
    let back = cadkit::read_with(bytes, read).map_err(|e| format!("{file}: read failed: {e}"))?;
    Ok((structure, back))
}

fn codes(doc: &Document) -> Vec<&str> {
    doc.warnings.iter().map(|w| w.code.as_str()).collect()
}

fn structure_summary(r: &StructureReport) -> Json {
    json!({
        "cfb_version": r.cfb_version,
        "storages": r.storages,
        "red_black_trees": r.red_black_trees,
        "top_down_trees": r.top_down_trees,
        "irregular_free_entries": r.irregular_free_entries,
        "writer_slot_order": r.writer_slot_order,
        "pages": r.pages,
        "pages_with_number_or_population": r.pages_with_number_or_population,
        "model_records_without_time": r.model_records_without_time,
        "warnings": r.warnings.iter().map(|w| w.code.clone()).collect::<Vec<_>>(),
    })
}

/// 01: the seed in a fresh container, streams unchanged.
fn repacked(kit: &mut Kit<'_>, seed: &[u8], seed_name: &str, seed_doc: &Document) -> KitResult<()> {
    let file = "01-seed-repacked.dgn";
    let read = ReadOptions::default();
    let bytes = cadkit::dgn::repack_v8(seed, &read)?;
    let (structure, back) = verify_container(file, &bytes, &read)?;
    if back.models != seed_doc.models || back.layers != seed_doc.layers {
        return Err(format!("{file}: read back differs from the seed").into());
    }
    if codes(&back) != codes(seed_doc) {
        return Err(format!("{file}: read warnings differ from the seed's").into());
    }
    let entities: usize = back.models.iter().map(|m| m.entities.len()).sum();
    std::fs::write(kit.dir.join(file), &bytes)?;
    kit.add_file(file)?;
    kit.items.push(json!({
        "file": file,
        "purpose": "The public seed repacked into a fresh compound file; every stream keeps its bytes.",
        "seed": seed_name,
        "bytes": bytes.len(),
        "read_back": {
            "entities": entities,
            "layers": back.layers.len(),
            "reader_warnings_as_in_seed": codes(&back),
        },
        "structure": structure_summary(&structure),
        "verified": {
            "read_back_equal_to_seed": true,
            "check_structure_clean": true,
            "cfb_open_strict": true,
        },
        "checks": [
            "Opens without an error or repair prompt.",
            "Shows the same drawing as the seed itself (the public file fetched by scripts/fetch-corpus.py) opened side by side: same elements, levels and views.",
            "Save in the application and send the saved copy back.",
        ],
    }));
    Ok(())
}

/// 02: the seed without its drawing content and document properties.
fn stripped(kit: &mut Kit<'_>, seed: &[u8], seed_name: &str) -> KitResult<Vec<u8>> {
    let file = "02-seed-stripped.dgn";
    let read = ReadOptions::default();
    let out = cadkit::strip_dgn_seed(seed, &StripSeedOptions::new(), &read)?;
    let (structure, back) = verify_container(file, &out.bytes, &read)?;
    let entities: usize = back.models.iter().map(|m| m.entities.len()).sum();
    if entities != 0 || V8Seed::parse(&out.bytes, &read)?.is_nonempty() {
        return Err(format!("{file}: stripped seed still holds {entities} entities").into());
    }
    let again = cadkit::strip_dgn_seed(&out.bytes, &StripSeedOptions::new(), &read)?;
    if again.bytes != out.bytes {
        return Err(format!("{file}: stripping again changed the bytes").into());
    }
    std::fs::write(kit.dir.join(file), &out.bytes)?;
    kit.add_file(file)?;
    let r = &out.report;
    kit.items.push(json!({
        "file": file,
        "purpose": "The seed stripped with strip_seed_v8 (default options): no graphic or control record, minimal property sets, tables kept.",
        "seed": seed_name,
        "bytes": out.bytes.len(),
        "strip_report": {
            "removed_graphic_records": r.removed_graphic_records,
            "removed_control_records": r.removed_control_records,
            "removed_property_sets": r.removed_property_sets,
            "levels": r.levels,
            "named_levels": r.named_levels,
            "fonts": r.fonts,
            "tag_sets": r.tag_sets,
            "cell_definitions": r.cell_definitions,
            "unknown_streams": r.unknown_streams,
        },
        "structure": structure_summary(&structure),
        "verified": {
            "no_entities": true,
            "idempotent": true,
            "check_structure_clean": true,
            "cfb_open_strict": true,
        },
        "checks": [
            "Opens without an error or repair prompt as an empty drawing.",
            "The level, font and other tables of the seed are listed as before.",
            "Draw a line, save, and send the saved copy back: it shows which records the application adds to a stripped seed (open question: which removed records an application needs).",
        ],
    }));
    Ok(out.bytes)
}

fn citygml(kit: &mut Kit<'_>) -> KitResult<()> {
    let file = "synthetic.gml";
    let ring = square(0.0, 0.0, 10.0, 0.0);
    let hole = clockwise(square(2.0, 2.0, 1.0, 0.0));
    let mut doc = Document::default();
    doc.units = kit.units;
    doc.models.push(Model::default().with_entities(vec![
            Entity::new(EntityKind::Polygon {
                exterior: ring,
                interiors: vec![hole],
            })
            .on_layer("Synthetic surfaces"),
        ]));
    let gml = cadkit::to_citygml(
        &doc,
        &cadkit::gml::ExportOptions::new("https://example.invalid/crs/synthetic-local", 1)
            .with_limits(cadkit::Limits::default()),
    )?;
    std::fs::write(kit.dir.join(file), &gml)?;
    kit.add_file(file)?;
    kit.items.push(json!({
        "file": file,
        "purpose": "A synthetic CityGML 2.0 generic object (a polygon with a hole), for scripts/validate-citygml.py; not part of the DGN protocol.",
        "bytes": gml.len(),
        "checks": ["python3 scripts/validate-citygml.py <kit>/synthetic.gml"],
    }));
    Ok(())
}

/// Tag set records (type 39) in the dictionary of `bytes`.
fn tag_set_records(bytes: &[u8]) -> KitResult<usize> {
    let native = cadkit::dgn::native::v8::read(bytes, &ReadOptions::default())?;
    Ok(native
        .named_pages
        .iter()
        .flat_map(|p| &p.elements)
        .filter(|e| e.type_code() == 39)
        .count())
}

// Geometry. Every coordinate is in the seed's master units.

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z)
}

fn square(x: f64, y: f64, size: f64, z: f64) -> Vec<Point3> {
    vec![
        p(x, y, z),
        p(x + size, y, z),
        p(x + size, y + size, z),
        p(x, y + size, z),
        p(x, y, z),
    ]
}

/// A ring traversed the other way round; holes oppose their exterior ring.
fn clockwise(mut ring: Vec<Point3>) -> Vec<Point3> {
    ring.reverse();
    ring
}

fn shape(points: Vec<Point3>) -> Entity {
    Entity::new(EntityKind::Polyline {
        vertices: points.into_iter().map(Vertex::at).collect(),
        closed: true,
        normal: Vec3::Z,
    })
}

fn line_string(points: Vec<Point3>) -> Entity {
    Entity::new(EntityKind::Polyline {
        vertices: points.into_iter().map(Vertex::at).collect(),
        closed: false,
        normal: Vec3::Z,
    })
}

/// `entity` on `level` with the colour index `color`.
fn on(entity: Entity, level: &str, color: i64) -> Entity {
    entity.on_layer(level).with_prop("dgn.color_index", color)
}

fn text(at: Point3, value: &str, halign: HAlign, valign: VAlign, rotation: f64) -> Entity {
    let height = 1.0;
    let mut t = Entity::text(at, height, value);
    if let EntityKind::Text {
        halign: h,
        valign: v,
        rotation: r,
        ..
    } = &mut t.kind
    {
        *h = halign;
        *v = valign;
        *r = rotation;
    }
    // Centre and right anchors need the baseline advance; cadkit measures no font, so the
    // kit supplies an invented one (0.8 x height per character) and the checklist records
    // the offset an application's real font produces.
    let advance = 0.8 * height * value.chars().count() as f64;
    t.with_prop("dgn.text_length", advance)
}

/// Two short red lines crossing at `at`, marking a text's justification point.
fn cross(at: Point3) -> [Entity; 2] {
    let d = 0.3;
    [
        on(
            Entity::line(p(at.x - d, at.y, at.z), p(at.x + d, at.y, at.z)),
            "Kit Text Anchors",
            3,
        ),
        on(
            Entity::line(p(at.x, at.y - d, at.z), p(at.x, at.y + d, at.z)),
            "Kit Text Anchors",
            3,
        ),
    ]
}

fn stripped_geometry() -> Vec<Spec> {
    let mut owner = on(shape(square(0.0, 0.0, 10.0, 0.0)), "Kit Stripped", 1);
    owner.attributes.push(
        Attribute::new("Note", Value::Text("from the stripped seed".into()))
            .with_set("Kit Stripped Tags")
            .with_invisible(true),
    );
    vec![
        spec(
            "closed square 10 x 10 at the origin, hidden tag Note",
            owner,
        ),
        spec(
            "diagonal line of the square",
            on(
                Entity::line(p(0.0, 0.0, 0.0), p(10.0, 10.0, 0.0)),
                "Kit Stripped",
                1,
            ),
        ),
        spec(
            "text 'Stripped seed', left-baseline",
            on(
                text(
                    p(0.0, 11.0, 0.0),
                    "Stripped seed",
                    HAlign::Left,
                    VAlign::Baseline,
                    0.0,
                ),
                "Kit Stripped",
                1,
            ),
        ),
    ]
}

fn geometry(z: f64) -> Vec<Spec> {
    let lines = |e| on(e, "Kit Lines", 1);
    let curves = |e| on(e, "Kit Curves", 2);
    let groups = |e| on(e, "Kit Groups", 4);
    let chain = Entity::new(EntityKind::Group {
        group_kind: GroupKind::ComplexChain,
        name: None,
        origin: None,
        children: vec![
            groups(Entity::line(p(0.0, 30.0, 0.0), p(5.0, 30.0, 0.0))),
            groups(Entity::arc(p(5.0, 32.0, 0.0), 2.0, -FRAC_PI_2, FRAC_PI_2)),
            groups(Entity::line(p(5.0, 34.0, 0.0), p(0.0, 34.0, 0.0))),
        ],
    });
    let cell = Entity::new(EntityKind::Group {
        group_kind: GroupKind::Cell,
        name: Some("KITCELL".into()),
        origin: Some(p(25.0, 30.0, 0.0)),
        children: vec![
            groups(shape(square(20.0, 25.0, 10.0, 0.0))),
            groups(Entity::circle(p(25.0, 30.0, 0.0), 3.0)),
        ],
    });
    let holed = Entity::new(EntityKind::Polygon {
        exterior: square(35.0, 25.0, 10.0, 0.0),
        interiors: vec![clockwise(square(38.0, 28.0, 4.0, 0.0))],
    });
    let mut specs = vec![
        spec(
            "line (0,0)-(10,0)",
            lines(Entity::line(p(0.0, 0.0, 0.0), p(10.0, 0.0, 0.0))),
        ),
        spec(
            "line (0,2)-(10,5)",
            lines(Entity::line(p(0.0, 2.0, 0.0), p(10.0, 5.0, 0.0))),
        ),
        spec(
            "line string of four points (zigzag)",
            lines(line_string(vec![
                p(12.0, 0.0, 0.0),
                p(15.0, 5.0, 0.0),
                p(18.0, 0.0, 0.0),
                p(21.0, 5.0, 0.0),
            ])),
        ),
        spec(
            "circle, centre (5,15), radius 4",
            curves(Entity::circle(p(5.0, 15.0, 0.0), 4.0)),
        ),
        spec(
            "arc, centre (15,15), radius 4, from 0 to 90 degrees",
            curves(Entity::arc(p(15.0, 15.0, 0.0), 4.0, 0.0, FRAC_PI_2)),
        ),
        spec(
            "ellipse, centre (27,15), semi-axes 5 and 2.5, major axis along X",
            curves(Entity::new(EntityKind::Ellipse {
                center: p(27.0, 15.0, 0.0),
                major_axis: Vec3::new(5.0, 0.0, 0.0),
                ratio: 0.5,
                start_param: 0.0,
                end_param: TAU,
                normal: Vec3::Z,
            })),
        ),
        spec(
            "complex chain: line, half-circle arc, line (a 'U' turned right)",
            groups(chain),
        ),
        spec(
            "closed shape (triangle)",
            groups(shape(vec![
                p(10.0, 25.0, 0.0),
                p(16.0, 25.0, 0.0),
                p(13.0, 31.0, 0.0),
                p(10.0, 25.0, 0.0),
            ])),
        ),
        spec(
            "cell 'KITCELL': square 10 x 10 with an inscribed circle",
            groups(cell),
        ),
        spec(
            "shape 10 x 10 with a 4 x 4 hole in its middle",
            groups(holed),
        ),
    ];
    if z != 0.0 {
        specs.push(spec(
            "line 'rising' from (0,40,0) to (10,40,5)",
            lines(Entity::line(p(0.0, 40.0, 0.0), p(10.0, 40.0, z))),
        ));
    }
    specs
}

const TEXT_LEFT: &str = "Sol taban ÇĞİÖŞÜ";
const TEXT_CENTRE: &str = "Orta çğıöşü";
const TEXT_RIGHT: &str = "Sağ üst İğne";
const TEXT_ROTATED: &str = "Döndürülmüş 30°";

fn texts() -> Vec<Spec> {
    let items = [
        (
            "text left-baseline",
            p(0.0, 0.0, 0.0),
            TEXT_LEFT,
            HAlign::Left,
            VAlign::Baseline,
            0.0,
        ),
        (
            "text centre-middle",
            p(10.0, 5.0, 0.0),
            TEXT_CENTRE,
            HAlign::Center,
            VAlign::Middle,
            0.0,
        ),
        (
            "text right-top",
            p(20.0, 10.0, 0.0),
            TEXT_RIGHT,
            HAlign::Right,
            VAlign::Top,
            0.0,
        ),
        (
            "text left-baseline rotated 30 degrees",
            p(0.0, 15.0, 0.0),
            TEXT_ROTATED,
            HAlign::Left,
            VAlign::Baseline,
            FRAC_PI_6,
        ),
    ];
    let mut specs = vec![];
    for (label, at, value, h, v, r) in items {
        specs.push(spec(label, on(text(at, value, h, v, r), "Kit Texts", 0)));
        for c in cross(at) {
            specs.push(spec("anchor cross line", c));
        }
    }
    specs
}

fn texts_cp1254() -> Vec<Spec> {
    let mut specs = texts();
    let mut owner = on(shape(square(0.0, 25.0, 5.0, 0.0)), "Kit Text Tags", 2);
    owner.attributes.push(
        Attribute::new("Açıklama", Value::Text("Işık şiddeti ölçümü".into()))
            .with_set("Kit Türkçe")
            .with_position(p(0.0, 31.0, 0.0))
            .with_display(AttributeDisplay::new(0.8, 0.8)),
    );
    specs.push(spec("square 5 x 5 with displayed tag 'Açıklama'", owner));
    specs
}

/// The three tags of 'Kit Tag Set' on one owner.
fn tag_values(
    owner: &mut Entity,
    label: &str,
    count: i64,
    length: f64,
    display: Option<(Point3, f64)>,
) {
    let values = [
        ("Label", Value::Text(label.into())),
        ("Count", Value::Int(count)),
        ("Length", Value::Float(length)),
    ];
    for (i, (tag, value)) in values.into_iter().enumerate() {
        let mut a = Attribute::new(tag, value)
            .with_set("Kit Tag Set")
            .with_layer("Kit Tags")
            .with_prop("dgn.color_index", 5i64);
        a = match display {
            Some((at, rotation)) => {
                // Lines 1.2 units apart, perpendicular to the text direction.
                let step = 1.2 * i as f64;
                let pos = p(
                    at.x - step * rotation.sin(),
                    at.y + step * rotation.cos(),
                    at.z,
                );
                a.with_position(pos)
                    .with_display(AttributeDisplay::new(0.8, 0.8).with_rotation(rotation))
            }
            None => a.with_invisible(true),
        };
        owner.attributes.push(a);
    }
}

fn tags() -> Vec<Spec> {
    let owner = |x: f64| on(shape(square(x, 0.0, 6.0, 0.0)), "Kit Tag Owners", 1);
    let mut a = owner(0.0);
    tag_values(&mut a, "Kapı A", 42, 12.5, Some((p(0.0, 7.0, 0.0), 0.0)));
    let mut b = owner(15.0);
    tag_values(
        &mut b,
        "Pencere B",
        -7,
        0.25,
        Some((p(15.0, 7.0, 0.0), FRAC_PI_6)),
    );
    let mut c = owner(30.0);
    tag_values(&mut c, "Gizli C", 2_147_483_647, 1234.125, None);
    vec![
        spec("square A with three displayed tags, unrotated", a),
        spec("square B with three displayed tags rotated 30 degrees", b),
        spec("square C with three hidden tags", c),
    ]
}

fn hidden() -> Vec<Spec> {
    let level = "Kit Visibility";
    vec![
        spec(
            "visible line (0,0)-(10,0)",
            on(Entity::line(p(0.0, 0.0, 0.0), p(10.0, 0.0, 0.0)), level, 1),
        ),
        spec(
            "hidden line (15,0)-(25,0), visible = false",
            on(Entity::line(p(15.0, 0.0, 0.0), p(25.0, 0.0, 0.0)), level, 1).with_visible(false),
        ),
        spec(
            "label 'VISIBLE' above the visible line",
            on(
                text(
                    p(0.0, 1.0, 0.0),
                    "VISIBLE",
                    HAlign::Left,
                    VAlign::Baseline,
                    0.0,
                ),
                level,
                0,
            ),
        ),
        spec(
            "label 'HIDDEN' above the hidden line",
            on(
                text(
                    p(15.0, 1.0, 0.0),
                    "HIDDEN",
                    HAlign::Left,
                    VAlign::Baseline,
                    0.0,
                ),
                level,
                0,
            ),
        ),
    ]
}

fn plan() -> Vec<Spec> {
    let level = "Kit Plan";
    vec![
        spec(
            "square 10 x 10 at the origin",
            on(shape(square(0.0, 0.0, 10.0, 0.0)), level, 2),
        ),
        spec(
            "circle, centre (5,5), radius 3",
            on(Entity::circle(p(5.0, 5.0, 0.0), 3.0), level, 2),
        ),
        spec(
            "text 'Plan in 3D'",
            on(
                text(
                    p(0.0, 11.0, 0.0),
                    "Plan in 3D",
                    HAlign::Left,
                    VAlign::Baseline,
                    0.0,
                ),
                level,
                0,
            ),
        ),
    ]
}

fn large(z: f64) -> Vec<Spec> {
    let (x, y) = (4_500_000.0, 4_400_000.0);
    let level = "Kit Far";
    vec![
        spec(
            "square 10 x 10 with its lower-left corner at (4500000, 4400000)",
            on(shape(square(x, y, 10.0, z)), level, 4),
        ),
        spec(
            "circle inscribed in the square",
            on(Entity::circle(p(x + 5.0, y + 5.0, z), 5.0), level, 4),
        ),
        spec(
            "text 'Far away' above the square",
            on(
                text(
                    p(x, y + 11.0, z),
                    "Far away",
                    HAlign::Left,
                    VAlign::Baseline,
                    0.0,
                ),
                level,
                0,
            ),
        ),
    ]
}

fn raster() -> Vec<Spec> {
    // 0.5 master units per pixel; the outline has the frame's corners.
    let (w, h) = (
        f64::from(RASTER_SIZE[0]) * 0.5,
        f64::from(RASTER_SIZE[1]) * 0.5,
    );
    let image = Entity::new(EntityKind::Image {
        path: Some(RASTER_FILE.into()),
        position: p(0.0, 0.0, 0.0),
        u_vector: Vec3::new(w, 0.0, 0.0),
        v_vector: Vec3::new(0.0, h, 0.0),
        size_px: Some(RASTER_SIZE),
    })
    .on_layer("Kit Raster");
    let outline = on(
        shape(vec![
            p(0.0, 0.0, 0.0),
            p(w, 0.0, 0.0),
            p(w, h, 0.0),
            p(0.0, h, 0.0),
            p(0.0, 0.0, 0.0),
        ]),
        "Kit Raster Frame",
        3,
    );
    vec![
        spec(
            "raster attachment 'kit-raster.tif', 32 x 16 units at the origin",
            image,
        ),
        spec("outline of the raster frame", outline),
    ]
}

fn chain_tag(entity: Entity, step: i64) -> Entity {
    let mut e = entity;
    e.attributes.push(
        Attribute::new("Step", Value::Int(step))
            .with_set("Kit Chain Tags")
            .with_invisible(true),
    );
    e
}

fn chain_step1() -> Vec<Spec> {
    vec![spec(
        "line (0,0)-(10,0) with hidden tag Step = 1",
        chain_tag(
            on(
                Entity::line(p(0.0, 0.0, 0.0), p(10.0, 0.0, 0.0)),
                "Kit Chain",
                1,
            ),
            1,
        ),
    )]
}

fn chain_step2() -> Vec<Spec> {
    vec![
        spec(
            "circle, centre (5,5), radius 3, with hidden tag Step = 2",
            chain_tag(on(Entity::circle(p(5.0, 5.0, 0.0), 3.0), "Kit Chain", 1), 2),
        ),
        spec(
            "line (0,10)-(10,10) on the new level, hidden tag Step = 3",
            chain_tag(
                on(
                    Entity::line(p(0.0, 10.0, 0.0), p(10.0, 10.0, 0.0)),
                    "Kit Chain Two",
                    2,
                ),
                3,
            ),
        ),
    ]
}

/// Colour index of the document layer 'Kit ByLayer'.
const BYLAYER_INDEX: i64 = 6;
/// The by-level colour value an element stores (FORMAT_NOTES, "Level sentinels").
const BY_LEVEL: i64 = 0xFFFF_FFFF;

fn bylayer() -> Vec<Spec> {
    let level = "Kit ByLayer";
    let mut materialized = spec(
        "line 'materialized': ByLayer without its own index, written with the layer's index 6",
        Entity::line(p(0.0, 0.0, 0.0), p(10.0, 0.0, 0.0)).on_layer(level),
    );
    materialized.written_index = Some(BYLAYER_INDEX);
    vec![
        materialized,
        spec(
            "line 'by level': stores the by-level colour value 0xFFFFFFFF",
            on(
                Entity::line(p(0.0, 3.0, 0.0), p(10.0, 3.0, 0.0)),
                level,
                BY_LEVEL,
            ),
        ),
        spec(
            "label 'materialized'",
            on(
                text(
                    p(11.0, 0.0, 0.0),
                    "materialized",
                    HAlign::Left,
                    VAlign::Baseline,
                    0.0,
                ),
                level,
                0,
            ),
        ),
        spec(
            "label 'by level'",
            on(
                text(
                    p(11.0, 3.0, 0.0),
                    "by level",
                    HAlign::Left,
                    VAlign::Baseline,
                    0.0,
                ),
                level,
                0,
            ),
        ),
    ]
}

// Read-back comparison.

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0)
}

fn close_point(a: Point3, b: Point3) -> bool {
    close(a.x, b.x) && close(a.y, b.y) && close(a.z, b.z)
}

fn close_vec(a: Vec3, b: Vec3) -> bool {
    close(a.x, b.x) && close(a.y, b.y) && close(a.z, b.z)
}

fn close_angle(a: f64, b: f64) -> bool {
    let d = (a - b).rem_euclid(TAU);
    d.min(TAU - d) <= 1e-9
}

fn compare_all(wanted: &[Entity], got: &[Entity]) -> Result<(), String> {
    if wanted.len() != got.len() {
        return Err(format!("{} entities, expected {}", got.len(), wanted.len()));
    }
    for (i, (w, g)) in wanted.iter().zip(got).enumerate() {
        compare(w, g).map_err(|e| format!("entity {i} ({}): {e}", w.kind.type_name()))?;
    }
    Ok(())
}

fn compare(w: &Entity, g: &Entity) -> Result<(), String> {
    if w.layer != g.layer {
        return Err(format!("level {:?}, expected {:?}", g.layer, w.layer));
    }
    if w.visible != g.visible {
        return Err(format!("visible {}, expected {}", g.visible, w.visible));
    }
    if let Some(index) = w.props.get("dgn.color_index")
        && g.props.get("dgn.color_index") != Some(index)
    {
        return Err(format!(
            "colour index {:?}, expected {index:?}",
            g.props.get("dgn.color_index")
        ));
    }
    compare_kind(&w.kind, &g.kind)?;
    compare_attributes(w, g)
}

fn compare_kind(w: &EntityKind, g: &EntityKind) -> Result<(), String> {
    let ok = match (w, g) {
        (EntityKind::Line { start, end }, EntityKind::Line { start: s, end: e }) => {
            close_point(*start, *s) && close_point(*end, *e)
        }
        (
            EntityKind::Polyline {
                vertices, closed, ..
            },
            EntityKind::Polyline {
                vertices: v,
                closed: c,
                ..
            },
        ) => {
            closed == c
                && same_points(
                    vertices.iter().map(|x| x.position),
                    v.iter().map(|x| x.position),
                )
        }
        (
            EntityKind::Circle { center, radius, .. },
            EntityKind::Circle {
                center: c,
                radius: r,
                ..
            },
        ) => close_point(*center, *c) && close(*radius, *r),
        (
            EntityKind::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                ..
            },
            EntityKind::Arc {
                center: c,
                radius: r,
                start_angle: s,
                end_angle: e,
                ..
            },
        ) => {
            close_point(*center, *c)
                && close(*radius, *r)
                && close_angle(*start_angle, *s)
                && close_angle(*end_angle, *e)
        }
        (
            EntityKind::Ellipse {
                center,
                major_axis,
                ratio,
                start_param,
                end_param,
                ..
            },
            EntityKind::Ellipse {
                center: c,
                major_axis: m,
                ratio: r,
                start_param: s,
                end_param: e,
                ..
            },
        ) => {
            close_point(*center, *c)
                && close_vec(*major_axis, *m)
                && close(*ratio, *r)
                && close(*start_param, *s)
                && close(*end_param, *e)
        }
        (
            EntityKind::Text {
                position,
                height,
                rotation,
                value,
                halign,
                valign,
                ..
            },
            EntityKind::Text {
                position: pos,
                height: h,
                rotation: r,
                value: v,
                halign: ha,
                valign: va,
                ..
            },
        ) => {
            if value != v {
                return Err(format!("text {v:?}, expected {value:?}"));
            }
            close_point(*position, *pos)
                && close(*height, *h)
                && close_angle(*rotation, *r)
                && halign == ha
                && valign == va
        }
        (
            EntityKind::Image {
                path,
                position,
                u_vector,
                v_vector,
                size_px,
            },
            EntityKind::Image {
                path: pa,
                position: po,
                u_vector: u,
                v_vector: v,
                size_px: s,
            },
        ) => {
            path == pa
                && close_point(*position, *po)
                && close_vec(*u_vector, *u)
                && close_vec(*v_vector, *v)
                && size_px == s
        }
        (
            EntityKind::Group {
                group_kind,
                name,
                children,
                ..
            },
            EntityKind::Group {
                group_kind: k,
                name: n,
                children: c,
                ..
            },
        ) => {
            if group_kind != k || name != n {
                return Err(format!(
                    "group {k:?} {n:?}, expected {group_kind:?} {name:?}"
                ));
            }
            if children.len() != c.len() {
                return Err(format!(
                    "{} components, expected {}",
                    c.len(),
                    children.len()
                ));
            }
            for (i, (a, b)) in children.iter().zip(c).enumerate() {
                compare(a, b).map_err(|e| format!("component {i}: {e}"))?;
            }
            true
        }
        // A polygon with holes is written as a cell of shapes, the first the exterior and
        // the others marked as holes (FORMAT_NOTES, "Seed-based V8 writer").
        (
            EntityKind::Polygon {
                exterior,
                interiors,
            },
            EntityKind::Group { children, .. },
        ) => {
            let rings: Vec<&Vec<Point3>> = std::iter::once(exterior).chain(interiors).collect();
            if rings.len() != children.len() {
                return Err(format!(
                    "{} rings, expected {}",
                    children.len(),
                    rings.len()
                ));
            }
            for (i, (ring, child)) in rings.into_iter().zip(children).enumerate() {
                let hole = child.props.get("dgn.hole") == Some(&Value::Bool(true));
                if hole != (i > 0) {
                    return Err(format!("ring {i}: hole flag {hole}"));
                }
                let EntityKind::Polyline { vertices, .. } = &child.kind else {
                    return Err(format!("ring {i}: {}", child.kind.type_name()));
                };
                if !same_points(ring.iter().copied(), vertices.iter().map(|v| v.position)) {
                    return Err(format!("ring {i}: vertices differ"));
                }
            }
            true
        }
        _ => {
            return Err(format!(
                "read back as {}, expected {}",
                g.type_name(),
                w.type_name()
            ));
        }
    };
    if ok {
        Ok(())
    } else {
        Err(format!("geometry differs: {g:?}"))
    }
}

/// Equal point lists; a closing point repeated on one side only is ignored.
fn same_points(a: impl Iterator<Item = Point3>, b: impl Iterator<Item = Point3>) -> bool {
    let trim = |mut v: Vec<Point3>| {
        if v.len() > 1
            && v.first()
                .zip(v.last())
                .is_some_and(|(f, l)| close_point(*f, *l))
        {
            v.pop();
        }
        v
    };
    let (a, b) = (trim(a.collect()), trim(b.collect()));
    a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| close_point(*x, *y))
}

fn compare_attributes(w: &Entity, g: &Entity) -> Result<(), String> {
    if w.attributes.len() != g.attributes.len() {
        return Err(format!(
            "{} tags, expected {}",
            g.attributes.len(),
            w.attributes.len()
        ));
    }
    for (a, b) in w.attributes.iter().zip(&g.attributes) {
        let value_ok = match (&a.value, &b.value) {
            (Value::Float(x), Value::Float(y)) => close(*x, *y),
            (x, y) => x == y,
        };
        let layer_ok = a.layer.is_none() || a.layer == b.layer;
        let position_ok = match (a.position, b.position) {
            (Some(x), Some(y)) => close_point(x, y),
            (None, _) => true,
            (Some(_), None) => false,
        };
        let rotation_ok = match (&a.display, &b.display) {
            (Some(x), Some(y)) => close_angle(x.rotation, y.rotation),
            (None, _) => true,
            (Some(_), None) => false,
        };
        if a.tag != b.tag
            || a.set != b.set
            || !value_ok
            || a.invisible != b.invisible
            || !layer_ok
            || !position_ok
            || !rotation_ok
        {
            return Err(format!("tag {:?} read back as {b:?}", a.tag));
        }
    }
    Ok(())
}

// Manifest.

static NULL: Json = Json::Null;

/// `v[key]`, or null.
fn field<'a>(v: &'a Json, key: &str) -> &'a Json {
    v.get(key).unwrap_or(&NULL)
}

fn put(out: &mut Json, key: &str, value: Json) {
    if let Json::Object(map) = out {
        map.insert(key.into(), value);
    }
}

fn rgb(color: cadkit::Color) -> Json {
    match color {
        cadkit::Color::Rgb { r, g, b } => json!(format!("#{r:02X}{g:02X}{b:02X}")),
        other => json!(format!("{other:?}")),
    }
}

fn degrees(r: f64) -> f64 {
    (r * 180.0 / PI * 1e6).round() / 1e6
}

fn value_json(v: &Value) -> (Json, &'static str) {
    match v {
        Value::Text(s) => (json!(s), "text"),
        Value::Int(i) => (json!(i), "integer"),
        Value::Float(f) => (json!(f), "double"),
        other => (json!(format!("{other:?}")), "other"),
    }
}

/// One entity as the manifest shows it, with the colour cadkit resolves on read-back.
fn describe(s: &Spec, back: &Entity) -> Json {
    let mut out = json!({
        "label": s.label,
        "kind": s.entity.kind.type_name(),
        "read_back_kind": back.kind.type_name(),
        "level": s.entity.layer,
        "color": format!("{:?}", s.entity.color),
        "color_index": back.props.get("dgn.color_index").and_then(Value::as_i64),
        "rgb": rgb(back.color),
        "visible": s.entity.visible,
    });
    if let EntityKind::Text {
        value,
        halign,
        valign,
        rotation,
        position,
        height,
        ..
    } = &s.entity.kind
    {
        put(
            &mut out,
            "text",
            json!({
                "value": value,
                "justification": format!("{halign:?}-{valign:?}"),
                "rotation_deg": degrees(*rotation),
                "anchor": [position.x, position.y, position.z],
                "height": height,
                "text_length": s.entity.props.get("dgn.text_length").and_then(Value::as_f64),
            }),
        );
    }
    if let EntityKind::Image {
        path,
        position,
        u_vector,
        v_vector,
        size_px,
    } = &s.entity.kind
    {
        put(
            &mut out,
            "image",
            json!({
                "path": path,
                "size_px": size_px,
                "position": [position.x, position.y, position.z],
                "u_vector": [u_vector.x, u_vector.y, u_vector.z],
                "v_vector": [v_vector.x, v_vector.y, v_vector.z],
            }),
        );
    }
    let tags: Vec<Json> = s
        .entity
        .attributes
        .iter()
        .zip(&back.attributes)
        .map(|(a, b)| {
            let (value, kind) = value_json(&a.value);
            json!({
                "set": a.set,
                "tag": a.tag,
                "value": value,
                "type": kind,
                "displayed": !a.invisible,
                "rotation_deg": a.display.as_ref().map(|d| degrees(d.rotation)),
                "position": a.position.map(|p| [p.x, p.y, p.z]),
                "level": a.layer.clone().or_else(|| s.entity.layer.clone()),
                "color_index": b.props.get("dgn.color_index").and_then(Value::as_i64),
                "rgb": b.color.map(rgb),
            })
        })
        .collect();
    if !tags.is_empty() {
        put(&mut out, "tags", json!(tags));
    }
    out
}

/// Distinct level names of an item in first-use order, entity levels before tag levels,
/// with the level colour cadkit reads back from the level entry.
fn levels(specs: &[Spec], back: &Document) -> Vec<Json> {
    level_names(specs)
        .into_iter()
        .map(|name| {
            let layer = back.layers.iter().find(|l| l.name == name);
            json!({
                "name": name,
                // None when cadkit does not decode the level entry's colour (104-byte
                // entries, FORMAT_NOTES "Level entry symbology").
                "read_back_color": layer
                    .filter(|l| matches!(l.color, cadkit::Color::Rgb { .. }))
                    .map(|l| rgb(l.color)),
                "read_back_color_index": layer
                    .and_then(|l| l.props.get("dgn.color_index"))
                    .and_then(Value::as_i64),
            })
        })
        .collect()
}

fn level_names(specs: &[Spec]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let entity_levels = specs.iter().filter_map(|s| s.entity.layer.clone());
    let tag_levels = specs
        .iter()
        .flat_map(|s| &s.entity.attributes)
        .filter_map(|a| a.layer.clone());
    for name in entity_levels.chain(tag_levels) {
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

fn readme(manifest: &Json) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# cadkit DGN acceptance kit\n");
    let _ = writeln!(
        s,
        "Generated by cadkit {} (`crates/cadkit/examples/interchange.rs`) from the seed `{}`. \
         Every name, text, tag value and coordinate is invented. Each file was read back by \
         cadkit and compared with what was written, reported no `check_structure` warning and \
         opened with the `cfb` crate in strict mode. **None of these files has been opened in \
         another DGN application by cadkit; no external acceptance is claimed.** The protocol, \
         the results table and the open questions are in `docs/dgn/ACCEPTANCE.md` of the \
         cadkit repository; `MANIFEST.json` holds the same content as this file in machine \
         form.\n",
        field(manifest, "cadkit_version")
            .as_str()
            .unwrap_or_default(),
        field(field(manifest, "seed"), "file")
            .as_str()
            .unwrap_or_default(),
    );
    let _ = writeln!(s, "| File | Purpose | Bytes | check_structure |");
    let _ = writeln!(s, "|---|---|---|---|");
    let items = field(manifest, "items")
        .as_array()
        .cloned()
        .unwrap_or_default();
    for item in &items {
        let file = field(item, "file").as_str().unwrap_or_default();
        if let Some(reason) = field(item, "skipped").as_str() {
            let _ = writeln!(s, "| {file} | skipped: {reason} | | |");
            continue;
        }
        let warnings = field(field(item, "structure"), "warnings")
            .as_array()
            .map(|w| {
                if w.is_empty() {
                    "clean".to_string()
                } else {
                    format!("{w:?}")
                }
            })
            .unwrap_or_else(|| "n/a".into());
        let _ = writeln!(
            s,
            "| {file} | {} | {} | {warnings} |",
            field(item, "purpose").as_str().unwrap_or_default(),
            field(item, "bytes"),
        );
    }
    for item in &items {
        if item.get("skipped").is_some() {
            continue;
        }
        let _ = writeln!(
            s,
            "\n## {}\n",
            field(item, "file").as_str().unwrap_or_default()
        );
        let _ = writeln!(
            s,
            "{}\n",
            field(item, "purpose").as_str().unwrap_or_default()
        );
        if let Some(seed) = field(item, "seed").as_str() {
            let _ = writeln!(s, "Seed: `{seed}`.\n");
        }
        if let Some(levels) = field(item, "levels").as_array() {
            let names: Vec<&str> = levels
                .iter()
                .filter_map(|l| field(l, "name").as_str())
                .collect();
            let _ = writeln!(s, "Levels used: {}.\n", names.join(", "));
        }
        if let Some(entities) = field(item, "entities").as_array() {
            let _ = writeln!(s, "| # | Element | Level | Colour index (RGB) | Visible |");
            let _ = writeln!(s, "|---|---|---|---|---|");
            for (i, e) in entities.iter().enumerate() {
                let _ = writeln!(
                    s,
                    "| {} | {} | {} | {} ({}) | {} |",
                    i + 1,
                    field(e, "label").as_str().unwrap_or_default(),
                    field(e, "level").as_str().unwrap_or_default(),
                    field(e, "color_index"),
                    field(e, "rgb").as_str().unwrap_or_default(),
                    field(e, "visible"),
                );
            }
            let texts: Vec<&Json> = entities.iter().filter_map(|e| e.get("text")).collect();
            if !texts.is_empty() {
                let _ = writeln!(s, "\nTexts:\n");
                for t in texts {
                    let _ = writeln!(
                        s,
                        "- `{}`: {}, {} degrees, anchor {}",
                        field(t, "value").as_str().unwrap_or_default(),
                        field(t, "justification").as_str().unwrap_or_default(),
                        field(t, "rotation_deg"),
                        field(t, "anchor"),
                    );
                }
            }
            let tags: Vec<&Json> = entities
                .iter()
                .filter_map(|e| field(e, "tags").as_array())
                .flatten()
                .collect();
            if !tags.is_empty() {
                let _ = writeln!(s, "\nTags:\n");
                for t in tags {
                    let shown = if field(t, "displayed").as_bool().unwrap_or(false) {
                        format!("displayed at {} degrees", field(t, "rotation_deg"))
                    } else {
                        "hidden".into()
                    };
                    let _ = writeln!(
                        s,
                        "- {} / {} = `{}` ({}, {shown}, level {})",
                        field(t, "set").as_str().unwrap_or_default(),
                        field(t, "tag").as_str().unwrap_or_default(),
                        field(t, "value"),
                        field(t, "type").as_str().unwrap_or_default(),
                        field(t, "level").as_str().unwrap_or_default(),
                    );
                }
            }
        }
        if let Some(report) = item.get("strip_report") {
            let _ = writeln!(s, "Strip report: `{report}`\n");
        }
        if let Some(back) = item.get("read_back") {
            let _ = writeln!(s, "Read back: `{back}`\n");
        }
        let _ = writeln!(s, "\nChecks:\n");
        for c in field(item, "checks").as_array().into_iter().flatten() {
            let _ = writeln!(s, "- [ ] {}", c.as_str().unwrap_or_default());
        }
    }
    s
}

/// A baseline TIFF 6.0 bilevel image (uncompressed, WhiteIsZero, 200 DPI): a one-pixel
/// black border and a solid black square in the top-left corner, so that the orientation of
/// the attachment can be seen.
fn tiff() -> Vec<u8> {
    let [w, h] = RASTER_SIZE;
    let row = w.div_ceil(8) as usize;
    let mut pixels = vec![0u8; row * h as usize];
    for y in 0..h {
        for x in 0..w {
            let border = x == 0 || y == 0 || x == w - 1 || y == h - 1;
            let block = (3..13).contains(&x) && (3..13).contains(&y);
            if (border || block)
                && let Some(byte) = pixels.get_mut(y as usize * row + x as usize / 8)
            {
                *byte |= 0x80 >> (x % 8);
            }
        }
    }
    // Header (8), IFD of 12 entries (2 + 12 x 12 + 4 = 150), two RATIONALs, then the strip.
    const ENTRIES: u16 = 12;
    let ifd_end = 8 + 2 + 12 * u32::from(ENTRIES) + 4;
    let (x_res, y_res) = (ifd_end, ifd_end + 8);
    let data = ifd_end + 16;
    let mut b = b"II*\0".to_vec();
    b.extend(8u32.to_le_bytes());
    b.extend(ENTRIES.to_le_bytes());
    let mut entry = |tag: u16, kind: u16, value: u32| {
        b.extend(tag.to_le_bytes());
        b.extend(kind.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        b.extend(value.to_le_bytes());
    };
    const SHORT: u16 = 3;
    const LONG: u16 = 4;
    const RATIONAL: u16 = 5;
    entry(256, SHORT, w);
    entry(257, SHORT, h);
    entry(258, SHORT, 1);
    entry(259, SHORT, 1);
    entry(262, SHORT, 0);
    entry(273, LONG, data);
    entry(277, SHORT, 1);
    entry(278, SHORT, h);
    entry(279, LONG, pixels.len() as u32);
    entry(282, RATIONAL, x_res);
    entry(283, RATIONAL, y_res);
    entry(296, SHORT, 2);
    b.extend(0u32.to_le_bytes());
    for _ in 0..2 {
        b.extend(200u32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
    }
    b.extend(pixels);
    b
}

fn write_tiff(dir: &Path) -> KitResult<()> {
    std::fs::write(dir.join(RASTER_FILE), tiff())?;
    Ok(())
}
