//! DWG, DGN, DXF ve CityGML 2.0 belgelerini inceleyen ve dönüştüren komut satırı aracı.
//!
//! Exit codes: 0 success, 1 the drawing could not be read / written, 2 usage error.
#![allow(clippy::print_stdout)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cadkit::{Color, Document, DxfVersion, Entity, EntityKind, ReadOptions, SvgOptions};
use clap::{Parser, Subcommand};

/// DWG, DGN, DXF ve CityGML 2.0 belgelerini inceler ve dönüştürür.
#[derive(Parser, Debug)]
#[command(name = "cadkit", version, about, long_about = None)]
struct Cli {
    /// Keep the original record bytes of every entity (large; mostly useful with `dump --json`).
    #[arg(long, global = true)]
    keep_raw: bool,

    /// Code page (WHATWG label, e.g. windows-1254) for 8-bit text when the file declares none.
    #[arg(long, global = true, value_name = "LABEL")]
    codepage: Option<String>,

    /// Refuse input files larger than this many bytes.
    #[arg(long, global = true, value_name = "BYTES")]
    max_input_bytes: Option<u64>,

    /// Stop when decompressed data would exceed this many bytes.
    #[arg(long, global = true, value_name = "BYTES")]
    max_decompressed_bytes: Option<u64>,

    /// Stop after this many records/objects/elements.
    #[arg(long, global = true, value_name = "N")]
    max_objects: Option<u64>,

    /// Largest distance of a polygon ring point from its exterior ring's plane, in coordinate
    /// units, for CityGML validation and writing and for DGN output (default 0.01: 1 cm in
    /// metres). Must exceed the rounding step of the source coordinates.
    #[arg(long, global = true, value_name = "UNITS", value_parser = parse_tolerance)]
    planarity_tolerance: Option<f64>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Summarize a drawing: format, units, models, layers, entity histogram.
    Info {
        /// DWG, DGN, DXF veya CityGML 2.0 dosyası.
        file: PathBuf,
        /// Also list the warnings recorded while reading.
        #[arg(long)]
        warnings: bool,
        /// Number of warnings to list with --warnings.
        #[arg(long, default_value_t = 20, value_name = "N")]
        max_warnings: usize,
    },
    /// List the layers with their state and entity counts.
    Layers {
        /// DWG, DGN, DXF veya CityGML 2.0 dosyası.
        file: PathBuf,
    },
    /// Çizimi .json, .svg, .dxf, .dgn veya .gml biçimine dönüştürür.
    Convert {
        /// DWG, DGN, DXF veya CityGML 2.0 dosyası.
        input: PathBuf,
        /// Output file; its extension selects the format.
        output: PathBuf,
        /// Model to render (SVG only).
        #[arg(long, default_value_t = 0, value_name = "N")]
        model: usize,
        /// DXF version to write: r12, r2000, r2004, r2007, r2010, r2013 or r2018.
        #[arg(long, value_parser = parse_dxf_version, value_name = "VERSION")]
        dxf_version: Option<DxfVersion>,
        /// SVG width in pixels.
        #[arg(long, value_parser = parse_width, value_name = "PX")]
        width: Option<f64>,
        /// Pretty-print JSON output.
        #[arg(long)]
        pretty: bool,
        /// DGN V8 çıktısı için seed dosyası.
        #[arg(long)]
        seed: Option<PathBuf>,
        /// Dolu seed'in model içeriğini temizlemeye açık izin.
        #[arg(long)]
        clear_seed_model: bool,
        /// Yeni CityGML çıktısındaki koordinatların mevcut CRS tanımı.
        #[arg(long)]
        crs: Option<String>,
        /// CityGML için ayrıntı düzeyi (0..4).
        #[arg(long, default_value_t = 1)]
        lod: u8,
        /// Yalnız GML → GML aktarımında kaynak geometri hatalarını korumaya açık izin.
        #[arg(long)]
        preserve_invalid_geometry: bool,
    },
    /// CityGML yapı ve geometri sorunlarını JSON olarak raporlar; sorun varsa çıkış kodu 1.
    ValidateGml { file: PathBuf },
    /// DGN V8 akışlarını içeriklerini değiştirmeden yeni CFB konteynerine paketler.
    RepackDgn { input: PathBuf, output: PathBuf },
    /// Print one line per entity, or the full document as JSON.
    Dump {
        /// DWG, DGN, DXF veya CityGML 2.0 dosyası.
        file: PathBuf,
        /// Print the whole document as pretty JSON instead.
        #[arg(long)]
        json: bool,
    },
}

fn parse_dxf_version(s: &str) -> Result<DxfVersion, String> {
    match s.to_ascii_lowercase().as_str() {
        "r12" => Ok(DxfVersion::R12),
        "r2000" => Ok(DxfVersion::R2000),
        "r2004" => Ok(DxfVersion::R2004),
        "r2007" => Ok(DxfVersion::R2007),
        "r2010" => Ok(DxfVersion::R2010),
        "r2013" => Ok(DxfVersion::R2013),
        "r2018" => Ok(DxfVersion::R2018),
        _ => Err(format!(
            "unknown DXF version `{s}` (expected r12, r2000, r2004, r2007, r2010, r2013 or r2018)"
        )),
    }
}

fn parse_width(s: &str) -> Result<f64, String> {
    match s.parse::<f64>() {
        Ok(v) if v.is_finite() && v > 0.0 => Ok(v),
        _ => Err(format!(
            "invalid width `{s}` (expected a positive number of pixels)"
        )),
    }
}

fn parse_tolerance(s: &str) -> Result<f64, String> {
    match s.parse::<f64>() {
        Ok(v) if v.is_finite() && v > 0.0 => Ok(v),
        _ => Err(format!(
            "invalid tolerance `{s}` (expected a positive number of coordinate units)"
        )),
    }
}

/// How a command failed; decides the exit code.
#[derive(Debug)]
enum CliError {
    /// Bad arguments (exit 2).
    Usage(String),
    /// Reading, converting or writing failed (exit 1).
    Failure(String),
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            // clap prints help/version to stdout and real usage errors to stderr.
            let code = if e.use_stderr() { 2 } else { 0 };
            let _ = e.print();
            return ExitCode::from(code);
        }
    };
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Usage(msg)) => {
            eprintln!("error: {msg}");
            ExitCode::from(2)
        }
        Err(CliError::Failure(msg)) => {
            eprintln!("error: {msg}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: &Cli) -> Result<(), CliError> {
    match &cli.command {
        Command::Info {
            file,
            warnings,
            max_warnings,
        } => {
            let doc = load(cli, file)?;
            print_out(&strip_controls(&info_text(
                file,
                &doc,
                *warnings,
                *max_warnings,
            )));
        }
        Command::Layers { file } => {
            let doc = load(cli, file)?;
            print_out(&strip_controls(&layers_text(&doc)));
        }
        Command::Convert {
            input,
            output,
            model,
            dxf_version,
            width,
            pretty,
            seed,
            clear_seed_model,
            crs,
            lod,
            preserve_invalid_geometry,
        } => {
            let target = OutputKind::from_path(output)?;
            let options = read_options(cli);
            if matches!(target, OutputKind::Gml) {
                let bytes = load_bytes(input, options.limits.max_input_bytes)?;
                let output_bytes = if cadkit::gml::sniff(&bytes) {
                    let native = cadkit::gml::read_native(&bytes, &options)
                        .map_err(|e| failure(input, &e))?;
                    cadkit::gml::write(
                        &native,
                        &gml_validation(cli, &options, !preserve_invalid_geometry),
                    )
                    .map_err(|e| failure(output, &e))?
                } else {
                    if *preserve_invalid_geometry {
                        return Err(CliError::Usage(
                            "--preserve-invalid-geometry requires CityGML input".into(),
                        ));
                    }
                    let doc =
                        cadkit::read_with(&bytes, &options).map_err(|e| failure(input, &e))?;
                    let srs_name = crs.clone().ok_or_else(|| {
                        CliError::Usage("new CityGML output requires --crs".into())
                    })?;
                    cadkit::to_citygml(
                        &doc,
                        &cadkit::gml::ExportOptions {
                            srs_name,
                            lod: *lod,
                            limits: options.limits,
                        },
                    )
                    .map_err(|e| failure(output, &e))?
                };
                std::fs::write(output, output_bytes).map_err(|e| failure(output, &e))?;
                return Ok(());
            }
            let doc = load(cli, input)?;
            if matches!(target, OutputKind::Dgn) {
                let seed = seed
                    .as_ref()
                    .ok_or_else(|| CliError::Usage("DGN V8 output requires --seed".into()))?;
                let bytes = load_bytes(seed, options.limits.max_input_bytes)?;
                let mut options = cadkit::dgn::WriteOptions {
                    limits: options.limits,
                    codepage: cli.codepage.clone(),
                    clear_seed_model: *clear_seed_model,
                    ..Default::default()
                };
                if let Some(t) = cli.planarity_tolerance {
                    options.planarity_tolerance = t;
                }
                let bytes =
                    cadkit::to_dgn(&doc, &bytes, &options).map_err(|e| failure(output, &e))?;
                std::fs::write(output, bytes).map_err(|e| failure(output, &e))?;
                return Ok(());
            }
            let text = match target {
                OutputKind::Json => {
                    cadkit::to_json(&doc, *pretty).map_err(|e| failure(output, &e))?
                }
                OutputKind::Svg => {
                    if *model >= doc.models.len() {
                        return Err(CliError::Usage(format!(
                            "model {model} does not exist (the drawing has {} models)",
                            doc.models.len()
                        )));
                    }
                    let mut options = SvgOptions {
                        model: *model,
                        ..SvgOptions::default()
                    };
                    if let Some(w) = width {
                        options.width_px = *w;
                    }
                    cadkit::to_svg(&doc, &options)
                }
                OutputKind::Dxf => cadkit::to_dxf(&doc, dxf_version.unwrap_or_default())
                    .map_err(|e| failure(output, &e))?,
                OutputKind::Dgn | OutputKind::Gml => {
                    return Err(CliError::Failure(
                        "unexpected binary output dispatch".into(),
                    ));
                }
            };
            std::fs::write(output, text).map_err(|e| failure(output, &e))?;
        }
        Command::ValidateGml { file } => {
            let options = read_options(cli);
            let bytes = load_bytes(file, options.limits.max_input_bytes)?;
            let doc = cadkit::gml::read_native(&bytes, &options).map_err(|e| failure(file, &e))?;
            let validation = gml_validation(cli, &options, false);
            let structure =
                cadkit::gml::validate(&doc, &validation).map_err(|e| failure(file, &e))?;
            let issues =
                cadkit::gml::geometry_issues(&doc, &validation).map_err(|e| failure(file, &e))?;
            let text = serde_json::to_string_pretty(
                &serde_json::json!({"structure":structure,"geometry_issues":issues}),
            )
            .map_err(|e| failure(file, &e))?;
            print_out(&format!("{}\n", json_for_terminal(&text)));
            if !issues.is_empty() {
                return Err(CliError::Failure(
                    "CityGML geometry validation failed".into(),
                ));
            }
        }
        Command::RepackDgn { input, output } => {
            let options = read_options(cli);
            let bytes = load_bytes(input, options.limits.max_input_bytes)?;
            let bytes = cadkit::dgn::repack_v8(&bytes, &options).map_err(|e| failure(input, &e))?;
            std::fs::write(output, bytes).map_err(|e| failure(output, &e))?;
        }
        Command::Dump { file, json } => {
            let doc = load(cli, file)?;
            if *json {
                let text = cadkit::to_json(&doc, true).map_err(|e| failure(file, &e))?;
                print_out(&format!("{}\n", json_for_terminal(&text)));
            } else {
                print_out(&strip_controls(&dump_text(&doc)));
            }
        }
    }
    Ok(())
}

fn failure(path: &Path, e: &dyn std::fmt::Display) -> CliError {
    CliError::Failure(format!("{}: {e}", path.display()))
}

fn load(cli: &Cli, path: &Path) -> Result<Document, CliError> {
    cadkit::open_with(path, &read_options(cli)).map_err(|e| failure(path, &e))
}

fn load_bytes(path: &Path, max: u64) -> Result<Vec<u8>, CliError> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| failure(path, &e))?;
    if file.metadata().map_err(|e| failure(path, &e))?.len() > max {
        return Err(CliError::Failure("input byte limit exceeded".into()));
    }
    let mut bytes = vec![];
    file.take(max.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| failure(path, &e))?;
    if bytes.len() as u64 > max {
        return Err(CliError::Failure("input byte limit exceeded".into()));
    }
    Ok(bytes)
}

fn gml_validation(
    cli: &Cli,
    options: &ReadOptions,
    geometry: bool,
) -> cadkit::gml::ValidationOptions {
    let mut validation = cadkit::gml::ValidationOptions {
        limits: options.limits,
        geometry,
        ..Default::default()
    };
    if let Some(t) = cli.planarity_tolerance {
        validation.planarity_tolerance = t;
    }
    validation
}

fn read_options(cli: &Cli) -> ReadOptions {
    let mut options = ReadOptions {
        keep_raw: cli.keep_raw,
        fallback_codepage: cli.codepage.clone(),
        ..ReadOptions::default()
    };
    if let Some(v) = cli.max_input_bytes {
        options.limits.max_input_bytes = v;
    }
    if let Some(v) = cli.max_decompressed_bytes {
        options.limits.max_decompressed_bytes = v;
    }
    if let Some(v) = cli.max_objects {
        options.limits.max_objects = v;
    }
    options
}

/// Writes to stdout, ignoring a closed pipe instead of panicking like `println!` would.
fn print_out(text: &str) {
    let _ = std::io::stdout().write_all(text.as_bytes());
}

/// Makes a file-controlled string safe to print on a terminal: control characters (ESC, CSI,
/// newlines, ...) become `?` so a drawing cannot inject escape sequences or forge output lines.
fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// Last line of defense for text output: only `\n` and `\t` may remain as control characters.
fn strip_controls(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                '?'
            } else {
                c
            }
        })
        .collect()
}

/// JSON text with raw C1/DEL control characters (which serde leaves unescaped but terminals
/// interpret) written as `\u00XX` escapes. Valid JSON, same data.
fn json_for_terminal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_control() && c != '\n' && c != '\t' {
            let _ = write!(out, "\\u{:04x}", c as u32);
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Debug, Clone, Copy)]
enum OutputKind {
    Json,
    Svg,
    Dxf,
    Dgn,
    Gml,
}

impl OutputKind {
    fn from_path(path: &Path) -> Result<Self, CliError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        match ext.as_deref() {
            Some("json") => Ok(Self::Json),
            Some("svg") => Ok(Self::Svg),
            Some("dxf") => Ok(Self::Dxf),
            Some("dgn") => Ok(Self::Dgn),
            Some("gml") | Some("citygml") => Ok(Self::Gml),
            _ => Err(CliError::Usage(format!(
                "cannot choose an output format for `{}`: use .json, .svg, .dxf, .dgn or .gml",
                path.display()
            ))),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Text output
// ---------------------------------------------------------------------------------------------

/// Calls `f` for every entity, descending into groups (the group itself is visited first).
fn for_each_entity<'a>(list: &'a [Entity], depth: usize, f: &mut dyn FnMut(&'a Entity, usize)) {
    for e in list {
        f(e, depth);
        if let EntityKind::Group { children, .. } = &e.kind {
            for_each_entity(children, depth + 1, f);
        }
    }
}

fn num(v: f64) -> String {
    if !v.is_finite() {
        return "nan".to_owned();
    }
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}

fn color_text(c: Color) -> String {
    match c {
        Color::ByLayer => "bylayer".to_owned(),
        Color::ByBlock => "byblock".to_owned(),
        Color::Aci { index } => format!("aci {index}"),
        Color::Rgb { r, g, b } => format!("#{r:02x}{g:02x}{b:02x}"),
    }
}

fn info_text(file: &Path, doc: &Document, show_warnings: bool, max_warnings: usize) -> String {
    let mut out = String::new();
    let src = &doc.source;
    let _ = writeln!(out, "File:        {}", clean(&file.display().to_string()));
    let _ = writeln!(out, "Format:      {:?}", src.format);
    let _ = writeln!(
        out,
        "Version:     {}",
        if src.version.is_empty() {
            "-".to_owned()
        } else {
            clean(&src.version)
        }
    );
    let _ = writeln!(
        out,
        "Application: {}",
        clean(src.application.as_deref().unwrap_or("-"))
    );
    let _ = writeln!(
        out,
        "Codepage:    {}",
        clean(src.codepage.as_deref().unwrap_or("-"))
    );
    let meters = doc
        .units
        .meters_per_unit()
        .map_or_else(|| "unknown".to_owned(), |m| format!("{m} m/unit"));
    let _ = writeln!(out, "Units:       {:?} ({meters})", doc.units.unit);
    let _ = writeln!(out, "Layers:      {}", doc.layers.len());
    let _ = writeln!(out, "Blocks:      {}", doc.blocks.len());
    let _ = writeln!(out, "Models:      {}", doc.models.len());
    let mut histogram: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (i, m) in doc.models.iter().enumerate() {
        let mut count = 0usize;
        for_each_entity(&m.entities, 0, &mut |e, _| {
            count += 1;
            *histogram.entry(e.kind.type_name()).or_insert(0) += 1;
        });
        let _ = writeln!(
            out,
            "  [{i}] {} ({:?}{}): {count} entities",
            clean(&m.name),
            m.kind,
            if m.is_3d { ", 3D" } else { "" }
        );
    }
    let _ = writeln!(out, "Entity types:");
    if histogram.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    let mut rows: Vec<(&&str, &usize)> = histogram.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (name, n) in rows {
        let _ = writeln!(out, "  {name:<10} {n}");
    }
    let _ = writeln!(out, "Warnings:    {}", doc.warnings.len());
    if show_warnings {
        for w in doc.warnings.iter().take(max_warnings) {
            let at = w.offset.map_or_else(String::new, |o| format!(" @0x{o:x}"));
            let _ = writeln!(out, "  [{}]{at} {}", clean(&w.code), clean(&w.message));
        }
        if doc.warnings.len() > max_warnings {
            let _ = writeln!(out, "  ... {} more", doc.warnings.len() - max_warnings);
        }
    }
    out
}

fn layers_text(doc: &Document) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for m in &doc.models {
        for_each_entity(&m.entities, 0, &mut |e, _| {
            *counts.entry(e.layer.as_deref().unwrap_or("0")).or_insert(0) += 1;
        });
    }
    let mut rows: Vec<[String; 6]> = Vec::new();
    for l in &doc.layers {
        let yes = |b: bool| if b { "yes" } else { "no" }.to_owned();
        rows.push([
            clean(&l.name),
            color_text(l.color),
            yes(l.visible),
            yes(l.frozen),
            yes(l.locked),
            counts.remove(l.name.as_str()).unwrap_or(0).to_string(),
        ]);
    }
    // Layers referenced by entities but absent from the layer table.
    for (name, n) in counts {
        rows.push([
            format!("{} (undefined)", clean(name)),
            "-".into(),
            "-".into(),
            "-".into(),
            "-".into(),
            n.to_string(),
        ]);
    }
    let head = ["NAME", "COLOR", "VISIBLE", "FROZEN", "LOCKED", "ENTITIES"].map(str::to_owned);
    let mut widths = head.clone().map(|h| h.chars().count());
    for r in &rows {
        for (w, c) in widths.iter_mut().zip(r.iter()) {
            *w = (*w).max(c.chars().count());
        }
    }
    let mut out = String::new();
    let mut line = |cells: &[String; 6]| {
        let mut parts = Vec::new();
        for (c, w) in cells.iter().zip(widths.iter()) {
            parts.push(format!("{c:<w$}"));
        }
        let _ = writeln!(out, "{}", parts.join("  ").trim_end());
    };
    line(&head);
    for r in &rows {
        line(r);
    }
    out
}

fn pt(p: &cadkit::Point3) -> String {
    format!("({}, {}, {})", num(p.x), num(p.y), num(p.z))
}

fn summary(kind: &EntityKind) -> String {
    match kind {
        EntityKind::Point { position } => pt(position),
        EntityKind::Line { start, end } => format!("{} -> {}", pt(start), pt(end)),
        EntityKind::Polyline {
            vertices, closed, ..
        } => {
            format!(
                "{} vertices{}",
                vertices.len(),
                if *closed { ", closed" } else { "" }
            )
        }
        EntityKind::Circle { center, radius, .. } => {
            format!("center {} r {}", pt(center), num(*radius))
        }
        EntityKind::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            ..
        } => format!(
            "center {} r {} {}..{} deg",
            pt(center),
            num(*radius),
            num(start_angle.to_degrees()),
            num(end_angle.to_degrees())
        ),
        EntityKind::Ellipse {
            center,
            major_axis,
            ratio,
            ..
        } => {
            format!(
                "center {} major ({}, {}) ratio {}",
                pt(center),
                num(major_axis.x),
                num(major_axis.y),
                num(*ratio)
            )
        }
        EntityKind::Spline {
            degree,
            control_points,
            fit_points,
            ..
        } => {
            format!(
                "degree {degree}, {} control, {} fit points",
                control_points.len(),
                fit_points.len()
            )
        }
        EntityKind::Text {
            position,
            height,
            value,
            ..
        } => {
            format!(
                "at {} h {} {:?}",
                pt(position),
                num(*height),
                shorten(value)
            )
        }
        EntityKind::MText {
            position,
            height,
            plain,
            value,
            ..
        } => {
            let text = if plain.is_empty() { value } else { plain };
            format!("at {} h {} {:?}", pt(position), num(*height), shorten(text))
        }
        EntityKind::Insert {
            block,
            position,
            scale,
            rotation,
            columns,
            rows,
            ..
        } => {
            let array = if *columns > 1 || *rows > 1 {
                format!(" array {columns}x{rows}")
            } else {
                String::new()
            };
            format!(
                "block {:?} at {} scale ({}, {}) rot {} deg{array}",
                clean(block),
                pt(position),
                num(scale.x),
                num(scale.y),
                num(rotation.to_degrees())
            )
        }
        EntityKind::Hatch {
            loops,
            solid,
            pattern,
            ..
        } => {
            let fill = if *solid {
                "solid".to_owned()
            } else {
                clean(pattern.as_deref().unwrap_or("pattern"))
            };
            format!("{} loops, {fill}", loops.len())
        }
        EntityKind::Dimension {
            dimension_type,
            points,
            measurement,
            ..
        } => {
            let m = measurement.map_or_else(String::new, |m| format!(" = {}", num(m)));
            format!("{dimension_type:?}, {} points{m}", points.len())
        }
        EntityKind::Face { points, filled } => format!(
            "{} points{}",
            points.len(),
            if *filled { ", filled" } else { "" }
        ),
        EntityKind::Leader { vertices, .. } => format!("{} vertices", vertices.len()),
        EntityKind::Image { path, position, .. } => format!(
            "{:?} at {}",
            clean(path.as_deref().unwrap_or("-")),
            pt(position)
        ),
        EntityKind::Viewport {
            center,
            width,
            height,
            ..
        } => {
            format!("center {} {}x{}", pt(center), num(*width), num(*height))
        }
        EntityKind::Mesh { vertices, faces } => {
            format!("{} vertices, {} faces", vertices.len(), faces.len())
        }
        EntityKind::Group {
            group_kind,
            name,
            children,
            ..
        } => {
            format!(
                "{group_kind:?} {:?} with {} children",
                clean(name.as_deref().unwrap_or("")),
                children.len()
            )
        }
        EntityKind::Unknown { type_name } => clean(type_name),
        EntityKind::Polygon {
            exterior,
            interiors,
        } => format!(
            "{} exterior positions, {} holes",
            exterior.len(),
            interiors.len()
        ),
    }
}

fn shorten(s: &str) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if flat.chars().count() > 40 {
        let cut: String = flat.chars().take(40).collect();
        format!("{cut}...")
    } else {
        flat
    }
}

fn dump_text(doc: &Document) -> String {
    let mut out = String::new();
    for (i, m) in doc.models.iter().enumerate() {
        let _ = writeln!(
            out,
            "# model {i} {:?} ({} top-level entities)",
            clean(&m.name),
            m.entities.len()
        );
        for_each_entity(&m.entities, 0, &mut |e, depth| {
            let id = e.id.map_or_else(|| "-".to_owned(), |id| id.to_string());
            let _ = writeln!(
                out,
                "{}{id}\t{}\t{}\t{}",
                "  ".repeat(depth),
                clean(e.layer.as_deref().unwrap_or("0")),
                e.kind.type_name(),
                summary(&e.kind)
            );
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadkit::{Layer, Model, Point3};

    fn sample() -> Document {
        let mut line = Entity::new(EntityKind::Line {
            start: Point3::xy(0.0, 0.0),
            end: Point3::xy(10.0, 5.5),
        });
        line.id = Some(42);
        line.layer = Some("walls".into());
        Document {
            layers: vec![Layer {
                name: "walls".into(),
                visible: true,
                ..Layer::default()
            }],
            models: vec![Model {
                name: "Model".into(),
                entities: vec![line],
                ..Model::default()
            }],
            ..Document::default()
        }
    }

    #[test]
    fn number_formatting() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(-0.00001), "0");
        assert_eq!(num(2.50), "2.5");
        assert_eq!(num(f64::NAN), "nan");
    }

    #[test]
    fn dump_and_layers_text() {
        let d = sample();
        let dump = dump_text(&d);
        assert!(
            dump.contains("42\twalls\tline\t(0, 0, 0) -> (10, 5.5, 0)"),
            "{dump}"
        );
        let layers = layers_text(&d);
        assert!(
            layers.contains("walls") && layers.lines().count() == 2,
            "{layers}"
        );
        let info = info_text(Path::new("x.dxf"), &d, true, 5);
        assert!(
            info.contains("line       1") && info.contains("Warnings:    0"),
            "{info}"
        );
    }

    #[test]
    fn terminal_output_is_sanitized() {
        assert_eq!(clean("a\u{1b}[31mb\nc\u{9b}"), "a?[31mb?c?");
        assert_eq!(strip_controls("ok\tx\n\u{1b}y"), "ok\tx\n?y");
        assert_eq!(
            json_for_terminal("{\"a\":\"\u{9b}\"}"),
            "{\"a\":\"\\u009b\"}"
        );
        let mut d = sample();
        d.source.application = Some("evil\u{1b}]0;title\u{7}".into());
        d.layers[0].name = "la\nyer".into();
        let text = strip_controls(&info_text(Path::new("x"), &d, false, 0));
        assert!(
            !text.contains('\u{1b}') && text.contains("evil?]0;title?"),
            "{text}"
        );
        assert!(layers_text(&d).contains("la?yer"));
    }

    #[test]
    fn version_and_width_parsing() {
        assert_eq!(parse_dxf_version("R2000"), Ok(DxfVersion::R2000));
        assert!(parse_dxf_version("r13").is_err());
        assert!(parse_width("0").is_err());
        assert_eq!(parse_width("800"), Ok(800.0));
    }
}
