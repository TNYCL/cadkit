import json
import pathlib

import pytest

import cadkit
from conftest import REPO, read_or_skip


def test_import_and_exceptions():
    assert isinstance(cadkit.__version__, str)
    for exc in (
        cadkit.UnknownFormatError,
        cadkit.UnsupportedError,
        cadkit.LimitExceededError,
        cadkit.InvalidDataError,
    ):
        assert issubclass(exc, cadkit.CadkitError)
    assert issubclass(cadkit.CadkitError, Exception)


def test_detect_garbage_and_empty():
    assert cadkit.detect(b"") is None
    assert cadkit.detect(b"definitely not a drawing") is None


def test_detect_dwg_header():
    result = cadkit.detect(b"AC1032" + b"\0" * 100)
    if result is None:
        pytest.skip("DWG sniffing not implemented yet")
    assert result == "dwg"


def test_detect_dxf(dxf_bytes):
    result = cadkit.detect(dxf_bytes)
    if result is None:
        pytest.skip("DXF sniffing not implemented yet")
    assert result == "dxf"


def test_garbage_raises_unknown_format():
    with pytest.raises(cadkit.UnknownFormatError):
        cadkit.read(b"this is not a CAD file at all")
    with pytest.raises(cadkit.CadkitError):
        cadkit.read(b"")


def test_missing_file_raises_oserror(tmp_path):
    with pytest.raises(FileNotFoundError):
        cadkit.read(tmp_path / "nope.dxf")


def test_bad_source_type():
    with pytest.raises(TypeError):
        cadkit.read(123)  # type: ignore[arg-type]


def test_read_bytes_and_buffers(dxf_bytes):
    doc = read_or_skip(dxf_bytes)
    assert doc.format == "dxf"
    assert read_or_skip(bytearray(dxf_bytes)).format == "dxf"
    assert read_or_skip(memoryview(dxf_bytes)).format == "dxf"
    assert isinstance(doc, cadkit.Document)


def test_read_path_str_and_pathlike(tmp_path, dxf_bytes):
    p = tmp_path / "t.dxf"
    p.write_bytes(dxf_bytes)
    a = read_or_skip(p)
    b = read_or_skip(str(p))
    assert a.format == b.format == "dxf"
    assert len(list(a.entities())) == len(list(b.entities()))


def test_document_properties(dxf_doc):
    assert isinstance(dxf_doc.version, str)
    assert dxf_doc.application is None or isinstance(dxf_doc.application, str)
    assert dxf_doc.codepage is None or isinstance(dxf_doc.codepage, str)
    assert isinstance(dxf_doc.units, dict) and "unit" in dxf_doc.units
    assert isinstance(dxf_doc.layers, list)
    assert isinstance(dxf_doc.blocks, list)
    assert isinstance(dxf_doc.models, list) and dxf_doc.models
    assert isinstance(dxf_doc.warnings, list)
    text = repr(dxf_doc)
    assert "dxf" in text and "entities=" in text


def test_entities_and_dict(dxf_doc):
    ents = list(dxf_doc.entities())
    assert ents
    assert all(isinstance(e, dict) and "type" in e["kind"] for e in ents)
    kinds = {e["kind"]["type"] for e in ents}
    assert {"line", "circle"} <= kinds
    d = dxf_doc.to_dict()
    assert {"source", "units", "layers", "models", "warnings"} <= d.keys()


def test_entities_model_out_of_range(dxf_doc):
    with pytest.raises(IndexError):
        dxf_doc.entities(999)


def test_to_json(dxf_doc):
    compact = dxf_doc.to_json()
    pretty = dxf_doc.to_json(pretty=True)
    assert json.loads(compact) == json.loads(pretty) == dxf_doc.to_dict()
    assert "\n" in pretty and "\n" not in compact


def test_to_svg(dxf_doc):
    svg = dxf_doc.to_svg()
    assert "<svg" in svg and "</svg>" in svg
    assert "#ffffff" in svg
    small = dxf_doc.to_svg(model=0, width=400, stroke=2.0, background=None, text=False)
    assert "</svg>" in small
    with pytest.raises(IndexError):
        dxf_doc.to_svg(model=999)
    with pytest.raises(ValueError):
        dxf_doc.to_svg(width=0)


def test_to_dxf_versions(dxf_doc):
    with pytest.raises(ValueError):
        dxf_doc.to_dxf("r1999")
    try:
        out = dxf_doc.to_dxf("r2000")
    except cadkit.UnsupportedError:
        pytest.skip("DXF writer not implemented yet")
    assert "ENTITIES" in out


def test_keep_raw_and_codepage(dxf_bytes):
    doc = read_or_skip(dxf_bytes, keep_raw=True, codepage="windows-1254")
    assert doc.format == "dxf"


def _corpus_files():
    root = REPO / "corpus" / "public"
    if not root.is_dir():
        return []
    exts = {".dxf", ".dwg", ".dgn"}
    files = sorted(p for p in root.rglob("*") if p.suffix.lower() in exts)
    return files[:12]


@pytest.mark.parametrize("path", _corpus_files() or [None], ids=lambda p: p.name if p else "no-corpus")
def test_corpus_files(path: "pathlib.Path | None"):
    if path is None:
        pytest.skip("corpus/public not present")
    doc = read_or_skip(path)
    assert doc.format in {"dwg", "dxf", "dgn_v7", "dgn_v8"}
    assert isinstance(doc.models, list)
    assert "<svg" in doc.to_svg()
    json.loads(doc.to_json())


def test_limits_exceeded(dxf_bytes):
    with pytest.raises(cadkit.LimitExceededError):
        cadkit.read(dxf_bytes, max_input_bytes=10)
    assert cadkit.read(dxf_bytes, max_depth=64, max_objects=None).format == "dxf"


def test_to_svg_options(dxf_doc):
    assert "</svg>" in dxf_doc.to_svg(expand_blocks=False, images=False)
    for bad in (0, -1.0, float("nan"), float("inf")):
        with pytest.raises(ValueError):
            dxf_doc.to_svg(stroke=bad)


def test_inspect_buffers_paths_and_limits(dxf_bytes, tmp_path):
    path = tmp_path / "header.dxf"
    path.write_bytes(dxf_bytes)
    for source in (dxf_bytes, bytearray(dxf_bytes), memoryview(dxf_bytes), path, str(path)):
        summary = cadkit.inspect(source)
        assert summary["format"] == "dxf"
        assert summary["version"] == "AC1015"
        assert summary["units"]["unit"] == "millimeter"
        with pytest.raises(cadkit.LimitExceededError):
            cadkit.inspect(source, max_input_bytes=1)


def test_document_operations_are_immutable(dxf_doc):
    original = dxf_doc.to_dict()
    converted = dxf_doc.convert_units("meter")
    assert converted.units["unit"] == "meter"
    line = next(e for e in converted.entities() if e["kind"]["type"] == "line")
    assert line["kind"]["end"]["x"] == pytest.approx(0.1)
    assert dxf_doc.to_dict() == original
    assert dxf_doc.explode_inserts().to_dict() == original
    with pytest.raises(ValueError):
        dxf_doc.convert_units("invalid-unit")


def test_explode_insert_geometry():
    data = b"0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nB\n10\n0\n20\n0\n0\nLINE\n10\n0\n20\n0\n11\n2\n21\n0\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nB\n10\n10\n20\n0\n0\nENDSEC\n0\nEOF\n"
    doc = cadkit.read(data)
    expanded = doc.explode_inserts()
    assert next(doc.entities())["kind"]["type"] == "insert"
    line = next(expanded.entities())["kind"]
    assert line["type"] == "line"
    assert line["start"]["x"] == pytest.approx(10)
    assert line["end"]["x"] == pytest.approx(12)


def test_dxf_bytes_codepage_and_hidden_svg(dxf_bytes):
    data = dxf_bytes.replace(b"WALLS\n70\n0", b"WALLS\n70\n1")
    doc = cadkit.read(data)
    assert 'data-layer="WALLS"' not in doc.to_svg()
    assert 'data-layer="WALLS"' in doc.to_svg(include_hidden_layers=True)
    out = doc.to_dxf_bytes("r2000", codepage="DOS857")
    assert isinstance(out, bytes)
    assert cadkit.read(out).format == "dxf"
    with pytest.raises(ValueError):
        doc.to_dxf_bytes("unknown")


def test_v8seed_validation():
    with pytest.raises(cadkit.CadkitError):
        cadkit.V8Seed(b"not a seed")
    assert "V8Seed" in cadkit.__all__ and "inspect" in cadkit.__all__


def test_v8seed_public_roundtrip(dxf_doc):
    path = REPO / "corpus/public/gdal/test_dgnv8.dgn"
    if not path.exists():
        pytest.skip("public seed missing")
    seed = cadkit.V8Seed(path)
    assert cadkit.V8Seed(seed).to_bytes() == seed.to_bytes() == path.read_bytes()
    assert isinstance(seed.units, dict)
    assert isinstance(seed.is_3d, bool)
    assert isinstance(seed.is_nonempty, bool)
    assert isinstance(seed.has_raster_template, bool)
    prepared = seed.prepare_document(dxf_doc)
    options = json.dumps({"clear_seed_model": True})
    encoded = prepared.to_dgn(seed, options)
    assert encoded == prepared.to_dgn(seed.to_bytes(), options)
    assert cadkit.read(encoded).format == "dgn_v8"


def test_legacy_dxf_bytes_preserve_turkish_text():
    drawing = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1021\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nTEXT\n10\n0\n20\n0\n40\n1\n1\nŞğİı\n0\nENDSEC\n0\nEOF\n"
    encoded = cadkit.read(drawing.encode()).to_dxf_bytes("r2000", "DOS857")
    assert "Şğİı".encode("cp857") in encoded
    decoded = cadkit.read(encoded)
    assert next(decoded.entities())["kind"]["value"] == "Şğİı"
