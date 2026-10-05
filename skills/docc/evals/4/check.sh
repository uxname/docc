#!/bin/sh
# Section renamed, both document links and the code tag follow; no DOC312 (check covers it).
docc check --max-warnings 0 || exit 1
! grep -rn 'Возврат средств' docs/src code || { echo "old section name left"; exit 1; }
jq -e '.rules["Возврат и отмена заказа"]' docs/src/billing.json >/dev/null || { echo "section not renamed"; exit 1; }
n=$(grep -rl 'billing#rules/Возврат и отмена заказа' docs/src code | wc -l); [ "$n" -eq 3 ] || { echo "expected 3 referring files, got $n"; exit 1; }
echo PASS
