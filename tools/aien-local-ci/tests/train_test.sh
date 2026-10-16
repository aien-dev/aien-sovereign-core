#!/usr/bin/env bash
# train_test.sh: offline proofs for tools/aien-local-ci/train.sh (no network, no real gh, worker, key or crumb).
# Fakes: gh (JSON files + the real --jq filter; writes are logged), aien-test (verify-bundle by the bundle's "fake"
# member, as in pipeline_test.sh), aien-local-ci once (writes a bundle: FAIL when the tested tree holds BAD or MAINBAD,
# no bundle when the sha was never pushed to origin), quietlock (held while $T/quiet exists), crumb (compile writes a
# deterministic .crumb from the tracked files; verify compares), git (logs every push so order can be checked).
# The real pipeline.sh does the publishing. Throwaway bare repositories stand in for GitHub and aien-receipts.
#   bash tools/aien-local-ci/tests/train_test.sh
set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TRAIN=$HERE/../train.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
pass=0 fail=0
ok() { pass=$((pass + 1)); echo "ok   $*"; }
bad() { fail=$((fail + 1)); echo "FAIL $*"; }
check() { local name=$1; shift; if "$@"; then ok "$name"; else bad "$name"; fi; }

export GIT_CONFIG_GLOBAL=$T/gitconfig GIT_CONFIG_NOSYSTEM=1
: > "$GIT_CONFIG_GLOBAL"
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid
export FAKE_LOG=$T/calls.log FAKE_GH=$T/gh ORIGIN=$T/origin-demo.git
export AIEN_STATE=$T/state AIEN_CLONES=$T/clones AIEN_RECEIPTS=$T/receipts AIEN_TEST_BIN=$T/bin/aien-test
export GH=$T/bin/gh GIT=$T/bin/git AIEN_REPOS=demo AIEN_LOCAL_CI=$T/bin/aien-local-ci QUIETLOCK=$T/bin/quietlock
export AIEN_CRUMB_TOOL=$T/bin/crumb AIEN_LOCK_WAIT=1 AIEN_RETRY_SLEEP=0 AIEN_QUIET_WAIT=0 AIEN_TRAIN_POLL=0 AIEN_CHECK_WAIT=0
mkdir -p "$T/bin" "$FAKE_GH" "$AIEN_STATE" "$AIEN_CLONES"
: > "$FAKE_LOG"
S=aien-spark/demo
CTX1="Workspace Verification & Invariant Audit"
L=ready-to-merge

cat > "$T/bin/gh" <<'EOF'
#!/usr/bin/env bash
echo "gh $*" >> "$FAKE_LOG"
[[ $1 == api ]] || exit 9
shift
method=GET expr="" path=""
while [[ $# -gt 0 ]]; do
  case $1 in -X) method=$2; shift 2 ;; --jq) expr=$2; shift 2 ;; -f|-F) shift 2 ;; --paginate) shift ;; *) path=$1; shift ;; esac
done
if [[ $method != GET ]]; then
  if [[ $method == POST && $path == */pulls ]]; then echo '{"number":900}' | { [[ -n $expr ]] && jq -r "$expr" || cat; }; fi
  exit 0
fi
f=$FAKE_GH/$(printf '%s' "$path" | tr -c 'A-Za-z0-9._-' '_').json
if [[ ! -f $f ]]; then
  case $path in
    */commits/*/check-runs*) sha=${path#*/commits/}; sha=${sha%%/*}
      if grep -qx "$sha" "$FAKE_GH/redcheck" 2>/dev/null; then c=failure; else c=success; fi
      printf '{"check_runs":[{"name":"%s","status":"completed","conclusion":"%s"}]}' "Workspace Verification & Invariant Audit" "$c" > "$f" ;;
    */commits/*/status*) echo '{"statuses":[]}' > "$f" ;;
    */pulls?state=open\&head=*) echo '[]' > "$f" ;;
    *) echo "gh: HTTP 404 $path" >&2; exit 1 ;;
  esac
fi
if [[ -n $expr ]]; then jq -r "$expr" "$f"; else cat "$f"; fi
EOF
cat > "$T/bin/aien-test" <<'EOF'
#!/usr/bin/env bash
echo "aien-test $*" >> "$FAKE_LOG"
[[ $1 == verify-bundle ]] || exit 9
file=$2; shift 2
while [[ $# -gt 0 ]]; do
  case $1 in --repo) repo=$2 ;; --commit) commit=$2 ;; --pubkey) key=$2 ;; --require) ;; *) exit 2 ;; esac; shift 2
done
[[ $(jq -r .repository "$file") == "$repo" && $(jq -r .commit "$file") == "$commit" ]] || exit 3
[[ $(jq -r .fake.key "$file") == "$key" ]] || exit 3
exit "$(jq -r .fake.rc "$file")"
EOF
cat > "$T/bin/aien-local-ci" <<'EOF'
#!/usr/bin/env bash
# fake worker: once REPO SHA -> bundle; FAIL when BAD or MAINBAD is in the tree; nothing when the sha is not at origin
echo "once $2 $3" >> "$FAKE_LOG"
[[ $1 == once ]] || exit 9
repo=$2 sha=$3
git --git-dir="$ORIGIN" cat-file -e "$sha^{commit}" 2>/dev/null || { echo "fake worker: $sha not at origin"; exit 1; }
rc=0
git --git-dir="$ORIGIN" cat-file -e "$sha:BAD" 2>/dev/null && rc=1
git --git-dir="$ORIGIN" cat-file -e "$sha:MAINBAD" 2>/dev/null && rc=1
d=$AIEN_STATE/bundles/$repo/$sha; mkdir -p "$d"
K2=$(printf '2%.0s' {1..64})
jq -n --arg repo "github.com/aien-dev/$repo" --arg c "$sha" --arg k "$K2" --argjson rc "$rc" \
  --arg s "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  '{repository:$repo, commit:$c, started_utc:$s, finished_utc:$s, verdict:(if $rc==0 then "PASS" else "FAIL" end),
    plan:{blocked:null}, signature:{pubkey:$k}, fake:{key:$k, rc:$rc}}' > "$d/bundle.json"
EOF
cat > "$T/bin/quietlock" <<'EOF'
#!/usr/bin/env bash
echo "quietlock $*" >> "$FAKE_LOG"
[[ $1 == check ]] || exit 9
[[ -e $T_QUIET ]] && { echo "QUIETLOCK_REFUSED held"; exit 75; }
exit 0
EOF
cat > "$T/bin/crumb" <<'EOF'
#!/usr/bin/env bash
# fake pinned crumb tool: .crumb at the root = one line derived from the tracked files (not the crumbs themselves)
echo "crumb $*" >> "$FAKE_LOG"
want="crumb-fake files=$(git ls-files | grep -v -E '(^|/)\.crumb$' | sort | sha256sum | cut -c1-16)"
case $1 in
  compile) printf '%s\n' "$want" > .crumb ;;
  verify) [[ -f .crumb && $(cat .crumb) == "$want" ]] ;;
  *) exit 9 ;;
esac
EOF
cat > "$T/bin/git" <<'EOF'
#!/usr/bin/env bash
# logs pushes (with the directory) so the order of receipts publication and the main fast-forward can be checked;
# races one commit onto main right before the train's push when $T_RACE names a seed clone
if [[ " $* " == *" push "* ]]; then
  echo "push $*" >> "$FAKE_LOG"
  if [[ -n ${T_RACE:-} && " $* " == *":refs/heads/main "* && ! -e $T_RACE/.raced ]]; then
    touch "$T_RACE/.raced"
    /usr/bin/git -C "$T_RACE" pull -q --rebase origin main && echo raced >> "$T_RACE/RACED" && /usr/bin/git -C "$T_RACE" add -A \
      && /usr/bin/git -C "$T_RACE" commit -q -m raced && /usr/bin/git -C "$T_RACE" push -q origin main
  fi
fi
exec /usr/bin/git "$@"
EOF
chmod +x "$T/bin/"*
export T T_QUIET=$T/quiet

ghset() { printf '%s\n' "$2" > "$FAKE_GH/$(printf '%s' "$1" | tr -c 'A-Za-z0-9._-' '_').json"; }
K1=$(printf '1%.0s' {1..64}) K2=$(printf '2%.0s' {1..64})
jl() { jq -c "select($1)" "$AIEN_STATE/train.jsonl" | wc -l; }
origin_main() { /usr/bin/git --git-dir="$ORIGIN" rev-parse main; }
origin_has() { /usr/bin/git --git-dir="$ORIGIN" cat-file -e "$1:$2" 2>/dev/null; }
remote_has() { /usr/bin/git --git-dir="$T/receipts.git" cat-file -e "main:demo/$1/bundle.json" 2>/dev/null; }
export -f origin_has remote_has

# origin "demo" with main (crumbs compiled by the fake tool), the trusted clone, receipts remote + clone.
/usr/bin/git init -q --bare -b main "$ORIGIN"
/usr/bin/git clone -q "$ORIGIN" "$T/seed" 2>/dev/null
SEED=$T/seed
mkdir -p "$SEED/.aien"
printf '# keys\n%s\n%s\n' "$K1" "$K2" > "$SEED/.aien/verify.pub"
printf 'g1\ng2\n' > "$SEED/.aien/always"
echo base > "$SEED/README"
(cd "$SEED" && /usr/bin/git add -A && "$T/bin/crumb" compile . && /usr/bin/git add -A && /usr/bin/git commit -q -m init && /usr/bin/git push -q origin main)
/usr/bin/git clone -q "$ORIGIN" "$AIEN_CLONES/demo" 2>/dev/null
/usr/bin/git init -q --bare -b main "$T/receipts.git"
/usr/bin/git clone -q "$T/receipts.git" "$T/rseed" 2>/dev/null
echo shelf > "$T/rseed/README.md" && /usr/bin/git -C "$T/rseed" add -A && /usr/bin/git -C "$T/rseed" commit -q -m shelf && /usr/bin/git -C "$T/rseed" push -q origin main
/usr/bin/git clone -q "$T/receipts.git" "$AIEN_RECEIPTS"

# pr NUM FILE CONTENT [README_LINE]: a pull request head off main, as refs/pull/NUM/head at origin. Prints its sha.
pr() {
  local num=$1 file=$2 content=$3 readme=${4:-}
  (cd "$SEED" && /usr/bin/git checkout -q --detach main >/dev/null 2>&1
   mkdir -p "$(dirname "$file")"; printf '%s\n' "$content" > "$file"
   [[ -z $readme ]] || printf '%s\n' "$readme" > README
   /usr/bin/git add -A && /usr/bin/git commit -q -m "pr $num: $file" && /usr/bin/git push -q origin "HEAD:refs/pull/$num/head" && /usr/bin/git rev-parse HEAD)
}
declare -A PR
PR[11]=$(pr 11 a.txt a one)                              # good; also edits README (conflicts with 15)
PR[12]=$(pr 12 BAD bad)                                  # the culprit
PR[13]=$(pr 13 c.txt c)                                  # good
PR[14]=$(pr 14 .github/workflows/x.yml "on: push")       # BLOCKED_OPERATOR
PR[15]=$(pr 15 e.txt e five)                             # conflicts with 11 on README
PR[16]=$(pr 16 f.txt f)                                  # draft
PR[17]=$(pr 17 g.txt g)                                  # not labeled
PR[18]=$(pr 18 h.txt h)                                  # listing says another head (moved)
# pulls "NUM:SHA:DRAFT:LABEL:FILE" ...: the open PR listing, the single-PR views (same facts) and the file lists.
pulls() {
  local out="[" sep="" n s d l f spec
  for spec in "$@"; do
    IFS=: read -r n s d l f <<< "$spec"
    out+="$sep{\"number\":$n,\"draft\":$d,\"state\":\"open\",\"title\":\"PR $n\",\"head\":{\"sha\":\"$s\"},\"base\":{\"ref\":\"main\"},\"labels\":[{\"name\":\"other\"}${l:+,{\"name\":\"$l\"\}}]}"
    sep=,
    ghset "repos/$S/pulls/$n" "{\"number\":$n,\"draft\":$d,\"state\":\"open\",\"head\":{\"sha\":\"$s\"},\"labels\":[${l:+{\"name\":\"$l\"\}}]}"
    ghset "repos/$S/pulls/$n/files?per_page=100" "[{\"filename\":\"$f\"},{\"filename\":\"README\"}]"
  done
  ghset "repos/$S/pulls?state=open&per_page=100" "$out]"
}
ghset "repos/aien-dev/demo" '{"full_name":"aien-spark/demo"}'
ghset "repos/$S/branches/main/protection/required_status_checks" "{\"contexts\":[\"$CTX1\"]}"
ghset "repos/$S/branches/main/protection" '{"required_pull_request_reviews":null,"enforce_admins":{"enabled":true}}'
pulls "11:${PR[11]}:false:$L:a.txt" "12:${PR[12]}:false:$L:BAD" "13:${PR[13]}:false:$L:c.txt" \
      "14:${PR[14]}:false:$L:.github/workflows/x.yml" "15:${PR[15]}:false:$L:e.txt" "16:${PR[16]}:true:$L:f.txt" \
      "17:${PR[17]}:false::g.txt" "18:$(printf '%040x' 999):false:$L:h.txt"
MAIN0=$(origin_main)

echo "== usage and configuration"
rc=0; bash "$TRAIN" bogus >/dev/null 2>&1 || rc=$?; check "unknown command exits 2" [ "$rc" = 2 ]
rc=0; bash "$TRAIN" cycle --repos 'bad/name' >/dev/null 2>&1 || rc=$?; check "bad --repos exits 2" [ "$rc" = 2 ]
rc=0; bash "$TRAIN" loop --every x >/dev/null 2>&1 || rc=$?; check "bad --every exits 2" [ "$rc" = 2 ]

echo "== cadence (measured from bundles, never assumed)"
mkbundle() { mkdir -p "$AIEN_STATE/bundles/demo/$1"; jq -n --arg s "$2" --arg e "$3" --arg v "$4" --argjson b "${5:-null}" \
  '{started_utc:$s, finished_utc:$e, verdict:$v, plan:{blocked:$b}}' > "$AIEN_STATE/bundles/demo/$1/bundle.json"; }
mkbundle "$(printf '%040x' 1)" 2026-10-16T00:00:00Z 2026-10-16T00:05:00Z PASS
mkbundle "$(printf '%040x' 2)" 2026-10-16T01:00:00Z 2026-10-16T01:02:00Z FAIL
mkbundle "$(printf '%040x' 3)" 2026-10-16T02:00:00Z 2026-10-16T03:00:00Z NOT_RUN
mkbundle "$(printf '%040x' 4)" 2026-10-16T02:00:00Z 2026-10-16T03:00:00Z PASS '"gpu"'
out=$(bash "$TRAIN" cadence)
check "cadence: T is the longest PASS/FAIL unblocked run (300s), 2 runs counted" grep -qE '^demo +2 +300s +120s +120s +300s' <<< "$out"
check "cadence: a repository without bundles is unknown, not guessed" grep -qE '^none +0 .*unknown' <<< "$(AIEN_REPOS=none bash "$TRAIN" cadence)"
rm -rf "$AIEN_STATE/bundles/demo"

echo "== dry run"
bash "$TRAIN" cycle --dry-run > "$T/dry" 2>&1
check "dry run prints the plan with stack 3" grep -q 'stack 3 = [0-9a-f]\{40\}' "$T/dry"
check "dry run ran no worker, pushed nothing, wrote nothing to GitHub" bash -c "! grep -qE '^(once|push) |^gh api -X' '$FAKE_LOG'"
check "dry run left main alone" [ "$(origin_main)" = "$MAIN0" ]
check "journal: dry-run line" [ "$(jl '.result=="dry-run" and .stack=="3"')" = 1 ]
: > "$AIEN_STATE/train.jsonl"

echo "== cycle 1: three stacked, the middle one is the culprit"
bash "$TRAIN" cycle > "$T/c1" 2>&1
check "BLOCKED_OPERATOR PR excluded, label kept" bash -c "[ \"$(jl '.pr=="14" and .result=="excluded" and (.reason|test("BLOCKED_OPERATOR.*workflows"))')\" = 1 ] && ! grep -q 'issues/14/labels' '$FAKE_LOG'"
check "conflicting PR excluded" [ "$(jl '.pr=="15" and .result=="excluded" and (.reason|test("conflict"))')" = 1 ]
check "draft excluded" [ "$(jl '.pr=="16" and .result=="excluded" and .reason=="draft"')" = 1 ]
check "unlabeled PR skipped silently" [ "$(jl '.pr=="17"')" = 0 ]
check "head not matching refs/pull/N/head excluded" [ "$(jl '.pr=="18" and .result=="excluded" and (.reason|test("head moved"))')" = 1 ]
check "queue is 11, 12, 13 in order" [ "$(jq -r 'select(.result=="queued") | .pr' "$AIEN_STATE/train.jsonl" | paste -sd,)" = "11,12,13" ]
runs=$(grep -c '^once demo ' "$FAKE_LOG" || true)
check "worker ran exactly three stacks (3, then bisect 1 and 2), never main" [ "$runs" = 3 ]
S3=$(jq -r 'select(.result=="running" and .stack=="3") | .sha' "$AIEN_STATE/train.jsonl")
S1=$(jq -r 'select(.result=="running" and .stack=="1") | .sha' "$AIEN_STATE/train.jsonl")
S2=$(jq -r 'select(.result=="running" and .stack=="2") | .sha' "$AIEN_STATE/train.jsonl")
check "stack shas were pushed before the worker fetched them" bash -c "grep -n '^push .*$S3:refs/heads/train/demo' '$FAKE_LOG' | head -1 | cut -d: -f1 | xargs -I{} test {} -lt \$(grep -n '^once demo $S3' '$FAKE_LOG' | cut -d: -f1)"
check "stack 3 FAIL, stack 1 PASS, stack 2 FAIL" [ "$(jq -r 'select(.result=="verdict") | "\(.stack)=\(.verdict)"' "$AIEN_STATE/train.jsonl" | paste -sd,)" = "3=FAIL,1=PASS,2=FAIL" ]
check "culprit: PR 12 commented with the receipt link of stack 2" grep -q "issues/12/comments -f body=Merge train: stack 2 .*tested sha $S2) FAILED.*stack 1 ($S1) passed.*aien-receipts/blob/main/demo/$S2/bundle.json" "$FAKE_LOG"
check "culprit: label removed, PR not closed" bash -c "grep -q '^gh api -X DELETE repos/$S/issues/12/labels/ready-to-merge' '$FAKE_LOG' && ! grep -q 'pulls/12 -f state' '$FAKE_LOG'"
check "journal: culprit line" [ "$(jl '.result=="culprit" and .pr=="12" and .stack=="2"')" = 1 ]
check "main fast-forwarded to the exact tested stack 1 sha" [ "$(origin_main)" = "$S1" ]
check "tested sha ends with the crumb recompile commit" [ "$(/usr/bin/git --git-dir="$ORIGIN" log -1 --format=%s main)" = "train: crumb recompile (crumb tool override:$T/bin/crumb)" ]
check "below it, the merge of PR 11" [ "$(/usr/bin/git --git-dir="$ORIGIN" log -1 --format=%s 'main^')" = "Merge pull request #11: PR 11" ]
check "stack 1 contains PR 11 and nothing else" bash -c "origin_has main a.txt && ! origin_has main BAD && ! origin_has main c.txt && ! origin_has main .github/workflows/x.yml"
/usr/bin/git clone -q "$ORIGIN" "$T/chk" 2>/dev/null && check "crumbs in the merged sha verify with the pinned tool" bash -c "cd '$T/chk' && '$T/bin/crumb' verify ."
rm -rf "$T/chk"
check "stack 1 bundle published to receipts (digest of the local bundle)" [ "$(/usr/bin/git --git-dir="$T/receipts.git" show "main:demo/$S1/bundle.json" | sha256sum | cut -d' ' -f1)" = "$(sha256sum "$AIEN_STATE/bundles/demo/$S1/bundle.json" | cut -d' ' -f1)" ]
check "stack 2 FAIL bundle published too (the receipt the culprit comment links)" remote_has "$S2"
rpush=$(grep -n "^push -C $AIEN_RECEIPTS " "$FAKE_LOG" | head -1 | cut -d: -f1); mpush=$(grep -n "^push .*:refs/heads/main" "$FAKE_LOG" | head -1 | cut -d: -f1)
check "receipts were pushed before main was fast-forwarded" [ -n "$rpush" ] && [ -n "$mpush" ] && [ "$rpush" -lt "$mpush" ]
check "fast-forward is a plain push, never forced" bash -c "grep -q '^push -C .* -q origin $S1:refs/heads/main\$' '$FAKE_LOG' && ! grep -q 'push.*-f.*refs/heads/main' '$FAKE_LOG'"
check "train pull request opened for the hosted check, then closed" bash -c "grep -q 'gh api -X POST repos/$S/pulls -f title=train: demo stack' '$FAKE_LOG' && grep -q 'gh api -X PATCH repos/$S/pulls/900 -f state=closed' '$FAKE_LOG'"
check "train branch removed after the merge" bash -c "! /usr/bin/git --git-dir='$ORIGIN' rev-parse -q --verify refs/heads/train/demo >/dev/null"
check "journal: merged line names the PRs and the published digest" [ "$(jl '.result=="merged" and .prs=="11" and .stack=="1" and (.digest|length)==64')" = 1 ]
check "quiet flag was checked before every worker run" [ "$(grep -c '^quietlock check' "$FAKE_LOG")" -ge 3 ]

echo "== cycle 2: freshness at merge time (label removed after stacking)"
: > "$FAKE_LOG"
PR[19]=$(pr 19 d.txt d)
pulls "13:${PR[13]}:false:$L:c.txt" "19:${PR[19]}:false:$L:d.txt"
ghset "repos/$S/pulls/19" "{\"number\":19,\"draft\":false,\"state\":\"open\",\"head\":{\"sha\":\"${PR[19]}\"},\"labels\":[]}"
bash "$TRAIN" cycle > "$T/c2" 2>&1
check "stack 2 passed, then PR 19 found stale under the lock" [ "$(jl '.result=="stale" and .pr=="19" and (.reason|test("label"))')" = 1 ]
check "prefix shortened to stack 1 and that stack was tested before merging" bash -c "[ \"$(jl '.result=="shortened" and .stack=="2"')\" = 1 ] && [ \"$(jq -r 'select(.result=="verdict") | .stack' "$AIEN_STATE/train.jsonl" | tail -n 2 | paste -sd,)\" = '2,1' ]"
check "main now holds PR 13 and not PR 19" bash -c "origin_has main c.txt && ! origin_has main d.txt"
check "stale PR kept its label (no removal, no comment)" bash -c "! grep -q 'issues/19' '$FAKE_LOG'"

echo "== cycle 3: lock contention with pipeline.sh merge"
: > "$FAKE_LOG"
pulls "19:${PR[19]}:false:$L:d.txt"
before=$(origin_main)
exec 8>>"$AIEN_STATE/pipeline.lock"; flock 8
bash "$TRAIN" cycle > "$T/c3" 2>&1
flock -u 8; exec 8>&-
check "lock held: refused, nothing pushed to main" bash -c "[ \"$(jl '.result=="refused" and (.reason|test("pipeline lock busy"))')\" = 1 ] && [ \"$(origin_main)\" = '$before' ]"
check "lock held: the stack was still tested, nothing published (publish needs the same lock)" bash -c "grep -q '^once demo ' '$FAKE_LOG' && ! remote_has \$(jq -r 'select(.result==\"running\") | .sha' '$AIEN_STATE/train.jsonl' | tail -n 1)"
bash "$TRAIN" cycle > "$T/c3b" 2>&1
check "lock released: merged" bash -c "[ \"$(jl '.result=="merged" and .prs=="19"')\" = 1 ] && origin_has main d.txt"

echo "== cycle 4: main itself fails, so nobody is blamed"
: > "$FAKE_LOG"
(cd "$SEED" && /usr/bin/git checkout -q main && /usr/bin/git pull -q origin main && echo x > MAINBAD && /usr/bin/git add -A && /usr/bin/git commit -q -m mainbad && /usr/bin/git push -q origin main)
PR[20]=$(pr 20 i.txt i)
pulls "20:${PR[20]}:false:$L:i.txt"
before=$(origin_main)
bash "$TRAIN" cycle > "$T/c4" 2>&1
check "stack 1 FAIL, then main tested and FAIL" [ "$(jq -r 'select(.result=="verdict") | "\(.stack)=\(.verdict)"' "$AIEN_STATE/train.jsonl" | tail -n 2 | paste -sd,)" = "1=FAIL,0=FAIL" ]
check "no culprit, label kept, main untouched" bash -c "[ \"$(jl '.result=="refused" and (.reason|test("main itself fails"))')\" = 1 ] && ! grep -q 'issues/20' '$FAKE_LOG' && [ \"$(origin_main)\" = '$before' ]"

echo "== cycle 5: quiet flag held"
: > "$FAKE_LOG"; touch "$T_QUIET"
bash "$TRAIN" cycle > "$T/c5" 2>&1
check "quiet flag held: no worker run, refusal journaled" bash -c "! grep -q '^once ' '$FAKE_LOG' && [ \"$(jl '.result=="refused" and (.reason|test("quiet flag held"))')\" = 1 ]"
rm -f "$T_QUIET"

echo "== cycle 6: protection requires a pull request; main moves under the train"
: > "$FAKE_LOG"
(cd "$SEED" && /usr/bin/git checkout -q main && /usr/bin/git rm -q MAINBAD && /usr/bin/git commit -q -m fixmain && /usr/bin/git push -q origin main)
ghset "repos/$S/branches/main/protection" '{"required_pull_request_reviews":{"required_approving_review_count":0},"enforce_admins":{"enabled":true}}'
before=$(origin_main)
T_RACE=$SEED bash "$TRAIN" cycle > "$T/c6" 2>&1
check "warning journaled: protection requires a pull request, fast-forward will be refused" [ "$(jl '.result=="warning" and (.reason|test("requires a pull request"))')" = 1 ]
check "main moved first: the push was refused (non fast-forward), never forced" bash -c "[ \"$(jl '.result=="failed" and (.reason|test("fast-forward push refused"))')\" = 1 ] && ! grep -q 'push.*-f.*refs/heads/main' '$FAKE_LOG'"
check "main is the racer's commit, untouched by the train" bash -c "origin_has main RACED && ! origin_has main i.txt"
check "the failed stack's bundle was still published (receipt binding before any push)" bash -c "s=\$(jq -r 'select(.result==\"failed\") | .sha' '$AIEN_STATE/train.jsonl' | tail -n 1); remote_has \"\$s\""

echo "== secrets and protection"
check "train.sh never names the signing key file" bash -c "! grep -q 'verify\\.key' '$TRAIN'"
check "train.sh never touches branch protection or uses --admin" bash -c "! grep -qE 'PUT .*protection|DELETE .*protection|--admin' '$TRAIN'"

echo
echo "train_test: $pass passed, $fail failed"
[[ $fail == 0 ]]
