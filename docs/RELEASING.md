# Releasing cadkit

One tag publishes everything. `.github/workflows/release.yml` builds and tests every
artifact, waits for approval on the `release` environment, then publishes:

| Channel | Package | Contents |
|---|---|---|
| crates.io | `cadkit-core`, `cadkit-dgn`, `cadkit-dxf`, `cadkit-gml`, `cadkit-dwg`, `cadkit`, `cadkit-cli` | Rust sources; docs.rs builds the API documentation |
| PyPI | `pycadkit` (imports as `cadkit`) | abi3 wheels for Linux x86_64/aarch64 (manylinux), macOS arm64/x86_64, Windows x64, and an sdist |
| npm | `cadkit-wasm` | one package: `cadkit-wasm` (ES module, browser and bundlers) and `cadkit-wasm/node` (CommonJS) |
| GitHub Releases | `cadkit-<target>` | CLI for Linux x86_64/aarch64 (static musl), macOS arm64/x86_64, Windows x64 |
| GitHub Releases | `cadkit-capi-<target>` | C/C++ headers, shared and static libraries (Linux glibc 2.28+, macOS, Windows x64) |

`cadkit-py`, `cadkit-wasm` and `cadkit-capi` have `publish = false`: they are distributed
through PyPI, npm and GitHub Releases, not crates.io. The release also carries
`SHA256SUMS` and GitHub build-provenance attestations for every file
(`gh attestation verify <file> -R TNYCL/cadkit`). PyPI and npm attach their own
provenance through trusted publishing.

Archive names carry no version, so the newest build always has a stable link, for example
`https://github.com/TNYCL/cadkit/releases/latest/download/cadkit-x86_64-pc-windows-msvc.zip`.
`cargo binstall cadkit-cli` downloads these archives.

## Dry run

Run the workflow by hand from the Actions tab (or `gh workflow run release.yml --ref <branch>`).
It runs every build, test and package step, including `cargo publish --dry-run` for the whole
workspace, and publishes nothing. Do this before tagging whenever the packaging changed.

## One-time setup

Registry accounts belong to the maintainer; enable two-factor authentication on each.

1. **GitHub environment.** Settings → Environments → `release`: add yourself as a required
   reviewer and limit deployments to tags matching `v*`. Secrets for the first release
   (below) go into this environment, not the repository.
2. **PyPI.** Add a *pending* trusted publisher at <https://pypi.org/manage/account/publishing/>:
   project `pycadkit`, owner `TNYCL`, repository `cadkit`, workflow `release.yml`,
   environment `release`. No token is ever needed. A pending publisher does not reserve the
   name; the first upload does.
3. **crates.io** (trusted publishing works only for crates that already exist). For the
   first release, create an API token at <https://crates.io/settings/tokens> with the scopes
   `publish-new` and `publish-update`, crate pattern `cadkit*` and a short expiry, and save it
   as the `CARGO_REGISTRY_TOKEN` secret of the `release` environment. After the first
   release, add a trusted publisher on each crate's settings page (repository
   `TNYCL/cadkit`, workflow `release.yml`, environment `release`), then delete the secret and
   revoke the token. Without the secret the workflow uses trusted publishing.
4. **npm** (same restriction: the package must exist first). For the first release, create a
   granular access token at npmjs.com with read and write access, limited to publishing, with
   a short expiry, and save it as the `NPM_TOKEN` secret of the `release` environment. After
   the first release, add a trusted publisher in the `cadkit-wasm` package settings (GitHub
   Actions, `TNYCL/cadkit`, workflow `release.yml`, environment `release`), delete the secret
   and revoke the token.

## Each release

1. On a branch: `python scripts/release.py bump X.Y.Z`. It sets the workspace version and the
   internal dependency versions, refreshes `Cargo.lock` and turns `## [Unreleased]` in
   `CHANGELOG.md` into `## [X.Y.Z] - <date>` below a new empty `Unreleased` section.
   Edit the changelog section; it becomes the GitHub release notes.
2. `python scripts/release.py check --tag vX.Y.Z` must pass. Open a pull request, let CI pass,
   optionally run the dry run on the branch, and merge.
3. Tag the merge commit on `main` and push the tag:
   `git tag -a vX.Y.Z -m "cadkit X.Y.Z" && git push origin vX.Y.Z`.
4. When the builds pass, approve the `release` deployment on the workflow run. The three
   publish jobs start together; the GitHub release is created after all of them succeed.
5. Check docs.rs, the PyPI and npm pages, and the release assets.

A version with a suffix (`0.2.0-rc.1`) becomes a GitHub pre-release and npm dist-tag `next`.

## When something fails

- Before approval nothing is published: fix on `main`, delete the tag
  (`git push --delete origin vX.Y.Z && git tag -d vX.Y.Z`) and tag again.
- After approval, use *Re-run failed jobs*. Every publish step is idempotent: crates already
  on the index are skipped, PyPI uses `skip-existing`, npm skips an existing version.
- A published version cannot be replaced on any registry. If a release is broken, fix it in
  the next patch version; `cargo yank` and `npm deprecate` mark the bad one.

## License copies

Every package carries `LICENSE-MIT`, `LICENSE-APACHE` and `NOTICE`. Cargo and maturin package
only files inside the package directory, so each published crate directory and
`crates/cadkit-py` hold copies. After editing a root file, run
`python scripts/release.py sync-licenses`; the release check fails while a copy differs.
