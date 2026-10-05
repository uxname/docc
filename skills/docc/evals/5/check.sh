#!/bin/sh
# New page between Пользователи and Требования by order, no typed numbers (DOC314 via check).
docc check --max-warnings 0 || exit 1
seq=$(jq -rs '[.[] | select(.parent == "index")] | sort_by([.order, .title]) | map(.title) | join("|")' docs/src/*.json)
[ "$seq" = "Цели|Пользователи|Ограничения|Требования|Термины" ] || { echo "sibling order: $seq"; exit 1; }
! jq -r .title docs/src/*.json | grep -qE '^[0-9]' || { echo "number typed into a title"; exit 1; }
echo PASS
