#!/usr/bin/env python3
"""Dev-time check: audit DXF files written by cadkit-dxf with ezdxf (not a cargo dependency).

Usage (from the repository root):

    .venv/Scripts/python -m pip install ezdxf
    python scripts/buildlock.py cargo test -p cadkit-dxf --test roundtrip   # writes the files
    .venv/Scripts/python crates/cadkit-dxf/tests/validate_with_ezdxf.py [DIR_OR_FILE ...]

The default directory is `target/tmp/dxf-out` (CARGO_TARGET_TMPDIR of the roundtrip test).
For every file the script loads it twice: strictly (`ezdxf.readfile`) and leniently
(`ezdxf.recover.readfile`), runs `doc.audit()` and prints one line per file with the number of
audit errors, audit fixes, structure errors found while loading and the entity histogram of
the modelspace. Pass `--quiet` to print failures only. The exit status is non-zero when any file has an error.
"""

from __future__ import annotations

import collections
import pathlib
import sys

import ezdxf
from ezdxf import recover


def check(path: pathlib.Path) -> tuple[int, str]:
    problems = 0
    notes = []
    try:
        strict = ezdxf.readfile(str(path))
        strict_audit = strict.audit()
        problems += len(strict_audit.errors)
        notes.append(f"strict-load ok, audit errors={len(strict_audit.errors)} fixes={len(strict_audit.fixes)}")
        for err in strict_audit.errors:
            notes.append(f"    ERROR {err.code} {err.message}")
        for fix in strict_audit.fixes[:5]:
            notes.append(f"    fix   {fix.code} {fix.message}")
    except Exception as exc:  # noqa: BLE001 - report every failure
        problems += 1
        notes.append(f"strict-load FAILED: {exc!r}")
    try:
        doc, auditor = recover.readfile(str(path))
        audit = doc.audit()
        errors = len(auditor.errors) + len(audit.errors)
        problems += errors
        hist = collections.Counter(e.dxftype() for e in doc.modelspace())
        notes.append(
            f"recover ok, structure errors={len(auditor.errors)} audit errors={len(audit.errors)} "
            f"fixes={len(audit.fixes)} layouts={len(doc.layouts)} modelspace={dict(sorted(hist.items()))}"
        )
        for err in list(auditor.errors) + list(audit.errors):
            notes.append(f"    ERROR {err.code} {err.message}")
    except Exception as exc:  # noqa: BLE001
        problems += 1
        notes.append(f"recover FAILED: {exc!r}")
    return problems, "\n  ".join(notes)


def main(argv: list[str]) -> int:
    quiet = "--quiet" in argv
    argv = [a for a in argv if a != "--quiet"]
    targets = [pathlib.Path(a) for a in argv] or [pathlib.Path("target/tmp/dxf-out")]
    files: list[pathlib.Path] = []
    for t in targets:
        files.extend(sorted(t.glob("*.dxf")) if t.is_dir() else [t])
    if not files:
        print("no .dxf files found")
        return 2
    total = 0
    for f in files:
        problems, text = check(f)
        total += problems
        if problems or not quiet:
            print(f"{'OK  ' if problems == 0 else 'FAIL'} {f.name}\n  {text}")
    print(f"\n{len(files)} file(s), {total} problem(s)")
    return 1 if total else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
