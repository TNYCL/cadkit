#!/usr/bin/env python3
"""Release helper for the cadkit workspace. The process is described in docs/RELEASING.md.

    python scripts/release.py version                 print the workspace version
    python scripts/release.py check [--tag vX.Y.Z]    versions, CHANGELOG section, license copies
    python scripts/release.py notes X.Y.Z             print the CHANGELOG section of a version
    python scripts/release.py bump X.Y.Z              set the version and open its CHANGELOG section
    python scripts/release.py sync-licenses           copy the root license files into each package
    python scripts/release.py publish-crates [--dry-run]
    python scripts/release.py mirror DIR --version X.Y.Z [--dry-run]

mirror uploads the release files in DIR (with their SHA256SUMS) to release.cadkit.org, reads
them back and moves latest.txt forward; it needs the aws CLI and the R2 credentials named in
docs/RELEASING.md. publish-crates publishes every crate whose manifest allows it, in dependency order, and
skips versions already on the crates.io index, so a failed release can simply be re-run.
Cargo commands run through scripts/buildlock.py. Only the standard library is used.
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
LOCK = [sys.executable, os.path.join(ROOT, "scripts", "buildlock.py")]
LICENSE_FILES = ("LICENSE-MIT", "LICENSE-APACHE", "NOTICE")
# Packaged from their own directory but not published to crates.io.
EXTRA_LICENSE_DIRS = ("crates/cadkit-py",)
VERSION_RE = re.compile(r"^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$")
INDEX = "https://index.crates.io"
USER_AGENT = "cadkit-release (https://github.com/TNYCL/cadkit)"
# Public download host and the Cloudflare R2 bucket behind it (docs/RELEASING.md).
MIRROR_URL = "https://release.cadkit.org"
MIRROR_BUCKET = "cadkit-release"
# A version directory never changes once published; latest.txt does.
CACHE_IMMUTABLE = "public, max-age=31536000, immutable"
CACHE_LATEST = "public, max-age=300"


def fail(message: str) -> None:
    sys.exit(f"release.py: {message}")


def read_text(path: str) -> tuple[str, bool]:
    """Returns the text with LF line endings and whether the file used CRLF."""
    with open(os.path.join(ROOT, path), "rb") as f:
        raw = f.read().decode("utf-8")
    return raw.replace("\r\n", "\n"), "\r\n" in raw


def write_text(path: str, text: str, crlf: bool) -> None:
    with open(os.path.join(ROOT, path), "wb") as f:
        f.write((text.replace("\n", "\r\n") if crlf else text).encode("utf-8"))


def metadata() -> dict:
    out = subprocess.run(
        LOCK + ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
        cwd=ROOT, check=True, capture_output=True, text=True,
    ).stdout
    return json.loads(out)


def is_publishable(package: dict) -> bool:
    # `publish = false` is reported as an empty registry list.
    return package["publish"] is None or "crates-io" in package["publish"]


def publish_order(packages: list[dict]) -> list[dict]:
    """Publishable packages, dependencies first. Dev-dependencies count: crates.io
    requires every dependency in the published manifest to exist already."""
    names = {p["name"] for p in packages}
    deps = {p["name"]: {d["name"] for d in p["dependencies"] if d["name"] in names} for p in packages}
    order: list[str] = []
    while len(order) < len(packages):
        ready = sorted(n for n in names - set(order) if deps[n] <= set(order))
        if not ready:
            fail(f"dependency cycle among {sorted(names - set(order))}")
        order.extend(ready)
    by_name = {p["name"]: p for p in packages}
    return [by_name[n] for n in order]


def workspace_version(meta: dict) -> str:
    versions = {p["name"]: p["version"] for p in meta["packages"]}
    distinct = set(versions.values())
    if len(distinct) != 1:
        fail(f"workspace members disagree on the version: {versions}")
    return distinct.pop()


def changelog_section(version: str) -> str | None:
    text, _ = read_text("CHANGELOG.md")
    m = re.search(rf"^## \[{re.escape(version)}\][^\n]*\n(.*?)(?=^## \[|\Z)", text, re.M | re.S)
    if not m:
        return None
    # Drop link reference definitions; they belong to the whole file.
    body = "\n".join(line for line in m.group(1).splitlines() if not re.match(r"^\[[^\]]+\]: ", line))
    return body.strip() or None


def license_dirs(meta: dict) -> list[str]:
    dirs = [
        os.path.relpath(os.path.dirname(p["manifest_path"]), ROOT).replace(os.sep, "/")
        for p in meta["packages"]
        if is_publishable(p)
    ]
    return sorted(set(dirs) | set(EXTRA_LICENSE_DIRS))


def stale_license_copies(meta: dict) -> list[str]:
    stale = []
    for name in LICENSE_FILES:
        with open(os.path.join(ROOT, name), "rb") as f:
            want = f.read()
        for d in license_dirs(meta):
            path = os.path.join(ROOT, d, name)
            if not os.path.exists(path):
                stale.append(f"{d}/{name}")
                continue
            with open(path, "rb") as f:
                if f.read() != want:
                    stale.append(f"{d}/{name}")
    return stale


def cmd_version(_args: argparse.Namespace) -> None:
    print(workspace_version(metadata()))


def cmd_check(args: argparse.Namespace) -> None:
    meta = metadata()
    version = workspace_version(meta)
    problems = []
    toml, _ = read_text("Cargo.toml")
    for name, pinned in re.findall(r'^(cadkit[\w-]*) = \{ path = "[^"]+", version = "([^"]+)"(?:,[^}\n]*)? \}', toml, re.M):
        if pinned != version:
            problems.append(f"[workspace.dependencies] {name} pins {pinned}, workspace is {version}")
    if args.tag is not None:
        if args.tag != f"v{version}":
            problems.append(f"tag {args.tag} does not match the workspace version {version}")
        if changelog_section(version) is None:
            problems.append(f"CHANGELOG.md has no non-empty '## [{version}]' section")
    problems += [f"{p} differs from the root copy (run: python scripts/release.py sync-licenses)"
                 for p in stale_license_copies(meta)]
    if problems:
        fail("\n  " + "\n  ".join(problems))
    print(f"release check passed for {version}")


def cmd_notes(args: argparse.Namespace) -> None:
    body = changelog_section(args.version)
    if body is None:
        fail(f"CHANGELOG.md has no non-empty '## [{args.version}]' section")
    print(body)


def cmd_bump(args: argparse.Namespace) -> None:
    new = args.version
    if not VERSION_RE.match(new):
        fail(f"'{new}' is not a semantic version")
    old = workspace_version(metadata())
    toml, crlf = read_text("Cargo.toml")
    toml, n = re.subn(r'(\[workspace\.package\][^\[]*?\nversion = ")[^"]+(")', rf"\g<1>{new}\g<2>", toml, count=1)
    if n != 1:
        fail("workspace version not found in Cargo.toml")
    toml = re.sub(r'^(cadkit[\w-]* = \{ path = "[^"]+", version = ")[^"]+("(?:,[^}\n]*)? \})', rf"\g<1>{new}\g<2>", toml, flags=re.M)
    write_text("Cargo.toml", toml, crlf)

    log, crlf = read_text("CHANGELOG.md")
    if re.search(rf"^## \[{re.escape(new)}\]", log, re.M):
        fail(f"CHANGELOG.md already has a section for {new}")
    today = datetime.date.today().isoformat()
    log, n = re.subn(r"^## \[Unreleased\]\n", f"## [Unreleased]\n\n## [{new}] - {today}\n", log, count=1, flags=re.M)
    if n != 1:
        fail("CHANGELOG.md has no '## [Unreleased]' heading")
    log = re.sub(
        r"^\[Unreleased\]: (https://[^\s]+/compare/)v[^.\s]+\.[^.\s]+\.[^\s.]+\.\.\.HEAD$",
        rf"[Unreleased]: \g<1>v{new}...HEAD\n[{new}]: \g<1>v{old}...v{new}",
        log,
        count=1,
        flags=re.M,
    )
    write_text("CHANGELOG.md", log, crlf)

    subprocess.run(LOCK + ["cargo", "update", "--workspace"], cwd=ROOT, check=True)
    print(f"{old} -> {new}. Review CHANGELOG.md, commit, merge, then tag v{new} on main (docs/RELEASING.md).")


def cmd_sync_licenses(_args: argparse.Namespace) -> None:
    meta = metadata()
    for name in LICENSE_FILES:
        with open(os.path.join(ROOT, name), "rb") as f:
            data = f.read()
        for d in license_dirs(meta):
            with open(os.path.join(ROOT, d, name), "wb") as f:
                f.write(data)
    print("license files copied into: " + ", ".join(license_dirs(meta)))


def index_path(name: str) -> str:
    # https://doc.rust-lang.org/cargo/reference/registry-index.html#index-files
    if len(name) <= 2:
        return f"{len(name)}/{name}"
    if len(name) == 3:
        return f"3/{name[0]}/{name}"
    return f"{name[:2]}/{name[2:4]}/{name}"


def on_index(name: str, version: str) -> bool:
    request = urllib.request.Request(f"{INDEX}/{index_path(name.lower())}", headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            lines = response.read().decode("utf-8").splitlines()
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return False
        raise
    return any(json.loads(line).get("vers") == version for line in lines if line.strip())


def cmd_publish_crates(args: argparse.Namespace) -> None:
    meta = metadata()
    version = workspace_version(meta)
    crates = publish_order([p for p in meta["packages"] if is_publishable(p)])
    excluded = [p["name"] for p in meta["packages"] if not is_publishable(p)]
    print("publish order: " + " -> ".join(p["name"] for p in crates))
    if not args.dry_run:
        done = [p["name"] for p in crates if on_index(p["name"], version)]
        if done:
            print(f"already on crates.io at {version}, skipped: {', '.join(done)}")
        if len(done) == len(crates):
            print("nothing left to publish")
            return
        excluded += done
    # cargo >= 1.90 publishes a workspace in dependency order and waits for the index.
    cmd = ["cargo", "publish", "--workspace", "--locked"]
    cmd += [arg for name in excluded for arg in ("--exclude", name)]
    if args.dry_run:
        cmd.append("--dry-run")
    print("+ " + " ".join(cmd), flush=True)
    subprocess.run(LOCK + cmd, cwd=ROOT, check=True)


def sha256_file(path: str) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_sums(directory: str) -> dict[str, str]:
    """SHA256SUMS as written by sha256sum: '<hex>  <name>' (or ' *<name>') per line."""
    path = os.path.join(directory, "SHA256SUMS")
    if not os.path.exists(path):
        fail(f"{path} is missing (run: cd {directory} && sha256sum -- * > SHA256SUMS)")
    sums = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            if line.strip():
                digest, name = line.split(None, 1)
                sums[name.strip().lstrip("*")] = digest.lower()
    return sums


def mirror_content_type(name: str) -> str:
    # Set explicitly: a guessed "Content-Encoding: gzip" would make browsers unpack the archives.
    if name.endswith((".tar.gz", ".tgz")):
        return "application/gzip"
    if name.endswith((".zip", ".whl")):
        return "application/zip"
    return "text/plain; charset=utf-8"


def mirror_get(key: str) -> bytes | None:
    """The object as release.cadkit.org serves it, or None when it does not exist."""
    request = urllib.request.Request(f"{MIRROR_URL}/{key}", headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=120) as response:
            return response.read()
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return None
        raise


def stable_version(text: str) -> tuple[int, int, int] | None:
    m = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", text.strip())
    return (int(m.group(1)), int(m.group(2)), int(m.group(3))) if m else None


def cmd_mirror(args: argparse.Namespace) -> None:
    version = args.version
    if not VERSION_RE.match(version):
        fail(f"'{version}' is not a semantic version")
    directory = os.path.abspath(args.directory)
    sums = read_sums(directory)
    files = sorted(n for n in os.listdir(directory) if n != "SHA256SUMS")
    if set(files) != set(sums):
        fail(f"SHA256SUMS and {directory} list different files: {sorted(set(files) ^ set(sums))}")
    for name in files:
        if sha256_file(os.path.join(directory, name)) != sums[name]:
            fail(f"{name} does not match its SHA256SUMS entry")

    account = os.environ.get("R2_ACCOUNT_ID", "")
    if not args.dry_run and not (account and os.environ.get("AWS_ACCESS_KEY_ID") and os.environ.get("AWS_SECRET_ACCESS_KEY")):
        fail("set R2_ACCOUNT_ID, AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY (docs/RELEASING.md)")
    endpoint = f"https://{account or 'R2_ACCOUNT_ID'}.r2.cloudflarestorage.com"

    def put(path: str, key: str, cache: str) -> None:
        cmd = ["aws", "s3", "cp", path, f"s3://{MIRROR_BUCKET}/{key}", "--endpoint-url", endpoint, "--region", "auto",
               "--content-type", mirror_content_type(key), "--cache-control", cache, "--only-show-errors"]
        print("+ " + " ".join(cmd), flush=True)
        if not args.dry_run:
            subprocess.run(cmd, check=True)

    # SHA256SUMS last: once it is there, every file it names is there too.
    for name in files + ["SHA256SUMS"]:
        put(os.path.join(directory, name), f"v{version}/{name}", CACHE_IMMUTABLE)
    if args.dry_run:
        print(f"dry run: nothing uploaded; {len(files)} files and SHA256SUMS would go to {MIRROR_URL}/v{version}/")
        return

    for name in files:
        body = mirror_get(f"v{version}/{name}")
        if body is None or hashlib.sha256(body).hexdigest() != sums[name]:
            fail(f"{MIRROR_URL}/v{version}/{name} does not serve the uploaded file")
    print(f"read back {len(files)} files from {MIRROR_URL}/v{version}/: checksums match")

    # latest.txt names the newest stable release; the installers read it (site/public/install.*).
    new = stable_version(version)
    if new is None:
        print(f"{version} is a pre-release: latest.txt is left alone")
        return
    current = mirror_get("latest.txt")
    old = stable_version(current.decode("utf-8", "replace")) if current is not None else None
    if old is not None and old >= new:
        print(f"latest.txt stays at {'.'.join(map(str, old))}")
        return
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "latest.txt")
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(version + "\n")
        put(path, "latest.txt", CACHE_LATEST)
    print(f"latest.txt -> {version}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("version").set_defaults(run=cmd_version)
    p = sub.add_parser("check")
    p.add_argument("--tag", help="the release tag, for example v0.1.0")
    p.set_defaults(run=cmd_check)
    p = sub.add_parser("notes")
    p.add_argument("version")
    p.set_defaults(run=cmd_notes)
    p = sub.add_parser("bump")
    p.add_argument("version")
    p.set_defaults(run=cmd_bump)
    sub.add_parser("sync-licenses").set_defaults(run=cmd_sync_licenses)
    p = sub.add_parser("publish-crates")
    p.add_argument("--dry-run", action="store_true")
    p.set_defaults(run=cmd_publish_crates)
    p = sub.add_parser("mirror")
    p.add_argument("directory", help="release files and their SHA256SUMS")
    p.add_argument("--version", required=True, help="the release version, for example 0.5.3")
    p.add_argument("--dry-run", action="store_true")
    p.set_defaults(run=cmd_mirror)
    args = parser.parse_args()
    args.run(args)


if __name__ == "__main__":
    main()
