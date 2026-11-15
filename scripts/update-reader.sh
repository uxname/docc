#!/bin/sh
# Embed the reader built in the Docspace frontend: run its standalone build,
# copy reader.js and reader.css into src/render/reader/ and write the frontend
# commit into src/render/reader/VERSION. Then: cargo test && cargo build --release.
# Usage: scripts/update-reader.sh <path to the docspace frontend folder>
set -eu
front=${1:?usage: scripts/update-reader.sh <path to the docspace frontend folder>}
here=$(cd "$(dirname "$0")/.." && pwd)
dest="$here/src/render/reader"
(cd "$front" && npm run build:standalone)
for f in reader.js reader.css; do
  test -s "$front/dist-standalone/$f" || { echo "update-reader: $front/dist-standalone/$f is missing or empty" >&2; exit 1; }
done
# the bundle goes into a public repository: no server address may be in it
if grep -Eq 'https?://[a-z0-9.-]+\.(internal|local|lan)|gitea\.' "$front/dist-standalone/reader.js"; then
  echo "update-reader: reader.js names a private server; fix the standalone build first" >&2
  exit 1
fi
cp "$front/dist-standalone/reader.js" "$front/dist-standalone/reader.css" "$dest/"
commit=$(git -C "$front" rev-parse HEAD)
dirty=$(git -C "$front" status --porcelain -- . | head -n 1)
echo "${commit}${dirty:+-dirty}" > "$dest/VERSION"
echo "update-reader: embedded the reader of $commit${dirty:+ (with uncommitted changes)}"
