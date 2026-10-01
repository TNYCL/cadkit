"""CityGML yapısını koruma ve DGN bayt çıktısı için bütünleştirme testleri."""
import json
from pathlib import Path
import pytest
import cadkit

XML = b"<CityModel xmlns='http://www.opengis.net/citygml/2.0'/>"
DXF = b"0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1\n21\n2\n0\nENDSEC\n0\nEOF\n"

def test_native_citygml_and_json():
    native = cadkit.read_citygml(XML)
    assert native.validate()["elements"] == 1
    assert cadkit.detect(native.to_gml()) == "city_gml"
    assert cadkit.CityGmlDocument.from_json(native.to_json()).to_gml() == native.to_gml()
    assert native.to_document().format == "city_gml"
    with pytest.raises(cadkit.InvalidDataError):
        cadkit.read_citygml(b"<!DOCTYPE a>" + XML)

def test_generic_export_and_dgn_bytes():
    doc = cadkit.read(DXF)
    gml = doc.to_citygml("urn:ogc:def:crs:EPSG::4979")
    assert b"GenericCityObject" in gml
    assert cadkit.read_citygml(gml).validate()["ids"] == 1
    path = Path(__file__).resolve().parents[3] / "corpus/public/gdal/test_dgnv8.dgn"
    if not path.exists():
        pytest.skip("public seed missing")
    seed = path.read_bytes()
    source = doc.to_dict()
    source["models"][0]["is_3d"] = cadkit.read(seed).to_dict()["models"][0]["is_3d"]
    rebuilt = cadkit.Document.from_json(json.dumps(source))
    result = rebuilt.to_dgn(seed, json.dumps({"clear_seed_model": True}))
    assert isinstance(result, bytes)
    assert cadkit.read(result).format == "dgn_v8"
    assert len(cadkit.read(result).to_dict()["models"][0]["entities"]) == 1
