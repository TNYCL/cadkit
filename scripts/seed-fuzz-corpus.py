#!/usr/bin/env python3
"""Copies public corpus samples into fuzz/corpus/<target>/ (git-ignored).

    python scripts/seed-fuzz-corpus.py

Only corpus/public is used; corpus/private is never read.
"""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PUBLIC = ROOT / "corpus" / "public"
OUT = ROOT / "fuzz" / "corpus"

TARGETS = {
    "read_any": {".dwg", ".dgn", ".dxf"},
    "dgn": {".dgn"},
    "dwg": {".dwg"},
    "dxf": {".dxf"},
    "svg": {".dwg", ".dgn", ".dxf"},
    "dxf_roundtrip": {".dxf"},
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
    print(f"seeded {total} files into {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
