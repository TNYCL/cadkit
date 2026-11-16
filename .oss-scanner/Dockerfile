# Build image for Anthropic's OSS Scanner (https://red.anthropic.com/oss-scanner/).
#
# The scanner builds this file with the repository as the build context and with network
# access, then audits the result in /src with no network at all. Everything an auditor needs
# offline is therefore fetched and built here: crate sources, test binaries, the public sample
# corpus, the fuzz targets and the Python binding.
#
# Try it locally from the repository root:
#   docker build -f .oss-scanner/Dockerfile -t cadkit-oss-scanner .
#   docker run --rm --network none cadkit-oss-scanner cargo test --locked
FROM rust:1-trixie

ENV DEBIAN_FRONTEND=noninteractive

# cmake and the C/C++ compilers build the C API examples; python3 runs the corpus scripts
# and hosts the Python binding.
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential ca-certificates clang cmake curl git pkg-config \
      python3 python3-dev python3-pip python3-venv \
    && rm -rf /var/lib/apt/lists/*

# Scanner contract: the checkout lives inside the image, at /src.
COPY . /src
WORKDIR /src

# Frame pointers keep backtraces and profiles readable; the dev and fuzz profiles carry
# debug info already. Parallelism comes from .cargo/config.toml (4 jobs).
ENV RUSTFLAGS="-C force-frame-pointers=yes"

# Crate sources for every workspace member and every target platform.
RUN cargo fetch --locked

# Public sample files (DWG, DGN, DXF), pinned by SHA-256 in the script. Corpus tests skip
# silently when the files are missing, so a failed download must fail the build.
RUN python3 scripts/fetch-corpus.py

# The workspace as CI builds it: libraries, the `cadkit` CLI, the C API, and every test,
# example and bench target.
RUN cargo build --locked --all-targets

# Fuzzing: nightly toolchain, cargo-fuzz, the eight targets prebuilt, corpora seeded from the
# public samples. Best effort, because a broken nightly must not fail the whole image. The
# fuzz workspace is built without --locked, as in CI.
RUN (rustup toolchain install nightly --profile minimal \
      && cargo install cargo-fuzz --locked \
      && python3 scripts/seed-fuzz-corpus.py \
      && cargo +nightly fuzz build) \
    || echo "WARNING: fuzz tooling is incomplete; see the log above"

# Python binding (PyO3, FFI boundary) installed into a virtual environment. Best effort.
RUN (python3 -m venv /opt/cadkit-venv \
      && /opt/cadkit-venv/bin/pip install --no-cache-dir "maturin>=1.9.3,<2.0" pytest \
      && cd crates/cadkit-py \
      && VIRTUAL_ENV=/opt/cadkit-venv /opt/cadkit-venv/bin/maturin develop --locked) \
    || echo "WARNING: the Python binding was not built; see the log above"

# Nothing after this point may need the network.
ENV CARGO_NET_OFFLINE=true

# Run the tests once so the image is known to work; a failing test is reported, not fatal.
RUN cargo test --locked || echo "WARNING: cargo test reported failures"
