#!/usr/bin/env python3
"""Builds cadkit-wasm into pkg/ (--target web) and pkg-node/ (--target nodejs), then
assembles the npm package `cadkit-wasm` in npm/.

Run it from anywhere; it calls scripts/buildlock.py itself for every build step, so do NOT
wrap it in the lock again. The wasm-bindgen CLI must match the `wasm-bindgen` version in
Cargo.lock; it is looked up in .tools/, then on PATH. If wasm-opt (binaryen) is on PATH the
module is also shrunk with `wasm-opt -Oz` (optional).

The shipped modules have no "name" custom section (function names, about 45 % of the
module); `--debug-names` keeps it for readable stack traces. `--check-size` fails when the
web module is larger than SIZE_BUDGET.

npm/ holds one .wasm (web/cadkit_wasm_bg.wasm): the Node glue in node/ loads it from there.
The package root resolves to the Node build under Node (ESM through node/cadkit_wasm.mjs,
require through node/cadkit_wasm.js) and to the web build everywhere else.
"""
import argparse
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
TARGET_DIR = os.path.abspath(os.environ.get("CARGO_TARGET_DIR") or os.path.join(ROOT, "target"))
WASM = os.path.join(TARGET_DIR, "wasm32-unknown-unknown", "release", "cadkit_wasm.wasm")

# Budget of the shipped web module in bytes, checked by `--check-size` (CI). The module
# without its name section measured 2,438,423 bytes when the budget was set (0.5.4, no
# wasm-opt); raise the budget only together with a note of what made the module grow.
SIZE_BUDGET = 2_700_000

# rustc target features of wasm32-unknown-unknown -> wasm-opt flags. wasm-opt validates the
# module against the features it is told about, so every feature the compiler may emit must
# be listed.
WASM_OPT_FEATURES = {
    "bulk-memory": "--enable-bulk-memory",
    "bulk-memory-opt": "--enable-bulk-memory-opt",
    "call-indirect-overlong": "--enable-call-indirect-overlong",
    "exception-handling": "--enable-exception-handling",
    "extended-const": "--enable-extended-const",
    "multivalue": "--enable-multivalue",
    "mutable-globals": "--enable-mutable-globals",
    "nontrapping-fptoint": "--enable-nontrapping-float-to-int",
    "reference-types": "--enable-reference-types",
    "sign-ext": "--enable-sign-ext",
    "simd128": "--enable-simd",
    "tail-call": "--enable-tail-call",
}
DEFAULT_FEATURES = ["bulk-memory", "multivalue", "mutable-globals", "nontrapping-fptoint", "reference-types", "sign-ext"]

NODE_WASM_PATH = "`${__dirname}/cadkit_wasm_bg.wasm`"
NODE_WASM_SHARED = "`${__dirname}/../web/cadkit_wasm_bg.wasm`"


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


def write_text(path, text):
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


def target_features():
    """Target features rustc enables for wasm32-unknown-unknown (the module may use them)."""
    try:
        out = subprocess.run(
            ["rustc", "--print", "cfg", "--target", "wasm32-unknown-unknown"],
            capture_output=True, text=True, check=True,
        ).stdout
    except (OSError, subprocess.CalledProcessError):
        return DEFAULT_FEATURES
    found = re.findall(r'^target_feature="([^"]+)"$', out, re.M)
    return found or DEFAULT_FEATURES


def wasm_opt_flags():
    flags = []
    for feature in target_features():
        flag = WASM_OPT_FEATURES.get(feature)
        if flag is None:
            sys.exit(f"wasm-opt: no flag known for target feature {feature!r}; add it to WASM_OPT_FEATURES")
        flags.append(flag)
    return flags


def leb128(data, pos):
    """Unsigned LEB128 at data[pos]; returns (value, next position)."""
    value = shift = 0
    while True:
        if pos >= len(data):
            raise ValueError("truncated LEB128")
        byte = data[pos]
        pos += 1
        value |= (byte & 0x7F) << shift
        if byte < 0x80:
            return value, pos
        shift += 7


def strip_name_section(path):
    """Removes the custom "name" section (function and local names) from a .wasm file."""
    data = open(path, "rb").read()
    if data[:4] != b"\0asm":
        sys.exit(f"{path}: not a wasm module")
    out = bytearray(data[:8])
    pos = 8
    removed = 0
    while pos < len(data):
        start = pos
        section_id = data[pos]
        size, body = leb128(data, pos + 1)
        end = body + size
        if end > len(data):
            sys.exit(f"{path}: section at {start} runs past the end")
        if section_id == 0:
            name_len, name_at = leb128(data, body)
            if data[name_at:name_at + name_len] == b"name":
                removed += end - start
                pos = end
                continue
        out += data[start:end]
        pos = end
    with open(path, "wb") as f:
        f.write(out)
    return removed


def node_esm_wrapper(glue):
    """ESM entry for Node that re-exports the CommonJS glue, with no-op init functions so that
    code written for the web build (`await init()`) runs unchanged."""
    names = sorted(set(re.findall(r"^exports\.([A-Za-z_$][\w$]*) = ", glue, re.M)))
    if not names:
        sys.exit("node glue: no exports found")
    lines = [
        "// ESM entry of cadkit-wasm under Node (the \"node\" condition of the package root).",
        "// The module is instantiated when it is imported; init() and initSync() do nothing and",
        "// exist so that code written for the web build runs unchanged. Generated by build.py.",
        'import cadkit from "./cadkit_wasm.js";',
        "",
        f"export const {{ {', '.join(names)} }} = cadkit;",
        "",
        "export function initSync() {",
        "  return undefined;",
        "}",
        "",
        "export default async function init() {",
        "  return undefined;",
        "}",
        "",
    ]
    return "\n".join(lines)


def assemble_npm():
    """npm/: web/ (ES module, needs `await init()`, holds the one .wasm) and node/ (CommonJS
    glue reading ../web/cadkit_wasm_bg.wasm, plus an ESM entry)."""
    dest = os.path.join(HERE, "npm")
    shutil.rmtree(dest, ignore_errors=True)
    web_wasm = os.path.join(HERE, "pkg", "cadkit_wasm_bg.wasm")
    node_wasm = os.path.join(HERE, "pkg-node", "cadkit_wasm_bg.wasm")
    if open(web_wasm, "rb").read() != open(node_wasm, "rb").read():
        sys.exit("the web and Node builds produced different modules; the package cannot share one")
    shutil.copytree(os.path.join(HERE, "pkg"), os.path.join(dest, "web"), ignore=shutil.ignore_patterns("package.json"))
    shutil.copytree(
        os.path.join(HERE, "pkg-node"), os.path.join(dest, "node"),
        ignore=shutil.ignore_patterns("package.json", "cadkit_wasm_bg.wasm"),
    )
    glue_path = os.path.join(dest, "node", "cadkit_wasm.js")
    glue = open(glue_path, encoding="utf-8").read()
    if glue.count(NODE_WASM_PATH) != 1:
        sys.exit(f"node glue: expected one {NODE_WASM_PATH}; the wasm-bindgen output changed")
    glue = glue.replace(NODE_WASM_PATH, NODE_WASM_SHARED)
    write_text(glue_path, glue)
    write_text(os.path.join(dest, "node", "cadkit_wasm.mjs"), node_esm_wrapper(glue))
    # The package root is "type": "module"; the Node glue is CommonJS.
    write_json(os.path.join(dest, "node", "package.json"), {"type": "commonjs"})
    for name in ("LICENSE-MIT", "LICENSE-APACHE", "NOTICE"):
        shutil.copy(os.path.join(ROOT, name), dest)
    shutil.copy(os.path.join(HERE, "README.md"), dest)
    write_json(os.path.join(dest, "package.json"), {
        "name": "cadkit-wasm",
        "version": workspace_version(),
        "description": "CAD file I/O and document operations in browser/Node WebAssembly: read DWG/DGN/DXF/CityGML; write DXF, seed-based DGN V8, CityGML, SVG and JSON.",
        "license": "MIT OR Apache-2.0",
        "author": "cadkit maintainers <maintainers@cadkit.org> (https://cadkit.org)",
        "repository": {"type": "git", "url": "git+https://github.com/TNYCL/cadkit.git", "directory": "crates/cadkit-wasm"},
        "homepage": "https://github.com/TNYCL/cadkit/blob/main/docs/wasm.md",
        "bugs": "https://github.com/TNYCL/cadkit/issues",
        "keywords": ["cad", "dwg", "dgn", "dxf", "citygml", "wasm"],
        "type": "module",
        "main": "./web/cadkit_wasm.js",
        "module": "./web/cadkit_wasm.js",
        "types": "./web/cadkit_wasm.d.ts",
        "exports": {
            ".": {
                "types": "./web/cadkit_wasm.d.ts",
                "node": {"import": "./node/cadkit_wasm.mjs", "require": "./node/cadkit_wasm.js"},
                "default": "./web/cadkit_wasm.js",
            },
            "./web": {"types": "./web/cadkit_wasm.d.ts", "default": "./web/cadkit_wasm.js"},
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
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--debug-names", action="store_true", help="keep the wasm name section")
    parser.add_argument("--check-size", action="store_true", help=f"fail when the web module exceeds {SIZE_BUDGET:,} bytes")
    args = parser.parse_args()

    version = lock_version()
    cli = find_cli(version)
    out = subprocess.run([cli, "--version"], capture_output=True, text=True).stdout.strip()
    if version not in out:
        sys.exit(f"CLI is '{out}', Cargo.lock wants wasm-bindgen {version}")

    env = dict(os.environ)
    env["CARGO_TARGET_DIR"] = TARGET_DIR
    # Size-oriented release settings applied by env, because the workspace profile is shared.
    env.update(
        CARGO_PROFILE_RELEASE_OPT_LEVEL="z",
        CARGO_PROFILE_RELEASE_LTO="fat",
        CARGO_PROFILE_RELEASE_CODEGEN_UNITS="1",
        CARGO_PROFILE_RELEASE_PANIC="abort",
        CARGO_PROFILE_RELEASE_STRIP="debuginfo",
    )
    locked(["cargo", "build", "-p", "cadkit-wasm", "--release", "--target", "wasm32-unknown-unknown"], env=env)

    opt = shutil.which("wasm-opt")
    opt_flags = wasm_opt_flags() if opt else []
    if not opt:
        print("wasm-opt not found: skipping (optional)")
    for target, outdir in (("web", "pkg"), ("nodejs", "pkg-node")):
        dest = os.path.join(HERE, outdir)
        shutil.rmtree(dest, ignore_errors=True)
        locked([cli, "--target", target, "--out-dir", dest, "--out-name", "cadkit_wasm", WASM], env=env)
        w = os.path.join(dest, "cadkit_wasm_bg.wasm")
        if opt:
            flags = opt_flags + (["--debuginfo"] if args.debug_names else [])
            locked([opt, "-Oz", *flags, "-o", w, w], env=env)
        if not args.debug_names:
            removed = strip_name_section(w)
            print(f"{outdir}: removed {removed:,} bytes of names")
        write_dev_package_json(dest, target == "nodejs")
        size = os.path.getsize(w)
        print(f"{outdir}/cadkit_wasm_bg.wasm: {size:,} bytes")
    assemble_npm()
    size = os.path.getsize(os.path.join(HERE, "npm", "web", "cadkit_wasm_bg.wasm"))
    if args.check_size:
        if size > SIZE_BUDGET:
            sys.exit(f"web module is {size:,} bytes, budget {SIZE_BUDGET:,} (build.py SIZE_BUDGET)")
        print(f"size check: {size:,} of {SIZE_BUDGET:,} bytes")


def write_dev_package_json(dest, node):
    # pkg/ and pkg-node/ serve the viewer and the tests; only npm/ is published.
    pkg = {"name": "cadkit-wasm-dev", "version": workspace_version(), "private": True}
    if not node:
        pkg["type"] = "module"
    write_json(os.path.join(dest, "package.json"), pkg)


if __name__ == "__main__":
    main()
