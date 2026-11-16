# How we would like reports written

## Layout and length

One root cause per report, about one page, with these sections in this order:

1. Summary: one or two sentences, naming the crate and the function.
2. Impact: what an attacker who controls only the input file achieves, and the severity
   according to the rubric in threat_model.md.
3. Affected code: file path and line, at the commit that was scanned.
4. Reproduction.
5. Root cause.
6. Suggested fix.

Start the title with the crate name, for example "cadkit-dwg: unbounded allocation in
section map decode".

## What every report should state

- The commit hash that was scanned, and whether the latest release tag has the same code.
- The entry point: Rust API function, CLI command, C API function, Python or WebAssembly
  call.
- The limits in effect: Limits::new() (the default), Limits::strict(), or custom values.
- The build profile (debug or release). Say so when a bug needs debug assertions or
  overflow checks to show.
- What was observed: the panic message and location, peak memory, run time, or the
  sanitizer output.
- For resource bugs: input size against memory or time used.

## Reproduction

- Prefer a self-contained Rust unit test that builds the input bytes in the test itself
  and calls the public API, with comments that explain the layout of the bytes. This is
  how the project writes its own fixtures.
- Otherwise give the exact command and the input as a hex dump, or a short generator
  script, and the SHA-256 of the input.
- Keep the input minimal. When it is derived from a file under corpus/public, name that
  file and describe the mutation (offset, old bytes, new bytes) instead of attaching the
  whole file.
- Bound every reproduction of a hang or of memory growth with a timeout or a memory limit.

## Style

Plain Markdown, no tables wider than 100 columns. Short declarative sentences. Call the
formats DWG, DGN, DXF and CityGML; do not use vendor or product names. State uncertainty
plainly: say which parts were run and which were inferred from reading the code.
