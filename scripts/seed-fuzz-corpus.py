#!/usr/bin/env python3
"""Seeds fuzz/corpus/<target>/ from corpus/public/ and synthetic CityGML samples.

    python scripts/seed-fuzz-corpus.py

Only corpus/public and synthetic in-script samples are used; corpus/private is never read.
"""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PUBLIC = ROOT / "corpus" / "public"
OUT = ROOT / "fuzz" / "corpus"

TARGETS = {
    "read_any": {".dwg", ".dgn", ".dxf", ".gml"},
    "dgn": {".dgn"},
    "dgn_write": {".dgn"},
    "gml": {".gml"},
    "dwg": {".dwg"},
    "dxf": {".dxf"},
    "svg": {".dwg", ".dgn", ".dxf", ".gml"},
    "dxf_roundtrip": {".dxf"},
}

SYNTHETIC_GML = {
    "synthetic_building_solid.gml": """<?xml version="1.0" encoding="UTF-8"?>
<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0"
                xmlns:bldg="http://www.opengis.net/citygml/building/2.0"
                xmlns:gen="http://www.opengis.net/citygml/generics/2.0"
                xmlns:gml="http://www.opengis.net/gml"
                xmlns:xlink="http://www.w3.org/1999/xlink">
  <gml:boundedBy>
    <gml:Envelope srsName="EPSG:4979" srsDimension="3">
      <gml:lowerCorner>0 0 0</gml:lowerCorner>
      <gml:upperCorner>10 10 4</gml:upperCorner>
    </gml:Envelope>
  </gml:boundedBy>
  <core:cityObjectMember>
    <bldg:Building gml:id="bldg_1">
      <gen:stringAttribute name="usage"><gen:value>residential</gen:value></gen:stringAttribute>
      <gen:doubleAttribute name="height_m"><gen:value>4.0</gen:value></gen:doubleAttribute>
      <bldg:lod2Solid>
        <gml:Solid gml:id="solid_1">
          <gml:exterior>
            <gml:CompositeSurface>
              <gml:surfaceMember><gml:Polygon gml:id="p_bot"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">0 0 0 0 10 0 10 10 0 10 0 0 0 0 0</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember>
              <gml:surfaceMember><gml:Polygon gml:id="p_top"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">0 0 4 10 0 4 10 10 4 0 10 4 0 0 4</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember>
              <gml:surfaceMember><gml:Polygon gml:id="p_s"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">0 0 0 10 0 0 10 0 4 0 0 4 0 0 0</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember>
              <gml:surfaceMember><gml:Polygon gml:id="p_e"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">10 0 0 10 10 0 10 10 4 10 0 4 10 0 0</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember>
              <gml:surfaceMember><gml:Polygon gml:id="p_n"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">10 10 0 0 10 0 0 10 4 10 10 4 10 10 0</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember>
              <gml:surfaceMember><gml:Polygon gml:id="p_w"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">0 10 0 0 0 0 0 0 4 0 10 4 0 10 0</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember>
            </gml:CompositeSurface>
          </gml:exterior>
        </gml:Solid>
      </bldg:lod2Solid>
    </bldg:Building>
  </core:cityObjectMember>
</core:CityModel>
""",
    "synthetic_generic_geometry.gml": """<?xml version="1.0" encoding="UTF-8"?>
<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0"
                xmlns:gen="http://www.opengis.net/citygml/generics/2.0"
                xmlns:gml="http://www.opengis.net/gml">
  <core:cityObjectMember>
    <gen:GenericCityObject gml:id="gen_poly">
      <gen:intAttribute name="layer_index"><gen:value>1</gen:value></gen:intAttribute>
      <gen:lod2Geometry>
        <gml:Polygon gml:id="poly_hole" srsName="urn:cadkit:local" srsDimension="3">
          <gml:exterior>
            <gml:LinearRing>
              <gml:posList srsDimension="3">0 0 0 20 0 0 20 20 0 0 20 0 0 0 0</gml:posList>
            </gml:LinearRing>
          </gml:exterior>
          <gml:interior>
            <gml:LinearRing>
              <gml:posList srsDimension="3">5 5 0 5 15 0 15 15 0 15 5 0 5 5 0</gml:posList>
            </gml:LinearRing>
          </gml:interior>
        </gml:Polygon>
      </gen:lod2Geometry>
    </gen:GenericCityObject>
  </core:cityObjectMember>
  <core:cityObjectMember>
    <gen:GenericCityObject gml:id="gen_circle">
      <gen:lod2Geometry>
        <gml:Curve gml:id="curve_circle" srsName="urn:cadkit:local" srsDimension="3">
          <gml:segments>
            <gml:Circle numArc="1">
              <gml:posList srsDimension="3">5 0 0 0 5 0 -5 0 0</gml:posList>
            </gml:Circle>
          </gml:segments>
        </gml:Curve>
      </gen:lod2Geometry>
    </gen:GenericCityObject>
  </core:cityObjectMember>
</core:CityModel>
""",
}


def main() -> int:
    if not PUBLIC.is_dir():
        print("corpus/public is missing; run scripts/fetch-corpus.py first", file=sys.stderr)
        return 1
    files = [p for p in PUBLIC.rglob("*") if p.is_file()]
    total = 0
    for target, exts in TARGETS.items():
        dest = OUT / target
        dest.mkdir(parents=True, exist_ok=True)
        for src in files:
            if src.suffix.lower() in exts:
                shutil.copyfile(src, dest / f"{src.parent.name}_{src.name}")
                total += 1
        if ".gml" in exts:
            for name, xml in SYNTHETIC_GML.items():
                (dest / name).write_text(xml, encoding="utf-8")
                total += 1
    print(f"seeded {total} files into {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
