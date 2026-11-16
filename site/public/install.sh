#!/bin/sh
# cadkit installer for Linux and macOS.
#
#   curl -fsSL https://cadkit.org/install.sh | sh
#
# Downloads the prebuilt "cadkit" command-line tool for this machine, checks its SHA-256
# against the release's SHA256SUMS and copies the binary into a directory of yours. It needs
# no root, writes one file and does not edit shell start-up files.
#
# Environment:
#   CADKIT_VERSION       version to install, for example 0.5.3 (default: the latest release)
#   CADKIT_INSTALL_DIR   where the binary goes (default: $HOME/.local/bin)
#   CADKIT_DOWNLOAD_URL  release mirror to try first (default: https://release.cadkit.org)
#
# Files come from release.cadkit.org; when it cannot be reached, the same files are taken
# from the GitHub release. Windows: https://cadkit.org/install.ps1
#
# Source: https://github.com/TNYCL/cadkit/blob/main/site/public/install.sh
# License: MIT OR Apache-2.0

set -eu

REPO_URL="https://github.com/TNYCL/cadkit"
DEFAULT_DOWNLOAD_URL="https://release.cadkit.org"

say() {
	printf 'cadkit-install: %s\n' "$1"
}

fail() {
	printf 'cadkit-install: error: %s\n' "$1" >&2
	exit 1
}

have() {
	command -v "$1" > /dev/null 2>&1
}

# download URL FILE: quiet, HTTPS only, fails on HTTP errors.
download() {
	if have curl; then
		curl --proto '=https' --tlsv1.2 -fsSL --retry 2 -o "$2" "$1"
	else
		wget -q -O "$2" "$1"
	fi
}

sha256_of() {
	if have sha256sum; then
		sha256sum "$1" | cut -d ' ' -f 1
	elif have shasum; then
		shasum -a 256 "$1" | cut -d ' ' -f 1
	else
		openssl dgst -sha256 -r "$1" | cut -d ' ' -f 1
	fi
}

# Prints the Rust target triple of the archive for this machine.
detect_target() {
	os=$(uname -s)
	arch=$(uname -m)
	case "$os" in
		Linux)
			# Static musl binaries: they run on glibc and musl distributions alike.
			case "$arch" in
				x86_64 | amd64) echo "x86_64-unknown-linux-musl" ;;
				aarch64 | arm64) echo "aarch64-unknown-linux-musl" ;;
				*) fail "no prebuilt binary for Linux $arch; build from source with: cargo install cadkit-cli" ;;
			esac
			;;
		Darwin)
			# A shell running under Rosetta reports x86_64 on Apple Silicon.
			if [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2> /dev/null || echo 0)" = 1 ]; then
				arch=arm64
			fi
			case "$arch" in
				x86_64) echo "x86_64-apple-darwin" ;;
				arm64 | aarch64) echo "aarch64-apple-darwin" ;;
				*) fail "no prebuilt binary for macOS $arch; build from source with: cargo install cadkit-cli" ;;
			esac
			;;
		MINGW* | MSYS* | CYGWIN*)
			fail "on Windows use PowerShell: irm https://cadkit.org/install.ps1 | iex"
			;;
		*)
			fail "no prebuilt binary for $os; build from source with: cargo install cadkit-cli"
			;;
	esac
}

# fetch_release BASE: downloads the archive and SHA256SUMS of one release into $tmp.
# BASE is the URL of the directory that holds both files.
fetch_release() {
	rm -f "$tmp/$archive" "$tmp/SHA256SUMS"
	download "$1/$archive" "$tmp/$archive" 2> /dev/null && download "$1/SHA256SUMS" "$tmp/SHA256SUMS" 2> /dev/null
}

main() {
	have uname || fail "uname is required"
	have tar || fail "tar is required"
	have mktemp || fail "mktemp is required"
	have curl || have wget || fail "curl or wget is required"
	have sha256sum || have shasum || have openssl || fail "sha256sum, shasum or openssl is required"

	target=$(detect_target)
	archive="cadkit-$target.tar.gz"
	mirror="${CADKIT_DOWNLOAD_URL:-$DEFAULT_DOWNLOAD_URL}"
	mirror="${mirror%/}"
	install_dir="${CADKIT_INSTALL_DIR:-}"
	if [ -z "$install_dir" ]; then
		[ -n "${HOME:-}" ] || fail "HOME is not set; set CADKIT_INSTALL_DIR"
		install_dir="$HOME/.local/bin"
	fi

	tmp=$(mktemp -d)
	trap 'rm -rf "$tmp"' EXIT
	trap 'exit 130' INT
	trap 'exit 143' TERM

	version="${CADKIT_VERSION:-}"
	version="${version#v}"
	if [ -z "$version" ] && download "$mirror/latest.txt" "$tmp/latest.txt" 2> /dev/null; then
		version=$(tr -d ' \t\r\n' < "$tmp/latest.txt")
	fi
	# The version becomes part of a URL: accept only what a version can contain.
	case "$version" in
		*[!0-9A-Za-z.+-]*) fail "not a version: $version" ;;
	esac

	source=""
	if [ -n "$version" ] && fetch_release "$mirror/v$version"; then
		source="$mirror"
	elif [ -n "$version" ] && fetch_release "$REPO_URL/releases/download/v$version"; then
		source="GitHub Releases"
	elif [ -z "$version" ] && fetch_release "$REPO_URL/releases/latest/download"; then
		source="GitHub Releases"
	else
		fail "could not download $archive${version:+ $version} from $mirror or $REPO_URL/releases"
	fi

	want=$(awk -v name="$archive" '$2 == name || $2 == "*" name { print $1; exit }' "$tmp/SHA256SUMS")
	[ -n "$want" ] || fail "$archive is not listed in SHA256SUMS"
	got=$(sha256_of "$tmp/$archive")
	[ "$got" = "$want" ] || fail "checksum mismatch for $archive: expected $want, got $got"

	tar -xzf "$tmp/$archive" -C "$tmp"
	binary="$tmp/cadkit-$target/cadkit"
	[ -f "$binary" ] || fail "$archive does not contain cadkit-$target/cadkit"

	mkdir -p "$install_dir"
	# Copy next to the destination, then rename: a running cadkit is replaced, never truncated.
	cp "$binary" "$install_dir/.cadkit.tmp.$$"
	chmod 755 "$install_dir/.cadkit.tmp.$$"
	mv -f "$install_dir/.cadkit.tmp.$$" "$install_dir/cadkit"

	installed=$("$install_dir/cadkit" --version 2> /dev/null) || installed="cadkit${version:+ $version}"
	say "installed $installed ($target, from $source) to $install_dir/cadkit"

	case ":${PATH:-}:" in
		*":$install_dir:"*) ;;
		*)
			say "$install_dir is not on your PATH; add this line to your shell profile:"
			say "  export PATH=\"$install_dir:\$PATH\""
			;;
	esac
}

# Everything runs from this last line, so a download that stops half-way executes nothing.
main "$@"
