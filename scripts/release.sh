#!/bin/sh
# Builds static binaries for Linux, macOS and Windows into dist/ and packs each
# with the agent skill. Usage: scripts/release.sh [version]
# Needs the five rustup targets, cargo-zigbuild and zig (cargo install
# cargo-zigbuild; pip install ziglang, or zig on PATH).
set -eu
version=${1:-dev}
cd "$(dirname "$0")/.."
rm -rf dist
mkdir -p dist
for target in linux/amd64 linux/arm64 darwin/amd64 darwin/arm64 windows/amd64; do
  os=${target%/*}
  arch=${target#*/}
  case $target in
    linux/amd64) triple=x86_64-unknown-linux-musl ;;
    linux/arm64) triple=aarch64-unknown-linux-musl ;;
    darwin/amd64) triple=x86_64-apple-darwin ;;
    darwin/arm64) triple=aarch64-apple-darwin ;;
    windows/amd64) triple=x86_64-pc-windows-gnu ;;
  esac
  name=docc_${version}_${os}_${arch}
  bin=docc
  [ "$os" = windows ] && bin=docc.exe
  mkdir -p "dist/$name/skills/docc"
  DOCC_VERSION=$version cargo zigbuild --quiet --release --locked --target "$triple"
  cp "target/$triple/release/$bin" "dist/$name/$bin"
  cp skills/docc/SKILL.md "dist/$name/skills/docc/SKILL.md"
  if [ "$os" = windows ]; then
    (cd dist && zip -qr "$name.zip" "$name")
  else
    tar -C dist -czf "dist/$name.tar.gz" "$name"
  fi
  rm -rf "dist/$name"
done
(cd dist && sha256sum docc_* > checksums.txt)
ls -l dist
