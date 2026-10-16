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
if [[ $1 == pr && $2 == merge ]]; then
  shift 2; num=$1; match=""
  while [[ $# -gt 0 ]]; do case $1 in --match-head-commit) match=$2; shift 2 ;; -R|--subject) shift 2 ;; *) shift ;; esac; done
  [[ $num == 900 ]] || exit 1
  [[ -z ${FAKE_PUT_FAIL:-} ]] || { echo "HTTP 409 Head branch was modified" >&2; exit 1; }
  head=$(git --git-dir="$ORIGIN" rev-parse -q --verify refs/heads/train/demo) || { echo "HTTP 404" >&2; exit 1; }
  [[ $match == "$head" ]] || { echo "head branch was modified" >&2; exit 1; }
  m=$(git --git-dir="$ORIGIN" rev-parse main)
  c=$(git --git-dir="$ORIGIN" commit-tree "$head^{tree}" -p "$m" -p "$head" -m "Merge pull request #900")
  git --git-dir="$ORIGIN" update-ref refs/heads/main "$c"
  exit 0
fi
[[ $1 == api ]] || exit 9
shift
method=GET expr="" path="" sha=""
while [[ $# -gt 0 ]]; do
  case $1 in -X) method=$2; shift 2 ;; --jq) expr=$2; shift 2 ;; -f|-F) case $2 in sha=*) sha=${2#sha=} ;; esac; shift 2 ;; --paginate) shift ;; *) path=$1; shift ;; esac
done
if [[ $method != GET ]]; then
  if [[ $method == POST && $path == */pulls ]]; then echo '{"number":900}' | { [[ -n $expr ]] && jq -r "$expr" || cat; }; fi
  exit 0
fi
if [[ $path == */pulls/900 ]]; then
  h=${FAKE_TRAIN_HEAD:-$(git --git-dir="$ORIGIN" rev-parse -q --verify refs/heads/train/demo || echo none)}
  j="{\"number\":900,\"state\":\"open\",\"base\":{\"ref\":\"main\"},\"head\":{\"sha\":\"$h\"}}"
  if [[ -n $expr ]]; then jq -r "$expr" <<< "$j"; else echo "$j"; fi; exit 0
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
[ -e "/proc/$$/fd/8" ] && echo "worker-fd8 open" >> "$FAKE_LOG" || echo "worker-fd8 closed" >> "$FAKE_LOG"
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
  compile) printf '%s\n' "$want" > .crumb
    [[ -z ${FAKE_CRUMB_TRUST:-} ]] || { mkdir -p .aien; echo x > .aien/.crumb; }
    [[ -z ${FAKE_CRUMB_JUNK:-} ]] || echo junk > stray.txt ;;
  verify) [[ -f .crumb && $(cat .crumb) == "$want" ]] ;;
  *) exit 9 ;;
esac
EOF
cat > "$T/bin/git" <<'EOF'
#!/usr/bin/env bash
# logs pushes (with the directory) so the order of receipts publication and the merge of the train PR can be checked;
# races one commit onto main right before the train pushes its train/<repo> branch when $T_RACE names a seed clone
if [[ " $* " == *" push "* ]]; then
  echo "push $*" >> "$FAKE_LOG"
  if [[ -n ${T_RACE:-} && " $* " == *":refs/heads/train/"* && ! -e $T_RACE/.raced ]]; then
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
check "main landed by a merge commit whose second parent is the exact tested stack 1 sha" [ "$(/usr/bin/git --git-dir="$ORIGIN" rev-parse main^2)" = "$S1" ]
check "merged tree equals the tested tree" [ "$(/usr/bin/git --git-dir="$ORIGIN" rev-parse "main^{tree}")" = "$(/usr/bin/git --git-dir="$ORIGIN" rev-parse "$S1^{tree}")" ]
check "tested sha ends with the crumb recompile commit" [ "$(/usr/bin/git --git-dir="$ORIGIN" log -1 --format=%s main^2)" = "train: crumb recompile (crumb tool override:$T/bin/crumb)" ]
check "below it, the merge of PR 11" [ "$(/usr/bin/git --git-dir="$ORIGIN" log -1 --format=%s main^2^)" = "Merge pull request #11: PR 11" ]
check "stack 1 contains PR 11 and nothing else" bash -c "origin_has main a.txt && ! origin_has main BAD && ! origin_has main c.txt && ! origin_has main .github/workflows/x.yml"
/usr/bin/git clone -q "$ORIGIN" "$T/chk" 2>/dev/null && check "crumbs in the merged sha verify with the pinned tool" bash -c "cd '$T/chk' && '$T/bin/crumb' verify ."
rm -rf "$T/chk"
check "stack 1 bundle published to receipts (digest of the local bundle)" [ "$(/usr/bin/git --git-dir="$T/receipts.git" show "main:demo/$S1/bundle.json" | sha256sum | cut -d' ' -f1)" = "$(sha256sum "$AIEN_STATE/bundles/demo/$S1/bundle.json" | cut -d' ' -f1)" ]
check "stack 2 FAIL bundle published too (the receipt the culprit comment links)" remote_has "$S2"
rpush=$(grep -n "^push -C $AIEN_RECEIPTS " "$FAKE_LOG" | head -1 | cut -d: -f1); mpush=$(grep -n "^gh pr merge 900 -R $S" "$FAKE_LOG" | head -1 | cut -d: -f1)
check "receipts were pushed before the train PR was merged" [ -n "$rpush" ] && [ -n "$mpush" ] && [ "$rpush" -lt "$mpush" ]
check "no push to main at all; the merge call pins the tested head (--match-head-commit)" bash -c "! grep -q '^push .*refs/heads/main' '$FAKE_LOG' && grep -q \"^gh pr merge 900 -R $S --merge --match-head-commit $S1 \" '$FAKE_LOG'"
check "train pull request opened for the hosted check and merged by the train" bash -c "grep -q 'gh api -X POST repos/$S/pulls -f title=train: demo stack' '$FAKE_LOG'"
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

echo "== cycle 6: main moves after the stack was built: abort, no merge, next cycle restacks"
: > "$FAKE_LOG"
(cd "$SEED" && /usr/bin/git checkout -q main && /usr/bin/git rm -q MAINBAD && /usr/bin/git commit -q -m fixmain && /usr/bin/git push -q origin main)
T_RACE=$SEED bash "$TRAIN" cycle > "$T/c6" 2>&1
check "main moved: cycle aborted, restack named" [ "$(jl '.result=="aborted" and (.reason|test("main moved.*restacks"))')" = 1 ]
check "aborted cycle never called the merge API and never pushed to main" bash -c "! grep -q '^gh pr merge 900' '$FAKE_LOG' && ! grep -q '^push .*refs/heads/main' '$FAKE_LOG'"
check "main is the racer's commit, untouched by the train" bash -c "origin_has main RACED && ! origin_has main i.txt"
check "the aborted stack's bundle was still published (receipt binding before any merge)" bash -c "s=\$(jq -r 'select(.result==\"aborted\") | .sha' '$AIEN_STATE/train.jsonl' | tail -n 1); remote_has \"\$s\""
: > "$FAKE_LOG"
bash "$TRAIN" cycle > "$T/c6b" 2>&1
check "next cycle restacked on the new main and landed PR 20 by merge commit" bash -c "[ \"$(jl '.result=="merged" and .prs=="20"')\" = 1 ] && origin_has main i.txt && origin_has main RACED && grep -q '^gh pr merge 900' '$FAKE_LOG'"
check "restacked merge: second parent is the new tested sha, trees equal" bash -c "g=/usr/bin/git; o='$ORIGIN'; [ \"\$(\$g --git-dir=\$o rev-parse 'main^{tree}')\" = \"\$(\$g --git-dir=\$o rev-parse 'main^2^{tree}')\" ] && [ \"\$(\$g --git-dir=\$o rev-parse main^1)\" != \"\$(\$g --git-dir=\$o rev-parse main^2)\" ]"

echo "== cycle 7: train PR head is not the tested sha: refuse"
: > "$FAKE_LOG"
(cd "$SEED" && /usr/bin/git checkout -q main && /usr/bin/git pull -q origin main)
PR[21]=$(pr 21 j.txt j)
pulls "21:${PR[21]}:false:$L:j.txt"
before=$(origin_main)
FAKE_TRAIN_HEAD=$(printf '%040x' 777) bash "$TRAIN" cycle > "$T/c7" 2>&1
check "head differs from tested sha: refused, merge API never called, main untouched" bash -c "[ \"$(jl '.result=="refused" and (.reason|test("not the tested sha"))')\" = 1 ] && ! grep -q '^gh pr merge 900' '$FAKE_LOG' && [ \"$(origin_main)\" = '$before' ]"
: > "$FAKE_LOG"
bash "$TRAIN" cycle > "$T/c7b" 2>&1
check "same queue with a matching head lands" bash -c "[ \"$(jl '.result=="merged" and .prs=="21"')\" = 1 ] && origin_has main j.txt"
: > "$FAKE_LOG"
(cd "$SEED" && /usr/bin/git checkout -q main && /usr/bin/git pull -q origin main)
PR[22]=$(pr 22 k.txt k)
pulls "22:${PR[22]}:false:$L:k.txt"
before=$(origin_main)
FAKE_PUT_FAIL=1 bash "$TRAIN" cycle > "$T/c7c" 2>&1
check "GitHub refusing the merge (moved head via sha=) is journaled failed, main untouched" bash -c "[ \"$(jl '.result=="failed" and (.reason|test("merge of train pull request refused"))')\" = 1 ] && [ \"$(origin_main)\" = '$before' ]"

echo "== listing is paginated"
check "open-PR listing uses --paginate" grep -q 'pulls?state=open&per_page=100 --paginate' "$FAKE_LOG"

echo "== committer identity is checked at startup"
rc=0; env -u GIT_COMMITTER_NAME -u GIT_COMMITTER_EMAIL -u GIT_AUTHOR_NAME -u GIT_AUTHOR_EMAIL -u EMAIL bash "$TRAIN" cycle --dry-run > "$T/id.out" 2> "$T/id.err" || rc=$?
check "no git identity: exit 2 with a plain message, no cycle started" bash -c "[ $rc = 2 ] && grep -q 'no committer identity' '$T/id.err' && [ ! -s '$T/id.out' ]"

echo "== one train per repository"
: > "$FAKE_LOG"
exec 6>>"$AIEN_STATE/train.demo.lock"; flock 6
bash "$TRAIN" cycle > "$T/lock.out" 2>&1
flock -u 6; exec 6>&-
check "second cycle on the same repository skips: journaled, no worker, no push, no GitHub write" bash -c "[ \"$(jl '.result=="skipped" and (.reason|test("another train cycle"))')\" = 1 ] && ! grep -qE '^(once|push) |^gh (api -X|pr merge)' '$FAKE_LOG'"

echo "== crumb recompile never writes into a trust-surface directory"
: > "$FAKE_LOG"
before=$(origin_main)
FAKE_CRUMB_TRUST=1 bash "$TRAIN" cycle > "$T/trust.out" 2>&1
check "recompile touching .aien/.crumb: stack skipped, journaled, no worker, main untouched" bash -c "[ \"$(jl '.result=="refused" and (.reason|test("trust-surface directory"))')\" = 1 ] && ! grep -q '^once ' '$FAKE_LOG' && [ \"$(origin_main)\" = '$before' ]"
FAKE_CRUMB_JUNK=1 bash "$TRAIN" cycle > "$T/junk.out" 2>&1
check "compile changing a non-.crumb file: stack skipped" [ "$(jl '.result=="refused" and (.reason|test("something other than .crumb"))')" -ge 1 ]

echo "== crumb pin comes from the target repository, fail closed"
cat > "$T/crumb-min.c" <<'EOC'
#include <stdio.h>
#include <string.h>
int main(int c, char **v) {
  char b[16] = {0};
  FILE *f;
  if (c < 2) return 9;
  if (!strcmp(v[1], "compile")) { f = fopen(".crumb", "w"); if (!f) return 1; fputs("c-tool\n", f); fclose(f); return 0; }
  if (!strcmp(v[1], "verify")) { f = fopen(".crumb", "r"); if (!f) return 1; if (!fgets(b, 16, f)) return 1; fclose(f); return strcmp(b, "c-tool\n") ? 1 : 0; }
  return 9;
}
EOC
/usr/bin/git init -q -b main "$T/archseed"
mkdir -p "$T/archseed/tools/crumb"; cp "$T/crumb-min.c" "$T/archseed/tools/crumb/crumb.c"
(cd "$T/archseed" && /usr/bin/git add -A && /usr/bin/git commit -q -m A && echo "/* b */" >> tools/crumb/crumb.c && /usr/bin/git commit -q -am B)
PIN_A=$(/usr/bin/git -C "$T/archseed" rev-parse HEAD~1); PIN_B=$(/usr/bin/git -C "$T/archseed" rev-parse HEAD)
/usr/bin/git clone -q --bare "$T/archseed" "$T/arch.git"
(cd "$SEED" && /usr/bin/git checkout -q main && /usr/bin/git pull -q origin main && mkdir -p .github/workflows \
  && printf 'steps:\n  - name: Checkout the pinned crumb tool source (aien-architecture @ %s)\n    uses: actions/checkout@v4\n    with:\n      repository: x/aien-architecture\n      ref: %s\n' "$PIN_A" "$PIN_A" > .github/workflows/aien-verification.yml \
  && /usr/bin/git add -A && /usr/bin/git commit -q -m workflow && /usr/bin/git push -q origin main)
nopin() { env -u AIEN_CRUMB_TOOL AIEN_ARCH_URL="$T/arch.git" bash "$TRAIN" cycle --dry-run 2>&1; }
check "pin read from the repository's own aien-verification.yml" grep -q "crumb tool $PIN_A)" <<< "$(nopin)"
(cd "$SEED" && printf '%s\n' "$PIN_B" > crumb.lock && /usr/bin/git add -A && /usr/bin/git commit -q -m lock && /usr/bin/git push -q origin main)
check "crumb.lock wins where it exists" grep -q "crumb tool $PIN_B)" <<< "$(nopin)"
(cd "$SEED" && printf 'nothex\n' > crumb.lock && /usr/bin/git commit -q -am badlock && /usr/bin/git push -q origin main)
check "malformed crumb.lock fails closed" grep -q "crumb.lock does not hold a 40-hex commit" <<< "$(nopin)"
(cd "$SEED" && /usr/bin/git rm -q crumb.lock .github/workflows/aien-verification.yml && /usr/bin/git commit -q -m nopin && /usr/bin/git push -q origin main)
out=$(nopin)
check "neither crumb.lock nor workflow: fail closed, no default pin" bash -c "grep -q 'no crumb pin' <<< \"\$1\" && ! grep -q 'dry-run' <<< \"\$1\"" _ "$out"
check "train.sh carries no default pin of any repository" bash -c "! grep -q '94fffbcf' '$TRAIN'"

echo "== a PR cannot choose the crumb tool"
(cd "$SEED" && /usr/bin/git checkout -q main && /usr/bin/git pull -q origin main && mkdir -p .github/workflows \
  && printf 'steps:\n  - name: Checkout the pinned crumb tool source (aien-architecture @ %s)\n    with:\n      ref: %s\n' "$PIN_A" "$PIN_A" > .github/workflows/aien-verification.yml \
  && /usr/bin/git add -A && /usr/bin/git commit -q -m basepin && /usr/bin/git push -q origin main)
PR[23]=$(pr 23 crumb.lock "$PIN_B")
PR[24]=$(pr 24 m.txt m)
pulls "23:${PR[23]}:false:$L:crumb.lock" "24:${PR[24]}:false:$L:m.txt"
: > "$FAKE_LOG"; before=$(origin_main)
out=$(env -u AIEN_CRUMB_TOOL AIEN_ARCH_URL="$T/arch.git" bash "$TRAIN" cycle --dry-run 2>&1)
check "PR changing crumb.lock is BLOCKED_OPERATOR and left out of the stack" bash -c "[ \"$(jl '.pr=="23" and .result=="excluded" and (.reason|test("BLOCKED_OPERATOR: touches crumb.lock"))')\" = 1 ] && [ \"$(jl '.pr=="23" and .result=="queued"')\" = 0 ]"
check "the stack is built with the base pin (A), not the PR's pin (B)" bash -c "grep -q 'crumb tool $PIN_A)' <<< \"\$1\" && ! grep -q 'crumb tool $PIN_B' <<< \"\$1\"" _ "$out"
check "pin is read from the base ref, never from the stack" bash -c "grep -q 'show \"\\\$MAIN:crumb.lock\"' '$TRAIN' && grep -q 'show \"\\\$MAIN:.github/workflows/aien-verification.yml\"' '$TRAIN' && ! grep -q 'HEAD:crumb.lock' '$TRAIN'"

echo "== the per-repository lock is not inherited by children"
: > "$FAKE_LOG"
pulls "24:${PR[24]}:false:$L:m.txt"
bash "$TRAIN" cycle > "$T/fd.out" 2>&1
check "the worker ran with fd 8 closed" bash -c "grep -q '^worker-fd8 closed' '$FAKE_LOG' && ! grep -q '^worker-fd8 open' '$FAKE_LOG'"

echo "== secrets and protection"
check "train.sh never names the signing key file" bash -c "! grep -q 'verify\\.key' '$TRAIN'"
check "train.sh never touches branch protection or uses --admin" bash -c "! grep -qE 'PUT .*protection|DELETE .*protection|--admin' '$TRAIN'"

echo
echo "train_test: $pass passed, $fail failed"
[[ $fail == 0 ]]
