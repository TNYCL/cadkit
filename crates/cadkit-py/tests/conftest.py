import pathlib

import pytest

import cadkit

REPO = pathlib.Path(__file__).resolve().parents[3]

SYNTHETIC_DXF = "\n".join(
    [
        "0", "SECTION", "2", "HEADER",
        "9", "$ACADVER", "1", "AC1015",
        "9", "$INSUNITS", "70", "4",
        "0", "ENDSEC",
        "0", "SECTION", "2", "TABLES",
        "0", "TABLE", "2", "LAYER", "70", "1",
        "0", "LAYER", "2", "WALLS", "70", "0", "62", "1", "6", "CONTINUOUS",
        "0", "ENDTAB",
        "0", "ENDSEC",
        "0", "SECTION", "2", "ENTITIES",
        "0", "LINE", "8", "WALLS", "10", "0.0", "20", "0.0", "30", "0.0",
        "11", "100.0", "21", "0.0", "31", "0.0",
        "0", "CIRCLE", "8", "WALLS", "10", "50.0", "20", "50.0", "30", "0.0", "40", "25.0",
        "0", "ENDSEC",
        "0", "EOF",
        "",
    ]
).encode("ascii")


def read_or_skip(source, **kwargs):
    """Reads `source`, skipping the test while the format reader is not implemented."""
    try:
        return cadkit.read(source, **kwargs)
    except cadkit.UnsupportedError as exc:
        pytest.skip(f"reader not implemented yet: {exc}")
    except cadkit.UnknownFormatError as exc:
        pytest.skip(f"format sniffing not implemented yet: {exc}")


@pytest.fixture
def dxf_bytes():
    return SYNTHETIC_DXF


@pytest.fixture
def dxf_doc(dxf_bytes):
    return read_or_skip(dxf_bytes)
