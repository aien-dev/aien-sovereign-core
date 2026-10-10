#!/bin/sh
# aien-verify tests: sha256 vectors, both committed fixtures, four mutation tests on copies.
# Usage: tests/run.sh   (set TMPDIR to a scratch folder; fixtures are never written to)
set -u
here=$(cd "$(dirname "$0")/.." && pwd)
repo=$(cd "$here/../.." && pwd)
runs=$repo/docs/campaigns/whole-system-e2e/runs
FA=$runs/RUN-1-dry-20261010T133749Z
FB=$runs/RUN-1-dry-20261010T134202Z
# contract digest of aien-architecture main 3c584a49 (passed on the command line, as the verifier requires)
CS=8ba653848287bc77db253466f6d3ff37c70a42be55601ad47eb1f838a41d20f0
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
bin=$work/aien-verify
pass=0; fail=0
ok()  { pass=$((pass + 1)); echo "ok   $1"; }
bad() { fail=$((fail + 1)); echo "FAIL $1"; }
check() { # name, command...
  n=$1; shift
  if "$@" >/dev/null 2>&1; then ok "$n"; else bad "$n"; fi
}
tree_sum() { (cd "$1" && find . -type f | LC_ALL=C sort | xargs sha256sum | sha256sum | cut -d' ' -f1); }

before=$(tree_sum "$runs")
"$here/build.sh" "$bin" >/dev/null 2>&1 || { echo "build failed"; exit 1; }
rustc --edition 2021 --test "$here/src/main.rs" -o "$work/unit" 2>/dev/null && check "unit tests (sha256 vectors incl. empty and abc)" "$work/unit"
check "binary selftest (sha256 vectors)" "$bin" --selftest

rows_of() { grep -E '^(E[1-6]|CTRL-E[1-6]) ' ; }
expect() { # name, run dir, expected rows (here-doc via $3)
  out=$("$bin" "$2" --contract-sha256 $CS 2>/dev/null)
  got=$(printf '%s\n' "$out" | rows_of)
  if [ "$got" = "$3" ]; then ok "$1 rows"; else bad "$1 rows"; printf 'got:\n%s\n' "$got"; fi
}
expect "fixture A" "$FA" "E1 NOT_RUN
E2 NOT_RUN
E3 FAIL
E4 NOT_RUN
E5 NOT_RUN
E6 PASS
CTRL-E1 NOT_RUN
CTRL-E2 NOT_RUN
CTRL-E3 NOT_RUN
CTRL-E4 NOT_RUN
CTRL-E5 NOT_RUN
CTRL-E6 NOT_RUN"
expect "fixture B" "$FB" "E1 NOT_RUN
E2 NOT_RUN
E3 FAIL
E4 FAIL
E5 NOT_RUN
E6 PASS
CTRL-E1 NOT_RUN
CTRL-E2 NOT_RUN
CTRL-E3 NOT_RUN
CTRL-E4 NOT_RUN
CTRL-E5 NOT_RUN
CTRL-E6 NOT_RUN"

vid() { "$bin" "$1" --contract-sha256 $CS 2>/dev/null | sed -n 's/^verdict_id //p'; }
A1=$(vid "$FA"); A2=$(vid "$FA"); B1=$(vid "$FB"); B2=$(vid "$FB")
[ -n "$A1" ] && [ "$A1" = "$A2" ] && ok "fixture A verdict_id stable ($A1)" || bad "fixture A verdict_id stable"
[ -n "$B1" ] && [ "$B1" = "$B2" ] && ok "fixture B verdict_id stable ($B1)" || bad "fixture B verdict_id stable"
[ "$A1" = 728ae9152ef546ab4ccf1b7619fd22d21acb2914870197ac58e8843f9afaaf77 ] && ok "fixture A verdict_id pinned" || bad "fixture A verdict_id pinned"
[ "$B1" = d7d57162ac318ec05f2d97ad9530d9d3a571275409b040f65ccd0e391f6c9615 ] && ok "fixture B verdict_id pinned" || bad "fixture B verdict_id pinned"
for F in "$FA" "$FB"; do
  "$bin" "$F" --contract-sha256 $CS 2>/dev/null | grep -q '^chain_unbroken true$' && ok "$(basename $F) chain unbroken" || bad "$(basename $F) chain unbroken"
  "$bin" "$F" --contract-sha256 $CS >/dev/null 2>&1; [ $? -eq 1 ] && ok "$(basename $F) exit 1 (FAIL rows)" || bad "$(basename $F) exit 1"
done
"$bin" "$FB" --contract-sha256 $CS --objective-id 093e66ec92a6b428b49a0aa2a75678e9fdb155f423e1e7c71c5ae9fef11a60b1 2>/dev/null | grep -q '^chain_unbroken true$' && ok "objective id argument accepted" || bad "objective id argument accepted"
"$bin" "$FB" --contract-sha256 $CS --objective-id 00000000000000000000000000000000000000000000000000000000000000aa 2>/dev/null | grep -q 'OBJECTIVE_ID' && ok "wrong objective id named" || bad "wrong objective id named"
"$bin" "$FB" >/dev/null 2>&1; [ $? -eq 2 ] && ok "missing --contract-sha256 refused" || bad "missing --contract-sha256 refused"

# E5 with a peer: equal base id passes, unequal fails; both machines then print one final id
PASSID=$("$bin" "$FB" --contract-sha256 $CS --peer-verdict-id "$B1" 2>/dev/null | sed -n 's/^verdict_id //p')
"$bin" "$FB" --contract-sha256 $CS --peer-verdict-id "$B1" 2>/dev/null | grep -q '^E5 PASS$' && ok "E5 PASS with matching peer base id" || bad "E5 PASS with matching peer base id"
[ "$PASSID" != "$B1" ] && ok "E5 PASS changes the final id ($PASSID)" || bad "E5 changes final id"
"$bin" "$FB" --contract-sha256 $CS --peer-verdict-id "$A1" 2>/dev/null | grep -q '^E5 FAIL$' && ok "E5 FAIL with differing peer id" || bad "E5 FAIL with differing peer id"

# --write writes both files into a copy, never into the fixture
cp -r "$FB" "$work/w" && "$bin" "$work/w" --contract-sha256 $CS --write >/dev/null 2>&1
[ -s "$work/w/VERIFIER_VERDICT.txt" ] && [ -s "$work/w/VERIFIER_RECEIPT.json" ] && ok "--write wrote verdict and receipt" || bad "--write wrote verdict and receipt"
grep -q '"chain_unbroken": true' "$work/w/VERIFIER_RECEIPT.json" && grep -q '"verifier_sha256": "' "$work/w/VERIFIER_RECEIPT.json" && ok "receipt carries chain_unbroken and verifier_sha256" || bad "receipt fields"
"$bin" "$work/w" --contract-sha256 $CS 2>/dev/null | grep -q '^chain_unbroken true$' && ok "re-verify after --write still unbroken" || bad "re-verify after --write"

mutant() { rm -rf "$work/m"; cp -r "$1" "$work/m"; }
detect() { # name, expected reason text, expected row line or empty
  out=$("$bin" "$work/m" --contract-sha256 $CS 2>/dev/null); rc=$?
  if [ $rc -ne 0 ] && printf '%s\n' "$out" | grep -q -- "$2"; then ok "$1 detected ($2)"; else bad "$1 detected ($2)"; printf '%s\n' "$out" | grep '^# ' | head -5; fi
  if [ -n "${3:-}" ]; then printf '%s\n' "$out" | grep -q -- "$3" && ok "$1 row: $3" || bad "$1 row: $3"; fi
}
# M1: one byte changed in an output file
mutant "$FB"; printf 'X' | dd of="$work/m/artifacts/E3m-report.md" bs=1 seek=3 conv=notrunc 2>/dev/null
detect "M1 one byte changed in an output file" MANIFEST_DIGEST "# (supplementary) E3m FAIL: DIGEST_MISMATCH"
# M2: one chain link broken
mutant "$FB"; sed -i 's/"prev_receipt_sha256": "[0-9a-f]*"/"prev_receipt_sha256": "1111111111111111111111111111111111111111111111111111111111111111"/' "$work/m/chain/006-E3m.json"
detect "M2 chain link broken" LINK_BROKEN
# M3: one file dropped from SHA256SUMS
mutant "$FB"; grep -v 'artifacts/E3m-report.md' "$work/m/chain/SHA256SUMS" >"$work/ms" && cp "$work/ms" "$work/m/chain/SHA256SUMS"
detect "M3 file dropped from SHA256SUMS" MANIFEST_UNLISTED
# M4a: a status edited FAIL to PASS, links and manifest NOT repaired
mutant "$FB"; sed -i 's/"status": "FAIL"/"status": "PASS"/' "$work/m/chain/005-E3.json"
detect "M4a receipt status FAIL->PASS, unrepaired" LINK_BROKEN
# M4b: same edit with every link and manifest line repaired, so only the false status word remains
mutant "$FB"; sed -i 's/"status": "FAIL"/"status": "PASS"/' "$work/m/chain/005-E3.json"
prev=0000000000000000000000000000000000000000000000000000000000000000
for f in $(ls "$work/m/chain"/0*.json | LC_ALL=C sort); do
  sed -i "s/\"prev_receipt_sha256\": \"[0-9a-f]*\"/\"prev_receipt_sha256\": \"$prev\"/" "$f"
  prev=$(sha256sum "$f" | cut -d' ' -f1)
done
: >"$work/newsums"
while read -r h p; do
  if [ -f "$work/m/$p" ]; then printf '%s  %s\n' "$(sha256sum "$work/m/$p" | cut -d' ' -f1)" "$p" >>"$work/newsums"; else printf '%s  %s\n' "$h" "$p" >>"$work/newsums"; fi
done <"$work/m/chain/SHA256SUMS"
cp "$work/newsums" "$work/m/chain/SHA256SUMS"
out=$("$bin" "$work/m" --contract-sha256 $CS 2>/dev/null)
printf '%s\n' "$out" | grep -q '^chain_unbroken true$' && ok "M4b repaired chain is unbroken (only the lie remains)" || bad "M4b repaired chain unbroken"
printf '%s\n' "$out" | grep -q '^E3 FAIL$' && ok "M4b E3 stays FAIL without the facts" || bad "M4b E3 stays FAIL"
printf '%s\n' "$out" | grep -q 'status_disagreement: E3 receipt says PASS but the facts derive FAIL' && ok "M4b status_disagreement named" || bad "M4b status_disagreement named"
# M5: a file added to the folder but not the manifest; M6: a link target removed (sequence gap)
mutant "$FB"; echo extra >"$work/m/artifacts/extra.txt"
detect "M5 unlisted extra file" MANIFEST_UNLISTED
mutant "$FB"; rm "$work/m/chain/007-E4.json"
detect "M6 receipt file dropped" SEQUENCE_GAP

after=$(tree_sum "$runs")
[ "$before" = "$after" ] && ok "fixtures untouched" || bad "fixtures untouched"
echo "passed $pass, failed $fail"
[ $fail -eq 0 ]
