# How we would like patches made

## Size and scope of a fix

- As narrow as the root cause allows. No refactoring, renaming or reformatting of code the
  fix does not need.
- When the same mistake exists in sibling code paths (another DWG version, another element
  type), fix them in the same patch and list them.
- Prefer to degrade over failing: keep reading, record a Warning with a stable code such
  as "dgn.unknown_element", and map what cannot be decoded to EntityKind::Unknown.
  Return an Error only when no useful document can be produced.
- Keep the public API as it is. The 0.5.x series only adds; cargo-semver-checks runs in CI.
- No new dependencies and no new unsafe code. unsafe is allowed only in cadkit-capi,
  cadkit-py and cadkit-wasm, and every block there carries a SAFETY comment.

## Code style

- Rust edition 2024, MSRV 1.88, rustfmt defaults; the result must pass
  cargo clippy --all-targets -- -D warnings.
- No unwrap, expect, panic! or slice indexing outside tests. Use
  cadkit_core::bytes::ByteReader, .get(), and checked or saturating arithmetic.
- Validate every size, count and offset read from the file against ReadOptions::limits
  before allocating or looping. Never call Vec::with_capacity with a value from the file
  unless it is capped. Bound recursion with limits.max_depth.
- Errors are cadkit_core::Error values with absolute byte offsets.
- Comments explain why (the format quirk, the specification section), not what. Code,
  comments and documentation are in English.

## Tests

- Add a unit test next to the fixed code, with a synthetic byte fixture built in the test
  and comments that document its layout. The test must fail before the fix and pass after.
- Do not add sample drawings to the repository, and do not weaken or delete an existing
  test to make a patch pass.
- Run cargo test --locked -p <crate> for the crate that changed.

## Provenance

cadkit is a clean-room implementation. A patch must come from the code in this repository,
its docs/ folder and the public format descriptions cited in docs/PROVENANCE.md. Do not
base a fix on, or copy from, GPL, LGPL or AGPL CAD software (for example LibreDWG,
libdxfrw, LibreCAD or QCAD), vendor SDKs or their headers, or decompiled code.

## Branch and commit message

- Against the main branch, as a unified diff that applies with git apply.
- Subject line in the form "fix(dwg): bound section map entries by the input size", with
  the crate short name in the parentheses (core, dwg, dgn, dxf, gml, capi, py, wasm, cli).
- Add a line under "Unreleased" in CHANGELOG.md that describes the change for users,
  without reproduction details.
