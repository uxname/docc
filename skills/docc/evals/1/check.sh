#!/bin/sh
# A new kind with a boolean additionalProperties, a document of that kind for /api/orders, the kind named in the answer.
docc check --max-warnings 0 || exit 1
doc=$(grep -l '/api/orders' docs/src/*.json | head -1); [ -n "$doc" ] || { echo "no document mentions /api/orders"; exit 1; }
kind=$(jq -r .kind "$doc"); [ "$kind" != page ] || { echo "document uses kind page"; exit 1; }
jq -e '.additionalProperties | type == "boolean"' "docs/schemas/$kind.json" >/dev/null || { echo "schema $kind.json lacks boolean additionalProperties"; exit 1; }
grep -q "$kind" answer.txt || { echo "answer does not name the kind $kind"; exit 1; }
echo PASS
