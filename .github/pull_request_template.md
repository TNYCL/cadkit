## Summary

- 

## Checklist

- [ ] No `unwrap`, `expect`, `panic!`, or unchecked slice indexing outside `#[cfg(test)]`
- [ ] Untrusted sizes/counts/recursion checked against `Limits` before allocating or recursing
- [ ] Clean-room compliance verified (no GPL/LGPL/AGPL or proprietary SDK references; new public specs/samples cited in `docs/PROVENANCE.md` if applicable)
- [ ] Unit/integration tests added with synthetic or public-corpus inputs (no private data)
- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and relevant crate tests pass
