#!/bin/sh
# Builds static binaries for Linux, macOS and Windows into dist/ and packs each
# with the agent skill. Usage: scripts/release.sh [version]
set -eu
version=${1:-dev}
cd "$(dirname "$0")/.."
rm -rf dist
mkdir -p dist
for target in linux/amd64 linux/arm64 darwin/amd64 darwin/arm64 windows/amd64; do
  os=${target%/*}
  arch=${target#*/}
  name=docc_${version}_${os}_${arch}
  bin=docc
  [ "$os" = windows ] && bin=docc.exe
  mkdir -p "dist/$name/skills/docc"
  CGO_ENABLED=0 GOOS=$os GOARCH=$arch go build -trimpath -ldflags "-s -w -X main.version=$version" -o "dist/$name/$bin" ./cmd/docc
  cp skills/docc/SKILL.md "dist/$name/skills/docc/SKILL.md"
  if [ "$os" = windows ]; then
    (cd dist && zip -qr "$name.zip" "$name")
  else
    tar -C dist -czf "dist/$name.tar.gz" "$name"
  fi
  rm -rf "dist/$name"
done
ls -l dist
