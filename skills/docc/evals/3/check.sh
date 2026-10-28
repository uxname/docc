#!/bin/sh
# A docc:begin/end region around WriteCSV only, pointing at the CSV format paragraph ^RE-2 with its version hash and the region's fingerprint.
docc check --max-warnings 0 || exit 1
region=$(sed -n '/docc:begin.*req-export#^RE-2/,/docc:end/p' code/export.go)
echo "$region" | grep -q 'func WriteCSV' || { echo "no region around WriteCSV"; exit 1; }
! echo "$region" | grep -q 'func FileName' || { echo "region also covers FileName"; exit 1; }
grep -qE 'docc:begin.*req-export#\^RE-2@[2-9a-hjkmnp-z]{4}.* ~[2-9a-hjkmnp-z]{4}' code/export.go || { echo "the region has no hash or fingerprint"; exit 1; }
echo PASS
