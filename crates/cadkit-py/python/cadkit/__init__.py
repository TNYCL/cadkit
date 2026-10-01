"""Read DWG, DGN and DXF drawings into one format-neutral model."""

from cadkit._cadkit import (
    CadkitError,
    Document,
    CityGmlDocument,
    InvalidDataError,
    LimitExceededError,
    UnknownFormatError,
    UnsupportedError,
    __version__,
    detect,
    read,
    read_citygml,
)

__all__ = [
    "CadkitError",
    "Document",
    "CityGmlDocument",
    "InvalidDataError",
    "LimitExceededError",
    "UnknownFormatError",
    "UnsupportedError",
    "__version__",
    "detect",
    "read",
    "read_citygml",
]
