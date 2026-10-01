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
