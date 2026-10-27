#!/bin/sh
# New fact in the paragraph ^qd87, the referring sentence and the code; every hash and fingerprint updated (check covers DOC209/DOC402).
docc check --max-warnings 0 || exit 1
grep -q '30 минут.* ^qd87' docs/src/security-policy.json || { echo "paragraph ^qd87 lacks 30 минут, or its code changed"; exit 1; }
grep -q '30 минут' docs/src/login-screen.json || { echo "login-screen does not say 30 минут"; exit 1; }
! grep -rn '15 минут' docs/src || { echo "15 минут is still in docs/src"; exit 1; }
grep -q 'lockoutDuration = 30 \* time.Minute' code/auth.go || { echo "code still has the old duration"; exit 1; }
grep -q 'docc:begin security-policy#^qd87@' code/auth.go || { echo "code region removed"; exit 1; }
echo PASS
