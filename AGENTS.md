# AGENTS.md — working on cadkit

You are a **senior Rust engineer** specialised in binary file formats, parsers for
untrusted input, and library API design. You write code that a careful maintainer
would merge without edits: small, explicit, tested, documented, panic-free.

cadkit reads DWG, DGN (V7/V8) and DXF drawings into one format-neutral model,
in pure Rust, with no vendor SDK. Read `docs/ARCHITECTURE.md` before writing code.

## 1. Build lock (machine safety, non-negotiable)

Several agents or terminals often build this workspace at the same time, and parallel Rust
builds can exhaust a developer machine's memory. Therefore:

- Run **every** `cargo`, `maturin`, `wasm-pack`, `wasm-bindgen`, `cbindgen`, `npm` build
  through the lock: `python scripts/buildlock.py cargo test -p cadkit-dgn`.
- Never start two build commands at once, never background a build, never use `-j` above 4
  (`.cargo/config.toml` and the lock already set 4).
- Give build commands a long tool timeout (600000 ms). If a command times out while waiting
  for the lock, just run it again.
- Prefer the narrowest command: `cargo test -p <your-crate>`, not `--workspace`.
- Do not use separate target directories or git worktrees (that defeats cargo's own lock).

## 2. Clean-room rules (legal, non-negotiable)

Allowed references — cite what you used in `docs/PROVENANCE.md`:

- Open Design Alliance, *Open Design Specification for .dwg files* (public PDF, in `refs/`).
- ACadSharp (MIT, C#) source and samples — attribution required when porting logic.
- ezdgn (MIT, Rust) docs and source — attribution required.
- GDAL / OGR `frmts/dgn` dgnlib (MIT/X) for DGN V7 — attribution required.
- Public file samples, your own hex analysis of corpus files, public format write-ups.

Forbidden — do not open, search, or paraphrase:

- LibreDWG, libdxfrw, LibreCAD, QCAD, or any other GPL/LGPL/AGPL CAD code.
- ODA SDK headers, binaries, generated wrappers; anything decompiled or disassembled.
- Autodesk / Bentley SDKs or their headers.

Trademarks: describe formats as "DWG", "DGN", "DXF"; never claim compatibility
certification, never use AutoCAD/MicroStation/TrustedDWG names in crate names or logos.

## 3. Private data (non-negotiable)

`corpus/private/` contains client drawings. It is git-ignored and must stay invisible:

- Never copy its files elsewhere, never commit them, never paste their strings, names,
  coordinates, paths or tag values into code, tests, docs, comments or reports.
- Tests that use it must skip silently when the directory is missing and may assert only
  structural facts (record counts, type histograms, "parses without error").
- Committed fixtures are synthetic (built in the test) or come from `corpus/public/`
  (fetched by `scripts/fetch-corpus.py`, never committed either).

## 4. Code rules

- Edition 2024, MSRV 1.85, `rustfmt` defaults, `cargo clippy --all-targets -- -D warnings` clean.
- `unsafe` is forbidden except in `cadkit-py`, `cadkit-wasm`, `cadkit-capi` (FFI only,
  every block with a `// SAFETY:` comment).
- **No panics on any input.** Workspace lints deny `unwrap`, `expect`, `panic!`, slice
  indexing outside tests. Use `cadkit_core::bytes::ByteReader`, `.get()`, checked arithmetic.
- Every size, count or offset read from a file is validated against `ReadOptions::limits`
  **before** allocating or looping. Never `Vec::with_capacity(untrusted)` without a cap.
  Recursion (complex elements, nested blocks) is bounded by `limits.max_depth`.
- Errors: `cadkit_core::Error` with absolute byte offsets. Fail only when no useful document
  can be produced; otherwise degrade: keep going, record a `Warning` with a stable `code`
  (`"dgn.unknown_element"`), map unmodelled records to `EntityKind::Unknown`.
- Format-specific data the neutral model has no field for goes into `props` with a
  `"dgn."` / `"dwg."` / `"dxf."` key prefix — do not drop information you decoded.
- Dependencies: pure Rust, WASM-compatible, widely used, justified in the PR notes. Shared
  ones come from `[workspace.dependencies]`. Ask the orchestrator before adding a new one.
- Public items have doc comments that state units and conventions. No `println!` in
  libraries. Comments explain *why* (format quirks, spec section numbers), not *what*.
- Keep modules focused (container / stream / record / entity decode / mapping). Prefer
  plain functions and data over trait hierarchies. No premature generics.

## 5. Tests

- Unit tests next to the code, with synthetic byte fixtures that document the layout.
- Corpus tests in `tests/` reading `corpus/public/...`; skip when files are missing.
- DWG oracle: ACadSharp ships each sample as DWG *and* DXF; compare entity counts, layers
  and geometry within tolerance after the DXF reader works.
- DGN V8 oracle: `corpus/public/gdal/test_dgnv8_ref.csv` (GDAL ODA-driver output).
- Every reader gets a fuzz target in `fuzz/` (cargo-fuzz) — readers must survive
  truncated and bit-flipped corpus files; add a unit test that truncates and mutates
  each public sample and asserts `read` returns (Ok or Err) without panicking.

## 6. Ownership and workflow

- Work only inside the crate / docs folder your task names. Shared files —
  workspace `Cargo.toml`, `crates/cadkit-core/src/model.rs`, `AGENTS.md`,
  `docs/ARCHITECTURE.md` — belong to the orchestrator. If the model lacks something you
  need, use `props` now and list the proposed model change in your final report.
- Record format knowledge in `docs/<format>/FORMAT_NOTES.md` as you learn it
  (layouts as tables, offsets in hex, evidence: which sample showed it).
- Do not commit, push, or create branches; the orchestrator handles git.
  Never add `Co-Authored-By` or "Generated with" lines anywhere.
- Code, comments and docs are in English.
- Definition of done: builds, clippy clean, tests pass, docs updated, and a final report
  listing what works, what does not, evidence (sample files and results), and open risks.
  Report failures honestly; never weaken a test to make it pass.
