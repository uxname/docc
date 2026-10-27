#!/bin/sh
# Each error fixed at its cause: field renamed, link fixed (no invented id), placeholder removed without a guessed owner, no docIgnore, schema untouched.
docc check --max-warnings 0 || exit 1
jq -e '(.priority | sub(" \\^[2-9a-z]{4}$"; "")) == "high" and (has("priorty") | not)' docs/src/req-login.json >/dev/null || { echo "priority not fixed"; exit 1; }
grep -q '\[\[req-login@' docs/src/req-reset.json || { echo "link not fixed"; exit 1; }
[ ! -e docs/src/req-logn.json ] || { echo "invented req-logn"; exit 1; }
! jq -r 'del(.notes) | .. | strings' docs/src/*.json | grep -q 'N/A' || { echo "N/A left outside notes"; exit 1; }
jq -e 'has("owner") | not' docs/src/req-login.json >/dev/null || { echo "owner of req-login was guessed"; exit 1; }
! grep -rln docIgnore docs/src || { echo "docIgnore used"; exit 1; }
cmp -s docs/schemas/requirement.json "$FIX/docs/schemas/requirement.json" || { echo "schema changed"; exit 1; }
echo PASS
