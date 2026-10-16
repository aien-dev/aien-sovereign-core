#!/usr/bin/env bash
# train.sh: the Spark merge train (Drake, 2026-10-16). Tests have a known duration T, so pull requests are batched
# on a timed rhythm and verified as stacked prefixes instead of one at a time.
#
#   train.sh cadence [--repos a,b]                  measured full-gate duration T per repository, from real bundles
#   train.sh cycle   [--repos a,b] [--dry-run]      one train cycle per repository (dry-run: print the plan, push nothing)
#   train.sh loop    [--repos a,b] [--every SECS]   cycles forever, one per repository every T (measured, or --every)
#
# One cycle for a repository:
#   1. queue = open pull requests labeled exactly ready-to-merge, base main, not draft, oldest number first.
#      Excluded (label kept, one journal line each): BLOCKED_OPERATOR paths (.aien/, tests/gates/,
#      .github/workflows/, .forgejo/, tools/aien-test/), head not equal to refs/pull/N/head, merge conflict with the
#      stack so far, queue longer than AIEN_MAX_STACK.
#   2. stack k = origin/main + PR1..PRk merged in order (merge commits, as `gh pr merge --merge` makes them), then one
#      crumb recompile commit made with the repository's pinned crumb tool (crumb.lock in the tree, else the
#      sovereign-core CI pin). Stack 0 is main itself, untouched.
#   3. The worker (aien-local-ci once) verifies stack shas one at a time (the worker serves heads sequentially; the
#      gate pools are per head): first stack N; on FAIL a bisection finds the longest passing prefix p and the first
#      failing prefix p+1, whose last pull request is the culprit: a comment with the receipt link, label removed, not
#      closed. When stack 1 fails, main itself is tested before blaming PR1.
#   4. Merge of stack p (p >= 1): its bundle is published to aien-receipts (pipeline.sh publish, checked by digest at
#      origin), the required checks of main are green on that exact sha (the stack is pushed as branch train/<repo>
#      with one open train pull request so the hosted check runs), then under the pipeline lock: every PR in the prefix
#      still open, labeled, not draft, head unchanged; origin/main unchanged since stacking; then main is
#      fast-forwarded to the tested sha (`git push`, never forced, never re-merged). If a PR went stale the prefix ends
#      before it and the longest tested passing prefix inside it is taken (re-tested when it was never run).
#
# It never reads the signing key, never signs, never changes branch protection, never closes a pull request.
# Every action and refusal is one JSON line in $STATE/train.jsonl. Exit 0 when the cycle ran (refusals included),
# 2 on usage or configuration error. Spark etiquette: `quietlock check` must pass before every worker run.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
STATE=${AIEN_STATE:-$HOME/.local/state/aien-local-ci}
CLONES=${AIEN_CLONES:-$HOME/workspace/pipeline/clones}
RECEIPTS=${AIEN_RECEIPTS:-$HOME/workspace/pipeline/aien-receipts}
RECEIPTS_BRANCH=${AIEN_RECEIPTS_BRANCH:-main}
BIN=${AIEN_TEST_BIN:-$HOME/workspace/pipeline/target-sc/release/aien-test}
OWNER=${AIEN_GH_OWNER:-aien-dev}
REPOS=${AIEN_REPOS:-aien-sovereign-core,omega,aienos,physics,aien-protocols,aegis-runtime,spark-rsi,aien-architecture,benchmarks}
GH=${GH:-gh}
GIT=${GIT:-git}
PIPELINE=${AIEN_PIPELINE:-$HERE/pipeline.sh}
LOCAL_CI=${AIEN_LOCAL_CI:-$HOME/.local/bin/aien-local-ci}
QUIETLOCK=${QUIETLOCK:-quietlock}
TRAIN=${AIEN_TRAIN_CLONES:-$STATE/train-clones}
ARCH_URL=${AIEN_ARCH_URL:-https://github.com/aien-spark/aien-architecture}
CRUMB_PIN_DEFAULT=${AIEN_CRUMB_PIN:-94fffbcf88492c5a1c54211b5126c8a6c370a98d}   # .github/workflows/aien-verification.yml
CRUMB_TOOL=${AIEN_CRUMB_TOOL:-}            # set: use this crumb binary for every repository (tests); unset: build the pin
MAX_STACK=${AIEN_MAX_STACK:-20}
CRUMB_PIN=unresolved CRUMB_BIN=""              # set by crumb_tool for the stack being built
LOCK_WAIT=${AIEN_LOCK_WAIT:-900}           # seconds to wait for the pipeline lock at merge time
CHECK_WAIT=${AIEN_CHECK_WAIT:-3600}        # seconds to wait for the required hosted checks on the stack sha
QUIET_WAIT=${AIEN_QUIET_WAIT:-7200}        # seconds to wait for the quiet flag before a worker run
POLL=${AIEN_TRAIN_POLL:-30}                # seconds between polls (quiet flag, hosted checks)
RETRY_SLEEP=${AIEN_RETRY_SLEEP:-5}
MERGE_LABEL="ready-to-merge"
BLOCKED_OPERATOR_PREFIXES=(".aien/" "tests/gates/" ".github/workflows/" ".forgejo/" "tools/aien-test/")

usage() { echo "usage: train.sh cadence|cycle|loop [--repos a,b] [--dry-run] [--every SECS]" >&2; exit 2; }
die() { echo "train.sh: $*" >&2; exit 2; }

[[ $# -ge 1 ]] || usage
cmd=$1; shift
case "$cmd" in cadence|cycle|loop) ;; *) usage ;; esac
DRY=0 EVERY=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --repos) [[ $# -ge 2 ]] || usage; REPOS=$2; shift 2 ;;
    --dry-run) DRY=1; shift ;;
    --every) [[ $# -ge 2 && $2 =~ ^[0-9]+$ ]] || usage; EVERY=$2; shift 2 ;;
    *) usage ;;
  esac
done
IFS=, read -r -a repo_list <<< "$REPOS"
[[ ${#repo_list[@]} -gt 0 ]] || die "no repositories named"
for r in "${repo_list[@]}"; do
  [[ $r =~ ^[A-Za-z0-9._-]+$ && $r != . && $r != .. ]] || die "bad repository name '$r'"
done
command -v jq >/dev/null || die "jq not found"
command -v "$GIT" >/dev/null || die "git ($GIT) not found"
mkdir -p "$STATE"
JOURNAL=$STATE/train.jsonl
if [[ $cmd != cadence ]]; then
  command -v flock >/dev/null || die "flock not found"
  command -v sha256sum >/dev/null || die "sha256sum not found"
  command -v "$GH" >/dev/null || die "gh ($GH) not found"
  [[ -d $CLONES ]] || die "clones directory $CLONES missing"
  [[ -f $PIPELINE ]] || die "pipeline.sh not found at $PIPELINE"
  if [[ $DRY == 0 ]]; then
    [[ -x $BIN ]] || die "aien-test binary $BIN missing or not executable"
    [[ -d $RECEIPTS/.git ]] || die "receipts clone $RECEIPTS is not a git clone"
    [[ -x $LOCAL_CI ]] || die "aien-local-ci ($LOCAL_CI) missing or not executable"
    command -v "$QUIETLOCK" >/dev/null || die "quietlock ($QUIETLOCK) not found"
  fi
  mkdir -p "$TRAIN" "$STATE/train-logs"
fi
WORK=$(mktemp -d "$STATE/.train.XXXXXX")
trap 'rm -rf "$WORK"' EXIT

# journal key=value ...: one JSON line (built by jq), a short line on stdout.
journal() {
  local args=(--arg utc "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg action train) kv
  for kv in "$@"; do args+=(--arg "${kv%%=*}" "${kv#*=}"); done
  jq -nc "${args[@]}" '$ARGS.named' >> "$JOURNAL"
  echo "$*"
}
say() { echo "train: $*"; }
digest() { sha256sum "$1" | cut -d' ' -f1; }

# ---------------------------------------------------------------- cadence: T per repository from real bundles
# A qualifying run: plan.blocked null, verdict PASS or FAIL, both timestamps present. T = the longest such run.
cadence_of() { # REPO -> "runs max median last" in seconds, or "0 0 0 0"
  local f durs=() s e
  for f in "$STATE/bundles/$1"/*/bundle.json; do
    [[ -f $f ]] || continue
    s="" e=""
    read -r s e < <(jq -r 'select(.plan.blocked == null and (.verdict == "PASS" or .verdict == "FAIL"))
      | "\(.started_utc // "") \(.finished_utc // "")"' "$f" 2>/dev/null) || true
    [[ -n ${s:-} && -n ${e:-} ]] || continue
    s=$(date -u -d "$s" +%s 2>/dev/null) || continue
    e=$(date -u -d "$e" +%s 2>/dev/null) || continue
    [[ $e -ge $s ]] && durs+=("$s $((e - s))")
  done
  [[ ${#durs[@]} -gt 0 ]] || { echo "0 0 0 0"; return 0; }
  local n last mx med sorted
  n=${#durs[@]}
  last=$(printf '%s\n' "${durs[@]}" | sort -n | tail -n 1 | cut -d" " -f2)
  sorted=$(printf '%s\n' "${durs[@]}" | cut -d" " -f2 | sort -n)
  mx=$(tail -n 1 <<< "$sorted")
  med=$(sed -n "$(( (n + 1) / 2 ))p" <<< "$sorted")
  echo "$n $mx $med $last"
}
do_cadence() {
  local repo n mx med last
  printf '%-22s %5s %8s %8s %8s %8s\n' repository runs max median last T
  for repo in "${repo_list[@]}"; do
    read -r n mx med last < <(cadence_of "$repo")
    if [[ $n == 0 ]]; then printf '%-22s %5s %8s %8s %8s %8s\n' "$repo" 0 - - - "unknown"; continue; fi
    printf '%-22s %5s %7ss %7ss %7ss %7ss\n' "$repo" "$n" "$mx" "$med" "$last" "$mx"
  done
}

# ---------------------------------------------------------------- GitHub helpers
declare -A SLUGS=()
slug() {
  local s
  if [[ -z ${SLUGS[$1]:-} ]]; then
    s=$("$GH" api "repos/$OWNER/$1" --jq .full_name 2>/dev/null) || return 1
    [[ $s =~ ^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$ ]] || return 1
    SLUGS[$1]=$s
  fi
  echo "${SLUGS[$1]}"
}
keys() {
  "$GIT" -C "$1" show "$2:.aien/verify.pub" 2>/dev/null | sed 's/#.*//' | tr -d ' \t\r' | tr 'A-F' 'a-f' \
    | grep -E '^[0-9a-f]{64}$' || true
}
verify() { # FILE REPO SHA KEYS [REQUIRE] -> "best codes"
  local file=$1 repo=$2 sha=$3 keylist=$4 require=${5:-} key rc codes="" best="" extra=()
  [[ -n $require ]] && extra=(--require "$require")
  while IFS= read -r key; do
    [[ -n $key ]] || continue
    rc=0
    "$BIN" verify-bundle "$file" --repo "github.com/$OWNER/$repo" --commit "$sha" --pubkey "$key" "${extra[@]}" \
      >/dev/null 2>&1 || rc=$?
    codes+="${codes:+,}$rc"
  done <<< "$keylist"
  for rc in 0 1 2 3; do [[ ,$codes, == *",$rc,"* ]] && { best=$rc; break; }; done
  echo "${best:-${codes%%,*}} ${codes:-none}"
}
verdict_name() { case "$1" in 0) echo PASS ;; 1) echo FAIL ;; 2) echo NOT_RUN ;; 3) echo REJECTED ;; *) echo "EXIT_$1" ;; esac; }
published() { # REPO SHA -> digest at receipts origin, or empty (caller fetched)
  local tmp=$WORK/published
  if "$GIT" -C "$RECEIPTS" show "origin/$RECEIPTS_BRANCH:$1/$2/bundle.json" > "$tmp" 2>/dev/null; then digest "$tmp"; else echo ""; fi
}
receipt_url() { echo "https://github.com/$OWNER/aien-receipts/blob/$RECEIPTS_BRANCH/$1/$2/bundle.json"; }

# checks_state SLUG SHA: 0 every required context of main is success on SHA, 1 some are missing or pending,
# 2 some failed, 3 cannot tell (API or no required checks). Mirrors pipeline.sh merge_one.
checks_state() {
  local s=$1 sha=$2 ctxs checks statuses ctx found bad
  ctxs=$("$GH" api "repos/$s/branches/main/protection/required_status_checks" --jq '.contexts[]' 2>/dev/null) || { CHECK_NOTE="cannot read required status checks of main"; return 3; }
  [[ -n $ctxs ]] || { CHECK_NOTE="main has no required status checks"; return 3; }
  checks=$("$GH" api "repos/$s/commits/$sha/check-runs?per_page=100" --jq '.check_runs[] | [.name, (.conclusion // .status)] | @tsv' 2>/dev/null) || { CHECK_NOTE="cannot read check runs"; return 3; }
  statuses=$("$GH" api "repos/$s/commits/$sha/status?per_page=100" --jq '.statuses[] | [.context, .state] | @tsv' 2>/dev/null) || { CHECK_NOTE="cannot read commit statuses"; return 3; }
  local pending=0
  while IFS= read -r ctx; do
    [[ -n $ctx ]] || continue
    found=$(printf '%s\n%s\n' "$checks" "$statuses" | C="$ctx" awk -F'\t' '$1 == ENVIRON["C"] {print $2}')
    if [[ -z $found ]]; then CHECK_NOTE="required context '$ctx' missing"; pending=1; continue; fi
    bad=$(grep -vx success <<< "$found" | head -n 1 || true)
    [[ -z $bad ]] && continue
    case $bad in
      queued|in_progress|pending|waiting|requested|null) CHECK_NOTE="required context '$ctx' is $bad"; pending=1 ;;
      *) CHECK_NOTE="required context '$ctx' is $bad"; return 2 ;;
    esac
  done <<< "$ctxs"
  [[ $pending == 0 ]] || return 1
  CHECK_NOTE="required checks green"; return 0
}

# ---------------------------------------------------------------- crumb tool for the stack (pinned per repository)
crumb_tool() { # sets CRUMB_BIN (the crumb binary for the tree at HEAD of $TC) and CRUMB_PIN
  local pin built src
  if [[ -n $CRUMB_TOOL ]]; then CRUMB_PIN="override:$CRUMB_TOOL"; CRUMB_BIN=$CRUMB_TOOL; return 0; fi
  pin=$("$GIT" -C "$TC" show HEAD:crumb.lock 2>/dev/null | tr -d ' \t\r\n' || true)
  [[ $pin =~ ^[0-9a-f]{40}$ ]] || pin=$CRUMB_PIN_DEFAULT
  CRUMB_PIN=$pin
  built=$STATE/train-crumb/$pin/crumb
  if [[ ! -x $built ]]; then
    src=$TRAIN/aien-architecture.git
    [[ -d $src ]] || "$GIT" clone -q --bare --filter=blob:none "$ARCH_URL" "$src" || return 1
    "$GIT" --git-dir="$src" cat-file -e "$pin^{commit}" 2>/dev/null || "$GIT" --git-dir="$src" fetch -q origin "$pin" || return 1
    mkdir -p "$(dirname "$built")"
    "$GIT" --git-dir="$src" show "$pin:tools/crumb/crumb.c" > "$WORK/crumb.c" || return 1
    cc -std=c11 -O2 -Wall -Wextra -Werror -o "$WORK/crumb" "$WORK/crumb.c" || return 1
    mv "$WORK/crumb" "$built"
  fi
  CRUMB_BIN=$built
}

# ---------------------------------------------------------------- stacks
# Globals per repository cycle: TC (train clone), MAIN, QN[] QS[] QT[] (queue: number, head sha, title), N,
# STACK[k] (sha of stack k, built lazily), RESULT[sha] (PASS|FAIL|<other>).
declare -a QN QS QT STACK
declare -A RESULT
train_clone() { # REPO -> sets TC, MAIN; the train's own scratch clone of the trusted clone's origin
  local repo=$1 url
  TC=$TRAIN/$repo
  url=$("$GIT" -C "$CLONES/$repo" remote get-url origin) || return 1
  if [[ ! -d $TC/.git ]]; then "$GIT" clone -q --no-checkout "$url" "$TC" || return 1; fi
  "$GIT" -C "$TC" remote set-url origin "$url"
  "$GIT" -C "$TC" fetch -q --prune origin || return 1
  MAIN=$("$GIT" -C "$TC" rev-parse --verify -q origin/main) || return 1
}
reset_to() { "$GIT" -C "$TC" checkout -q --detach "$1" && "$GIT" -C "$TC" reset -q --hard && "$GIT" -C "$TC" clean -fdxq; }
merge_pr() { # i: merge queue entry i onto HEAD of $TC; 1 on conflict (aborted)
  if ! "$GIT" -C "$TC" merge -q --no-ff --no-edit -m "Merge pull request #${QN[$1]}: ${QT[$1]}" "${QS[$1]}" >/dev/null 2>&1; then
    "$GIT" -C "$TC" merge --abort >/dev/null 2>&1 || true; return 1
  fi
}
recompile() { # crumb recompile commit on HEAD of $TC with the pinned tool, then verify with the same tool
  local changed
  crumb_tool || { RECOMPILE_NOTE="cannot build the pinned crumb tool"; return 1; }
  (cd "$TC" && "$CRUMB_BIN" compile .) >/dev/null 2>&1 || { RECOMPILE_NOTE="crumb compile failed (tool $CRUMB_PIN)"; return 1; }
  changed=$("$GIT" -C "$TC" ls-files -m -o --exclude-standard | grep -E '(^|/)\.crumb$' || true)
  if [[ -n $changed ]]; then
    printf '%s\n' "$changed" | "$GIT" -C "$TC" add --pathspec-from-file=-
    "$GIT" -C "$TC" commit -q -m "train: crumb recompile (crumb tool $CRUMB_PIN)"
  fi
  (cd "$TC" && "$CRUMB_BIN" verify .) >/dev/null 2>&1 || { RECOMPILE_NOTE="crumb verify failed after compile (tool $CRUMB_PIN)"; return 1; }
}
build_stack() { # k -> sets BUILT to the sha of stack k (0 = main), builds and caches it in STACK[k]
  local k=$1 i
  if [[ $k == 0 ]]; then BUILT=$MAIN; return 0; fi
  if [[ -n ${STACK[$k]:-} ]]; then BUILT=${STACK[$k]}; return 0; fi
  reset_to "$MAIN" || return 1
  for ((i = 1; i <= k; i++)); do merge_pr "$i" || { RECOMPILE_NOTE="PR #${QN[$i]} no longer merges"; return 1; }; done
  recompile || return 1
  STACK[$k]=$("$GIT" -C "$TC" rev-parse HEAD)
  BUILT=${STACK[$k]}
}

# admit REPO SLUG: fill the queue from GitHub, excluding what may not ride. Sets N.
admit() {
  local repo=$1 s=$2 prs num sha base draft labeled title files p hit fetched i
  QN=() QS=() QT=() STACK=() RESULT=(); N=0
  prs=$("$GH" api "repos/$s/pulls?state=open&per_page=100" --jq \
    ".[] | [.number, .head.sha, .base.ref, .draft, ([.labels[].name] | index(\"$MERGE_LABEL\") != null), (.title | gsub(\"[\\t\\n\\r]\"; \" \"))] | @tsv" \
    | sort -n) || { journal repo="$repo" result=refused reason="cannot list pull requests"; return 1; }
  reset_to "$MAIN" || return 1
  i=0
  while IFS=$'\t' read -r num sha base draft labeled title; do
    [[ -n $num ]] || continue
    [[ $labeled == true ]] || continue                      # not the train's: skipped silently, like pipeline.sh
    [[ $sha =~ ^[0-9a-f]{40}$ ]] || { journal repo="$repo" pr="$num" result=excluded reason="bad head sha"; continue; }
    [[ $base == main ]] || { journal repo="$repo" pr="$num" sha="$sha" result=excluded reason="base is $base, not main"; continue; }
    [[ $draft == false ]] || { journal repo="$repo" pr="$num" sha="$sha" result=excluded reason="draft"; continue; }
    if [[ $N -ge $MAX_STACK ]]; then journal repo="$repo" pr="$num" sha="$sha" result=deferred reason="stack full ($MAX_STACK)"; continue; fi
    files=$("$GH" api "repos/$s/pulls/$num/files?per_page=100" --paginate --jq '.[] | .filename, (.previous_filename // empty)') \
      || { journal repo="$repo" pr="$num" sha="$sha" result=excluded reason="cannot list changed files"; continue; }
    hit=""
    for p in "${BLOCKED_OPERATOR_PREFIXES[@]}"; do
      hit=$(grep -m1 -E "^$(printf '%s' "$p" | sed 's/[.[\*^$]/\\&/g')" <<< "$files" || true)
      [[ -z $hit ]] || break
    done
    if [[ -n $hit ]]; then journal repo="$repo" pr="$num" sha="$sha" result=excluded reason="BLOCKED_OPERATOR: touches $hit"; continue; fi
    if ! "$GIT" -C "$TC" fetch -q origin "refs/pull/$num/head" 2>/dev/null; then
      journal repo="$repo" pr="$num" sha="$sha" result=excluded reason="cannot fetch refs/pull/$num/head"; continue
    fi
    fetched=$("$GIT" -C "$TC" rev-parse FETCH_HEAD)
    [[ $fetched == "$sha" ]] || { journal repo="$repo" pr="$num" sha="$sha" result=excluded reason="head moved to $fetched"; continue; }
    i=$((N + 1)); QN[$i]=$num; QS[$i]=$sha; QT[$i]=$title
    if ! merge_pr "$i"; then
      unset "QN[$i]" "QS[$i]" "QT[$i]"
      journal repo="$repo" pr="$num" sha="$sha" result=excluded reason="merge conflict with main or an earlier stacked PR"; continue
    fi
    N=$i
    journal repo="$repo" pr="$num" sha="$sha" result=queued reason="position $N"
  done <<< "$prs"
  return 0
}

# ---------------------------------------------------------------- running a stack on the Spark
wait_quiet() {
  local waited=0
  while ! "$QUIETLOCK" check >/dev/null 2>&1; do
    [[ $waited -lt $QUIET_WAIT ]] || return 1
    sleep "$POLL"; waited=$((waited + POLL))
  done
}
push_train_branch() { "$GIT" -C "$TC" push -q -f origin "$2:refs/heads/train/$1" >/dev/null 2>&1; }
run_stack() { # REPO k SHA -> sets RESULT[sha] to PASS|FAIL|<name>; 0 when PASS or FAIL (a verdict), 1 otherwise
  local repo=$1 k=$2 sha=$3 lfile v best keylist require
  if [[ -n ${RESULT[$sha]:-} ]]; then [[ ${RESULT[$sha]} == PASS || ${RESULT[$sha]} == FAIL ]]; return; fi
  if [[ $k != 0 ]] && ! push_train_branch "$repo" "$sha"; then
    RESULT[$sha]=NOT_PUSHED; journal repo="$repo" stack="$k" sha="$sha" result=refused reason="cannot push train/$repo"; return 1
  fi
  if ! wait_quiet; then
    RESULT[$sha]=QUIET; journal repo="$repo" stack="$k" sha="$sha" result=refused reason="quiet flag held for ${QUIET_WAIT}s"; return 1
  fi
  journal repo="$repo" stack="$k" sha="$sha" result=running reason="worker once"
  "$LOCAL_CI" once "$repo" "$sha" > "$STATE/train-logs/$repo.$sha.log" 2>&1 || true
  lfile=$STATE/bundles/$repo/$sha/bundle.json
  if [[ ! -f $lfile ]]; then
    RESULT[$sha]=NO_BUNDLE; journal repo="$repo" stack="$k" sha="$sha" result=refused reason="worker left no bundle"; return 1
  fi
  keylist=$(keys "$CLONES/$repo" origin/main)
  [[ -n $keylist ]] || { RESULT[$sha]=NO_KEY; journal repo="$repo" stack="$k" sha="$sha" result=refused reason="no key in origin/main:.aien/verify.pub"; return 1; }
  require=$("$GIT" -C "$CLONES/$repo" show "origin/main:.aien/always" 2>/dev/null | grep -v '^#' | grep -v '^$' | paste -sd, - || true)
  v=$(verify "$lfile" "$repo" "$sha" "$keylist" "$require"); best=${v%% *}
  RESULT[$sha]=$(verdict_name "$best")
  journal repo="$repo" stack="$k" sha="$sha" result=verdict verdict="${RESULT[$sha]}" reason="verify-bundle exit codes per key: ${v#* }"
  [[ $best == 0 || $best == 1 ]]
}
# test_stack REPO k -> 0 PASS, 1 FAIL, 2 cannot tell (journaled). Builds the stack when needed.
test_stack() {
  local repo=$1 k=$2 sha
  build_stack "$k" || { journal repo="$repo" stack="$k" result=refused reason="cannot build stack: ${RECOMPILE_NOTE:-unknown}"; return 2; }
  sha=$BUILT
  run_stack "$repo" "$k" "$sha" || return 2
  [[ ${RESULT[$sha]} == PASS ]] && return 0
  return 1
}

# ---------------------------------------------------------------- culprit and merge
blame() { # REPO SLUG k: PR k is the culprit (stack k failed, stack k-1 passed)
  local repo=$1 s=$2 k=$3 num=${QN[$3]} sha=${STACK[$3]} prev body url pdig
  build_stack $((k - 1)); prev=$BUILT
  bash "$PIPELINE" publish --repos "$repo" >/dev/null 2>&1 || true
  "$GIT" -C "$RECEIPTS" fetch -q origin 2>/dev/null || true
  pdig=$(published "$repo" "$sha")
  url=$(receipt_url "$repo" "$sha")
  [[ -n $pdig ]] || url="$url (not published yet; local bundle $STATE/bundles/$repo/$sha/bundle.json)"
  body="Merge train: stack $k (main + ${k} pull request(s) in queue order, tested sha $sha) FAILED on the Spark while stack $((k - 1)) ($prev) passed, so this pull request is the one that breaks the gates. Receipt: $url. The \`$MERGE_LABEL\` label was removed; fix, then add it back to ride the next train. Nothing was closed."
  if "$GH" api -X POST "repos/$s/issues/$num/comments" -f body="$body" >/dev/null 2>&1 \
     && "$GH" api -X DELETE "repos/$s/issues/$num/labels/$MERGE_LABEL" >/dev/null 2>&1; then
    journal repo="$repo" pr="$num" sha="${QS[$k]}" stack="$k" tested="$sha" result=culprit reason="stack $k FAIL, stack $((k - 1)) PASS; label removed"
  else
    journal repo="$repo" pr="$num" sha="${QS[$k]}" stack="$k" tested="$sha" result=failed reason="culprit comment or label removal failed"
  fi
}
ensure_train_pr() { # REPO SLUG SHA: one open PR from train/<repo> to main so the hosted required check runs on SHA
  local repo=$1 s=$2 sha=$3 num body
  num=$("$GH" api "repos/$s/pulls?state=open&head=${s%%/*}:train/$repo&per_page=5" --jq '.[0].number // empty' 2>/dev/null || true)
  if [[ -z $num ]]; then
    body="Merge train stack for \`$repo\` (tools/aien-local-ci/train.sh). This pull request exists so the required hosted check runs on the exact tested stack sha; main is fast-forwarded to that sha by the train, never merged through this pull request. Do not merge it by hand."
    num=$("$GH" api -X POST "repos/$s/pulls" -f title="train: $repo stack (do not merge by hand)" -f head="train/$repo" -f base=main -f body="$body" --jq .number 2>/dev/null) || return 1
  fi
  TRAIN_PR=$num
}
wait_checks() { # SLUG SHA -> 0 green, 1 otherwise (CHECK_NOTE says why)
  local waited=0 rc
  while :; do
    rc=0; checks_state "$1" "$2" || rc=$?
    case $rc in 0) return 0 ;; 1) ;; *) return 1 ;; esac
    [[ $waited -lt $CHECK_WAIT ]] || { CHECK_NOTE="$CHECK_NOTE after ${CHECK_WAIT}s"; return 1; }
    sleep "$POLL"; waited=$((waited + POLL))
  done
}
publish_stack() { # REPO SHA -> 0 when the local bundle's digest is at receipts origin
  local repo=$1 sha=$2 ldig pdig attempt
  ldig=$(digest "$STATE/bundles/$repo/$sha/bundle.json")
  for attempt in 1 2 3; do
    bash "$PIPELINE" publish --repos "$repo" >/dev/null 2>&1 || true
    "$GIT" -C "$RECEIPTS" fetch -q origin 2>/dev/null || true
    pdig=$(published "$repo" "$sha")
    [[ $pdig == "$ldig" ]] && { PUB_DIGEST=$ldig; return 0; }
    sleep "$RETRY_SLEEP"
  done
  PUB_NOTE="local sha256:$ldig, published ${pdig:+sha256:}${pdig:-nothing}"
  return 1
}
# fresh REPO SLUG p: every PR 1..p still open, labeled, not draft, head unchanged. Prints the first stale index or 0.
fresh() {
  local repo=$1 s=$2 p=$3 i num pr state draft head labeled
  for ((i = 1; i <= p; i++)); do
    num=${QN[$i]}
    if ! pr=$("$GH" api "repos/$s/pulls/$num" --jq "[.state, .draft, .head.sha, ([.labels[].name] | index(\"$MERGE_LABEL\") != null)] | @tsv" 2>/dev/null); then
      journal repo="$repo" pr="$num" result=stale reason="cannot read pull request" >&2; echo "$i"; return 0
    fi
    IFS=$'\t' read -r state draft head labeled <<< "$pr"
    if [[ $state != open ]]; then journal repo="$repo" pr="$num" result=stale reason="state $state" >&2; echo "$i"; return 0; fi
    if [[ $draft != false ]]; then journal repo="$repo" pr="$num" result=stale reason="draft" >&2; echo "$i"; return 0; fi
    if [[ $labeled != true ]]; then journal repo="$repo" pr="$num" result=stale reason="label $MERGE_LABEL removed" >&2; echo "$i"; return 0; fi
    if [[ $head != "${QS[$i]}" ]]; then journal repo="$repo" pr="$num" result=stale reason="head moved to $head" >&2; echo "$i"; return 0; fi
  done
  echo 0
}
# merge_stack REPO SLUG p: publish, hosted checks, lock, freshness, main unchanged, fast-forward. 0 merged.
merge_stack() {
  local repo=$1 s=$2 p=$3 sha stale i rc now prot reviews err
  while [[ $p -ge 1 ]]; do
    build_stack "$p" || { journal repo="$repo" stack="$p" result=refused reason="cannot build stack $p: ${RECOMPILE_NOTE:-unknown}"; return 1; }
    sha=$BUILT
    if [[ ${RESULT[$sha]:-} != PASS ]]; then
      # a prefix never run (bisection skipped it, or a shorter prefix after a stale drop-out): test it now
      rc=0; test_stack "$repo" "$p" || rc=$?
      [[ $rc == 0 ]] || { journal repo="$repo" stack="$p" sha="$sha" result=refused reason="stack $p is ${RESULT[$sha]:-untested}, nothing to merge"; return 1; }
    fi
    # pipeline.sh publish takes the same lock with flock -n and exits 0 silently when it is busy: wait for it first.
    exec 9>>"$STATE/pipeline.lock"
    if ! flock -w "$LOCK_WAIT" 9; then
      exec 9>&-; journal repo="$repo" stack="$p" sha="$sha" result=refused reason="pipeline lock busy for ${LOCK_WAIT}s (pipeline.sh round or another train); publication and merge deferred"; return 1
    fi
    flock -u 9; exec 9>&-
    if ! publish_stack "$repo" "$sha"; then
      journal repo="$repo" stack="$p" sha="$sha" result=refused reason="bundle not published to receipts: $PUB_NOTE"; return 1
    fi
    push_train_branch "$repo" "$sha" || { journal repo="$repo" stack="$p" sha="$sha" result=refused reason="cannot push train/$repo"; return 1; }
    ensure_train_pr "$repo" "$s" "$sha" || { journal repo="$repo" stack="$p" sha="$sha" result=refused reason="cannot open the train pull request"; return 1; }
    if ! wait_checks "$s" "$sha"; then
      journal repo="$repo" stack="$p" sha="$sha" result=refused reason="$CHECK_NOTE"; return 1
    fi
    exec 9>>"$STATE/pipeline.lock"
    if ! flock -w "$LOCK_WAIT" 9; then
      exec 9>&-; journal repo="$repo" stack="$p" sha="$sha" result=refused reason="pipeline lock busy for ${LOCK_WAIT}s (pipeline.sh merge or another train)"; return 1
    fi
    stale=$(fresh "$repo" "$s" "$p")
    if [[ $stale != 0 ]]; then
      flock -u 9; exec 9>&-
      journal repo="$repo" stack="$p" sha="$sha" result=shortened reason="PR #${QN[$stale]} stale; prefix ends before it"
      p=$((stale - 1)); continue
    fi
    "$GIT" -C "$TC" fetch -q origin main 2>/dev/null || true
    now=$("$GIT" -C "$TC" rev-parse --verify -q origin/main || echo "")
    if [[ $now != "$MAIN" ]]; then
      flock -u 9; exec 9>&-; journal repo="$repo" stack="$p" sha="$sha" result=refused reason="main moved to $now since stacking"; return 1
    fi
    if ! checks_state "$s" "$sha"; then
      flock -u 9; exec 9>&-; journal repo="$repo" stack="$p" sha="$sha" result=refused reason="$CHECK_NOTE (re-checked under lock)"; return 1
    fi
    prot=$("$GH" api "repos/$s/branches/main/protection" 2>/dev/null || echo '{}')
    reviews=$(jq -r '.required_pull_request_reviews != null' <<< "$prot" 2>/dev/null || echo unknown)
    if [[ $reviews == true ]]; then
      journal repo="$repo" stack="$p" sha="$sha" result=warning reason="main protection requires a pull request: GitHub refuses direct pushes, the fast-forward below will fail; design change needed (never loosen protection)"
    fi
    err=$("$GIT" -C "$TC" push -q origin "$sha:refs/heads/main" 2>&1 >/dev/null) && rc=0 || rc=$?
    flock -u 9; exec 9>&-
    if [[ $rc != 0 ]]; then
      journal repo="$repo" stack="$p" sha="$sha" result=failed reason="fast-forward push refused: $(tr '\n' ' ' <<< "$err" | cut -c1-300)"; return 1
    fi
    local nums="" ; for ((i = 1; i <= p; i++)); do nums+="${nums:+,}${QN[$i]}"; done
    journal repo="$repo" stack="$p" sha="$sha" digest="$PUB_DIGEST" prs="$nums" result=merged reason="main fast-forwarded to the tested stack sha"
    [[ -n ${TRAIN_PR:-} ]] && "$GH" api -X PATCH "repos/$s/pulls/$TRAIN_PR" -f state=closed >/dev/null 2>&1 || true
    "$GIT" -C "$TC" push -q origin --delete "train/$repo" >/dev/null 2>&1 || true
    return 0
  done
  journal repo="$repo" result=refused reason="no fresh tested prefix left to merge"; return 1
}

# ---------------------------------------------------------------- one cycle
cycle_repo() {
  local repo=$1 s rc lo hi mid p sha k
  [[ -d $CLONES/$repo ]] || { say "$repo: no trusted clone, skipped"; return 0; }
  if ! s=$(slug "$repo"); then journal repo="$repo" result=refused reason="cannot resolve GitHub slug"; return 0; fi
  if ! train_clone "$repo"; then journal repo="$repo" result=refused reason="cannot prepare the train clone"; return 0; fi
  admit "$repo" "$s" || return 0
  if [[ $N == 0 ]]; then say "$repo: nothing queued at $MAIN"; return 0; fi
  if [[ $DRY == 1 ]]; then
    build_stack "$N" || { journal repo="$repo" result=refused reason="cannot build stack $N: ${RECOMPILE_NOTE:-unknown}"; return 0; }
    sha=$BUILT
    say "$repo DRY RUN: main $MAIN, queue of $N:"
    for ((k = 1; k <= N; k++)); do say "  $k. #${QN[$k]} ${QS[$k]} ${QT[$k]}"; done
    crumb_tool || true
    say "  stack $N = $sha (top: $("$GIT" -C "$TC" log -1 --format=%s "$sha"), crumb tool $CRUMB_PIN)"
    say "  would: push train/$repo, quietlock check, aien-local-ci once $repo $sha, bisect on FAIL, publish, fast-forward main"
    journal repo="$repo" stack="$N" sha="$sha" result=dry-run reason="plan printed, nothing pushed"
    return 0
  fi
  journal repo="$repo" result=cycle reason="main $MAIN, $N queued"
  rc=0; test_stack "$repo" "$N" || rc=$?
  case $rc in
    0) merge_stack "$repo" "$s" "$N" || true; return 0 ;;
    2) return 0 ;;
  esac
  lo=0; hi=$N
  while [[ $((hi - lo)) -gt 1 ]]; do
    mid=$(((lo + hi) / 2))
    rc=0; test_stack "$repo" "$mid" || rc=$?
    case $rc in 0) lo=$mid ;; 1) hi=$mid ;; *) return 0 ;; esac
  done
  if [[ $lo == 0 ]]; then
    # Stack 1 failed: main itself must pass before PR 1 is blamed.
    rc=0; test_stack "$repo" 0 || rc=$?
    case $rc in
      1) journal repo="$repo" sha="$MAIN" result=refused reason="main itself fails the gates; no culprit named"; return 0 ;;
      2) return 0 ;;
    esac
  fi
  blame "$repo" "$s" "$hi"
  [[ $lo -ge 1 ]] && { merge_stack "$repo" "$s" "$lo" || true; }
  return 0
}
do_cycle() { local repo; for repo in "${repo_list[@]}"; do cycle_repo "$repo"; done; }

do_loop() {
  local repo n mx med last t
  declare -A PERIOD DUE
  for repo in "${repo_list[@]}"; do
    if [[ -n $EVERY ]]; then PERIOD[$repo]=$EVERY
    else
      read -r n mx med last < <(cadence_of "$repo")
      if [[ $n == 0 ]]; then say "$repo: no measured T (no real bundle yet), not scheduled; pass --every SECS"; continue; fi
      PERIOD[$repo]=$mx
    fi
    DUE[$repo]=$(date +%s)
    say "$repo: one cycle every ${PERIOD[$repo]}s"
  done
  [[ ${#PERIOD[@]} -gt 0 ]] || die "nothing to schedule"
  while :; do
    t=$(date +%s)
    for repo in "${!PERIOD[@]}"; do
      [[ $t -ge ${DUE[$repo]} ]] || continue
      cycle_repo "$repo"
      DUE[$repo]=$(( $(date +%s) + PERIOD[$repo] ))
    done
    sleep "$POLL"
  done
}

case "$cmd" in
  cadence) do_cadence ;;
  cycle) do_cycle ;;
  loop) do_loop ;;
esac
exit 0
