#!/bin/sh
# Installs vernier from a GitHub release on macOS or Linux.
#
#   curl -fsSL https://github.com/Go-Vernier/Vernier-OSS/releases/latest/download/install.sh | sh
#
# VERNIER_VERSION      a release such as v0.1.0 (default: the latest)
# VERNIER_INSTALL_DIR  where to put the binary (default: ~/.local/bin)
set -eu

REPO="https://github.com/Go-Vernier/Vernier-OSS"

say() { printf '%s\n' "$*"; }
die() {
  printf 'vernier install: %s\n' "$*" >&2
  exit 1
}

unsupported() { die "there is no prebuilt binary for $(uname -s) $(uname -m); build from source: $REPO"; }
case "$(uname -s)" in
  Darwin) os=apple-darwin ;;
  Linux) os=unknown-linux-musl ;;
  *) unsupported ;;
esac
case "$(uname -m)" in
  arm64 | aarch64) arch=aarch64 ;;
  x86_64 | amd64) arch=x86_64 ;;
  *) unsupported ;;
esac
asset="vernier-$arch-$os.tar.gz"

if [ -n "${VERNIER_BASE_URL:-}" ]; then
  base="$VERNIER_BASE_URL"
elif [ -n "${VERNIER_VERSION:-}" ]; then
  base="$REPO/releases/download/v${VERNIER_VERSION#v}"
else
  base="$REPO/releases/latest/download"
fi

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -qO "$2" "$1"; }
else
  die "needs curl or wget to download"
fi
if command -v sha256sum >/dev/null 2>&1; then
  check() { sha256sum -c "$1" >/dev/null 2>&1; }
elif command -v shasum >/dev/null 2>&1; then
  check() { shasum -a 256 -c "$1" >/dev/null 2>&1; }
else
  die "needs sha256sum or shasum to verify the download"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

say "Downloading $asset"
fetch "$base/$asset" "$tmp/$asset" || die "could not download $base/$asset"
fetch "$base/$asset.sha256" "$tmp/$asset.sha256" || die "could not download $base/$asset.sha256"
(cd "$tmp" && check "$asset.sha256") || die "checksum mismatch for $asset; nothing was installed"
tar -xzf "$tmp/$asset" -C "$tmp" vernier || die "$asset has no vernier binary"

dir="${VERNIER_INSTALL_DIR:-$HOME/.local/bin}"
mkdir -p "$dir"
cp "$tmp/vernier" "$dir/.vernier.new"
chmod 755 "$dir/.vernier.new"
mv -f "$dir/.vernier.new" "$dir/vernier"

say "Installed $("$dir/vernier" --version) to $dir/vernier"
case ":$PATH:" in
  *":$dir:"*) ;;
  *)
    say ""
    say "$dir is not on your PATH. Add it to your shell profile:"
    say "  export PATH=\"$dir:\$PATH\""
    ;;
esac
