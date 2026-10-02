#!/bin/sh
# Installs the latest `sessions` release for this machine.
#
#   curl -fsSL https://raw.githubusercontent.com/JoyoMDEV/session-tui/main/install.sh | sh
#   curl -fsSL .../install.sh | sh -s -- --setup --import
#
# Options:
#   --version vX.Y.Z   install this release instead of the latest
#   --setup            also add the SessionStart hook to Claude Code's settings.json
#   --import           also register your existing Claude Code sessions
#   -h, --help         show this help
#
# Environment:
#   INSTALL_DIR             where to put the binary (default: ~/.local/bin)
#   SESSIONS_RELEASE_BASE   release URL prefix (default: the GitHub releases of this repo)
set -eu

REPO="JoyoMDEV/session-tui"
BASE="${SESSIONS_RELEASE_BASE:-https://github.com/$REPO/releases}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "$*"; }
die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<EOF
Installs the latest sessions release for this machine.

Usage: install.sh [--version vX.Y.Z] [--setup] [--import]

  --version vX.Y.Z   install this release instead of the latest
  --setup            also add the SessionStart hook to Claude Code's settings.json
  --import           also register your existing Claude Code sessions
  -h, --help         show this help

Environment:
  INSTALL_DIR             where to put the binary (default: \$HOME/.local/bin)
  SESSIONS_RELEASE_BASE   release URL prefix (default: the GitHub releases of $REPO)
EOF
}

version=""
do_setup=0
do_import=0
while [ $# -gt 0 ]; do
    case "$1" in
    --version)
        [ $# -ge 2 ] || die "--version needs a value, e.g. v0.1.0"
        version="$2"
        shift 2
        ;;
    --setup)
        do_setup=1
        shift
        ;;
    --import)
        do_import=1
        shift
        ;;
    -h | --help)
        usage
        exit 0
        ;;
    *) die "unknown option: $1 (try --help)" ;;
    esac
done

for tool in curl tar uname mktemp install; do
    command -v "$tool" >/dev/null 2>&1 || die "$tool is required but not installed"
done
if command -v sha256sum >/dev/null 2>&1; then
    verify() { sha256sum -c "$1" >/dev/null; }
elif command -v shasum >/dev/null 2>&1; then
    verify() { shasum -a 256 -c "$1" >/dev/null; }
else
    die "sha256sum or shasum is required to verify the download"
fi

case "$(uname -s)" in
Darwin) os="apple-darwin" ;;
Linux) os="unknown-linux-gnu" ;;
*) die "unsupported OS: $(uname -s) (macOS and Linux only)" ;;
esac
case "$(uname -m)" in
arm64 | aarch64) arch="aarch64" ;;
x86_64 | amd64) arch="x86_64" ;;
*) die "unsupported CPU: $(uname -m)" ;;
esac
target="$arch-$os"

if [ -z "$version" ]; then
    # /releases/latest redirects to /releases/tag/<tag>; this avoids the rate-limited API.
    latest_url="$(curl -fsSIL -o /dev/null -w '%{url_effective}' "$BASE/latest")" ||
        die "could not look up the latest release (is the repository public and does it have a release?)"
    version="${latest_url##*/}"
    case "$version" in
    v[0-9]*) ;;
    *) die "could not read a version from $latest_url" ;;
    esac
fi

name="sessions-$version-$target"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

say "Installing sessions $version for $target"
curl -fsSL -o "$tmp/$name.tar.gz" "$BASE/download/$version/$name.tar.gz" ||
    die "no $target build of $version found at $BASE/download/$version/"
curl -fsSL -o "$tmp/$name.tar.gz.sha256" "$BASE/download/$version/$name.tar.gz.sha256" ||
    die "could not download the checksum for $name.tar.gz"

(cd "$tmp" && verify "$name.tar.gz.sha256") ||
    die "checksum mismatch for $name.tar.gz; nothing was installed"

tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
[ -f "$tmp/$name/sessions" ] || die "the archive does not contain a sessions binary"

mkdir -p "$INSTALL_DIR"
install -m 755 "$tmp/$name/sessions" "$INSTALL_DIR/sessions"
say "Installed $("$INSTALL_DIR/sessions" --version) to $INSTALL_DIR/sessions"

case ":$PATH:" in
*":$INSTALL_DIR:"*) ;;
*)
    say ""
    say "$INSTALL_DIR is not on your PATH. Add this to your shell profile:"
    say "  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac

if [ "$do_setup" -eq 1 ]; then
    say ""
    "$INSTALL_DIR/sessions" setup
fi
if [ "$do_import" -eq 1 ]; then
    say ""
    "$INSTALL_DIR/sessions" import
fi

if [ "$do_setup" -eq 0 ] || [ "$do_import" -eq 0 ]; then
    say ""
    say "Next steps:"
    [ "$do_setup" -eq 1 ] || say "  sessions setup    add the hook to Claude Code (or install the plugin instead)"
    [ "$do_import" -eq 1 ] || say "  sessions import   register your existing sessions"
    say "  sessions          open the browser"
fi
