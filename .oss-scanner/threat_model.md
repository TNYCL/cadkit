# cadkit threat model

This file is read by Anthropic's OSS Scanner before it audits the repository. The public
policy is in `SECURITY.md`; where the two differ, `SECURITY.md` wins.

## What this project does and where untrusted input enters

cadkit reads DWG, DGN (V7 and V8), DXF (ASCII and binary) and CityGML 2.0 drawings into one
format-neutral document model, and writes DXF, seed-based DGN V8, CityGML 2.0, SVG and JSON.
It is a pure Rust library with a CLI and with Python, WebAssembly and C/C++ bindings.

Drawings routinely arrive from third parties, so **every input file is hostile**. The
attacker controls all bytes of:

- the file given to a reader: `cadkit::detect`, `inspect`, `read`, `read_with`,
  `read_from`, `open`, and each format crate's `sniff` and `read`;
- the seed file given to the DGN V8 writer (`cadkit::to_dgn`, `strip_dgn_seed` and
  related functions);
- by extension, every string, count and coordinate in the `Document` that a reader
  produced, when that document is passed on to a writer or to SVG/JSON output.

The caller is trusted: Rust code that uses the API, the person who runs the CLI and chooses
its arguments and output paths, and the `ReadOptions` / `Limits` they configure. A
`Document` built by hand in trusted code is trusted, except for the size of the output it
asks a writer to produce.

## Security goals

1. **Memory safety.** Readers and writers are safe Rust (`unsafe_code = "forbid"` at
   workspace level). `unsafe` exists only at the FFI boundaries: `crates/cadkit-capi`,
   `crates/cadkit-py`, `crates/cadkit-wasm`. No file content may cause undefined behaviour
   through any of them, when the caller follows the documented contract.
2. **No panic on any input.** Library code must return `Ok` or `Err` for every byte
   sequence. Workspace lints deny `unwrap`, `expect`, `panic!` and unchecked indexing
   outside tests. The C API additionally catches panics at the boundary.
3. **Bounded resources.** Every size, count and offset read from a file is validated
   against `ReadOptions::limits` (input size, decompressed size, object count, nesting
   depth, string length, vertices per entity and per document) **before** memory is
   allocated or a loop runs. Recursion is bounded by `max_depth`.
4. **No ambient access.** Parsing bytes touches neither the file system nor the network.
   XML parsing resolves no external entities and no remote resources. Only the functions
   that take a path open a file, and only that path.
5. **Well-formed output.** Strings taken from a drawing are escaped in SVG, CityGML/XML,
   DXF and JSON output, so file content cannot add elements, attributes, group codes or
   records of its own to the output.

## Components that matter most / least

Most important, in this order:

- `crates/cadkit-dwg`: bit-level DWG decoding, section and page maps, decompression,
  CRC and Reed-Solomon handling, object and handle streams.
- `crates/cadkit-dgn`: DGN V7 element stream and DGN V8 compound-file container, element
  and complex-element decode, and the seed-based writer.
- `crates/cadkit-dxf`: ASCII and binary DXF reader, and the DXF writer.
- `crates/cadkit-gml`: CityGML 2.0 XML reader and writer.
- `crates/cadkit-core`: `ByteReader`, limits, the model, document operations, text
  decoding (code pages), JSON and SVG output.
- `crates/cadkit`: format detection and the facade functions.
- `crates/cadkit-capi`: the C ABI, which holds nearly all `unsafe` code: pointer and
  length handling, ownership and lifetime of handles, panics crossing the boundary.
- `crates/cadkit-py` and `crates/cadkit-wasm`: binding glue, and whether limits reach the
  readers from these bindings.
- `crates/cadkit-cli`: argument handling and output paths.
- The CLI installers `site/public/install.sh` and `site/public/install.ps1`, which people
  pipe into a shell. They download a release archive, must check it against `SHA256SUMS`
  before anything is installed, and must write only into the chosen install directory.

Less important, still in scope: `crates/cadkit-geom` (analytic geometry on caller-supplied
`f64` data), `crates/cadkit-scene` and `crates/cadkit-gpu` (rendering of data the host
application already holds). Report panics, hangs and unbounded allocation there only when a
document produced by a reader can trigger them.

Out of scope:

- Dependencies (`cfb`, `quick-xml`, `flate2`, `encoding_rs`, `wgpu`, `pyo3` and others).
  If the faulty code is in a dependency, say so and name it; report it here only when
  cadkit uses the dependency incorrectly.
- Software that consumes cadkit output, for example a viewer that renders correctly
  escaped SVG text in an unsafe way.
- Denial of service by inputs that are large but stay within the configured limits. The
  default limits are generous on purpose (2 GiB input, 4 GiB decompressed, 20 million
  objects); `Limits::strict()` is the profile for untrusted multi-tenant input.
- Wrong geometry, missing entities and other fidelity problems. They are ordinary bugs
  unless they lead to one of the impacts below.
- Misuse of the C API against the contract documented in `cadkit.h` (dangling or
  wrong-sized pointers, use after free by the caller, freeing a document while another
  thread still reads it).
- The rest of `site/` (the project website), `fuzz/` harness code, test code, and the
  fetched files under `corpus/` and `refs/`.
- Build and release automation (`.github/workflows/`, `scripts/`), except for a flaw that
  lets someone without write access run code with repository secrets or change a
  published release file.

## How to exercise it

The image has everything built in `/src` and needs no network.

- All tests: `cargo test --locked`. One crate: `cargo test --locked -p cadkit-dwg`.
- CLI: `target/debug/cadkit` with the commands `inspect`, `info`, `layers`, `blocks` and
  `convert`; `-` reads the input from stdin.
- Public samples: `corpus/public/` (`acadsharp/`, `gdal/`, `safe/`, `tika/`); synthetic
  CityGML seeds are in `fuzz/corpus/gml/`.
- Fuzz targets (`fuzz/fuzz_targets/`): `read_any`, `dwg`, `dgn`, `dgn_write`, `dxf`,
  `dxf_roundtrip`, `gml`, `svg`. They are prebuilt and seeded in `fuzz/corpus/`:
  `cargo +nightly fuzz run dwg -- -max_total_time=120 -rss_limit_mb=2048`.
- C API: header in `crates/cadkit-capi/include/`, examples in
  `crates/cadkit-capi/examples/`, library in `target/debug/`.
- Python binding: `/opt/cadkit-venv/bin/python -c "import cadkit"`, tests in
  `crates/cadkit-py/tests`.
- Format knowledge: `docs/dwg/FORMAT_NOTES.md`, `docs/dgn/FORMAT_NOTES.md`,
  `docs/dxf/NOTES.md`, `docs/gml/FORMAT_NOTES.md`; design in `docs/ARCHITECTURE.md`.

Tests are expected to pass. A panic counts only when it comes from library code, not from
an assertion in a test or from a fuzz harness.

## How we rate severity

- **Critical**: memory corruption or other undefined behaviour reachable from file content
  alone through a safe Rust API, the CLI, or a binding used as documented, with a credible
  path to code execution or to disclosure of process memory. Writing or overwriting a file
  whose path is chosen by file content.
- **High**: undefined behaviour reachable from file content without a demonstrated path to
  exploitation. Reading a file or contacting a host named by file content (external
  entities, references to other files). Disclosure of memory or of other documents' data
  into output. A limit in `Limits::strict()` bypassed by a large factor, for example a few
  kilobytes of input that allocate gigabytes or run for minutes. An installer that
  installs a download it did not verify, or that writes outside the install directory.
- **Medium**: a process abort that cannot be caught (stack overflow from unbounded
  recursion, allocation failure from an unvalidated count), a hang or an endless loop,
  under the default limits. A size or count used before it is validated against the
  limits. File content that injects markup or records into SVG, CityGML, DXF or JSON
  output.
- **Low**: an ordinary panic in library code on some input (index out of bounds, slice
  range, `unwrap`, arithmetic overflow, division by zero), including one that only happens
  with debug assertions. These break the project's no-panic rule and are wanted, but one
  report per root cause is enough.
- **Not a vulnerability**: everything listed as out of scope above.

Raise a rating by one step when the bug is reachable through `cadkit::detect` or
`inspect`, which callers use on files they have not decided to trust yet.

## Anything to leave alone

- Do not report `unwrap`, `expect`, indexing or `panic!` inside `#[cfg(test)]` code,
  `tests/`, `benches/`, `examples/` or `fuzz/`.
- Do not report that the default limits allow large allocations for large valid files.
- Do not report missing support for a format version or entity type; unmodelled records
  become `EntityKind::Unknown` with a `Warning` by design.
