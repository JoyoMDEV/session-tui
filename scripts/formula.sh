#!/bin/sh
# Prints the Homebrew formula for a release, using the checksums published with it.
#
#   scripts/formula.sh v0.1.0 > Formula/sessions.rb
#
# SESSIONS_RELEASE_BASE overrides the release URL prefix, like in install.sh.
# CHECKSUM_DIR reads the .sha256 files from a local directory instead of downloading them, which
# is how the release pipeline runs it (it works while the repository is still private).
set -eu

REPO="JoyoMDEV/session-tui"
BASE="${SESSIONS_RELEASE_BASE:-https://github.com/$REPO/releases}"
tag="${1:?usage: formula.sh vX.Y.Z}"

# Fetch every checksum before printing anything, so a missing asset can't leave half a formula.
sha() {
    file="sessions-$tag-$1.tar.gz.sha256"
    if [ -n "${CHECKSUM_DIR:-}" ]; then
        sum="$(awk '{print $1; exit}' "$CHECKSUM_DIR/$file")"
    else
        sum="$(curl -fsSL "$BASE/download/$tag/$file" | awk '{print $1; exit}')"
    fi
    [ -n "$sum" ] || {
        echo "error: no checksum for $1 in $tag" >&2
        exit 1
    }
    case "$sum" in
    *[!0-9a-f]*)
        echo "error: unexpected checksum for $1: $sum" >&2
        exit 1
        ;;
    esac
    [ "${#sum}" -eq 64 ] || {
        echo "error: unexpected checksum length for $1" >&2
        exit 1
    }
    printf '%s' "$sum"
}
url() { printf '%s/download/%s/sessions-%s-%s.tar.gz' "$BASE" "$tag" "$tag" "$1"; }

mac_arm="$(sha aarch64-apple-darwin)"
mac_intel="$(sha x86_64-apple-darwin)"
linux_arm="$(sha aarch64-unknown-linux-gnu)"
linux_intel="$(sha x86_64-unknown-linux-gnu)"

cat <<EOF
class Sessions < Formula
  desc "Terminal UI to browse and resume saved Claude Code sessions"
  homepage "https://github.com/$REPO"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "$(url aarch64-apple-darwin)"
      sha256 "$mac_arm"
    end
    on_intel do
      url "$(url x86_64-apple-darwin)"
      sha256 "$mac_intel"
    end
  end

  on_linux do
    on_arm do
      url "$(url aarch64-unknown-linux-gnu)"
      sha256 "$linux_arm"
    end
    on_intel do
      url "$(url x86_64-unknown-linux-gnu)"
      sha256 "$linux_intel"
    end
  end

  def install
    bin.install "sessions"
  end

  def caveats
    <<~EOS
      Add the hook to Claude Code and register your existing sessions:
        sessions setup
        sessions import
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/sessions --version")
  end
end
EOF
