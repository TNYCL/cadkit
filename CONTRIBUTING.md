# Contributing to cadkit

Thank you for helping. Read [AGENTS.md](AGENTS.md) and
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) first; they are binding for human and automated
contributors alike. This file summarizes the practical parts.

## Clean-room rules (summary)

cadkit must stay free of encumbered code.

- Allowed: the public Open Design Specification for DWG, ACadSharp (MIT), ezdgn (MIT),
  GDAL `frmts/dgn` (MIT/X), public sample files, your own hex analysis, public format
  write-ups. Attribute any ported logic.
- Forbidden: LibreDWG, libdxfrw, LibreCAD, QCAD or any GPL/LGPL/AGPL CAD code; ODA SDK
  material; Autodesk or Bentley SDKs; anything decompiled or disassembled. If you have read
  such code, say so before contributing in the affected area.
- Describe formats as "DWG", "DGN", "DXF". Do not use vendor product names in crate names,
  logos or claims of compatibility.

### Provenance ledger

Every source of format knowledge you rely on is recorded in
[docs/PROVENANCE.md](docs/PROVENANCE.md): add a row to the sources table, and one line to
"Attributions" for every ported algorithm or table (what, from where, which file). Format
knowledge you derive goes to `docs/<format>/FORMAT_NOTES.md` with the sample that showed it.

### Private data

`corpus/private/` holds client drawings and is git-ignored. Never copy those files, never
quote their strings, names, coordinates or paths in code, tests, docs, issues or logs. Tests
that use them must skip silently when the directory is missing and assert only structural
facts (counts, histograms, "parses without error"). Committed fixtures are synthetic or come
from the public corpus (fetched, not committed).

## Building and testing

This machine class has crashed under parallel builds, so all build commands go through the
lock, one at a time:

```sh
python scripts/buildlock.py cargo test -p cadkit-dxf
python scripts/buildlock.py cargo clippy -p cadkit-dxf --all-targets -- -D warnings
python scripts/buildlock.py cargo fmt --all -- --check
```

Prefer the narrowest command (`-p <crate>`). Never run builds in the background or with more
than 4 jobs. If you build without the lock on your own machine, that is your call; CI does.

Public samples for corpus tests:

```sh
python scripts/fetch-corpus.py
```

Code rules in short: edition 2024, MSRV 1.85, clippy clean with `-D warnings`, no `unwrap`,
`expect`, `panic!` or slice indexing in library code, every file-provided size checked
against `ReadOptions::limits` before allocating, recoverable problems become warnings.
`unsafe` is allowed only in `cadkit-py`, `cadkit-wasm` and `cadkit-capi`, each block with a
`// SAFETY:` comment.

## Fuzzing

Fuzz targets live in `fuzz/` (cargo-fuzz, excluded from the workspace): `read_any`, `dgn`,
`dwg`, `dxf`, `dxf_roundtrip`. libFuzzer needs a nightly toolchain and works on Linux and
macOS; on Windows the sanitizer runtime is usually missing, so use WSL or rely on CI.

```sh
python scripts/seed-fuzz-corpus.py                 # copies corpus/public samples into fuzz/corpus/
cd fuzz
python ../scripts/buildlock.py cargo +nightly fuzz run dxf -- -max_total_time=60
python ../scripts/buildlock.py cargo check --bins  # compile check, works everywhere
```

CI runs every target for 60 seconds on each pull request. Crashes land in `fuzz/artifacts/`
(git-ignored); reduce them to a unit test with a synthetic byte fixture.

## C API

`crates/cadkit-capi/include/cadkit.h` is hand-maintained. When you add or change an exported
function, update the header, `cadkit.hpp` if relevant, and [docs/c-api.md](docs/c-api.md).
`cargo test -p cadkit-capi` fails if an exported function is missing from the header.

## Pull requests

- Keep changes focused; add tests with synthetic fixtures that document the byte layout.
- Update docs and CHANGELOG.md (Unreleased section).
- Do not commit generated outputs, corpus files or anything under `corpus/`, `refs/`, `target/`.
- Code, comments and docs are in English.
- Report what works, what does not, evidence (samples and results) and open risks. Do not
  weaken a test to make it pass.
