#!/bin/sh
# Installs docc from GitHub Releases. Usage: curl -fsSL https://raw.githubusercontent.com/uxname/docc/main/install.sh | sh
# Env: DOCC_VERSION (tag, default latest), DOCC_INSTALL_DIR (default ~/.local/bin), DOCC_BASE_URL (mirror).
set -eu
base=${DOCC_BASE_URL:-https://github.com/uxname/docc/releases}
dir=${DOCC_INSTALL_DIR:-$HOME/.local/bin}
case $(uname -s) in
  Linux) os=linux ;;
  Darwin) os=darwin ;;
  *) echo "docc: unsupported system $(uname -s); on Windows use install.ps1" >&2; exit 1 ;;
esac
case $(uname -m) in
  x86_64 | amd64) arch=amd64 ;;
  aarch64 | arm64) arch=arm64 ;;
  *) echo "docc: unsupported CPU $(uname -m)" >&2; exit 1 ;;
esac
if [ -n "${DOCC_VERSION:-}" ]; then
  list=$base/download/$DOCC_VERSION/checksums.txt
else
  list=$base/latest/download/checksums.txt
fi
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL "$list" -o "$tmp/checksums.txt"
line=$(grep -E "_${os}_${arch}\.tar\.gz\$" "$tmp/checksums.txt" | head -n 1) || true
[ -n "$line" ] || { echo "docc: the release has no archive for $os/$arch" >&2; exit 1; }
want=${line%% *}
name=${line##* }
name=${name#\*}
tag=${name#docc_}
tag=${tag%_${os}_${arch}.tar.gz}
curl -fsSL "$base/download/$tag/$name" -o "$tmp/$name"
if command -v sha256sum >/dev/null 2>&1; then got=$(sha256sum "$tmp/$name" | cut -d' ' -f1); else got=$(shasum -a 256 "$tmp/$name" | cut -d' ' -f1); fi
[ "$got" = "$want" ] || { echo "docc: checksum mismatch for $name, nothing installed" >&2; exit 1; }
tar -xzf "$tmp/$name" -C "$tmp"
mkdir -p "$dir"
install -m 755 "$tmp/${name%.tar.gz}/docc" "$dir/docc.new"
mv -f "$dir/docc.new" "$dir/docc"
echo "docc $tag installed to $dir/docc"
case ":$PATH:" in *":$dir:"*) ;; *) echo "add $dir to PATH: export PATH=\"$dir:\$PATH\"" ;; esac
