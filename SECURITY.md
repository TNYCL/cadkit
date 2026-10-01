# Security policy

## Stance on untrusted input

cadkit parses binary file formats that are routinely received from third parties, so every
input is treated as hostile:

- Library code must not panic on any input; the workspace lints deny `unwrap`, `expect`,
  `panic!` and unchecked slice indexing.
- Every size, count and offset read from a file is validated against `ReadOptions::limits`
  (input size, decompressed size, object count, nesting depth, string length, vertex count)
  before memory is allocated or a loop runs. Recursion is bounded by `max_depth`.
- Readers fail only when no useful document can be built; recoverable damage becomes a
  `Warning` on the document.
- Pure Rust throughout the readers. `unsafe` exists only in the FFI crates (`cadkit-py`,
  `cadkit-wasm`, `cadkit-capi`), with a `// SAFETY:` comment on each block. The C API
  catches panics at the boundary and null-checks every pointer.
- Readers are fuzzed (`fuzz/`) in CI, and public samples are truncated and bit-flipped in
  tests.

The default limits are generous. If you process untrusted files, lower them through
`ReadOptions` (Rust API) or `cadkit_options` (C API); the Python, WebAssembly and CLI
bindings are gaining limit options and may not expose all of them yet, so check their
documentation. In any case run conversions of untrusted files in a sandboxed process with a
memory and time limit; parsing a format is not a security boundary by itself.

Out of scope: vulnerabilities in other software that consumes cadkit output (for example a
viewer that renders the generated SVG without sanitizing text), and denial of service by
inputs that are large but within the configured limits.

## Reporting a vulnerability

Please report privately and do not open a public issue:

- Use GitHub private vulnerability reporting on the repository
  (Security tab, "Report a vulnerability"), or
- email the maintainer listed on the GitHub profile of TNYCL.

Include the affected version or commit, a minimal input file (or the generator), and what
you observed (panic message, memory growth, hang). Do not send drawings that contain
confidential data; a reduced or synthetic file is preferred.

You can expect an acknowledgement within a few days and a fix or mitigation plan after
triage. Reporters are credited in the changelog unless they prefer to stay anonymous.

Supported versions: only the latest release (and the main branch) receive fixes while the
project is pre-1.0.
