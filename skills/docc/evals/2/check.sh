#!/bin/sh
# New fact in the anchored paragraph, the referring sentence and the code; no suspect links (check covers DOC209/DOC402).
docc check --max-warnings 0 || exit 1
grep -q '30 минут.* ^lockout' docs/src/security-policy.json || { echo "anchored paragraph lacks 30 минут"; exit 1; }
grep -q '30 минут' docs/src/login-screen.json || { echo "login-screen does not say 30 минут"; exit 1; }
! grep -rn '15 минут' docs/src || { echo "15 минут is still in docs/src"; exit 1; }
grep -q 'lockoutDuration = 30 \* time.Minute' code/auth.go || { echo "code still has the old duration"; exit 1; }
grep -q 'docc:begin security-policy#^lockout' code/auth.go || { echo "code region removed"; exit 1; }
echo PASS
