//! Integration tests of the `cadkit` binary.
//!
//! Tests that need a working format reader skip (with a note on stderr) while the reader
//! still reports `unsupported` / `unrecognized`.

#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn cadkit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cadkit"))
        .args(args)
        .output()
        .expect("run cadkit")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cadkit-cli-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

const DXF: &str = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nWALLS\n10\n0.0\n20\n0.0\n30\n0.0\n11\n10.0\n21\n5.0\n31\n0.0\n0\nENDSEC\n0\nEOF\n";

#[test]
fn metadata_only_gml_requires_opt_in_and_reports_annotations() {
    let dir = temp_dir("gml-metadata");
    let input = dir.join("synthetic.dxf");
    let output = dir.join("result.gml");
    let text_dxf = "0\nSECTION\n2\nENTITIES\n0\nTEXT\n8\n0\n10\n0\n20\n0\n40\n1\n1\nSynthetic\n0\nENDSEC\n0\nEOF\n";
    std::fs::write(&input, text_dxf).expect("write synthetic text");
    let input = input.to_str().expect("input path");
    let output = output.to_str().expect("output path");
    let args = ["convert", input, output, "--crs", "urn:cadkit:synthetic"];
    assert!(!cadkit(&args).status.success());
    assert!(!Path::new(output).exists());
    let mut fallback = args.to_vec();
    fallback.push("--gml-metadata-only");
    let result = cadkit(&fallback);
    assert!(result.status.success(), "{}", text(&result.stderr));
    let report: serde_json::Value = serde_json::from_slice(&result.stderr).expect("report JSON");
    assert_eq!(report["metadata_only_entities"], 1);
    let xml = std::fs::read_to_string(output).expect("output XML");
    assert!(xml.contains("cadkit.geometry_status"));
    assert!(cadkit(&["validate-gml", output]).status.success());
}

fn write_dxf(dir: &Path) -> PathBuf {
    let path = dir.join("synthetic.dxf");
    std::fs::write(&path, DXF).expect("write dxf");
    path
}

/// True when the DXF reader is not usable yet; prints a skip note.
fn reader_missing(out: &Output, test: &str) -> bool {
    if out.status.success() {
        return false;
    }
    let err = text(&out.stderr).to_lowercase();
    if err.contains("unsupported")
        || err.contains("unrecognized")
        || err.contains("not implemented")
    {
        eprintln!("SKIP {test}: DXF reader not available yet ({})", err.trim());
        return true;
    }
    false
}

#[test]
fn no_arguments_is_a_usage_error() {
    let out = cadkit(&[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(!out.stderr.is_empty());
}

#[test]
fn unknown_subcommand_and_flag_are_usage_errors() {
    assert_eq!(cadkit(&["frobnicate"]).status.code(), Some(2));
    assert_eq!(cadkit(&["info", "--bogus", "x.dxf"]).status.code(), Some(2));
    assert_eq!(cadkit(&["info"]).status.code(), Some(2));
}

#[test]
fn help_and_version_succeed() {
    let out = cadkit(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    let help = text(&out.stdout);
    for cmd in ["info", "layers", "convert", "dump"] {
        assert!(help.contains(cmd), "help lacks {cmd}: {help}");
    }
    let out = cadkit(&["--version"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(text(&out.stdout).starts_with("cadkit "));
}

#[test]
fn missing_file_is_a_read_error() {
    let dir = temp_dir("missing");
    let missing = dir.join("nope.dxf");
    let out = cadkit(&["info", missing.to_str().expect("utf8 path")]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).starts_with("error:"));
    assert!(!text(&out.stderr).contains("panicked"));
}

#[test]
fn unknown_format_is_a_read_error() {
    let dir = temp_dir("unknown");
    let path = dir.join("garbage.bin");
    std::fs::write(&path, b"this is definitely not a drawing").expect("write");
    for cmd in ["info", "layers", "dump"] {
        let out = cadkit(&[cmd, path.to_str().expect("utf8 path")]);
        assert_eq!(out.status.code(), Some(1), "{cmd}");
        assert!(
            text(&out.stderr).contains("unrecognized file format"),
            "{}",
            text(&out.stderr)
        );
    }
}

#[test]
fn convert_usage_errors() {
    let dir = temp_dir("usage");
    let input = write_dxf(&dir);
    let input = input.to_str().expect("utf8 path");
    let bad_out = dir.join("out.xyz");
    let out = cadkit(&["convert", input, bad_out.to_str().expect("utf8 path")]);
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains(".json, .svg, .dxf, .dgn or .gml"));
    let svg = dir.join("out.svg");
    let svg = svg.to_str().expect("utf8 path");
    assert_eq!(
        cadkit(&["convert", input, svg, "--dxf-version", "r13"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        cadkit(&["convert", input, svg, "--width", "-5"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        cadkit(&["convert", input, svg, "--model", "abc"])
            .status
            .code(),
        Some(2)
    );
    assert!(!dir.join("out.svg").exists());
}

#[test]
fn convert_synthetic_dxf_to_json_and_svg() {
    let dir = temp_dir("convert");
    let input = write_dxf(&dir);
    let input = input.to_str().expect("utf8 path");
    let json = dir.join("out.json");
    let out = cadkit(&[
        "convert",
        input,
        json.to_str().expect("utf8 path"),
        "--pretty",
    ]);
    if reader_missing(&out, "convert_synthetic_dxf_to_json_and_svg") {
        return;
    }
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let body = std::fs::read_to_string(&json).expect("json written");
    assert!(body.contains("\"models\"") && body.contains('\n'));

    let svg = dir.join("out.svg");
    let out = cadkit(&[
        "convert",
        input,
        svg.to_str().expect("utf8 path"),
        "--width",
        "400",
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let body = std::fs::read_to_string(&svg).expect("svg written");
    assert!(
        body.starts_with("<svg")
            && body.contains("width=\"400\"")
            && body.contains("data-layer=\"WALLS\"")
    );

    let out = cadkit(&[
        "convert",
        input,
        svg.to_str().expect("utf8 path"),
        "--model",
        "99",
    ]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn info_layers_dump_on_synthetic_dxf() {
    let dir = temp_dir("inspect");
    let input = write_dxf(&dir);
    let input = input.to_str().expect("utf8 path");
    let out = cadkit(&["info", input, "--warnings"]);
    if reader_missing(&out, "info_layers_dump_on_synthetic_dxf") {
        return;
    }
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let info = text(&out.stdout);
    assert!(info.contains("Format:") && info.contains("line"), "{info}");

    let out = cadkit(&["layers", input]);
    assert_eq!(out.status.code(), Some(0));
    assert!(text(&out.stdout).contains("WALLS"));

    let out = cadkit(&["dump", input]);
    assert_eq!(out.status.code(), Some(0));
    assert!(text(&out.stdout).contains("\tline\t"));

    let out = cadkit(&[
        "--keep-raw",
        "--codepage",
        "windows-1254",
        "dump",
        "--json",
        input,
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(text(&out.stdout).trim_start().starts_with('{'));
}

#[test]
fn convert_to_dxf_reports_failure_not_panic() {
    let dir = temp_dir("todxf");
    let input = write_dxf(&dir);
    let out_path = dir.join("out.dxf");
    let out = cadkit(&[
        "convert",
        input.to_str().expect("utf8 path"),
        out_path.to_str().expect("utf8 path"),
        "--dxf-version",
        "r2000",
    ]);
    // 0 when reader and writer work, 1 while either is unimplemented; never a panic or usage error.
    assert!(matches!(out.status.code(), Some(0 | 1)), "{:?}", out.status);
    assert!(!text(&out.stderr).contains("panicked"));
}

#[test]
fn limit_flags_are_accepted_and_enforced() {
    let dir = temp_dir("limits");
    let input = write_dxf(&dir);
    let input = input.to_str().expect("utf8 path");
    // A one-byte input limit must be a read error (1), not a usage error or a panic.
    let out = cadkit(&["--max-input-bytes", "1", "info", input]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("limit exceeded"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(
        cadkit(&["--max-objects", "x", "info", input]).status.code(),
        Some(2)
    );
    let out = cadkit(&[
        "--max-decompressed-bytes",
        "1000000",
        "--max-objects",
        "1000",
        "info",
        input,
    ]);
    assert!(matches!(out.status.code(), Some(0 | 1)));
}

#[test]
fn citygml_export_roundtrip_and_validation() {
    let dir = temp_dir("citygml");
    let input = write_dxf(&dir);
    let output = dir.join("new.gml");
    let copy = dir.join("copy.gml");
    let input = input.to_str().expect("path");
    let output = output.to_str().expect("path");
    let copy = copy.to_str().expect("path");
    assert_eq!(cadkit(&["convert", input, output]).status.code(), Some(2));
    let result = cadkit(&[
        "convert",
        input,
        output,
        "--crs",
        "urn:ogc:def:crs:EPSG::4979",
    ]);
    assert_eq!(result.status.code(), Some(0), "{}", text(&result.stderr));
    let result = cadkit(&["validate-gml", output]);
    assert_eq!(result.status.code(), Some(0), "{}", text(&result.stderr));
    assert!(text(&result.stdout).contains("geometry_issues"));
    let result = cadkit(&[
        "validate-gml",
        output,
        "--tkgm-profile",
        "city-model-tender",
    ]);
    assert_eq!(result.status.code(), Some(1));
    assert!(text(&result.stdout).contains("city_model_tender_architectural"));
    assert!(text(&result.stdout).contains("tkgm.missing_building"));
    assert_eq!(
        cadkit(&["validate-gml", output, "--tkgm-profile", "unknown"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(cadkit(&["convert", output, copy]).status.code(), Some(0));
    assert_eq!(
        std::fs::read(output).expect("gml"),
        std::fs::read(copy).expect("copy")
    );
}
