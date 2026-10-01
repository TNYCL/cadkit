#!/usr/bin/env python3
"""Builds cadkit-wasm into pkg/ (--target web) and pkg-node/ (--target nodejs), then
assembles the npm package `cadkit-wasm` in npm/ from both (web as the main entry, the
Node build as the `cadkit-wasm/node` subpath).

Run it from anywhere; it calls scripts/buildlock.py itself for every build step, so do NOT
wrap it in the lock again. The wasm-bindgen CLI must match the `wasm-bindgen` version in
Cargo.lock; it is looked up in .tools/, then on PATH. If wasm-opt (binaryen) is on PATH the
module is also shrunk with `wasm-opt -Oz`.
"""
import glob
import json
import os
import re
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
LOCK = [sys.executable, os.path.join(ROOT, "scripts", "buildlock.py")]
WASM = os.path.join(ROOT, "target", "wasm32-unknown-unknown", "release", "cadkit_wasm.wasm")


def locked(cmd, env=None):
    print("+", " ".join(cmd), flush=True)
    subprocess.run(LOCK + cmd, cwd=ROOT, check=True, env=env)


def lock_version():
    text = open(os.path.join(ROOT, "Cargo.lock"), encoding="utf-8").read()
    m = re.search(r'name = "wasm-bindgen"\nversion = "([^"]+)"', text)
    if not m:
        sys.exit("wasm-bindgen not found in Cargo.lock")
    return m.group(1)


def find_cli(version):
    for p in glob.glob(os.path.join(HERE, ".tools", "**", "wasm-bindgen*"), recursive=True):
        if os.path.basename(p) in ("wasm-bindgen", "wasm-bindgen.exe"):
            return p
    found = shutil.which("wasm-bindgen")
    if found:
        return found
    sys.exit(
        f"wasm-bindgen CLI {version} not found. Download the prebuilt release from "
        f"https://github.com/wasm-bindgen/wasm-bindgen/releases/tag/{version} into "
        f"crates/cadkit-wasm/.tools/ (the version must equal Cargo.lock)."
    )


def workspace_version():
    text = open(os.path.join(ROOT, "Cargo.toml"), encoding="utf-8").read()
    m = re.search(r'\[workspace\.package\][^\[]*?[\r\n]version = "([^"]+)"', text)
    if not m:
        sys.exit("workspace version not found")
    return m.group(1)


def write_json(path, data):
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        json.dump(data, f, indent=2)
        f.write("\n")


def write_dev_package_json(dest, node):
    # pkg/ and pkg-node/ serve the viewer and the tests; only npm/ is published.
    pkg = {"name": "cadkit-wasm-dev", "version": workspace_version(), "private": True}
    if not node:
        pkg["type"] = "module"
    write_json(os.path.join(dest, "package.json"), pkg)


def assemble_npm():
    """npm/: web/ (ES module, needs `await init()`) and node/ (CommonJS, ready on load)."""
    dest = os.path.join(HERE, "npm")
    shutil.rmtree(dest, ignore_errors=True)
    for src, sub in (("pkg", "web"), ("pkg-node", "node")):
        shutil.copytree(os.path.join(HERE, src), os.path.join(dest, sub), ignore=shutil.ignore_patterns("package.json"))
    # The package root is "type": "module"; the Node build is CommonJS.
    write_json(os.path.join(dest, "node", "package.json"), {"type": "commonjs"})
    for name in ("LICENSE-MIT", "LICENSE-APACHE", "NOTICE"):
        shutil.copy(os.path.join(ROOT, name), dest)
    shutil.copy(os.path.join(HERE, "README.md"), dest)
    write_json(os.path.join(dest, "package.json"), {
        "name": "cadkit-wasm",
        "version": workspace_version(),
        "description": "Read DWG, DGN, DXF and CityGML drawings in the browser or Node via WebAssembly.",
        "license": "MIT OR Apache-2.0",
        "author": "TNYCL (https://tnycl.com)",
        "repository": {"type": "git", "url": "git+https://github.com/TNYCL/cadkit.git", "directory": "crates/cadkit-wasm"},
        "homepage": "https://github.com/TNYCL/cadkit/blob/main/docs/wasm.md",
        "bugs": "https://github.com/TNYCL/cadkit/issues",
        "keywords": ["cad", "dwg", "dgn", "dxf", "citygml", "wasm"],
        "type": "module",
        "main": "./web/cadkit_wasm.js",
        "module": "./web/cadkit_wasm.js",
        "types": "./web/cadkit_wasm.d.ts",
        "exports": {
            ".": {"types": "./web/cadkit_wasm.d.ts", "default": "./web/cadkit_wasm.js"},
            "./node": {"types": "./node/cadkit_wasm.d.ts", "default": "./node/cadkit_wasm.js"},
            "./cadkit_wasm_bg.wasm": "./web/cadkit_wasm_bg.wasm",
            "./package.json": "./package.json",
        },
        "files": ["web/", "node/", "NOTICE"],
        "sideEffects": ["./web/snippets/*"],
        "engines": {"node": ">=18"},
    })
    print(f"npm/: cadkit-wasm {workspace_version()}")


def main():
    version = lock_version()
    cli = find_cli(version)
    out = subprocess.run([cli, "--version"], capture_output=True, text=True).stdout.strip()
    if version not in out:
        sys.exit(f"CLI is '{out}', Cargo.lock wants wasm-bindgen {version}")

    env = dict(os.environ)
    # Size-oriented release settings applied by env, because the workspace profile is shared.
    env.update(
        CARGO_PROFILE_RELEASE_OPT_LEVEL="z",
        CARGO_PROFILE_RELEASE_LTO="fat",
        CARGO_PROFILE_RELEASE_CODEGEN_UNITS="1",
        CARGO_PROFILE_RELEASE_PANIC="abort",
        CARGO_PROFILE_RELEASE_STRIP="debuginfo",
    )
    locked(["cargo", "build", "-p", "cadkit-wasm", "--release", "--target", "wasm32-unknown-unknown"], env=env)

    for target, outdir in (("web", "pkg"), ("nodejs", "pkg-node")):
        dest = os.path.join(HERE, outdir)
        shutil.rmtree(dest, ignore_errors=True)
        locked([cli, "--target", target, "--out-dir", dest, "--out-name", "cadkit_wasm", WASM])
        opt = shutil.which("wasm-opt")
        if opt:
            w = os.path.join(dest, "cadkit_wasm_bg.wasm")
            locked([opt, "-Oz", "--enable-bulk-memory", "--enable-nontrapping-float-to-int", "-o", w, w])
        write_dev_package_json(dest, target == "nodejs")
        size = os.path.getsize(os.path.join(dest, "cadkit_wasm_bg.wasm"))
        print(f"{outdir}/cadkit_wasm_bg.wasm: {size:,} bytes")
    assemble_npm()


if __name__ == "__main__":
    main()
