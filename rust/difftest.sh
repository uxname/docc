#!/bin/sh
# Runs the Go oracle and the Rust build on every project of the corpus and
# compares what they print (stdout, stderr, exit code) and what they write.
# usage: rust/difftest.sh GO_BIN RUST_BIN [OUT_DIR]
# The corpus: testdata/*/{red,green}, skills/docc/evals/*, this repository and
# ~/Документы/Work/o2 (both as fresh clones, so history-based rules see Git).
# A difference is listed in OUT_DIR/summary.txt; OUT_DIR/<case>/ keeps both sides.
set -u
go_bin=$(realpath "$1") rs_bin=$(realpath "$2")
out=$(realpath -m "${3:-/tmp/docc-difftest}")
repo=$(cd "$(dirname "$0")/.." && pwd)
rm -rf "$out" && mkdir -p "$out"
work=$(mktemp -d) && trap 'rm -rf "$work"' EXIT
fails=0

# step CASE NAME DIR ARGS...: run both binaries in their copy of the project
step() {
	c=$1 n=$2 && shift 2
	mkdir -p "$out/$c"
	(cd "$work/$c/go" && "$go_bin" "$@" >"$out/$c/$n.go.out" 2>"$out/$c/$n.go.err"; echo $? >"$out/$c/$n.go.code")
	(cd "$work/$c/rs" && "$rs_bin" "$@" >"$out/$c/$n.rs.out" 2>"$out/$c/$n.rs.err"; echo $? >"$out/$c/$n.rs.code")
	for s in out err code; do
		if ! cmp -s "$out/$c/$n.go.$s" "$out/$c/$n.rs.$s"; then
			echo "$c: $* ($s)" >>"$out/summary.txt"
			fails=$((fails + 1))
		fi
	done
}

# project CASE SRC [ARGS...]: copy SRC twice and run the command set on it
project() {
	c=$1 src=$2 && shift 2
	mkdir -p "$work/$c"
	if [ -d "$src/.git" ]; then
		git clone -q --no-hardlinks "$src" "$work/$c/go" && git clone -q --no-hardlinks "$src" "$work/$c/rs"
	else
		cp -a "$src" "$work/$c/go" && cp -a "$src" "$work/$c/rs"
	fi
	if [ $# -gt 0 ]; then
		step "$c" args "$@" --format json --max-diagnostics 0
	fi
	step "$c" validate validate --format json --max-diagnostics 0
	step "$c" check-json check --format json --max-diagnostics 0
	step "$c" check check --max-diagnostics 0
	step "$c" build build
	step "$c" check-after check --max-warnings 0 --max-diagnostics 0
	if ! diff -r --exclude=.git "$work/$c/go" "$work/$c/rs" >"$out/$c/tree.diff" 2>&1; then
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
mkdir -p "$work/cli/go" "$work/cli/rs"
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
