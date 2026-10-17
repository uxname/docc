#!/bin/sh
# Runs two docc builds (say, the last release and a new build) on every project
# of the corpus and compares what they print (stdout, stderr, exit code) and
# what they write. It checked the Rust port against the Go version.
# usage: scripts/difftest.sh OLD_BIN NEW_BIN [OUT_DIR]
# The corpus: testdata/*/{red,green}, skills/docc/evals/*, this repository and
# ~/Документы/Work/o2 (both as fresh clones, so history-based rules see Git).
# A difference is listed in OUT_DIR/summary.txt; OUT_DIR/<case>/ keeps both sides.
set -u
old_bin=$(realpath "$1") new_bin=$(realpath "$2")
out=$(realpath -m "${3:-/tmp/docc-difftest}")
repo=$(cd "$(dirname "$0")/.." && pwd)
rm -rf "$out" && mkdir -p "$out"
work=$(mktemp -d) && trap 'rm -rf "$work"' EXIT
fails=0

# step CASE NAME DIR ARGS...: run both binaries in their copy of the project
step() {
	c=$1 n=$2 && shift 2
	mkdir -p "$out/$c"
	(cd "$work/old/$c" && "$old_bin" "$@" >"$out/$c/$n.old.out" 2>"$out/$c/$n.old.err"; echo $? >"$out/$c/$n.old.code")
	(cd "$work/new/$c" && "$new_bin" "$@" >"$out/$c/$n.new.out" 2>"$out/$c/$n.new.err"; echo $? >"$out/$c/$n.new.code")
	for s in out err code; do
		if ! cmp -s "$out/$c/$n.old.$s" "$out/$c/$n.new.$s"; then
			echo "$c: $* ($s)" >>"$out/summary.txt"
			fails=$((fails + 1))
		fi
	done
}

# unzip_html replaces the base64 zip in the data of each HTML file it is given
# with the names and contents of the files in it
unzip_html='
import base64, io, json, re, sys, zipfile
for f in sys.argv[1:]:
    s = open(f, encoding="utf-8").read()
    def sub(m):
        z = zipfile.ZipFile(io.BytesIO(base64.b64decode(m.group(1))))
        files = {i.filename: z.read(i).decode() for i in z.infolist()}
        return "\"zip\":" + json.dumps(files, sort_keys=True, ensure_ascii=False).replace("<", "\\u003c")
    open(f, "w", encoding="utf-8").write(re.sub(r"\"zip\":\"([A-Za-z0-9+/=]*)\"", sub, s))
'

# project CASE SRC [ARGS...]: copy SRC twice and run the command set on it
project() {
	c=$1 src=$2 && shift 2
	mkdir -p "$work/old" "$work/new"
	if [ -d "$src/.git" ]; then
		git clone -q --no-hardlinks "$src" "$work/old/$c" && git clone -q --no-hardlinks "$src" "$work/new/$c"
	else
		cp -a "$src" "$work/old/$c" && cp -a "$src" "$work/new/$c"
	fi
	if [ $# -gt 0 ]; then
		step "$c" args "$@" --format json --max-diagnostics 0
	fi
	step "$c" validate validate --format json --max-diagnostics 0
	step "$c" check-json check --format json --max-diagnostics 0
	step "$c" check check --max-diagnostics 0
	step "$c" build build
	step "$c" check-after check --max-warnings 0 --max-diagnostics 0
	step "$c" build-html build --single --html
	step "$c" build-full build --full --single --html
	# the zip inside the HTML is compressed by another library: compare what it holds
	find "$work/old/$c" "$work/new/$c" -name '*.html' -path '*docs/out*' -exec python3 -I -c "$unzip_html" {} +
	if ! diff -r --exclude=.git "$work/old/$c" "$work/new/$c" >"$out/$c/tree.diff" 2>&1; then
		echo "$c: written files differ" >>"$out/summary.txt"
		fails=$((fails + 1))
	fi
}

for d in "$repo"/testdata/*/red "$repo"/testdata/*/green; do
	[ -d "$d" ] || continue
	c=$(basename "$(dirname "$d")")-$(basename "$d")
	if [ -f "$d/args.txt" ]; then
		# shellcheck disable=SC2046
		project "$c" "$d" $(cat "$d/args.txt")
	else
		project "$c" "$d"
	fi
done
for d in "$repo"/skills/docc/evals/*/; do
	project "eval-$(basename "$d")" "$d"
done
project self "$repo"
[ -d "$HOME/Документы/Work/o2" ] && project o2 "$HOME/Документы/Work/o2"

# commands that need no project
mkdir -p "$work/old/cli" "$work/new/cli"
step cli help --help
step cli version --version
step cli none
step cli unknown nosuch
step cli rules rules
step cli rules-json rules --format json
step cli explain explain DOC209
step cli explain-bad explain DOC000
step cli schema-document schema document
step cli schema-config schema config
for cmd in validate build check init rules ack explain schema update comments; do
	step cli "help-$cmd" "$cmd" --help
done

echo "$fails difference(s); details in $out"
[ -f "$out/summary.txt" ] && cat "$out/summary.txt"
[ "$fails" -eq 0 ]
