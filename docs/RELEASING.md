# Releasing cadkit

One tag publishes everything. `.github/workflows/release.yml` builds and tests every
artifact, waits for approval on the `release` environment, then publishes:

| Channel | Package | Contents |
|---|---|---|
| crates.io | `cadkit-geom`, `cadkit-scene`, `cadkit-gpu`, `cadkit-core`, `cadkit-dgn`, `cadkit-dxf`, `cadkit-gml`, `cadkit-dwg`, `cadkit`, `cadkit-cli`, `cadkit-capi` | Rust sources; docs.rs builds the API documentation |
| PyPI | `pycadkit` (imports as `cadkit`) | abi3 wheels for Linux x86_64/aarch64 (manylinux), macOS arm64/x86_64, Windows x64, and an sdist |
| npm | `cadkit-wasm` | one package: `cadkit-wasm` (ES module, browser and bundlers) and `cadkit-wasm/node` (CommonJS) |
| GitHub Releases | `cadkit-<target>` | CLI for Linux x86_64/aarch64 (static musl), macOS arm64/x86_64, Windows x64 |
| GitHub Releases | `cadkit-capi-<target>` | C/C++ headers, shared and static libraries (Linux glibc 2.28+, macOS, Windows x64) |
| release.cadkit.org | every file of the GitHub release | `/vX.Y.Z/<file>` with the same `SHA256SUMS`, and `/latest.txt`; the CLI installers download from here |

`cadkit-py` and `cadkit-wasm` have `publish = false`: they are distributed through PyPI and
npm, not crates.io. `cadkit-capi` is published to crates.io (sources, first published at
0.5.3) and its prebuilt libraries ship on GitHub Releases. The release also carries
`SHA256SUMS` and GitHub build-provenance attestations for every file
(`gh attestation verify <file> -R TNYCL/cadkit`). PyPI and npm attach their own
provenance through trusted publishing.

Archive names carry no version, so the newest build always has a stable link, for example
`https://github.com/TNYCL/cadkit/releases/latest/download/cadkit-x86_64-pc-windows-msvc.zip`.
`cargo binstall cadkit-cli` downloads these archives.

## release.cadkit.org and the installers

`https://release.cadkit.org` is a Cloudflare R2 bucket (`cadkit-release`) behind a custom
domain. The `publish-mirror` job runs `python scripts/release.py mirror dist --version X.Y.Z`,
which checks the files against `SHA256SUMS`, uploads them to `/vX.Y.Z/`, downloads each one
again through the public host and compares its checksum, and then writes the version into
`/latest.txt`. Pre-releases and versions older than the current `latest.txt` leave that file
alone. A version directory is never changed after its release.

`https://cadkit.org/install.sh` and `https://cadkit.org/install.ps1`
([site/public/](../site/public/)) install the CLI: they read `latest.txt`, download the
archive for the platform and `SHA256SUMS` from `/vX.Y.Z/`, verify the checksum and copy the
binary into a user directory. When release.cadkit.org cannot be reached they take the same
files from the GitHub release. They are deployed with the website, not with a release, so an
installer fix needs no new version. CI runs both on Linux, macOS and Windows against the
published release (job `installer`).

To mirror an existing release by hand (the same command, with the credentials of step 5
below in the environment):

```sh
gh release download vX.Y.Z --dir dist
python scripts/release.py mirror dist --version X.Y.Z
```

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
5. **release.cadkit.org.** In the Cloudflare dashboard (R2 → Manage API tokens) create an
   account API token with *Object Read & Write*, limited to the bucket `cadkit-release`. Save
   its S3 credentials as the secrets `R2_ACCESS_KEY_ID` and `R2_SECRET_ACCESS_KEY`, and the
   Cloudflare account ID as `R2_ACCOUNT_ID`, in the `release` environment. R2 has no trusted
   publishing, so this token stays; it can write to that one bucket and nothing else. The
   bucket and its custom domain are described in [WEBSITE.md](WEBSITE.md).

## Each release

1. On a branch: `python scripts/release.py bump X.Y.Z`. It sets the workspace version and the
   internal dependency versions, refreshes `Cargo.lock` and turns `## [Unreleased]` in
   `CHANGELOG.md` into `## [X.Y.Z] - <date>` below a new empty `Unreleased` section.
   Edit the changelog section; it becomes the GitHub release notes. For the first release
   (0.1.0), also delete the "not released yet" note at the top of `README.md`.
2. `python scripts/release.py check --tag vX.Y.Z` must pass. Open a pull request, let CI pass,
   optionally run the dry run on the branch, and merge.
3. Tag the merge commit on `main` and push the tag:
   `git tag -a vX.Y.Z -m "cadkit X.Y.Z" && git push origin vX.Y.Z`.
4. When the builds pass, approve the `release` deployment on the workflow run. The four
   publish jobs start together; the GitHub release is created after all of them succeed.
5. Check docs.rs, the PyPI and npm pages, the release assets, and that
   `curl -fsSL https://cadkit.org/install.sh | sh` installs the new version.
6. Update the website to the new version: merge Dependabot's `cadkit-wasm` update for `site/`
   (or bump it by hand) once the npm package is published; see [WEBSITE.md](WEBSITE.md).

A version with a suffix (`0.2.0-rc.1`) becomes a GitHub pre-release and npm dist-tag `next`.

## When something fails

- Before approval nothing is published: fix on `main`, delete the tag
  (`git push --delete origin vX.Y.Z && git tag -d vX.Y.Z`) and tag again.
- After approval, use *Re-run failed jobs*. Every publish step is idempotent: crates already
  on the index are skipped, PyPI uses `skip-existing`, npm skips an existing version, and
  the mirror upload writes the same files again.
- A published version cannot be replaced on any registry. If a release is broken, fix it in
  the next patch version; `cargo yank` and `npm deprecate` mark the bad one.

## License copies

Every package carries `LICENSE-MIT`, `LICENSE-APACHE` and `NOTICE`. Cargo and maturin package
only files inside the package directory, so each published crate directory and
`crates/cadkit-py` hold copies. After editing a root file, run
`python scripts/release.py sync-licenses`; the release check fails while a copy differs.
