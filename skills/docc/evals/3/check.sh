#!/bin/sh
# A docc:begin/end region around WriteCSV only, pointing at the csv-format paragraph, recorded with a code hash in docc.lock.
docc check --max-warnings 0 || exit 1
region=$(sed -n '/docc:begin.*req-export#^csv-format/,/docc:end/p' code/export.go)
echo "$region" | grep -q 'func WriteCSV' || { echo "no region around WriteCSV"; exit 1; }
! echo "$region" | grep -q 'func FileName' || { echo "region also covers FileName"; exit 1; }
grep -qE '^code/export.go req-export#\^csv-format [0-9a-f]+ [0-9a-f]+$' docc.lock || { echo "lock has no code-region line"; exit 1; }
echo PASS
