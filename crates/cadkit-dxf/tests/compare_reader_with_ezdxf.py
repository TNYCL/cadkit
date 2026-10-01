#!/usr/bin/env python3
"""Dev-time oracle: compare what cadkit-dxf reads with what ezdxf reads from the same file.

    python scripts/buildlock.py cargo build -p cadkit-dxf --example dump
    .venv/Scripts/python crates/cadkit-dxf/tests/compare_reader_with_ezdxf.py [FILE.dxf ...]

Without arguments every ASCII sample in corpus/public/acadsharp is used. For each file the
script compares per-type entity counts of the model space and of every block, the set of layer
names, the block names, and the summed geometry (LINE lengths, CIRCLE radii, ARC angles) with
ezdxf. (ezdxf drops DIMENSION entities that have no geometry block while loading, so files
written by cadkit from documents with block-less dimensions differ in that one count.)
Entity names map as follows: LWPOLYLINE/POLYLINE -> polyline (or mesh for polyface /
polygon meshes), SOLID/TRACE/3DFACE -> face, unmodelled types -> unknown.
"""

from __future__ import annotations

import collections
import json
import math
import pathlib
import subprocess
import sys

import ezdxf
from ezdxf import recover

ROOT = pathlib.Path(__file__).resolve().parents[3]
DUMP = ROOT / "target" / "debug" / "examples" / ("dump.exe" if sys.platform == "win32" else "dump")

KIND = {
    "LINE": "line", "POINT": "point", "CIRCLE": "circle", "ARC": "arc", "ELLIPSE": "ellipse",
    "LWPOLYLINE": "polyline", "SPLINE": "spline", "TEXT": "text", "MTEXT": "mtext", "INSERT": "insert",
    "HATCH": "hatch", "DIMENSION": "dimension", "SOLID": "face", "TRACE": "face", "3DFACE": "face",
    "LEADER": "leader", "IMAGE": "image", "VIEWPORT": "viewport",
}


def ezdxf_kind(e) -> str:
    t = e.dxftype()
    if t == "POLYLINE":
        return "mesh" if (e.is_poly_face_mesh or e.is_polygon_mesh) else "polyline"
    if t == "ATTDEF":
        return "unknown:ATTDEF"
    return KIND.get(t, "unknown:" + t)


def cadkit_kind(e: dict) -> str:
    k = e["kind"]
    if k["type"] == "unknown":
        return "unknown:" + k["type_name"].removeprefix("dxf.")
    return k["type"].replace("_", "")


def histogram(items) -> dict:
    return dict(sorted(collections.Counter(items).items()))


def geometry(entities_k, entities_e):
    """(sum of line lengths, sum of circle radii, sum of arc sweeps) for both readers."""
    def k_sum(es):
        ln = sum(math.dist((e["kind"]["start"]["x"], e["kind"]["start"]["y"], e["kind"]["start"]["z"]),
                           (e["kind"]["end"]["x"], e["kind"]["end"]["y"], e["kind"]["end"]["z"]))
                 for e in es if e["kind"]["type"] == "line")
        cr = sum(e["kind"]["radius"] for e in es if e["kind"]["type"] == "circle")
        ar = sum((e["kind"]["end_angle"] - e["kind"]["start_angle"]) % math.tau for e in es if e["kind"]["type"] == "arc")
        return ln, cr, ar

    def e_sum(es):
        ln = sum(math.dist(tuple(e.dxf.start), tuple(e.dxf.end)) for e in es if e.dxftype() == "LINE")
        cr = sum(e.dxf.radius for e in es if e.dxftype() == "CIRCLE")
        ar = sum(math.radians((e.dxf.end_angle - e.dxf.start_angle) % 360) for e in es if e.dxftype() == "ARC")
        return ln, cr, ar

    return k_sum(entities_k), e_sum(entities_e)


def compare(path: pathlib.Path) -> list[str]:
    problems: list[str] = []
    cad = json.loads(subprocess.run([str(DUMP), str(path)], capture_output=True, check=True).stdout)
    doc, _ = recover.readfile(str(path))

    # Layers (case-insensitive names).
    k_layers = sorted(l["name"].lower() for l in cad["layers"])
    # ezdxf adds a "Defpoints" layer to documents that lack one.
    e_layers = sorted(l.dxf.name.lower() for l in doc.layers if l.dxf.name.lower() != "defpoints" or "defpoints" in k_layers)
    if k_layers != e_layers:
        problems.append(f"layers differ: cadkit-only={set(k_layers) - set(e_layers)} ezdxf-only={set(e_layers) - set(k_layers)}")

    # Model space (and the paper-space layouts, in total).
    k_model = [e for m in cad["models"] if m["kind"] == "model" for e in m["entities"]]
    e_model = list(doc.modelspace())
    hk = histogram(cadkit_kind(e) for e in k_model)
    he = histogram(ezdxf_kind(e) for e in e_model)
    if hk != he:
        problems.append(f"model space histogram differs:\n    cadkit {hk}\n    ezdxf  {he}")
    (kl, kc, ka), (el, ec, ea) = geometry(k_model, e_model)
    for name, a, b in (("line length", kl, el), ("circle radius", kc, ec), ("arc sweep", ka, ea)):
        if not math.isclose(a, b, rel_tol=1e-9, abs_tol=1e-9):
            problems.append(f"{name} sum differs: cadkit {a!r} ezdxf {b!r}")

    # Paper space: all layouts together.
    k_paper = [e for m in cad["models"] if m["kind"] != "model" for e in m["entities"]]
    e_paper = [e for layout in doc.layouts if layout.name != "Model" for e in layout]
    hk = histogram(cadkit_kind(e) for e in k_paper)
    he = histogram(ezdxf_kind(e) for e in e_paper)
    if hk != he:
        problems.append(f"paper space histogram differs:\n    cadkit {hk}\n    ezdxf  {he}")

    # Blocks.
    e_blocks = {b.name.lower(): b for b in doc.blocks if not b.name.lower().startswith(("*model_space", "*paper_space"))}
    e_blocks = {n: b for n, b in e_blocks.items() if not n.startswith(("$model_space", "$paper_space"))}
    k_blocks = {b["name"].lower(): b for b in cad["blocks"]}
    if sorted(e_blocks) != sorted(k_blocks):
        problems.append(f"block names differ: cadkit-only={set(k_blocks) - set(e_blocks)} ezdxf-only={set(e_blocks) - set(k_blocks)}")
    for name, kb in k_blocks.items():
        eb = e_blocks.get(name)
        if eb is None:
            continue
        hk = histogram(cadkit_kind(e) for e in kb["entities"])
        he = histogram(ezdxf_kind(e) for e in eb)
        if hk != he:
            problems.append(f"block {name} histogram differs: cadkit {hk} ezdxf {he}")
    print(f"  model {sum(hk_ for hk_ in histogram(cadkit_kind(e) for e in k_model).values())} entities, "
          f"paper {len(k_paper)}, blocks {len(k_blocks)}, layers {len(k_layers)}")
    return problems


def main(argv: list[str]) -> int:
    files = [pathlib.Path(a) for a in argv] or sorted((ROOT / "corpus" / "public" / "acadsharp").glob("sample_*_ascii.dxf"))
    if not DUMP.exists():
        print(f"build the example first: {DUMP}")
        return 2
    bad = 0
    for f in files:
        print(f.name)
        problems = compare(f)
        for p in problems:
            print("  DIFF", p)
        bad += len(problems)
    print(f"\n{len(files)} file(s), {bad} difference(s)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
