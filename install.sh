#!/bin/sh
# Installs the latest bareframe release:
#   curl -fsSL https://raw.githubusercontent.com/rlaskaehd/bareframe/main/install.sh | sh
# Downloaded with curl, the app carries no quarantine flag, so it opens without Gatekeeper's
# prompt. Set BAREFRAME_INSTALL_DIR to install somewhere other than /Applications.
set -eu
repo=rlaskaehd/bareframe

fail() {
    echo "bareframe: $*" >&2
    exit 1
}

[ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] || fail "needs an Apple Silicon Mac"
[ "$(sw_vers -productVersion | cut -d. -f1)" -ge 14 ] || fail "needs macOS 14 or later"

dest=${BAREFRAME_INSTALL_DIR:-/Applications}
if [ -z "${BAREFRAME_INSTALL_DIR:-}" ] && [ ! -w "$dest" ]; then
    dest="$HOME/Applications"
fi
mkdir -p "$dest"
pgrep -qf "$dest/bareframe.app/" && fail "quit bareframe first, then run this again"

tag=$(curl -fsSL --proto '=https' "https://api.github.com/repos/$repo/releases/latest" |
    sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)
[ -n "$tag" ] || fail "could not find the latest release"
version=${tag#v}
name="bareframe-$version-macos-arm64.zip"
base="https://github.com/$repo/releases/download/$tag"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
echo "Downloading bareframe $version"
curl -fL --proto '=https' --progress-bar -o "$work/$name" "$base/$name"
curl -fsSL --proto '=https' -o "$work/$name.sha256" "$base/$name.sha256"
(cd "$work" && shasum -a 256 -c "$name.sha256" >/dev/null) || fail "checksum mismatch"
ditto -x -k "$work/$name" "$work/unpacked"

# Copied beside the old app first, so a failed copy leaves the old one in place.
rm -rf "$dest/.bareframe-install.app"
ditto "$work/unpacked/bareframe.app" "$dest/.bareframe-install.app"
rm -rf "$dest/bareframe.app"
mv "$dest/.bareframe-install.app" "$dest/bareframe.app"
echo "Installed $dest/bareframe.app — open it with: open \"$dest/bareframe.app\""
