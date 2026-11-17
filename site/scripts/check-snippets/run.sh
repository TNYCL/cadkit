#!/usr/bin/env bash
# Checks the code samples shown on the site against the released packages:
#
#   python, rust, c       the landing page snippet ("same task in four languages", snippets/)
#   docs-python           every docs/python/*.py, run in a venv against the fixtures
#   docs-rust             every docs/rust/*.rs, `cargo check`ed in one scratch crate
#                         (DOCS_RUN=1 also runs the binaries against the fixtures)
#   docs-c                every docs/c/*.c and *.cpp with gcc/g++ -fsyntax-only -Werror
#                         (CADKIT_CAPI_LIB_DIR=<dir with libcadkit_capi> also links and runs them)
#   docs-cli              every docs/cli/*.sh, run with `cadkit` on PATH (CADKIT_BIN, or a
#                         debug build of cadkit-cli through the build lock)
#
#   scripts/check-snippets/run.sh [target...]      (default: all of the above)
#
# The JavaScript samples run in `npm test` (test/snippets.test.mjs, test/docs-snippets.test.mjs).
# Fixtures: the sample plan, docs/fixtures/ (synthetic) and, when corpus/public/ exists, the
# public DWG and DGN V8 samples (scripts/fetch-corpus.py); samples that need them are skipped
# otherwise. Nothing from corpus/private/ is used.
#
# Environment:
#   WORK                 scratch directory (default: a new mktemp -d)
#   PYCADKIT_VERSION     pycadkit version installed into WORK/venv (default: site cadkit-wasm version)
#   CADKIT_CRATE         Rust dependency spec for the scratch crate (default: path to
#                        crates/cadkit of this checkout; e.g. '"=0.5.3"' for crates.io)
#   CADKIT_REGISTRY_VERSION  docs-rust: use cadkit, cadkit-geom, cadkit-scene and cadkit-gpu
#                        of this version from crates.io instead of the checkout (e.g. 0.5.3)
#   CARGO_TARGET_DIR     honoured; in this repository use the workspace target/ directory
#
# Cargo runs through scripts/buildlock.py (repository rule: one build at a time).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
site="$(cd "$here/../.." && pwd)"
repo="$(cd "$site/.." && pwd)"
snippets="$here/snippets"
sample="$site/public/samples/cadkit-sample-plan.dxf"
work="${WORK:-$(mktemp -d)}"
mkdir -p "$work"
docs="$here/docs"
targets=("$@")
[ ${#targets[@]} -eq 0 ] && targets=(python rust c docs-python docs-rust docs-c docs-cli)

venv="$work/venv"
ensure_venv() {
  local want="${PYCADKIT_VERSION:-$(node -p "JSON.parse(require('node:fs').readFileSync(process.argv[1], 'utf8')).dependencies['cadkit-wasm']" "$site/package.json")}"
  [ -x "$venv/bin/python" ] || python3 -m venv "$venv"
  "$venv/bin/python" -c "import importlib.metadata as m, sys; sys.exit(m.version('pycadkit') != '$want')" 2>/dev/null \
    || "$venv/bin/pip" install --quiet "pycadkit==$want"
}

# Copies the fixtures into $1. Returns 1 when the public corpus is missing (DWG/DGN samples).
fixtures() {
  mkdir -p "$1"
  cp "$sample" "$1/plan.dxf"
  cp "$sample" "$1/upload.dxf"
  cp "$docs/fixtures/surfaces.dxf" "$docs/fixtures/building.gml" "$1/"
  local corpus="$repo/corpus/public"
  [ -f "$corpus/acadsharp/sample_AC1032.dwg" ] && [ -f "$corpus/gdal/test_dgnv8.dgn" ] || return 1
  cp "$corpus/acadsharp/sample_AC1032.dwg" "$1/plan.dwg"
  cp "$corpus/gdal/test_dgnv8.dgn" "$1/plan.dgn"
  cp "$corpus/gdal/test_dgnv8.dgn" "$1/template.dgn"
}

# drawing.dgn for the C and CLI DGN samples: a DGN V8 file written for the template's seed
# family, made with the Python sample (seed preparation is not in the C API or the CLI).
drawing_dgn() {
  ensure_venv
  (cd "$1" && "$venv/bin/python" -I "$docs/python/write_dgn.py" > /dev/null && cp converted.dgn drawing.dgn)
}

is_svg() { head -c 4 "$1" | grep -q '<svg' || { echo "$1 is not SVG" >&2; exit 1; }; }

expect_layers() {
  for layer in WALLS DOORS WINDOWS DIMENSIONS TEXT GRID; do
    grep -qx "$layer" "$1" || { echo "missing layer $layer in output" >&2; exit 1; }
  done
}

for t in "${targets[@]}"; do
  case "$t" in
    python)
      ensure_venv
      run="$work/python"; rm -rf "$run"; mkdir -p "$run"; cp "$sample" "$run/"
      (cd "$run" && "$venv/bin/python" -I "$snippets/list_layers.py" > layers.txt)
      expect_layers "$run/layers.txt"
      head -c 4 "$run/plan.svg" | grep -q '<svg' || { echo "python: plan.svg is not SVG" >&2; exit 1; }
      echo "python: ok ($("$venv/bin/python" -c 'import importlib.metadata as m; print("pycadkit", m.version("pycadkit"))'))"
      ;;
    rust)
      crate="$work/rustcheck"; mkdir -p "$crate/src"
      dep="${CADKIT_CRATE:-{ path = \"$repo/crates/cadkit\" \}}"
      printf '[package]\nname = "rustcheck"\nversion = "0.0.0"\nedition = "2024"\npublish = false\n\n[dependencies]\ncadkit = %s\n\n[workspace]\n' "$dep" > "$crate/Cargo.toml"
      cp "$snippets/main.rs" "$crate/src/main.rs"
      python3 "$repo/scripts/buildlock.py" cargo check --quiet --manifest-path "$crate/Cargo.toml"
      echo "rust: cargo check ok ($dep)"
      ;;
    c)
      gcc -std=c11 -Wall -Wextra -Werror -fsyntax-only -I "$repo/crates/cadkit-capi/include" "$snippets/list_layers.c"
      echo "c: gcc -fsyntax-only ok"
      ;;
    docs-python)
      ensure_venv
      run="$work/docs-python"; rm -rf "$run"
      corpus=1; fixtures "$run" || corpus=0
      py() { (cd "$run" && "$venv/bin/python" -I "$docs/python/$1" "${@:2}"); }
      py quickstart.py > /dev/null; is_svg "$run/plan.svg"
      py dxf_to_svg.py; is_svg "$run/plan.svg"
      py inspect_drawing.py plan.dxf | grep -q '^WALLS ' || { echo "inspect_drawing.py: no WALLS layer" >&2; exit 1; }
      py citygml.py | grep -q 'acceptance_verified: False' || { echo "citygml.py: no preflight report" >&2; exit 1; }
      py untrusted.py | grep -q 'cadkit.Document' || { echo "untrusted.py: plan rejected" >&2; exit 1; }
      py read_dxf.py > /dev/null; grep -q AC1015 "$run/plan-r2000.dxf"
      py read_citygml.py | grep -q 'entities=6' || { echo "read_citygml.py: unexpected output" >&2; exit 1; }
      if [ $corpus = 1 ]; then
        py inspect_drawing.py plan.dwg > /dev/null
        py inspect_drawing.py plan.dgn > /dev/null
        py read_dwg.py > /dev/null; is_svg "$run/plan.svg"
        py read_dgn.py > /dev/null; is_svg "$run/plan.svg"
        py write_dgn.py | grep -q 'format=dgn_v8' || { echo "write_dgn.py: no DGN V8 output" >&2; exit 1; }
        echo "docs-python: ok (10 samples, DWG/DGN included)"
      else
        echo "docs-python: ok (7 samples; corpus/public missing, DWG/DGN samples skipped)"
      fi
      ;;
    docs-rust)
      crate="$work/docscheck"; rm -rf "$crate/src"; mkdir -p "$crate/src/bin"
      if [ -n "${CADKIT_REGISTRY_VERSION:-}" ]; then
        v="\"=$CADKIT_REGISTRY_VERSION\""
        deps="cadkit = $v
cadkit-geom = $v
cadkit-scene = { version = $v, features = [\"document\"] }
cadkit-gpu = $v"
      else
        deps="cadkit = { path = \"$repo/crates/cadkit\" }
cadkit-geom = { path = \"$repo/crates/cadkit-geom\" }
cadkit-scene = { path = \"$repo/crates/cadkit-scene\", features = [\"document\"] }
cadkit-gpu = { path = \"$repo/crates/cadkit-gpu\" }"
      fi
      printf '[package]\nname = "docscheck"\nversion = "0.0.0"\nedition = "2024"\npublish = false\n\n[dependencies]\n%s\n\n[workspace]\n' "$deps" > "$crate/Cargo.toml"
      lib=""
      for f in "$docs"/rust/*.rs; do
        name="$(basename "$f" .rs)"
        if grep -q '^fn main' "$f"; then cp "$f" "$crate/src/bin/$name.rs"
        else cp "$f" "$crate/src/$name.rs"; lib="$lib#[allow(dead_code)]\npub mod $name;\n"; fi
      done
      printf "//! Samples without a main function, checked as modules.\n$lib" > "$crate/src/lib.rs"
      python3 "$repo/scripts/buildlock.py" cargo check --quiet --manifest-path "$crate/Cargo.toml" --bins --lib
      echo "docs-rust: cargo check ok ($(ls "$docs"/rust/*.rs | wc -l) samples)"
      if [ "${DOCS_RUN:-0}" = 1 ]; then
        run="$work/docs-rust-run"; rm -rf "$run"
        corpus=1; fixtures "$run" || corpus=0
        python3 "$repo/scripts/buildlock.py" cargo build --quiet --manifest-path "$crate/Cargo.toml" --bins
        bin="${CARGO_TARGET_DIR:-$crate/target}/debug"
        for b in quickstart dxf_to_svg geometry scene_plan citygml untrusted read_dxf read_citygml; do
          (cd "$run" && "$bin/$b" > "$b.out") || { echo "docs-rust: $b failed" >&2; exit 1; }
        done
        (cd "$run" && "$bin/inspect_drawing" plan.dxf > inspect.out)
        if [ $corpus = 1 ]; then
          for b in read_dwg read_dgn write_dgn; do (cd "$run" && "$bin/$b" > "$b.out") || { echo "docs-rust: $b failed" >&2; exit 1; }; done
        fi
        echo "docs-rust: binaries ran (outputs in $run)"
      fi
      ;;
    docs-c)
      inc="$repo/crates/cadkit-capi/include"
      for f in "$docs"/c/*.c; do gcc -std=c11 -Wall -Wextra -Werror -fsyntax-only -I "$inc" "$f"; done
      for f in "$docs"/c/*.cpp; do g++ -std=c++17 -Wall -Wextra -Werror -fsyntax-only -I "$inc" "$f"; done
      echo "docs-c: gcc/g++ -fsyntax-only ok ($(ls "$docs"/c/*.c "$docs"/c/*.cpp | wc -l) samples)"
      if [ -n "${CADKIT_CAPI_LIB_DIR:-}" ]; then
        run="$work/docs-c-run"; rm -rf "$run"
        corpus=1; fixtures "$run" || corpus=0
        [ $corpus = 1 ] && drawing_dgn "$run"
        for f in "$docs"/c/*.c "$docs"/c/*.cpp; do
          name="$(basename "$f")"; exe="$run/${name//./_}"
          case "$name" in read_dwg.c|read_dgn.c|write_dgn.c) [ $corpus = 1 ] || continue ;; esac
          case "$name" in *.cpp) cc=(g++ -std=c++17) ;; *) cc=(gcc -std=c11) ;; esac
          "${cc[@]}" -I "$inc" "$f" -L "$CADKIT_CAPI_LIB_DIR" -lcadkit_capi -o "$exe"
          arg=(); [ "$name" = inspect_drawing.c ] && arg=(plan.dxf)
          (cd "$run" && LD_LIBRARY_PATH="$CADKIT_CAPI_LIB_DIR" "$exe" "${arg[@]}" > "$exe.out") || { echo "docs-c: $name failed" >&2; exit 1; }
        done
        # build.sh and cmake.sh (with CMakeLists.txt) beside an archive-shaped cadkit-capi/.
        proj="$run/project"; mkdir -p "$proj/cadkit-capi/lib"; cp -r "$inc" "$proj/cadkit-capi/"
        cp "$CADKIT_CAPI_LIB_DIR"/libcadkit_capi.* "$proj/cadkit-capi/lib/"
        cp "$docs/c/CMakeLists.txt" "$docs/c/quickstart.c" "$sample" "$proj/"; mv "$proj/$(basename "$sample")" "$proj/plan.dxf"
        (cd "$proj" && bash -euo pipefail "$docs/c/build.sh" > build.out && bash -euo pipefail "$docs/c/cmake.sh" > /dev/null \
          && LD_LIBRARY_PATH=cadkit-capi/lib build/quickstart > cmake.out)
        grep -q '161 entities' "$proj/build.out" "$proj/cmake.out"
        echo "docs-c: linked against $CADKIT_CAPI_LIB_DIR and ran; build.sh and cmake.sh ok (outputs in $run)"
      fi
      ;;
    docs-cli)
      if [ -z "${CADKIT_BIN:-}" ]; then
        python3 "$repo/scripts/buildlock.py" cargo build --quiet -p cadkit-cli --manifest-path "$repo/Cargo.toml"
        CADKIT_BIN="${CARGO_TARGET_DIR:-$repo/target}/debug/cadkit"
      fi
      bindir="$work/docs-cli-bin"; mkdir -p "$bindir"; ln -sf "$CADKIT_BIN" "$bindir/cadkit"
      run="$work/docs-cli"; rm -rf "$run"
      corpus=1; fixtures "$run" || corpus=0
      [ $corpus = 1 ] && drawing_dgn "$run"
      for f in "$docs"/cli/*.sh; do
        name="$(basename "$f")"
        case "$name" in inspect.sh|write-dgn.sh) [ $corpus = 1 ] || continue ;; esac
        (cd "$run" && PATH="$bindir:$PATH" bash -euo pipefail "$f" > "$name.out") || { echo "docs-cli: $name failed" >&2; exit 1; }
      done
      echo "docs-cli: ok ($("$CADKIT_BIN" --version))"
      ;;
    *) echo "unknown target $t" >&2; exit 2 ;;
  esac
done
